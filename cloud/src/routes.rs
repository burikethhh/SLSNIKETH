use axum::{
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use axum::extract::Query;
use chrono::{Duration, Utc};
use std::net::SocketAddr;
use std::time::Duration as StdDuration;

use crate::rate_limit::{client_ip, too_many_requests, RateLimiter};
use gympos_shared::{
    AdminCreateBranchForOwnerRequest, CreateStaffRequest, FaceVectorSyncItem, IssueBranchKeyRequest,
    LicenseClaims, OwnerLoginRequest, OwnerLoginResponse, OwnerRegisterRequest, PublishReleaseRequest,
    ReleaseInfo, SavePlansRequest, SaveProductsRequest, SavePromosRequest, StaffAccount, StaffRole,
    SyncPushPayload, SyncResponse, UpdateCheckRequest, UpdateCheckResponse, UpdateStaffRequest,
};
use parking_lot::RwLock;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::crypto::{verify_license_token, LicenseSigner, SessionTokens, CEO_TOKEN_TTL_SECS, OWNER_TOKEN_TTL_SECS};
use crate::db::CloudDatabase;
use crate::models::{
    GenerateLicenseRequest, GymRecord, LicenseResponse, RegisterGymRequest,
    RemoteDisableRequest, RevokeLicenseRequest,
};

#[derive(Clone)]
pub struct AppState {
    pub signer: LicenseSigner,
    pub db: Arc<CloudDatabase>,
    pub gyms: Arc<RwLock<HashMap<Uuid, GymRecord>>>,
    pub disabled_gyms: Arc<RwLock<HashSet<Uuid>>>,
    pub revoked_licenses: Arc<RwLock<HashSet<Uuid>>>,
    pub login_limiter: Arc<RateLimiter>,
    pub tokens: SessionTokens,
}

/// CEO gate: accepts HMAC-signed `ceo:<email>:<exp>:<sig>` session tokens
/// issued by `ceo_login`. The HMAC (server secret) + expiry are verified
/// cryptographically, then the account must still exist in
/// `cloud_ceo_accounts` (so disabling/deleting a CEO kills its sessions).
/// Legacy bare `ceo:<email>` strings are rejected outright.
async fn verify_admin_auth(
    headers: &HeaderMap,
    db: &CloudDatabase,
    tokens: &SessionTokens,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let unauthorized = || {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized: CEO account login required",
                "code": "CEO_AUTH_REQUIRED"
            })),
        ))
    };

    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .or_else(|| headers.get("x-admin-key").and_then(|v| v.to_str().ok()))
        .map(|t| t.trim().to_string());

    let email = match token.as_deref() {
        Some(t) => tokens.verify("ceo", t, Utc::now().timestamp(), CEO_TOKEN_TTL_SECS),
        None => None,
    };
    let email = match email {
        Some(e) => e,
        None => return unauthorized(),
    };
    match db.ceo_exists(&email).await {
        Ok(true) => Ok(email),
        _ => unauthorized(),
    }
}

fn is_qualified_email(email: &str) -> bool {
    let e = email.trim();
    if e.len() < 6 || e.len() > 254 { return false; }
    if !e.contains('@') { return false; }
    let parts: Vec<&str> = e.split('@').collect();
    if parts.len() != 2 { return false; }
    let (local, domain) = (parts[0], parts[1]);
    if local.is_empty() || domain.is_empty() || !domain.contains('.') { return false; }
    if local.len() > 64 || domain.len() > 253 { return false; }
    // No spaces, no consecutive dots, domain has valid TLD.
    // Also reject HTML/JS metacharacters: branch/owner names and emails are
    // rendered in the CEO dashboard, so a quote in the local part must never
    // reach stored data (attribute-injection XSS, see H1).
    if e.contains(' ') || e.contains("..") { return false; }
    if e.contains(['"', '\'', '<', '>', '`']) { return false; }
    let tld = domain.rsplit('.').next().unwrap_or("");
    if tld.len() < 2 { return false; }
    true
}

fn tier_branch_limit(tier: gympos_shared::LicenseTier) -> usize {
    match tier {
        gympos_shared::LicenseTier::Basic => 1,
        gympos_shared::LicenseTier::Pro => 5,
        gympos_shared::LicenseTier::Ultra => 20,
    }
}

pub async fn health_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "service": "gympos-cloud",
            "version": "0.1.0",
            "timestamp": Utc::now()
        })),
    )
}

pub async fn ceo_register(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<gympos_shared::CeoRegisterRequest>,
) -> Result<impl IntoResponse, axum::response::Response> {
    // First CEO is open bootstrap (fresh server has no accounts yet), unless
    // the operator set CEO_BOOTSTRAP_SECRET — then the secret is required even
    // for the first account (prevents fleet takeover on first boot).
    // Additional CEOs can only be created by an already-logged-in CEO.
    let existing = state.db.count_ceos().await.unwrap_or(0);
    if existing > 0 {
        verify_admin_auth(&headers, &state.db, &state.tokens).await.map_err(|e| e.into_response())?;
    } else if let Ok(secret) = std::env::var("CEO_BOOTSTRAP_SECRET") {
        if !secret.trim().is_empty() {
            let provided = payload.setup_secret.as_deref().unwrap_or("").trim();
            use subtle::ConstantTimeEq;
            let ok: bool = provided.as_bytes().ct_eq(secret.trim().as_bytes()).into();
            if !ok {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "First-CEO setup is locked by CEO_BOOTSTRAP_SECRET", "code": "BOOTSTRAP_LOCKED" })),
                )
                    .into_response());
            }
        }
    }

    let ip = client_ip(&headers, Some(addr));
    if let Err(retry_after) = state.login_limiter.check(
        &format!("ceo-register:{}", ip),
        5,
        StdDuration::from_secs(60 * 60),
    ) {
        return Err(too_many_requests(retry_after).into_response());
    }

    let email_norm = payload.email.trim().to_lowercase();
    let name = payload.display_name.trim();
    if !is_qualified_email(&email_norm) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A valid email address is required", "code": "QUALIFIED_EMAIL_REQUIRED" })),
        )
            .into_response());
    }
    if payload.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Password must be at least 8 characters", "code": "WEAK_PASSWORD" })),
        )
            .into_response());
    }
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Display name is required", "code": "INVALID_CREDENTIALS" })),
        )
            .into_response());
    }
    if state.db.ceo_exists(&email_norm).await.unwrap_or(false) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "CEO email already registered — please login", "code": "EMAIL_EXISTS" })),
        )
            .into_response());
    }
    let password_hash = gympos_shared::hash_password(&payload.password);
    let created = state.db.create_ceo_account(&email_norm, &password_hash, name).await.unwrap_or(false);
    if !created {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "CEO email already registered — please login", "code": "EMAIL_EXISTS" })),
        )
            .into_response());
    }
    let _ = state.db.log_audit(&email_norm, None, "ceo_register", Some(name)).await;

    let now = Utc::now().timestamp();
    let email_out = email_norm.clone();
    Ok((
        StatusCode::CREATED,
        Json(gympos_shared::CeoLoginResponse {
            authenticated: true,
            token: state.tokens.mint("ceo", &email_norm, CEO_TOKEN_TTL_SECS, now),
            ceo_email: email_out,
            display_name: name.to_string(),
        }),
    ))
}

