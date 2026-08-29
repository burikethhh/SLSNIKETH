use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use gympos_shared::{FaceVectorSyncItem, LicenseClaims, SyncPushPayload, SyncResponse};
use parking_lot::RwLock;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::crypto::{verify_license_token, LicenseSigner};
use crate::db::CloudDatabase;
use crate::models::{
    AdminLoginRequest, GenerateLicenseRequest, GymRecord, LicenseResponse, RegisterGymRequest,
    RemoteDisableRequest, RevokeLicenseRequest,
};

#[derive(Clone)]
pub struct AppState {
    pub signer: LicenseSigner,
    pub db: Arc<CloudDatabase>,
    pub gyms: Arc<RwLock<HashMap<Uuid, GymRecord>>>,
    pub disabled_gyms: Arc<RwLock<HashSet<Uuid>>>,
    pub revoked_licenses: Arc<RwLock<HashSet<Uuid>>>,
    pub admin_key: String,
}

fn verify_admin_auth(headers: &HeaderMap, admin_key: &str) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
        .or_else(|| headers.get("x-admin-key").and_then(|v| v.to_str().ok()));

    match auth_header {
        Some(token) if token.trim() == admin_key => Ok(()),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Unauthorized: Master Admin API key required",
                "code": "ADMIN_AUTH_REQUIRED"
            })),
        )),
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

pub async fn admin_login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AdminLoginRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    if payload.admin_key.trim() == state.admin_key {
        Ok((
            StatusCode::OK,
            Json(json!({
                "authenticated": true,
                "token": state.admin_key,
                "message": "CEO Admin session verified"
            })),
        ))
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid Master Admin Key", "authenticated": false })),
        ))
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
    verify_admin_auth(&headers, &state.admin_key)?;

    let gym_id = Uuid::new_v4();
    let license_id = Uuid::new_v4();
    let now = Utc::now();
    let duration = payload.duration_days.unwrap_or(30);
    let expires_at = now + Duration::days(duration);

    let gym_record = GymRecord {
        id: gym_id,
        name: payload.name.clone(),
        owner_email: payload.owner_email.clone(),
        tier: payload.tier,
        is_active: true,
        created_at: now,
    };

    let _ = state.db.upsert_gym(&gym_record);
    state.gyms.write().insert(gym_id, gym_record);

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.name.clone(),
        owner_email: payload.owner_email.clone(),
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
    let _ = state.db.insert_license(&claims, &license_key);

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
    verify_admin_auth(&headers, &state.admin_key)?;
    let gyms = state.gyms.read();
    let list: Vec<GymRecord> = gyms.values().cloned().collect();
    Ok((StatusCode::OK, Json(json!(list))))
}

pub async fn update_gym(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<gympos_shared::UpdateGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.admin_key)?;
    let mut gyms = state.gyms.write();
    if let Some(gym) = gyms.get_mut(&payload.id) {
        gym.name = payload.name.clone();
        gym.owner_email = payload.contact_email.clone();
        gym.tier = payload.tier;
        let _ = state.db.update_gym(&payload);
        Ok((StatusCode::OK, Json(json!(gym))))
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
    verify_admin_auth(&headers, &state.admin_key)?;
    let mut gyms = state.gyms.write();
    let mut disabled = state.disabled_gyms.write();
    if gyms.remove(&gym_id).is_some() {
        disabled.remove(&gym_id);
        let _ = state.db.delete_gym(&gym_id);
        Ok((StatusCode::OK, Json(json!({ "status": "deleted", "gym_id": gym_id }))))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Gym not found" })),
        ))
    }
}

// --- License Issuance & Management (Admin Protected) ---

pub async fn generate_license(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GenerateLicenseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let now = Utc::now();
    let expires_at = now + Duration::days(payload.duration_days.max(1));
    let gym_id = Uuid::new_v4();
    let license_id = Uuid::new_v4();

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.gym_name.clone(),
        owner_email: payload.owner_email.clone(),
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
    let _ = state.db.insert_license(&claims, &license_key);

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
    verify_admin_auth(&headers, &state.admin_key)?;
    let list = state.db.list_licenses().unwrap_or_default();
    Ok((StatusCode::OK, Json(json!(list))))
}

pub async fn revoke_license_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RevokeLicenseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.admin_key)?;
    let reason = payload.reason.as_deref().unwrap_or("Revoked by CEO / Platform Administrator");

    let _ = state.db.revoke_license(&payload.license_id, reason);
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
    Json(payload): Json<serde_json::Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
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
        || state.db.is_license_revoked(&claims.license_id).unwrap_or(false);

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
    headers: HeaderMap,
    Json(payload): Json<SyncPushPayload>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
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

    let mut is_disabled = state.disabled_gyms.read().contains(&payload.gym_id);
    // Check revocation/expiry via verified claims (trusted, not payload)
    let is_revoked = state.revoked_licenses.read().contains(&claims.license_id)
        || state.db.is_license_revoked(&claims.license_id).unwrap_or(false);
    let is_expired = Utc::now() > (claims.expires_at + Duration::days(3));
    if is_revoked || is_expired {
        is_disabled = true;
    }

    let trusted_owner = claims.owner_email.clone();
    // 2. Ingest newly added/updated members and face vectors from this branch (using trusted owner)
    let processed_members = state.db.upsert_cloud_members(&trusted_owner, &payload.members).unwrap_or(0);

    // 3. Ingest attendance records from this branch (using trusted owner)
    let processed_att = state.db.insert_attendance_logs(&trusted_owner, &payload.attendance_logs, &payload.gym_id).unwrap_or(0);
    let processed_vec = payload.face_vectors.len();

    // 4. Query all inter-branch members from sister gyms under the same owner (trusted)
    let sister_branch_members = state.db.get_sister_branch_members(&trusted_owner, &payload.gym_id).unwrap_or_default();

    Ok((
        StatusCode::OK,
        Json(SyncResponse {
            processed_attendance: processed_att,
            processed_members,
            processed_vectors: processed_vec,
            remote_disabled: is_disabled,
            sister_branch_members,
            server_time: Utc::now(),
        }),
    ))
}

pub async fn sync_vectors(
    State(_state): State<Arc<AppState>>,
    Json(vectors): Json<Vec<FaceVectorSyncItem>>,
) -> impl IntoResponse {
    let count = vectors.len();
    (
        StatusCode::OK,
        Json(json!({
            "status": "success",
            "synced_vectors": count,
            "timestamp": Utc::now()
        })),
    )
}

pub async fn remote_disable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RemoteDisableRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let mut disabled = state.disabled_gyms.write();
    if payload.disable {
        disabled.insert(payload.gym_id);
    } else {
        disabled.remove(&payload.gym_id);
    }
    let _ = state.db.set_disabled(&payload.gym_id, payload.disable);

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
    verify_admin_auth(&headers, &state.admin_key)?;

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
    let total_cloud_members: usize = state.db.count_cloud_members().unwrap_or(0);
    let total_attendance: usize = state.db.count_attendance().unwrap_or(0);
    let breach_flags: usize = state.db.count_tailgate_breaches().unwrap_or(0);

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
            "server_time": Utc::now(),
        })),
    ))
}

