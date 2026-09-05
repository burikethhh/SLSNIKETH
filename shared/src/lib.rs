use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- Password / PIN Hashing (shared by cloud + desktop so hashes produced by
// one side always verify on the other, e.g. staff PINs synced cloud -> desktop) ---

/// Hash a password or numeric PIN with Argon2id, using a fresh random salt.
/// Returns a self-describing string (`$argon2id$v=19$m=...$<salt>$<hash>`) that
/// can be stored directly and later checked with [`verify_password`].
///
/// Replaces the previous unsalted SHA-256 scheme, which was vulnerable to
/// precomputed rainbow-table attacks (especially for 4-6 digit numeric PINs,
/// which only have 10,000-1,000,000 possible values).
pub fn hash_password(password: &str) -> String {
    use argon2::password_hash::{PasswordHasher, SaltString};
    use argon2::Argon2;

    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("Argon2 hashing failed")
        .to_string()
}

/// Verify a plaintext password/PIN against a stored hash produced by
/// [`hash_password`]. Also transparently accepts legacy unsalted SHA-256 hex
/// digests (64 lowercase hex chars) so accounts created before the Argon2
/// migration keep working until their password/PIN is next changed.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    use argon2::Argon2;

    if let Ok(parsed) = PasswordHash::new(stored_hash) {
        return Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
    }

    // Legacy fallback: pre-migration accounts stored `sha256(password)` hex.
    if stored_hash.len() == 64 && stored_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let legacy = format!("{:x}", hasher.finalize());
        return legacy == stored_hash.to_lowercase();
    }

    false
}

/// True when `stored_hash` is a legacy unsalted SHA-256 digest rather than an
/// Argon2id PHC string. Callers use this right after a successful
/// [`verify_password`] to transparently re-hash the credential with Argon2id
/// (a legacy 4-digit PIN hash falls to offline brute-force instantly, so the
/// upgrade must happen on the next successful login, not "whenever").
pub fn password_is_legacy(stored_hash: &str) -> bool {
    !stored_hash.starts_with("$argon2")
}

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
    pub owner_email: String,
    pub tier: LicenseTier,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub max_members: u32,
    pub hardware_lock_enabled: bool,
    pub tailgate_detection_enabled: bool,

    // ---- Offline HWID binding + heartbeat + tamper-resistance (parity with SLS123 validator.py) ----
    /// Device fingerprint (SHA256 of MAC+MachineGuid+disk serial). Empty = hardware lock not yet bound.
    #[serde(default)]
    pub hwid: String,
    /// Last known public IP hint, recorded at activation for anomaly detection. Empty = not provided.
    #[serde(default)]
    pub ip_hint: String,
    /// Unix epoch seconds at which the license expires (mirror of expires_at, for offline math).
    #[serde(default)]
    pub exp_unix: i64,
    /// Unix epoch seconds at which the 3-day post-expiry grace window ends.
    #[serde(default)]
    pub grace_until: i64,
}

impl LicenseClaims {
    /// Unix-seconds helper for offline expiry maths; falls back to `expires_at` when `exp_unix` is unset.
    pub fn expiry_unix(&self) -> i64 {
        if self.exp_unix > 0 {
            self.exp_unix
        } else {
            self.expires_at.timestamp()
        }
    }

    /// Unix-seconds helper for grace deadline; falls back to expiry + 3 days.
    pub fn grace_unix(&self) -> i64 {
        if self.grace_until > 0 {
            self.grace_until
        } else {
            self.expiry_unix() + 3 * 24 * 3600
        }
    }
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
    #[serde(default)]
    pub photo_data_url: Option<String>,
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
    #[serde(default)]
    pub photo_data_url: Option<String>,
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
    #[serde(default)]
    pub photo_data_url: Option<String>,
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
    /// Stored enrollment vector (None for code-only passes). Enables
    /// renew-without-rescan: extend re-upserts this into the live store.
    #[serde(default)]
    pub face_vector: Option<Vec<f32>>,
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
    /// Phase E tunables (Hardware Settings → Recognition Tuning). All
    /// `#[serde(default)]` so old saved configs keep working.
    #[serde(default = "default_match_threshold")]
    pub match_threshold: f32,
    #[serde(default = "default_adapt_threshold")]
    pub adapt_threshold: f32,
    #[serde(default = "default_liveness_min_px")]
    pub liveness_min_px: f32,
    #[serde(default = "default_mog_sensitivity")]
    pub mog_sensitivity: f32,
}

