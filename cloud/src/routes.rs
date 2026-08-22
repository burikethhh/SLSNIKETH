use axum::{
    extract::State,
    http::StatusCode,
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
use crate::models::{GenerateLicenseRequest, GymRecord, LicenseResponse, RegisterGymRequest, RemoteDisableRequest};

#[derive(Clone)]
pub struct AppState {
    pub signer: LicenseSigner,
    pub gyms: Arc<RwLock<HashMap<Uuid, GymRecord>>>,
    pub disabled_gyms: Arc<RwLock<HashSet<Uuid>>>,
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

pub async fn get_public_key(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "public_key_pem": state.signer.public_key_pem()
        })),
    )
}

// --- Gym Registration & Management ---

pub async fn register_gym(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
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

    state.gyms.write().insert(gym_id, gym_record);

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.name.clone(),
        tier: payload.tier,
        issued_at: now,
        expires_at,
        max_members: payload.tier.max_members(),
        hardware_lock_enabled: true,
        tailgate_detection_enabled: true,
    };

    let license_key = state.signer.sign_license(&claims).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to sign license: {}", e) })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "gym_id": gym_id,
            "gym_name": payload.name,
            "tier": payload.tier,
            "owner_email": payload.owner_email,
            "license_key": license_key,
            "expires_at": expires_at,
            "max_members": claims.max_members,
        })),
    ))
}

pub async fn list_gyms(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let gyms = state.gyms.read();
    let list: Vec<GymRecord> = gyms.values().cloned().collect();
    (StatusCode::OK, Json(json!(list)))
}

pub async fn update_gym(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<gympos_shared::UpdateGymRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let mut gyms = state.gyms.write();
    if let Some(gym) = gyms.get_mut(&payload.id) {
        gym.name = payload.name.clone();
        gym.owner_email = payload.contact_email.clone();
        gym.tier = payload.tier;
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
    axum::extract::Path(gym_id): axum::extract::Path<Uuid>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let mut gyms = state.gyms.write();
    let mut disabled = state.disabled_gyms.write();
    if gyms.remove(&gym_id).is_some() {
        disabled.remove(&gym_id);
        Ok((StatusCode::OK, Json(json!({ "status": "deleted", "gym_id": gym_id }))))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Gym not found" })),
        ))
    }
}

pub async fn generate_license(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateLicenseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let now = Utc::now();
    let expires_at = now + Duration::days(payload.duration_days.max(1));
    let gym_id = Uuid::new_v4();
    let license_id = Uuid::new_v4();

    let claims = LicenseClaims {
        license_id,
        gym_id,
        gym_name: payload.gym_name.clone(),
        tier: payload.tier,
        issued_at: now,
        expires_at,
        max_members: payload.tier.max_members(),
        hardware_lock_enabled: payload.enable_lock.unwrap_or(true),
        tailgate_detection_enabled: payload.enable_tailgate.unwrap_or(true),
    };

    let license_key = state.signer.sign_license(&claims).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("Failed to sign license: {}", e) })),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(LicenseResponse {
            license_key,
            gym_id,
            gym_name: payload.gym_name,
            tier: payload.tier,
            expires_at,
            max_members: claims.max_members,
        }),
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

    let is_disabled = state.disabled_gyms.read().contains(&claims.gym_id);
    let status = if is_disabled {
        gympos_shared::LicenseStatus::Invalid {
            reason: "License remotely disabled by platform administrator".to_string(),
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
    Json(payload): Json<SyncPushPayload>,
) -> impl IntoResponse {
    let is_disabled = state.disabled_gyms.read().contains(&payload.gym_id);

    let processed_att = payload.attendance_logs.len();
    let processed_vec = payload.face_vectors.len();

    (
        StatusCode::OK,
        Json(SyncResponse {
            processed_attendance: processed_att,
            processed_vectors: processed_vec,
            remote_disabled: is_disabled,
            server_time: Utc::now(),
        }),
    )
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
    Json(payload): Json<RemoteDisableRequest>,
) -> impl IntoResponse {
    let mut disabled = state.disabled_gyms.write();
    if payload.disable {
        disabled.insert(payload.gym_id);
    } else {
        disabled.remove(&payload.gym_id);
    }

    (
        StatusCode::OK,
        Json(json!({
            "gym_id": payload.gym_id,
            "disabled": payload.disable,
            "reason": payload.reason,
            "updated_at": Utc::now()
        })),
    )
}