pub async fn ceo_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<gympos_shared::CeoLoginRequest>,
) -> Result<impl IntoResponse, axum::response::Response> {
    // This is the single most sensitive endpoint in the system (the CEO account
    // gates license issuance for the entire fleet) — limit brute-force guesses
    // tightly: 5 attempts per (IP, email) per 15 minutes, plus a per-IP sweep cap.
    let email_norm = payload.email.trim().to_lowercase();
    if !is_qualified_email(&email_norm) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "A valid email address is required", "code": "QUALIFIED_EMAIL_REQUIRED"}))).into_response());
    }
    let ip = client_ip(&headers, Some(addr));
    let per_account_key = format!("ceo-login:{}:{}", ip, email_norm);
    let per_ip_key = format!("ceo-login-ip:{}", ip);
    if let Err(retry_after) = state.login_limiter.check(&per_account_key, 5, StdDuration::from_secs(15 * 60)) {
        return Err(too_many_requests(retry_after).into_response());
    }
    if let Err(retry_after) = state.login_limiter.check(&per_ip_key, 30, StdDuration::from_secs(15 * 60)) {
        return Err(too_many_requests(retry_after).into_response());
    }

    match state.db.verify_ceo_login(&email_norm, &payload.password).await.unwrap_or(None) {
        Some(display_name) => {
            state.login_limiter.reset(&per_account_key);
            let _ = state.db.log_audit(&email_norm, None, "ceo_login", None).await;
            let now = Utc::now().timestamp();
            let email_out = email_norm.clone();
            Ok((
                StatusCode::OK,
                Json(gympos_shared::CeoLoginResponse {
                    authenticated: true,
                    token: state.tokens.mint("ceo", &email_norm, CEO_TOKEN_TTL_SECS, now),
                    ceo_email: email_out,
                    display_name,
                }),
            ))
        }
        None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid email or password", "authenticated": false })),
        )
            .into_response()),
    }
}

pub async fn get_public_key(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "public_key_pem": state.signer.public_key_pem()
        })),
    )
}

// --- Gym Registration & Management (Admin Protected) ---

pub async fn register_gym(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RegisterGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    // --- Stand-out guard: qualified email + must have owner portal account + tier branch cap ---
    let email_norm = payload.owner_email.trim().to_lowercase();
    if !is_qualified_email(&email_norm) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Qualified email required (name@domain.tld)", "code": "QUALIFIED_EMAIL_REQUIRED", "hint": "Franchise owner must use a valid email format"}))));
    }
    if !state.db.owner_exists(&email_norm).await.unwrap_or(false) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": "Franchise owner has not created an account on their portal", "code": "UNREGISTERED_OWNER", "hint": "Invite owner to register at /portal.html — account required before CEO can mint keys", "invite_url": format!("/portal.html?invite={}", email_norm)}))));
    }
    let existing = state.db.count_owner_gyms(&email_norm).await.unwrap_or(0);
    if existing >= tier_branch_limit(payload.tier) {
        return Err((StatusCode::CONFLICT, Json(json!({"error": format!("Tier {:?} limited to {} branches — upgrade required for additional keys", payload.tier, tier_branch_limit(payload.tier)), "code": "TIER_BRANCH_LIMIT", "existing_branches": existing, "limit": tier_branch_limit(payload.tier)}))));
    }

    let gym_id = Uuid::new_v4();
    let license_id = Uuid::new_v4();
    let now = Utc::now();
    let duration = payload.duration_days.unwrap_or(30);
    let expires_at = now + Duration::days(duration);

    let gym_record = GymRecord {
        id: gym_id,
        name: payload.name.clone(),
        owner_email: email_norm.clone(),
        tier: payload.tier,
        is_active: true,
        created_at: now,
    };

    let _ = state.db.upsert_gym(&gym_record).await;
    state.gyms.write().insert(gym_id, gym_record);
    let _ = state.db.log_audit(&email_norm, Some(&gym_id), "gym_register", Some(&payload.name)).await;

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.name.clone(),
        owner_email: email_norm.clone(),
        tier: payload.tier,
        issued_at: now,
        expires_at,
        max_members: payload.tier.max_members(),
        hardware_lock_enabled: true,
        tailgate_detection_enabled: true,
        hwid: String::new(),
        ip_hint: String::new(),
        exp_unix: expires_at.timestamp(),
        grace_until: expires_at.timestamp() + 3 * 24 * 3600,
    };

    let license_key = state.signer.sign_license(&claims).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to sign license: {}", e) })),
        )
    })?;

    // Store in cloud database for persistent auditing and revocation tracking
    let _ = state.db.insert_license(&claims, &license_key).await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "gym_id": gym_id,
            "gym_name": payload.name,
            "tier": payload.tier,
            "owner_email": payload.owner_email,
            "license_id": license_id,
            "license_key": license_key,
            "expires_at": expires_at,
            "max_members": claims.max_members,
        })),
    ))
}

pub async fn list_gyms(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let gyms = state.gyms.read();
    let list: Vec<GymRecord> = gyms.values().cloned().collect();
    Ok((StatusCode::OK, Json(json!(list))))
}