fn default_match_threshold() -> f32 {
    0.62
}
fn default_adapt_threshold() -> f32 {
    0.80
}
fn default_liveness_min_px() -> f32 {
    0.5
}
fn default_mog_sensitivity() -> f32 {
    0.5
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
            match_threshold: default_match_threshold(),
            adapt_threshold: default_adapt_threshold(),
            liveness_min_px: default_liveness_min_px(),
            mog_sensitivity: default_mog_sensitivity(),
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
    /// Tailgate attribution: whose admitted entry window was piggybacked.
    /// `None` for non-tailgate rows and legacy rows written before Phase A.
    #[serde(default)]
    pub linked_member_id: Option<String>,
    /// YOLO person count observed in the ROI when the incident fired.
    #[serde(default)]
    pub person_count: Option<i32>,
}

/// Per-branch tailgate policy, synced cloud → exe inside `SyncResponse`.
/// `None` on the wire means "no remote policy yet — keep local behavior".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailgatePolicy {
    pub enabled: bool,
    pub siren_cooldown_secs: u64,
}

impl Default for TailgatePolicy {
    fn default() -> Self {
        Self { enabled: true, siren_cooldown_secs: 300 }
    }
}

/// A tailgate incident as served by the CEO / owner incident feeds.
/// Shared so both dashboards and the exe resolve-view agree on field names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailgateIncident {
    pub id: String,
    pub gym_id: String,
    pub gym_name: String,
    pub owner_email: String,
    pub member_name: Option<String>,
    pub linked_member_id: Option<String>,
    pub person_count: Option<i32>,
    pub timestamp: DateTime<Utc>,
    pub acknowledged: bool,
    pub acknowledged_by: Option<String>,
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
    #[serde(default)]
    pub discount_type: String,
    #[serde(default)]
    pub discount_amount: f64,
}

// --- Expenses (local bookkeeping, surfaced in End-of-Day) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpenseRecord {
    pub id: String,
    pub title: String,
    pub category: String,
    pub amount: f64,
    pub payment_method: String,
    pub notes: String,
    pub spent_at: DateTime<Utc>,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateExpenseRequest {
    pub title: String,
    pub category: String,
    pub amount: f64,
    pub payment_method: String,
    pub notes: String,
    pub spent_at: Option<DateTime<Utc>>,
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

// --- Remote Catalog, Membership Plans, and Promo Vouchers (Cloud -> POS Sync) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCatalogProduct {
    pub id: String,
    pub name: String,
    pub price: f64,
    pub stock: i32,
    pub category: String,
    #[serde(default)]
    pub target_gym_id: Option<Uuid>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipPlanConfig {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default = "default_monthly_period")]
    pub billing_period: String,
    pub price_monthly: f64,
    pub student_discount_pct: f64,
    #[serde(default)]
    pub target_gym_id: Option<Uuid>,
    #[serde(default)]
    pub benefits: Vec<String>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

