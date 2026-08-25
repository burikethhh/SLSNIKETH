use chrono::{DateTime, Utc};
use gympos_shared::LicenseTier;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GymRecord {
    pub id: Uuid,
    pub name: String,
    pub owner_email: String,
    pub tier: LicenseTier,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterGymRequest {
    pub name: String,
    pub owner_email: String,
    pub tier: LicenseTier,
    pub duration_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerateLicenseRequest {
    pub gym_name: String,
    pub owner_email: String,
    pub tier: LicenseTier,
    pub duration_days: i64,
    pub enable_tailgate: Option<bool>,
    pub enable_lock: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseResponse {
    pub license_key: String,
    pub license_id: Uuid,
    pub gym_id: Uuid,
    pub gym_name: String,
    pub tier: LicenseTier,
    pub expires_at: DateTime<Utc>,
    pub max_members: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteDisableRequest {
    pub gym_id: Uuid,
    pub disable: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudLicenseRecord {
    pub license_id: Uuid,
    pub raw_token: String,
    pub gym_id: Uuid,
    pub gym_name: String,
    pub owner_email: String,
    pub tier: LicenseTier,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub max_members: u32,
    pub hardware_lock_enabled: bool,
    pub tailgate_detection_enabled: bool,
    pub is_revoked: bool,
    pub revoked_reason: Option<String>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminLoginRequest {
    pub admin_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeLicenseRequest {
    pub license_id: Uuid,
    pub reason: Option<String>,
}