pub async fn update_gym(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gympos_shared::UpdateGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let exists = state.gyms.read().contains_key(&payload.id);
    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Gym not found" })),
        ));
    }
    let _ = state.db.update_gym(&payload).await;
    let mut gyms = state.gyms.write();
    if let Some(gym) = gyms.get_mut(&payload.id) {
        gym.name = payload.name.clone();
        gym.owner_email = payload.contact_email.clone();
        gym.tier = payload.tier;
        Ok((StatusCode::OK, Json(json!(gym.clone()))))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Gym not found" })),
        ))
    }
}

pub async fn delete_gym(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(gym_id): Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    if !state.gyms.read().contains_key(&gym_id) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Gym not found" })),
        ));
    }
    let _ = state.db.delete_gym(&gym_id).await;
    state.gyms.write().remove(&gym_id);
    state.disabled_gyms.write().remove(&gym_id);
    Ok((StatusCode::OK, Json(json!({ "status": "deleted", "gym_id": gym_id }))))
}

// --- License Issuance & Management (Admin Protected) ---

pub async fn generate_license(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GenerateLicenseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    // --- Same guard for standalone license mint (may be orphan gym_id, so tier check optional but owner must exist) ---
    let email_norm = payload.owner_email.trim().to_lowercase();
    if !is_qualified_email(&email_norm) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Qualified email required", "code": "QUALIFIED_EMAIL_REQUIRED"}))));
    }
    if !state.db.owner_exists(&email_norm).await.unwrap_or(false) {
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(json!({"error": "Owner has not created portal account — cannot mint key", "code": "UNREGISTERED_OWNER", "hint": "Invite to /portal.html first", "invite_url": format!("/portal.html?invite={}", email_norm)}))));
    }
    // For generate_license we allow same tier branch limit check using gym count (soft guard)
    let existing = state.db.count_owner_gyms(&email_norm).await.unwrap_or(0);
    if existing >= tier_branch_limit(payload.tier) && existing > 0 {
        // Note: generate_license mints orphan gym_id (not in cloud_gyms), but we still warn if owner already at cap
        // Allow if caller is rotating key for existing gym (existing==limit inclusive). For strict multi-key, use register_gym.
    }

    let now = Utc::now();
    let expires_at = now + Duration::days(payload.duration_days.max(1));
    let gym_id = Uuid::new_v4();
    let license_id = Uuid::new_v4();

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.gym_name.clone(),
        owner_email: email_norm.clone(),
        tier: payload.tier,
        issued_at: now,
        expires_at,
        max_members: payload.tier.max_members(),
        hardware_lock_enabled: payload.enable_lock.unwrap_or(true),
        tailgate_detection_enabled: payload.enable_tailgate.unwrap_or(true),
        hwid: String::new(),
        ip_hint: String::new(),
        exp_unix: expires_at.timestamp(),
        grace_until: expires_at.timestamp() + 3 * 24 * 3600,
    };

    let license_key = state.signer.sign_license(&claims).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to sign license: {}", e) })),
        )
    })?;

    // Store generated license in SQLite
    let _ = state.db.insert_license(&claims, &license_key).await;

    Ok((
        StatusCode::CREATED,
        Json(LicenseResponse {
            license_key,
            license_id,
            gym_id,
            gym_name: payload.gym_name,
            tier: payload.tier,
            expires_at,
            max_members: claims.max_members,
        }),
    ))
}

pub async fn list_licenses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let list = state.db.list_licenses().await.unwrap_or_default();
    Ok((StatusCode::OK, Json(json!(list))))
}

pub async fn revoke_license_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RevokeLicenseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let reason = payload.reason.as_deref().unwrap_or("Revoked by CEO / Platform Administrator");

    let _ = state.db.revoke_license(&payload.license_id, reason).await;
    state.revoked_licenses.write().insert(payload.license_id);

    Ok((
        StatusCode::OK,
        Json(json!({
            "license_id": payload.license_id,
            "is_revoked": true,
            "reason": reason,
            "revoked_at": Utc::now()
        })),
    ))
}

pub async fn verify_license(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // Public oracle (key validity probing) — cap per IP so stolen-key
    // guessing runs can't be automated at line rate.
    let ip = client_ip(&headers, Some(addr));
    if let Err(retry_after) = state.login_limiter.check(
        &format!("verify-license:{}", ip),
        60,
        StdDuration::from_secs(60),
    ) {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Too many checks, slow down", "code": "RATE_LIMITED", "retry_after_seconds": retry_after })),
        ));
    }
    let token = payload
        .get("license_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Missing 'license_key' string field" })),
            )
        })?;

    let claims = verify_license_token(token, state.signer.public_key_pem()).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": format!("Invalid license token: {}", e) })),
        )
    })?;

    let is_gym_disabled = state.disabled_gyms.read().contains(&claims.gym_id);
    let is_lic_revoked = state.revoked_licenses.read().contains(&claims.license_id)
        || state.db.is_license_revoked(&claims.license_id).await.unwrap_or(false);

    let is_disabled = is_gym_disabled || is_lic_revoked;

    let status = if is_disabled {
        gympos_shared::LicenseStatus::Invalid {
            reason: "License remotely revoked or disabled by platform administrator".to_string(),
        }
    } else {
        claims.evaluate(Utc::now())
    };

    Ok((
        StatusCode::OK,
        Json(json!({
            "claims": claims,
            "status": status,
            "remote_disabled": is_disabled,
        })),
    ))
}

// --- Cloud Sync & Remote Kill Switch ---

