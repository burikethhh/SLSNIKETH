use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- License & Tier Domain Models ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LicenseTier {
    Basic, // Max 200 members, Single location
    Pro,   // Max 500 members, Multi-location sync
    Ultra, // Max 1000 members, Full API & priority support
}

impl LicenseTier {
    pub fn max_members(&self) -> u32 {
        match self {
            LicenseTier::Basic => 200,
            LicenseTier::Pro => 500,
            LicenseTier::Ultra => 1000,
        }
    }

    pub fn price_usd_per_month(&self) -> f64 {
        match self {
            LicenseTier::Basic => 99.0,
            LicenseTier::Pro => 199.0,
            LicenseTier::Ultra => 349.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseClaims {
    pub license_id: Uuid,
    pub gym_id: Uuid,
    pub gym_name: String,
    pub tier: LicenseTier,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub max_members: u32,
    pub hardware_lock_enabled: bool,
    pub tailgate_detection_enabled: bool,
}

impl LicenseClaims {
    pub fn evaluate(&self, now: DateTime<Utc>) -> LicenseStatus {
        if now <= self.expires_at {
            let days_remaining = (self.expires_at - now).num_days();
            LicenseStatus::Valid {
                tier: format!("{:?}", self.tier).to_lowercase(),
                gym_name: self.gym_name.clone(),
                days_remaining,
            }
        } else {
            let grace_period = Duration::days(3);
            let grace_expiry = self.expires_at + grace_period;

            if now <= grace_expiry {
                let grace_days_remaining = (grace_expiry - now).num_days();
                LicenseStatus::GracePeriod {
                    tier: format!("{:?}", self.tier).to_lowercase(),
                    grace_days_remaining,
                    expired_at: self.expires_at,
                }
            } else {
                LicenseStatus::Expired {
                    expired_at: self.expires_at,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum LicenseStatus {
    Valid {
        tier: String,
        gym_name: String,
        days_remaining: i64,
    },
    GracePeriod {
        tier: String,
        grace_days_remaining: i64,
        expired_at: DateTime<Utc>,
    },
    Expired {
        expired_at: DateTime<Utc>,
    },
    Invalid {
        reason: String,
    },
    Unlicensed,
}

impl LicenseStatus {
    pub fn is_operable(&self) -> bool {
        matches!(self, LicenseStatus::Valid { .. } | LicenseStatus::GracePeriod { .. })
    }
}

// --- Member Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub membership_type: String,
    pub status: String,
    pub face_vectors: Vec<Vec<f32>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMemberRequest {
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub membership_type: String,
    pub face_vectors: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemberRequest {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub membership_type: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProductRequest {
    pub name: String,
    pub price: f64,
    pub stock: i32,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProductRequest {
    pub id: String,
    pub name: String,
    pub price: f64,
    pub stock: i32,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCoachRequest {
    pub name: String,
    pub specialty: String,
    pub phone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCoachRequest {
    pub id: String,
    pub name: String,
    pub specialty: String,
    pub phone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateGymRequest {
    pub id: Uuid,
    pub name: String,
    pub tier: LicenseTier,
    pub max_members: u32,
    pub hardware_lock_enabled: bool,
    pub contact_email: String,
}

// --- Walk-In / Day Pass Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkInRecord {
    pub id: String,
    pub guest_name: String,
    pub phone: String,
    pub amount_paid: f64,
    pub payment_method: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWalkInRequest {
    pub guest_name: String,
    pub phone: String,
    pub amount_paid: f64,
    pub payment_method: String,
    pub face_vector: Option<Vec<f32>>,
}

// --- White-Label App Settings ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub camera1_entry_device_id: String,
    pub camera2_exit_device_id: String,
    pub camera3_tailgate_device_id: String,
    pub roi_x: f32,
    pub roi_y: f32,
    pub roi_width: f32,
    pub roi_height: f32,
    pub roi_sensitivity: f32,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            camera1_entry_device_id: "".to_string(),
            camera2_exit_device_id: "".to_string(),
            camera3_tailgate_device_id: "".to_string(),
            roi_x: 20.0,
            roi_y: 20.0,
            roi_width: 60.0,
            roi_height: 60.0,
            roi_sensitivity: 85.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub gym_name: String,
    pub logo_data_url: Option<String>,
    pub theme_color: String,
    pub walk_in_rate: f64,
    #[serde(default)]
    pub camera_config: Option<CameraConfig>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            gym_name: "Titan Fitness & Performance".to_string(),
            logo_data_url: None,
            theme_color: "#2563eb".to_string(),
            walk_in_rate: 10.0,
            camera_config: Some(CameraConfig::default()),
        }
    }
}

// --- Attendance & Tailgating Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttendanceRecord {
    pub id: String,
    pub member_id: Option<String>,
    pub member_name: Option<String>,
    pub direction: String,
    pub confidence: Option<f32>,
    pub tailgate_flag: bool,
    pub timestamp: DateTime<Utc>,
    pub sync_status: String,
}

// --- POS & Store Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductItem {
    pub id: String,
    pub name: String,
    pub price: f64,
    pub stock: i32,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartItem {
    pub product_id: String,
    pub product_name: String,
    pub unit_price: f64,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleTransaction {
    pub id: String,
    pub member_id: Option<String>,
    pub total_amount: f64,
    pub payment_method: String,
    pub items: Vec<CartItem>,
    pub timestamp: DateTime<Utc>,
}

// --- Coaches / Personal Trainers ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coach {
    pub id: String,
    pub name: String,
    pub specialty: String,
    pub phone: String,
    pub active_students: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoachSession {
    pub id: String,
    pub coach_id: String,
    pub coach_name: String,
    pub member_id: String,
    pub member_name: String,
    pub scheduled_at: String,
    pub duration_minutes: u32,
}

// --- Cloud Sync Payloads & Responses ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceVectorSyncItem {
    pub member_id: String,
    pub full_name: String,
    pub vectors: Vec<Vec<f32>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPushPayload {
    pub gym_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub attendance_logs: Vec<AttendanceRecord>,
    pub face_vectors: Vec<FaceVectorSyncItem>,
    pub sales: Vec<SaleTransaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub processed_attendance: usize,
    pub processed_vectors: usize,
    pub remote_disabled: bool,
    pub server_time: DateTime<Utc>,
}