fn default_monthly_period() -> String {
    "monthly".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoVoucherConfig {
    pub code: String,
    #[serde(default)]
    pub label: String,
    pub discount_type: String, // "percent" or "fixed"
    pub discount_value: f64,
    pub min_spend: f64,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

// --- CEO Account Authentication (replaces the shared master admin key) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeoRegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// Required only when the server sets CEO_BOOTSTRAP_SECRET (first-CEO lockdown).
    #[serde(default)]
    pub setup_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeoLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeoLoginResponse {
    pub authenticated: bool,
    pub token: String,
    pub ceo_email: String,
    pub display_name: String,
}

// --- Owner Portal DTOs & Analytics ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerRegisterRequest {
    pub email: String,
    pub password: String,
    pub company_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerLoginResponse {
    pub authenticated: bool,
    pub token: String,
    pub owner_email: String,
    pub company_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerBranchSummary {
    pub gym_id: Uuid,
    pub name: String,
    pub tier: LicenseTier,
    pub active_members: u32,
    pub today_checkins: u32,
    pub today_sales: f64,
    pub hwid: String,
    pub license_key: String,
    pub expires_at: DateTime<Utc>,
    pub is_heartbeat_healthy: bool,
    pub is_active: bool,
    #[serde(default)]
    pub is_disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerDashboardAnalytics {
    pub owner_email: String,
    pub company_name: String,
    pub total_branches: usize,
    pub total_active_members: u32,
    pub today_total_revenue: f64,
    pub month_total_revenue: f64,
    pub today_checkins: u32,
    pub branches: Vec<OwnerBranchSummary>,
    pub recent_transactions: Vec<SaleTransaction>,
    pub revenue_by_branch: std::collections::HashMap<String, f64>,
    pub revenue_by_category: std::collections::HashMap<String, f64>,
    pub hourly_traffic: Vec<u32>, // 24 hours (0..23)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveProductsRequest {
    pub products: Vec<RemoteCatalogProduct>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePlansRequest {
    pub plans: Vec<MembershipPlanConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavePromosRequest {
    pub promos: Vec<PromoVoucherConfig>,
}

// --- Cloud Sync Payloads & Responses ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudMemberSyncItem {
    pub id: String,
    pub home_gym_id: Uuid,
    pub home_gym_name: String,
    pub owner_email: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub membership_type: String,
    pub status: String,
    pub face_vectors: Vec<Vec<f32>>,
    #[serde(default)]
    pub photo_data_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

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
    pub gym_name: String,
    pub owner_email: String,
    pub timestamp: DateTime<Utc>,
    pub attendance_logs: Vec<AttendanceRecord>,
    pub members: Vec<CloudMemberSyncItem>,
    pub face_vectors: Vec<FaceVectorSyncItem>,
    pub sales: Vec<SaleTransaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub processed_attendance: usize,
    pub processed_members: usize,
    pub processed_vectors: usize,
    pub processed_sales: usize,
    pub remote_disabled: bool,
    pub sister_branch_members: Vec<CloudMemberSyncItem>,
    pub remote_catalog: Option<Vec<RemoteCatalogProduct>>,
    pub remote_plans: Option<Vec<MembershipPlanConfig>>,
    pub remote_promos: Option<Vec<PromoVoucherConfig>>,
    pub staff_accounts: Option<Vec<StaffAccount>>,
    /// Remote tailgate policy for the syncing branch (Phase A-D). `None`
    /// when the cloud has no explicit policy row yet — the exe keeps local
    /// behavior. Old exes ignore the field via `#[serde(default)]`.
    #[serde(default)]
    pub tailgate_policy: Option<TailgatePolicy>,
    pub server_time: DateTime<Utc>,
}

// --- Auto-Updater & Release Scalability Domain Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub channel: String, // "stable", "beta", "nightly"
    pub min_supported_version: String,
    pub download_url: String,
    pub sha256: String,
    pub release_notes: String,
    pub rollout_percentage: u32, // 0..100 for staged rollout
    pub is_mandatory: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckRequest {
    pub current_version: String,
    pub gym_id: Option<Uuid>,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResponse {
    pub update_available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub channel: String,
    pub download_url: String,
    pub sha256: String,
    pub release_notes: String,
    pub is_mandatory: bool,
    pub rollout_percentage: u32,
    pub server_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishReleaseRequest {
    pub version: String,
    pub channel: String,
    pub min_supported_version: Option<String>,
    pub download_url: String,
    pub sha256: String,
    pub release_notes: String,
    pub rollout_percentage: Option<u32>,
    pub is_mandatory: Option<bool>,
}

// --- Role-Based Access Control (RBAC) & Staff Accounts ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StaffRole {
    Staff,    // Front-desk cashier, gate kiosk, walk-in check-in
    Manager,  // Branch manager, inventory restock, shifts
    Owner,    // Franchise gym owner (master administrative privileges)
}

impl Default for StaffRole {
    fn default() -> Self {
        StaffRole::Staff
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffAccount {
    pub id: String,
    pub owner_email: String,
    pub gym_id: Option<Uuid>, // None means roaming across all owner branches
    pub gym_name: Option<String>,
    pub full_name: String,
    pub username: String,
    // NOTE: `pin_hash` serializes with the struct because the license-
    // authenticated `/sync/push` channel needs it (desktop ingests hashes for
    // offline PIN verify). Browser-facing endpoints must strip it instead:
    // desktop `list_terminal_staff` and cloud `owner_list_staff` both return
    // sanitized records. Argon2 hashes of 4-digit PINs fall to offline
    // brute-force instantly, so the webview/portal must never receive them.
    #[serde(default)]
    pub pin_hash: String,
    pub role: StaffRole,
    pub is_active: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateStaffRequest {
    pub full_name: String,
    pub username: String,
    pub pin_code: String, // 4-6 digit numeric PIN
    pub role: Option<StaffRole>,
    pub gym_id: Option<Uuid>,
    pub gym_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStaffRequest {
    pub full_name: Option<String>,
    pub pin_code: Option<String>,
    pub role: Option<StaffRole>,
    pub gym_id: Option<Uuid>,
    pub gym_name: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffLoginRequest {
    pub pin_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffLoginResponse {
    pub authenticated: bool,
    pub staff_id: String,
    pub full_name: String,
    pub username: String,
    pub role: StaffRole,
    pub gym_id: Option<Uuid>,
    pub gym_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSession {
    pub is_authenticated: bool,
    pub user_id: String,
    pub display_name: String,
    pub role: StaffRole,
    pub gym_id: Option<Uuid>,
    pub gym_name: Option<String>,
    pub logged_in_at: DateTime<Utc>,
    /// Last time the session was used for a gated action. Desktop RBAC treats
    /// a session idle longer than `SESSION_IDLE_TIMEOUT_SECS` as logged out
    /// (kiosk auto-lock). Defaults to `logged_in_at` for sessions persisted
    /// before this field existed.
    #[serde(default = "Utc::now")]
    pub last_activity_at: DateTime<Utc>,
}

/// A terminal session idle longer than this is treated as logged out by the
/// desktop RBAC gate — an unattended cashier/manager kiosk must not stay
/// authorized indefinitely. Owner/elevated sessions inherit the same bound.
pub const SESSION_IDLE_TIMEOUT_SECS: i64 = 30 * 60;

// --- CEO Hierarchical Multi-Owner Licensing Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerHierarchyBranch {
    pub gym_id: Uuid,
    pub name: String,
    pub tier: LicenseTier,
    pub is_active: bool,
    pub license_key: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub days_remaining: Option<i64>,
    pub is_license_active: bool,
    pub hwid: Option<String>,
    pub active_members: u32,
    pub today_sales: f64,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub is_disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerHierarchyAccount {
    pub owner_email: String,
    pub company_name: String,
    pub created_at: DateTime<Utc>,
    pub branches: Vec<OwnerHierarchyBranch>,
    pub total_branches: usize,
    pub active_licenses_count: usize,
    pub pending_licenses_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueBranchKeyRequest {
    pub tier: Option<LicenseTier>,
    pub duration_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminCreateBranchForOwnerRequest {
    pub branch_name: String,
    pub tier: LicenseTier,
    pub duration_days: Option<i64>,
    #[serde(default = "default_true")]
    pub auto_issue_license: bool,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod password_tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("correct-password");
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct-password", &hash));
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn same_password_produces_different_hashes() {
        // Random per-hash salt means two hashes of the same input must differ,
        // unlike the old unsalted SHA-256 scheme.
        let a = hash_password("1234");
        let b = hash_password("1234");
        assert_ne!(a, b);
        assert!(verify_password("1234", &a));
        assert!(verify_password("1234", &b));
    }

    #[test]
    fn legacy_sha256_hash_still_verifies() {
        // Pre-migration accounts stored a raw sha256(password) hex digest.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"legacy-pin");
        let legacy_hash = format!("{:x}", hasher.finalize());

        assert!(verify_password("legacy-pin", &legacy_hash));
        assert!(!verify_password("wrong-pin", &legacy_hash));
    }
}