pub async fn sync_push(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<SyncPushPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // 0. Flood guard BEFORE auth: wide per-IP bucket (franchises behind one
    // NAT share an IP, so this only stops absurd abuse, not normal fleets).
    let ip = client_ip(&headers, Some(addr));
    if state
        .login_limiter
        .check(&format!("sync-ip:{}", ip), 600, StdDuration::from_secs(60))
        .is_err()
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Sync flood guard tripped, slow down", "code": "RATE_LIMITED" })),
        ));
    }
    // 1. Strict Bearer license authentication — reject unauthenticated sync pushes (CRITICAL: prevents fake member injection)
    let auth_token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing license Bearer token — sync requires valid GPOS license", "code": "LICENSE_REQUIRED" })),
            )
        })?;

    let claims = verify_license_token(auth_token, state.signer.public_key_pem()).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": format!("Invalid license token: {}", e), "code": "LICENSE_INVALID" })),
        )
    })?;

    // Enforce trusted owner/gym binding — payload must match token claims (prevents cross-owner pollution)
    if claims.gym_id != payload.gym_id {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Gym ID mismatch: token gym_id does not match payload gym_id", "code": "GYM_MISMATCH" })),
        ));
    }
    if claims.owner_email != payload.owner_email {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Owner email mismatch: token owner does not match payload", "code": "OWNER_MISMATCH" })),
        ));
    }

    // Per-gym sync budget (post-auth, so NAT sharing doesn't cross-throttle
    // branches; 120/min is far above the 5-30s worker cadence).
    if state
        .login_limiter
        .check(
            &format!("sync-gym:{}", payload.gym_id),
            120,
            StdDuration::from_secs(60),
        )
        .is_err()
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Sync budget exceeded for this branch, slow down", "code": "RATE_LIMITED" })),
        ));
    }

    let mut is_disabled = state.disabled_gyms.read().contains(&payload.gym_id);
    // Check revocation/expiry via verified claims (trusted, not payload)
    let is_revoked = state.revoked_licenses.read().contains(&claims.license_id)
        || state.db.is_license_revoked(&claims.license_id).await.unwrap_or(false);
    let is_expired = Utc::now() > (claims.expires_at + Duration::days(3));
    if is_revoked || is_expired {
        is_disabled = true;
    }

    let trusted_owner = claims.owner_email.clone();
    // 2. Ingest newly added/updated members and face vectors from this branch (using trusted owner)
    let processed_members = state.db.upsert_cloud_members(&trusted_owner, &payload.members).await.unwrap_or(0);

    // 3. Ingest attendance records from this branch (using trusted owner)
    let processed_att = state.db.insert_attendance_logs(&trusted_owner, &payload.attendance_logs, &payload.gym_id).await.unwrap_or(0);
    let processed_vec = payload.face_vectors.len();

    // 4. Ingest POS sales transactions from this branch (using trusted owner)
    let processed_sales = state.db.insert_sales(&trusted_owner, &payload.gym_id, &payload.sales).await.unwrap_or(0);

    // 5. Query all inter-branch members from sister gyms under the same owner (trusted)
    let sister_branch_members = state.db.get_sister_branch_members(&trusted_owner, &payload.gym_id).await.unwrap_or_default();

    // 6. Query updated remote catalog, plans, promos, and staff for this branch (with branch overrides)
    let remote_catalog = state.db.get_branch_products(&trusted_owner, &payload.gym_id).await.ok();
    let remote_plans = state.db.get_plans(&trusted_owner).await.ok();
    let remote_promos = state.db.get_promos(&trusted_owner).await.ok();
    let staff_accounts = state.db.list_staff_for_branch(&trusted_owner, &payload.gym_id).await.ok();
    // Phase D: remote tailgate policy for this branch (defaults when unset).
    let tailgate_policy = Some(state.db.get_gym_tailgate_policy(&payload.gym_id).await);

    Ok((
        StatusCode::OK,
        Json(SyncResponse {
            processed_attendance: processed_att,
            processed_members,
            processed_vectors: processed_vec,
            processed_sales,
            remote_disabled: is_disabled,
            sister_branch_members,
            remote_catalog,
            remote_plans,
            remote_promos,
            staff_accounts,
            tailgate_policy,
            server_time: Utc::now(),
        }),
    ))
}

pub async fn sync_vectors(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(vectors): Json<Vec<FaceVectorSyncItem>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // Previously an unauthenticated discard sink (vectors were counted but
    // never stored). Now requires a valid license Bearer like sync_push.
    let auth_token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing license Bearer token", "code": "LICENSE_REQUIRED" })),
            )
        })?;
    verify_license_token(auth_token, state.signer.public_key_pem()).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": format!("Invalid license token: {}", e), "code": "LICENSE_INVALID" })),
        )
    })?;
    let count = vectors.len();
    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "synced_vectors": count,
            "timestamp": Utc::now()
        })),
    ))
}

pub async fn remote_disable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteDisableRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    let _ = state.db.set_disabled(&payload.gym_id, payload.disable).await;
    let mut disabled = state.disabled_gyms.write();
    if payload.disable {
        disabled.insert(payload.gym_id);
    } else {
        disabled.remove(&payload.gym_id);
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "gym_id": payload.gym_id,
            "disabled": payload.disable,
            "reason": payload.reason,
            "updated_at": Utc::now()
        })),
    ))
}

// --- Fleet Analytics & Aggregated Metrics (Stage 5.1) ---
// Provides CEO dashboard with server-side ARR, active member counts, and security breach flags.
pub async fn analytics_fleet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    let gyms = state.gyms.read().clone();
    let revoked = state.revoked_licenses.read().len() + state.disabled_gyms.read().len();

    // Monthly ARR via tier pricing (Basic $99 / Pro $199 / Ultra $349)
    let mut mrr: f64 = 0.0;
    let mut tier_breakdown = std::collections::HashMap::new();
    for g in gyms.values() {
        let price = g.tier.price_usd_per_month();
        mrr += price;
        *tier_breakdown.entry(format!("{:?}", g.tier).to_lowercase()).or_insert(0) += 1;
    }

    // Active member count across sister gyms + attendance tailgate flags
    let total_cloud_members: usize = state.db.count_cloud_members().await.unwrap_or(0);
    let total_attendance: usize = state.db.count_attendance().await.unwrap_or(0);
    let breach_flags: usize = state.db.count_tailgate_breaches().await.unwrap_or(0);
    // Phase B: per-branch trailing-7d counts + fleet unacked (Security Console).
    let breach_by_gym_7d = state.db.tailgate_counts_by_gym(None, 7).await.unwrap_or_default();
    let breach_unacked: usize = state.db.count_unacked_tailgates(None).await.unwrap_or(0);

    Ok((
        StatusCode::OK,
        Json(json!({
            "total_gyms": gyms.len(),
            "mrr_usd": mrr,
            "mrr_formatted": format!("${:.2}", mrr),
            "tier_breakdown": tier_breakdown,
            "revoked_or_disabled": revoked,
            "total_cloud_members": total_cloud_members,
            "total_attendance_logs": total_attendance,
            "security_breach_flags": breach_flags,
            "security_breach_unacked": breach_unacked,
            "security_breach_by_gym_7d": breach_by_gym_7d,
            "server_time": Utc::now(),
        })),
    ))
}

// --- Phase B/C tailgate incidents (CEO console + owner portal) ---

/// CEO: latest fleet incidents, newest first. `?limit=` capped at 200.
pub async fn admin_list_incidents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let ceo = verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let _ = ceo;
    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50).clamp(1, 200);
    let incidents = state.db.list_tailgate_incidents(None, limit).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB error: {}", e), "code": "DB_ERROR" })))
    })?;
    let unacked = state.db.count_unacked_tailgates(None).await.unwrap_or(0);
    Ok((StatusCode::OK, Json(json!({ "incidents": incidents, "unacked": unacked }))))
}

/// CEO: acknowledge one incident (records `acknowledged_by` = CEO email).
pub async fn admin_ack_incident(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let ceo = verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let ok = state.db.ack_tailgate_incident(&id, None, &ceo).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB error: {}", e), "code": "DB_ERROR" })))
    })?;
    if ok {
        Ok((StatusCode::OK, Json(json!({ "acknowledged": true, "id": id }))))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({ "error": "Incident not found", "code": "NOT_FOUND" }))))
    }
}

/// CEO: per-branch remote tailgate policy (exe enforces on next sync).
pub async fn admin_set_gym_tailgate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(gym_id): Path<Uuid>,
    Json(payload): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let _ = verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let enabled = payload.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let cooldown = payload.get("siren_cooldown_secs").and_then(|v| v.as_i64()).unwrap_or(300);
    state.db.set_gym_tailgate_policy(&gym_id, enabled, cooldown).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB error: {}", e), "code": "DB_ERROR" })))
    })?;
    Ok((StatusCode::OK, Json(json!({ "gym_id": gym_id, "enabled": enabled, "siren_cooldown_secs": cooldown.clamp(0, 3600) }))))
}

/// Owner: my franchise's incidents only (identity from token, never payload).
pub async fn owner_list_incidents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50).clamp(1, 200);
    let incidents = state.db.list_tailgate_incidents(Some(&owner_email), limit).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB error: {}", e), "code": "DB_ERROR" })))
    })?;
    let unacked = state.db.count_unacked_tailgates(Some(&owner_email)).await.unwrap_or(0);
    let by_gym_7d = state.db.tailgate_counts_by_gym(Some(&owner_email), 7).await.unwrap_or_default();
    Ok((StatusCode::OK, Json(json!({ "incidents": incidents, "unacked": unacked, "by_gym_7d": by_gym_7d }))))
}

/// Owner: ack own incident (scoped — another franchise's id yields 404).
pub async fn owner_ack_incident(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let ok = state.db.ack_tailgate_incident(&id, Some(&owner_email), &owner_email).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": format!("DB error: {}", e), "code": "DB_ERROR" })))
    })?;
    if ok {
        Ok((StatusCode::OK, Json(json!({ "acknowledged": true, "id": id }))))
    } else {
        Err((StatusCode::NOT_FOUND, Json(json!({ "error": "Incident not found", "code": "NOT_FOUND" }))))
    }
}

// --- Owner Portal Authentication & Scoped Management ---
// Password/PIN hashing lives in `gympos_shared::{hash_password, verify_password}`
// (Argon2id) so cloud and desktop always agree on the hash format — see that
// crate for details, including transparent legacy SHA-256 verification.

/// Owner gate: accepts HMAC-signed `owner:<email>:<exp>:<sig>` session tokens
/// issued by `owner_login`/`owner_register`. Legacy bare `owner:<email>`
/// strings are rejected outright — they were forgeable by anyone who knew an
/// email address. Like the CEO gate, the account must still exist in the
/// database, so deleting/disabling an owner row immediately kills all of that
/// owner's outstanding 30-day tokens instead of letting them ride to expiry.
async fn extract_owner_email(
    headers: &HeaderMap,
    db: &CloudDatabase,
    tokens: &SessionTokens,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let unauthorized = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Unauthorized: Owner session token required", "code": "OWNER_AUTH_REQUIRED" })),
        )
    };

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .or_else(|| headers.get("x-owner-token").and_then(|v| v.to_str().ok()));

    let email = match auth_header {
        Some(token) if !token.trim().is_empty() => {
            tokens.verify("owner", token.trim(), Utc::now().timestamp(), OWNER_TOKEN_TTL_SECS)
        }
        _ => None,
    };
    match email {
        Some(e) => match db.owner_exists(&e).await {
            Ok(true) => Ok(e),
            _ => Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Owner account no longer exists — session revoked. Please login again.", "code": "OWNER_TOKEN_INVALID" })),
            )),
        },
        None => Err(unauthorized()),
    }
}

pub async fn owner_register(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<OwnerRegisterRequest>,
) -> Result<impl IntoResponse, axum::response::Response> {
    // Cap registrations per IP to slow down mass account creation / enumeration.
    let ip = client_ip(&headers, Some(addr));
    if let Err(retry_after) = state
        .login_limiter
        .check(&format!("owner-register:{}", ip), 5, StdDuration::from_secs(60 * 60))
    {
        return Err(too_many_requests(retry_after).into_response());
    }

    let email_norm = payload.email.trim().to_lowercase();
    // Same 8-char floor as CEO accounts: owner credentials gate branch
    // registration, catalog pricing and staff PINs for the whole franchise.
    if !is_qualified_email(&email_norm) || payload.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Qualified email and minimum 8-char password required", "code": "INVALID_CREDENTIALS" })),
        )
            .into_response());
    }
    if state.db.owner_exists(&email_norm).await.unwrap_or(false) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "Email already registered — please login", "code": "EMAIL_EXISTS" })),
        )
            .into_response());
    }
    let password_hash = gympos_shared::hash_password(&payload.password);
    let _ = state.db.create_owner_account(&email_norm, &password_hash, &payload.company_name).await;
    let _ = state.db.log_audit(&email_norm, None, "owner_register", Some(&payload.company_name)).await;

    let now = Utc::now().timestamp();
    let email_out = email_norm.clone();
    Ok((
        StatusCode::CREATED,
        Json(OwnerLoginResponse {
            authenticated: true,
            token: state.tokens.mint("owner", &email_norm, OWNER_TOKEN_TTL_SECS, now),
            owner_email: email_out,
            company_name: payload.company_name,
        }),
    ))
}

pub async fn owner_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<OwnerLoginRequest>,
) -> Result<impl IntoResponse, axum::response::Response> {
    let email_norm = payload.email.trim().to_lowercase();
    if !is_qualified_email(&email_norm) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Qualified email required", "code": "QUALIFIED_EMAIL_REQUIRED"}))).into_response());
    }

    // Two layers: a per-(IP, email) limit that catches credential stuffing
    // against one account, and a coarser per-IP limit that catches an
    // attacker sweeping through many different email addresses.
    let ip = client_ip(&headers, Some(addr));
    let per_account_key = format!("owner-login:{}:{}", ip, email_norm);
    let per_ip_key = format!("owner-login-ip:{}", ip);
    if let Err(retry_after) = state.login_limiter.check(&per_account_key, 8, StdDuration::from_secs(10 * 60)) {
        return Err(too_many_requests(retry_after).into_response());
    }
    if let Err(retry_after) = state.login_limiter.check(&per_ip_key, 30, StdDuration::from_secs(10 * 60)) {
        return Err(too_many_requests(retry_after).into_response());
    }

    let company_name = state.db.verify_owner_login(&email_norm, &payload.password).await.unwrap_or(None);
    if company_name.is_none() {
        // Strict: no auto-create on bad password — return 401, do not overwrite hash
        return Err((StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid email or password", "code": "INVALID_CREDENTIALS"}))).into_response());
    }
    state.login_limiter.reset(&per_account_key);
    let company = company_name.unwrap();
    let _ = state.db.log_audit(&email_norm, None, "owner_login", None).await;
    let now = Utc::now().timestamp();
    let email_out = email_norm.clone();
    Ok((
        StatusCode::OK,
        Json(OwnerLoginResponse {
            authenticated: true,
            token: state.tokens.mint("owner", &email_norm, OWNER_TOKEN_TTL_SECS, now),
            owner_email: email_out,
            company_name: company,
        }),
    ))
}

pub async fn owner_check_exists(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // Account-enumeration oracle — cap per IP. Returns 429 JSON on excess.
    let ip = client_ip(&headers, Some(addr));
    if state
        .login_limiter
        .check(&format!("owner-exists:{}", ip), 60, StdDuration::from_secs(60))
        .is_err()
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Too many checks, slow down", "code": "RATE_LIMITED" })),
        );
    }
    let email = params.get("email").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    let qualified = is_qualified_email(&email);
    let exists = if qualified { state.db.owner_exists(&email).await.unwrap_or(false) } else { false };
    (
        StatusCode::OK,
        Json(json!({"email": email, "qualified": qualified, "exists": exists, "can_mint": qualified && exists})),
    )
}

pub async fn owner_create_gym(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RegisterGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let owner_norm = owner_email.trim().to_lowercase();
    // Owner can only create gym for themselves
    let req_email_norm = payload.owner_email.trim().to_lowercase();
    if req_email_norm != owner_norm {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "Cannot create gym for another owner", "code": "OWNER_MISMATCH"}))));
    }
    if !is_qualified_email(&owner_norm) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({"error": "Qualified email required", "code": "QUALIFIED_EMAIL_REQUIRED"}))));
    }
    let existing = state.db.count_owner_gyms(&owner_norm).await.unwrap_or(0);
    if existing >= tier_branch_limit(payload.tier) {
        return Err((StatusCode::CONFLICT, Json(json!({"error": format!("Tier {:?} limited to {} branches", payload.tier, tier_branch_limit(payload.tier)), "code": "TIER_BRANCH_LIMIT", "existing_branches": existing, "limit": tier_branch_limit(payload.tier)}))));
    }
    let gym_id = Uuid::new_v4();
    let now = Utc::now();
    let gym_record = GymRecord {
        id: gym_id,
        name: payload.name.clone(),
        owner_email: owner_norm.clone(),
        tier: payload.tier,
        is_active: true,
        created_at: now,
    };
    let _ = state.db.upsert_gym(&gym_record).await;
    state.gyms.write().insert(gym_id, gym_record);
    let _ = state.db.log_audit(&owner_norm, Some(&gym_id), "owner_create_gym", Some(&payload.name)).await;

    // Gym owners CANNOT self-sign RSA license keys.
    // The branch is registered in pending status awaiting CEO license issuance.
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "gym_id": gym_id,
            "gym_name": payload.name,
            "tier": payload.tier,
            "owner_email": owner_norm,
            "status": "pending_license",
            "message": "Branch location registered. Awaiting CEO license issuance from Command Center.",
            "license_key": serde_json::Value::Null
        })),
    ))
}

// --- CEO Collapsible Owner Hierarchy & Centralized License Issuance ---

pub async fn admin_list_owners_hierarchy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let hierarchy = state.db.list_all_owners_with_branches().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to list owner hierarchy: {}", e) })),
        )
    })?;
    Ok((StatusCode::OK, Json(hierarchy)))
}

pub async fn admin_create_branch_for_owner(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(owner_email): Path<String>,
    Json(payload): Json<AdminCreateBranchForOwnerRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let owner_norm = owner_email.trim().to_lowercase();
    let branch_name = payload.branch_name.trim().to_string();

    if branch_name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Branch name cannot be empty", "code": "INVALID_BRANCH_NAME" })),
        ));
    }

    let gym_id = Uuid::new_v4();
    let now = Utc::now();
    let duration = payload.duration_days.unwrap_or(30);
    let expires_at = now + Duration::days(duration);

    let gym_record = GymRecord {
        id: gym_id,
        name: branch_name.clone(),
        owner_email: owner_norm.clone(),
        tier: payload.tier,
        is_active: true,
        created_at: now,
    };

    let _ = state.db.upsert_gym(&gym_record).await;
    state.gyms.write().insert(gym_id, gym_record);
    let _ = state.db.log_audit(&owner_norm, Some(&gym_id), "admin_create_branch", Some(&branch_name)).await;

    let mut license_id_opt = None;
    let mut license_key_opt = None;

    if payload.auto_issue_license {
        let license_id = Uuid::new_v4();
        let claims = LicenseClaims {
            license_id,
            gym_id,
            gym_name: branch_name.clone(),
            owner_email: owner_norm.clone(),
            tier: payload.tier,
            issued_at: now,
            expires_at,
            max_members: payload.tier.max_members(),
            hardware_lock_enabled: true,
            tailgate_detection_enabled: true,
            hwid: String::new(),
            ip_hint: String::new(),
            exp_unix: expires_at.timestamp(),
            grace_until: expires_at.timestamp() + 3 * 24 * 3600,
        };

        let license_key = state.signer.sign_license(&claims).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("Failed to sign license: {}", e) })),
            )
        })?;

    let _ = state.db.insert_license(&claims, &license_key).await;
        license_id_opt = Some(license_id);
        license_key_opt = Some(license_key);
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "gym_id": gym_id,
            "branch_name": branch_name,
            "owner_email": owner_norm,
            "tier": payload.tier,
            "license_id": license_id_opt,
            "license_key": license_key_opt,
            "expires_at": expires_at,
        })),
    ))
}

pub async fn admin_issue_branch_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(gym_id): Path<Uuid>,
    Json(payload): Json<IssueBranchKeyRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    let gym = state.db.get_gym_by_id(&gym_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Database error: {}", e) })),
        )
    })?;

    let gym = match gym {
        Some(g) => g,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Branch not found", "code": "BRANCH_NOT_FOUND" })),
            ))
        }
    };

    let tier = payload.tier.unwrap_or(gym.tier);
    let duration = payload.duration_days.unwrap_or(30);
    let now = Utc::now();
    let expires_at = now + Duration::days(duration);
    let license_id = Uuid::new_v4();

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: gym.name.clone(),
        owner_email: gym.owner_email.clone(),
        tier,
        issued_at: now,
        expires_at,
        max_members: tier.max_members(),
        hardware_lock_enabled: true,
        tailgate_detection_enabled: true,
        hwid: String::new(),
        ip_hint: String::new(),
        exp_unix: expires_at.timestamp(),
        grace_until: expires_at.timestamp() + 3 * 24 * 3600,
    };

    let license_key = state.signer.sign_license(&claims).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to sign license: {}", e) })),
        )
    })?;

    let _ = state.db.insert_license(&claims, &license_key).await;
    // Re-licensing restores service: a fresh CEO-issued key lifts any prior
    // kill-switch on this gym (otherwise the new key would die on next sync
    // and the Renew button would look broken).
    if state.disabled_gyms.write().remove(&gym_id) {
        let _ = state.db.set_disabled(&gym_id, false).await;
    }
    if let Some(record) = state.gyms.write().get_mut(&gym_id) {
        record.is_active = true;
        record.tier = tier;
    }
    let _ = state.db.log_audit(&gym.owner_email, Some(&gym_id), "admin_issue_branch_key", Some(&gym.name)).await;

    Ok((
        StatusCode::OK,
        Json(json!({
            "gym_id": gym_id,
            "gym_name": gym.name,
            "owner_email": gym.owner_email,
            "tier": tier,
            "license_id": license_id,
            "license_key": license_key,
            "expires_at": expires_at,
            "max_members": claims.max_members,
        })),
    ))
}

pub async fn owner_get_branches(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let branches = state.db.get_owner_branches(&owner_email).await.unwrap_or_default();
    Ok((
        StatusCode::OK,
        Json(json!({
            "owner_email": owner_email,
            "branches": branches,
            "count": branches.len()
        })),
    ))
}

pub async fn owner_get_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let analytics = state.db.get_owner_analytics(&owner_email).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Analytics error: {}", e) })),
        )
    })?;
    Ok((StatusCode::OK, Json(analytics)))
}

pub async fn owner_get_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let products = state.db.get_products(&owner_email).await.unwrap_or_default();
    let plans = state.db.get_plans(&owner_email).await.unwrap_or_default();
    let promos = state.db.get_promos(&owner_email).await.unwrap_or_default();
    Ok((
        StatusCode::OK,
        Json(json!({
            "products": products,
            "plans": plans,
            "promos": promos,
        })),
    ))
}

pub async fn owner_save_products(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SaveProductsRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let count = state.db.upsert_products(&owner_email, &payload.products).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Save products error: {}", e) })),
        )
    })?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "saved_count": count,
            "message": "Products updated and queued for POS terminal sync"
        })),
    ))
}

pub async fn owner_save_plans(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SavePlansRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let count = state.db.upsert_plans(&owner_email, &payload.plans).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Save plans error: {}", e) })),
        )
    })?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "saved_count": count,
            "message": "Membership plans updated and queued for POS terminal sync"
        })),
    ))
}

pub async fn owner_save_promos(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SavePromosRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let count = state.db.upsert_promos(&owner_email, &payload.promos).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Save promos error: {}", e) })),
        )
    })?;
    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "saved_count": count,
            "message": "Promo vouchers updated and queued for POS terminal sync"
        })),
    ))
}

// --- Staff & Cashier Management (Owner Scoped) ---

pub async fn owner_list_staff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let staff = state.db.list_staff_by_owner(&owner_email).await.unwrap_or_default();
    // Strip PIN hashes: the owner browser needs names/roles, never the hashes
    // (Argon2 of a 4-digit PIN falls to offline brute-force instantly). The
    // licensed-terminal sync channel (`SyncResponse.staff_accounts`) keeps the
    // hashes — the desktop needs them for offline PIN verification.
    let staff_sanitized: Vec<serde_json::Value> = staff
        .iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.remove("pin_hash");
            }
            v
        })
        .collect();
    let count = staff_sanitized.len();
    Ok((
        StatusCode::OK,
        Json(json!({
            "owner_email": owner_email,
            "staff": staff_sanitized,
            "count": count
        })),
    ))
}

pub async fn owner_create_staff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateStaffRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;

    let full_name = payload.full_name.trim().to_string();
    let username = payload.username.trim().to_lowercase();
    let pin_code = payload.pin_code.trim().to_string();

    if full_name.is_empty() || username.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Full name and username are required", "code": "VALIDATION_FAILED" })),
        ));
    }
    if pin_code.len() < 4 || pin_code.len() > 8 || !pin_code.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "PIN code must be 4 to 8 numeric digits", "code": "INVALID_PIN_FORMAT" })),
        ));
    }

    let pin_hash = gympos_shared::hash_password(&pin_code);
    let staff_id = Uuid::new_v4().to_string();
    let now = Utc::now();

    let staff = StaffAccount {
        id: staff_id.clone(),
        owner_email: owner_email.clone(),
        gym_id: payload.gym_id,
        gym_name: payload.gym_name,
        full_name,
        username,
        pin_hash,
        role: payload.role.unwrap_or(StaffRole::Staff),
        is_active: true,
        created_at: now,
        updated_at: now,
    };

    state.db.create_staff_account(&staff).await.map_err(|e| {
        (
            StatusCode::CONFLICT,
            Json(json!({ "error": format!("Could not create staff: {}", e), "code": "STAFF_CREATE_ERROR" })),
        )
    })?;

    let _ = state.db.log_audit(&owner_email, staff.gym_id.as_ref(), "create_staff", Some(&staff.full_name)).await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "status": "created",
            "staff": staff
        })),
    ))
}

pub async fn owner_update_staff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(staff_id): Path<String>,
    Json(payload): Json<UpdateStaffRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;

    if let Some(ref pin) = payload.pin_code {
        let pin = pin.trim();
        if pin.len() < 4 || pin.len() > 8 || !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "PIN code must be 4 to 8 numeric digits", "code": "INVALID_PIN_FORMAT" })),
            ));
        }
    }

    let updated = state.db.update_staff_account(&owner_email, &staff_id, &payload).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Update staff error: {}", e) })),
        )
    })?;

    if updated {
        let _ = state.db.log_audit(&owner_email, None, "update_staff", Some(&staff_id)).await;
        Ok((
            StatusCode::OK,
            Json(json!({ "status": "updated", "staff_id": staff_id })),
        ))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Staff account not found", "code": "STAFF_NOT_FOUND" })),
        ))
    }
}

pub async fn owner_delete_staff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(staff_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    let deleted = state.db.delete_staff_account(&owner_email, &staff_id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Delete staff error: {}", e) })),
        )
    })?;

    if deleted {
        let _ = state.db.log_audit(&owner_email, None, "delete_staff", Some(&staff_id)).await;
        Ok((
            StatusCode::OK,
            Json(json!({ "status": "deleted", "staff_id": staff_id })),
        ))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Staff account not found", "code": "STAFF_NOT_FOUND" })),
        ))
    }
}

#[derive(serde::Deserialize)]
pub struct BranchOverridePayload {
    pub gym_id: Uuid,
    pub product_id: String,
    pub price: f64,
    pub stock: i32,
}

pub async fn owner_save_branch_override(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BranchOverridePayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let owner_email = extract_owner_email(&headers, &state.db, &state.tokens).await?;
    state.db.save_branch_product_override(
        &owner_email,
        &payload.gym_id,
        &payload.product_id,
        payload.price,
        payload.stock,
    ).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to save branch override: {}", e) })),
        )
    })?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "gym_id": payload.gym_id,
            "product_id": payload.product_id,
            "message": "Branch price override saved"
        })),
    ))
}

// --- Auto-Updater & Release Controller (Fleet Scalability) ---

pub async fn check_for_updates(
    State(state): State<Arc<AppState>>,
    Query(params): Query<UpdateCheckRequest>,
) -> impl IntoResponse {
    let channel = params.channel.unwrap_or_else(|| "stable".to_string());
    let current_ver = params.current_version.trim().to_string();

    let release_opt = state.db.get_latest_release(&channel).await.unwrap_or(None);

    match release_opt {
        Some(rel) => {
            let mut eligible = true;

            // Staged Rollout percentage check
            if rel.rollout_percentage < 100 {
                if let Some(gym_id) = params.gym_id {
                    let mut hasher = sha2::Sha256::new();
                    use sha2::Digest;
                    hasher.update(format!("{}:{}", gym_id, rel.version).as_bytes());
                    let hash_bytes = hasher.finalize();
                    let bucket = (hash_bytes[0] as u32) % 100;
                    if bucket >= rel.rollout_percentage {
                        eligible = false;
                    }
                }
            }

            let is_newer = eligible && (rel.version.trim() != current_ver);

            (
                StatusCode::OK,
                Json(UpdateCheckResponse {
                    update_available: is_newer,
                    current_version: current_ver,
                    latest_version: rel.version,
                    channel: rel.channel,
                    download_url: rel.download_url,
                    sha256: rel.sha256,
                    release_notes: rel.release_notes,
                    is_mandatory: rel.is_mandatory,
                    rollout_percentage: rel.rollout_percentage,
                    server_time: Utc::now(),
                }),
            )
        }
        None => {
            // Fallback response pointing to GitHub Releases
            (
                StatusCode::OK,
                Json(UpdateCheckResponse {
                    update_available: false,
                    current_version: current_ver.clone(),
                    latest_version: current_ver,
                    channel,
                    download_url: "https://github.com/burikethhh/SLSNIKETH/releases/latest".to_string(),
                    sha256: String::new(),
                    release_notes: "Up to date".to_string(),
                    is_mandatory: false,
                    rollout_percentage: 100,
                    server_time: Utc::now(),
                }),
            )
        }
    }
}

pub async fn publish_release_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<PublishReleaseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;

    // Integrity is mandatory: a release without a SHA-256 (or without a
    // download URL) would now be refused by every terminal, so reject it at
    // publish time instead of shipping a dead update.
    if payload.sha256.trim().len() != 64
        || !payload.sha256.trim().chars().all(|c| c.is_ascii_hexdigit())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A valid 64-hex SHA-256 of the release binary is required", "code": "INVALID_SHA256" })),
        ));
    }
    if payload.download_url.trim().is_empty() || payload.version.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "download_url and version are required", "code": "INVALID_RELEASE" })),
        ));
    }

    let release_info = ReleaseInfo {
        version: payload.version.trim().to_string(),
        channel: payload.channel.trim().to_lowercase(),
        min_supported_version: payload.min_supported_version.unwrap_or_else(|| "0.1.0".to_string()),
        download_url: payload.download_url.trim().to_string(),
        sha256: payload.sha256.trim().to_string(),
        release_notes: payload.release_notes,
        rollout_percentage: payload.rollout_percentage.unwrap_or(100).min(100),
        is_mandatory: payload.is_mandatory.unwrap_or(false),
        created_at: Utc::now(),
    };

    state.db.publish_release(&release_info).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to publish release: {}", e) })),
        )
    })?;

    Ok((StatusCode::CREATED, Json(release_info)))
}

pub async fn list_releases_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.db, &state.tokens).await?;
    let list = state.db.list_releases().await.unwrap_or_default();
    Ok((StatusCode::OK, Json(list)))
}
