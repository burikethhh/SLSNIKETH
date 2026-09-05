use chrono::Utc;
use gympos_shared::{
    AppSettings, CartItem, Coach, CoachSession, CreateCoachRequest, CreateExpenseRequest, CreateMemberRequest,
    CreateProductRequest, CreateWalkInRequest, ExpenseRecord, LicenseStatus, Member, ProductItem, SaleTransaction,
    StaffLoginResponse, StaffRole, TerminalSession, UpdateCoachRequest, UpdateMemberRequest,
    UpdateProductRequest, WalkInRecord,
};
use serde_json::json;
use std::sync::Arc;
use tauri::State;

use crate::db::Database;
use crate::face::FaceVectorStore;
use crate::hardware::HardwareManager;
use crate::license::LicenseManager;
use crate::vision::FaceEngine;

pub struct AppContext {
    pub db: Arc<Database>,
    pub license: Arc<LicenseManager>,
    pub hardware: HardwareManager,
    pub face_store: FaceVectorStore,
    pub session: Arc<parking_lot::RwLock<Option<TerminalSession>>>,
    /// `None` when the ONNX models could not be located/loaded at startup
    /// (e.g. missing `desktop/models/*.onnx`) — callers of `scan_face_frame`
    /// get a clear error instead of a panic in that case.
    pub face_engine: Arc<Option<FaceEngine>>,
    /// Person counter for overhead Camera 3 anti-tailgate ROI
    /// (`yolov8n.onnx`). `None` when the model failed to load — callers of
    /// `count_persons_in_frame` get a clear error instead of a panic.
    pub person_counter: Arc<Option<crate::vision::PersonCounter>>,
    /// Brute-force defense for the 4-8 digit staff PIN pad (only 10^4-10^8
    /// combinations, so an unlocked kiosk must not accept unlimited guesses).
    pub pin_gate: Arc<std::sync::Mutex<PinGate>>,
    /// Last-known remote tailgate policy for this branch (Phase A-D),
    /// refreshed by the sync worker from `SyncResponse.tailgate_policy`.
    /// Defaults to enabled + 300s siren cooldown when the cloud has no row.
    pub tailgate_policy: Arc<parking_lot::RwLock<gympos_shared::TailgatePolicy>>,
    /// When the siren last blasted. Incident ROWS are always written (evidence
    /// must never be dropped), but the physical siren is rate-limited by the
    /// policy cooldown so a lingering crowd doesn't strobe the gym.
    pub last_tailgate_alarm: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
}

/// Consecutive-failure lockout for staff PIN entry: after 5 wrong PINs the
/// terminal refuses further attempts for 30s, doubling per extra failure up
/// to 5 minutes. Reset by a successful login or app restart.
#[derive(Default)]
pub struct PinGate {
    consecutive_failures: u32,
    locked_until: Option<std::time::Instant>,
}

impl PinGate {
    const MAX_FAILURES_BEFORE_LOCK: u32 = 5;
    const BASE_LOCK_SECS: u64 = 30;
    const MAX_LOCK_SECS: u64 = 300;

    fn check_locked(&mut self) -> Result<(), String> {
        if let Some(until) = self.locked_until {
            let now = std::time::Instant::now();
            if now < until {
                let secs = (until - now).as_secs() + 1;
                return Err(format!(
                    "Too many failed PIN attempts. Terminal locked for {}s.",
                    secs
                ));
            }
            self.locked_until = None;
        }
        Ok(())
    }

    fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= Self::MAX_FAILURES_BEFORE_LOCK {
            let extra_steps = self.consecutive_failures - Self::MAX_FAILURES_BEFORE_LOCK;
            let secs = (Self::BASE_LOCK_SECS)
                .saturating_mul(1u64 << extra_steps.min(4))
                .min(Self::MAX_LOCK_SECS);
            self.locked_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(secs));
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.locked_until = None;
    }
}

fn check_license_active(state: &AppContext) -> Result<(), String> {
    // 1. Clock tamper check (mirrors SLS123 validator.py:232 `now < last_seen -60`)
    let now_unix = chrono::Utc::now().timestamp();
    let last_seen = state.db.last_seen_unix().unwrap_or(0);
    if crate::license::is_clock_tampered(now_unix, last_seen) {
        state.license.revoke();
        let _ = state.db.clear_cached_license();
        return Err("Access Denied: System clock tamper detected (rollback). License locked — online re-verification required.".to_string());
    }
    // 2. Heartbeat window check (7 days, mirrors validator.py:228 `now - last_verify > 7*86400`)
    let last_verify = state.db.last_verify_unix().unwrap_or(0);
    if crate::license::is_heartbeat_expired(now_unix, last_verify) {
        return Err("Access Denied: Offline heartbeat expired (7 days without online verification). Connect to internet to re-verify license.".to_string());
    }

    let result = match state.license.current_status() {
        LicenseStatus::Valid { .. } | LicenseStatus::GracePeriod { .. } => Ok(()),
        LicenseStatus::Expired { expired_at } => Err(format!(
            "Access Denied: Gym subscription expired on {}. System is locked out.",
            expired_at.format("%Y-%m-%d")
        )),
        LicenseStatus::Invalid { reason } => Err(format!("Access Denied: {}", reason)),
        LicenseStatus::Unlicensed => {
            Err("Access Denied: Gym is unlicensed. Please activate a license key to operate.".to_string())
        }
    };

    // 3. On successful active check, bump last_seen (mirrors validator.py:241 `last_seen=now`)
    if result.is_ok() {
        let _ = state.db.record_last_seen();
    }
    result
}

/// Role gate for sensitive terminal commands. The kiosk gate scans, POS
/// sales, walk-in intake and all read-only views stay open (no login needed
/// for members to enter), but anything that mutates money-adjacent, identity
/// or hardware state requires a terminal session with a sufficient role.
/// Cashier = front-desk sales/intake; Manager = inventory/members/hardware;
/// Owner = everything including license activation.
///
/// Also enforces the idle auto-lock: a session unused for longer than
/// `SESSION_IDLE_TIMEOUT_SECS` is dropped, so an unattended terminal does not
/// stay authorized all day.
fn require_role(state: &AppContext, allowed: &[StaffRole]) -> Result<(), String> {
    let mut guard = state.session.write();
    match &mut *guard {
        Some(s) if s.is_authenticated => {
            let idle_secs = (Utc::now() - s.last_activity_at).num_seconds();
            if idle_secs > gympos_shared::SESSION_IDLE_TIMEOUT_SECS {
                *guard = None;
                return Err("Access Denied: session expired after inactivity. Please log in again.".to_string());
            }
            if allowed.contains(&s.role) {
                s.last_activity_at = Utc::now();
                Ok(())
            } else {
                Err("Access Denied: your terminal role cannot perform this action. Ask a manager/owner.".to_string())
            }
        }
        Some(_) => Err("Access Denied: your terminal role cannot perform this action. Ask a manager/owner.".to_string()),
        None => Err("Access Denied: login on the terminal PIN screen first.".to_string()),
    }
}

fn require_manager(state: &AppContext) -> Result<(), String> {
    require_role(state, &[StaffRole::Manager, StaffRole::Owner])
}

/// Any authenticated terminal session (cashier/manager/owner). Used for
/// front-desk operations (sales, intake, close-out) that must be attributable
/// to a logged-in user but need no elevated privilege.
fn require_login(state: &AppContext) -> Result<(), String> {
    require_role(state, &[StaffRole::Staff, StaffRole::Manager, StaffRole::Owner])
}

fn require_owner(state: &AppContext) -> Result<(), String> {
    require_role(state, &[StaffRole::Owner])
}

// --- App Settings (White-Label Branding) ---

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppContext>) -> Result<AppSettings, String> {
    state.db.get_app_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_settings(settings: AppSettings, state: State<'_, AppContext>) -> Result<AppSettings, String> {
    require_manager(&state)?;
    state.db.save_app_settings(&settings).map_err(|e| e.to_string())?;
    Ok(settings)
}

// --- License & System Commands ---

#[tauri::command]
pub fn get_license_status(state: State<'_, AppContext>) -> serde_json::Value {
    let status = state.license.current_status();
    let claims = state.license.current_claims();
    json!({
        "status": status,
        "claims": claims
    })
}

#[tauri::command]
pub fn apply_license_key(key: String, state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    require_owner(&state)?;
    let status = state.license.verify_and_apply(&key)?;
    state.db.set_cached_license(&key).map_err(|e| e.to_string())?;
    // Fresh online verification → reset 7-day heartbeat (mirrors validator.py install_license last_verify=now)
    let _ = state.db.heartbeat_ok();
    Ok(json!({
        "success": true,
        "status": status
    }))
}

// --- Hardware & Door Access ---

#[tauri::command]
pub fn list_com_ports() -> Vec<String> {
    HardwareManager::list_available_ports()
}

#[tauri::command]
pub fn connect_com_port(port: String, baud: Option<u32>, state: State<'_, AppContext>) -> Result<String, String> {
    require_manager(&state)?;
    state.hardware.connect(&port, baud.unwrap_or(115200))
}

#[tauri::command]
pub fn unlock_magnetic_lock(duration_ms: Option<u32>, state: State<'_, AppContext>) -> Result<String, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    let claims = state.license.current_claims().ok_or("License required to trigger hardware lock")?;
    if !claims.hardware_lock_enabled {
        return Err("Hardware lock is disabled on this license tier".to_string());
    }

    // Log security audit trail so gym owner can track staff unlocking door without member payment
    let _ = state.db.log_attendance(
        None,
        Some("STAFF MANUAL OVERRIDE (Unauthenticated Pulse)"),
        "override",
        Some(0.0),
        false,
    );

    let ms = duration_ms.unwrap_or(3000);
    match state.hardware.unlock_door(ms) {
        Ok(msg) => Ok(msg),
        Err(_) => Ok(format!("Gate unlocked for {}ms (Hardware relay pulse / Standby mode)", ms)),
    }
}

#[tauri::command]
pub fn trigger_tailgate_alarm(
    reason: Option<String>,
    linked_member_id: Option<String>,
    person_count: Option<i32>,
    state: State<'_, AppContext>,
) -> Result<serde_json::Value, String> {
    // Phase A-D: incident rows are ALWAYS written (evidence first). The siren
    // honors the remote policy: disabled branches log silently, and repeat
    // blasts inside the cooldown window are suppressed (still logged).
    let policy = state.tailgate_policy.read().clone();
    let siren_due = if !policy.enabled {
        false
    } else {
        let mut last = state.last_tailgate_alarm.lock().expect("alarm clock poisoned");
        let now = std::time::Instant::now();
        let due = match *last {
            Some(t) => now.duration_since(t).as_secs() >= policy.siren_cooldown_secs,
            None => true,
        };
        if due {
            *last = Some(now);
        }
        due
    };
    if siren_due {
        // 1. Fire ESP32 hardware buzzer/strobe relay for 5 seconds
        let _ = state.hardware.trigger_alarm(5000);
    }

    // 2. Log attributed security violation (whose window + YOLO snapshot)
    let log = state
        .db
        .log_tailgate_incident(
            linked_member_id.as_deref(),
            "⚠️ Tailgate Intrusion",
            person_count,
        )
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "status": "ALARM_TRIGGERED",
        "reason": reason.unwrap_or_else(|| "Turnstile ROI multi-occupancy violation".to_string()),
        "siren_suppressed": !siren_due,
        "policy_enabled": policy.enabled,
        "log": log
    }))
}

#[tauri::command]
pub fn list_interbranch_members(state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    let detailed = state.db.list_interbranch_members_detailed().map_err(|e| e.to_string())?;
    // Also return local gym context for client-side filtering if needed
    let local_gym_id = state.license.current_claims().map(|c| c.gym_id.to_string()).unwrap_or_default();
    Ok(json!({
        "local_gym_id": local_gym_id,
        "members": detailed,
        "count": detailed.len(),
        "local_gym_name": state.license.current_claims().map(|c| c.gym_name).unwrap_or_else(|| app_settings_fallback_gym_name(&state))
    }))
}

fn app_settings_fallback_gym_name(state: &AppContext) -> String {
    state.db.get_app_settings().map(|s| s.gym_name).unwrap_or_else(|_| "Local Gym".to_string())
}

#[tauri::command]
pub fn get_dashboard_summary(state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    let member_count = state.db.count_members().map_err(|e| e.to_string())?;
    let today_checkins = state.db.count_today_checkins().unwrap_or(0);
    let tailgates = state.db.count_tailgates().unwrap_or(0);
    let license_status = state.license.current_status();
    let (hw_connected, port_name) = state.hardware.get_status();

    let (tier_name, max_members) = match state.license.current_claims() {
        Some(c) => (format!("{:?}", c.tier), c.max_members),
        None => ("Unlicensed".to_string(), 0),
    };

    Ok(json!({
        "active_members": member_count,
        "max_members": max_members,
        "today_checkins": today_checkins,
        "tailgate_count": tailgates,
        "tier": tier_name,
        "license_status": license_status,
        "hardware_connected": hw_connected,
        "hardware_port": port_name,
    }))
}

// --- Member Management Commands ---

#[tauri::command]
pub fn list_members(state: State<'_, AppContext>) -> Result<Vec<Member>, String> {
    state.db.list_members().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_member(id: String, state: State<'_, AppContext>) -> Result<Option<Member>, String> {
    state.db.get_member_by_id(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn register_member(req: CreateMemberRequest, state: State<'_, AppContext>) -> Result<Member, String> {
    check_license_active(&state)?;
    require_login(&state)?;

    // Check tier member limits
    let current_count = state.db.count_members().map_err(|e| e.to_string())?;
    if let Some(claims) = state.license.current_claims() {
        if current_count >= claims.max_members as usize {
            return Err(format!(
                "Member limit reached ({}/{}). Upgrade license tier to enroll more members.",
                current_count, claims.max_members
            ));
        }
    }

    let member = state.db.create_member(&req).map_err(|e| e.to_string())?;

    // Upsert face vectors to in-memory store for instant zero-latency recognition
    if !member.face_vectors.is_empty() {
        let full_name = format!("{} {}", member.first_name, member.last_name);
        state.face_store.upsert(member.id.clone(), full_name, member.face_vectors.clone());
    }

    Ok(member)
}

#[tauri::command]
pub fn update_member(req: UpdateMemberRequest, state: State<'_, AppContext>) -> Result<Member, String> {
    check_license_active(&state)?;
    require_login(&state)?;
    let member = state.db.update_member(&req).map_err(|e| e.to_string())?;

    // Update in-memory biometric display name if member exists in face store
    let full_name = format!("{} {}", member.first_name, member.last_name);
    if !member.face_vectors.is_empty() {
        state.face_store.upsert(member.id.clone(), full_name, member.face_vectors.clone());
    }

    Ok(member)
}

#[tauri::command]
pub fn delete_member(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.delete_member(&id).map_err(|e| e.to_string())?;
    state.face_store.remove(&id);
    Ok(())
}

// --- Walk-In / Day Pass Commands ---

#[tauri::command]
pub fn process_walk_in(req: CreateWalkInRequest, state: State<'_, AppContext>) -> Result<WalkInRecord, String> {
    check_license_active(&state)?;
    require_login(&state)?;

    // 1. Create walk-in record & write sale transaction
    let record = state.db.create_walk_in(&req).map_err(|e| e.to_string())?;

    // 2. If face vector provided, register temporary 8-hour pass in memory store
    if let Some(vec) = req.face_vector {
        let temp_id = format!("WALKIN-{}", record.id);
        state.face_store.upsert_with_expiry(
            temp_id,
            format!("Walk-In: {}", req.guest_name),
            vec![vec],
            Some(record.expires_at),
        );
    }

    // 3. Log initial gate entrance
    let _ = state
        .db
        .log_attendance(Some(&record.id), Some(&format!("Walk-In: {}", req.guest_name)), "in", Some(1.0), false);

    // 4. Trigger magnetic lock unlock for 3 seconds
    let _ = state.hardware.unlock_door(3000);

    Ok(record)
}

#[tauri::command]
pub fn list_walk_ins(state: State<'_, AppContext>) -> Result<Vec<WalkInRecord>, String> {
    state.db.list_walk_ins().map_err(|e| e.to_string())
}

// --- Face Recognition & Gate Kiosk (Scan In & Out) ---

/// Runs the REAL ONNX detection + alignment + embedding pipeline
/// (`crate::vision::FaceEngine`) on a camera frame captured by the webview,
/// returning a genuine embedding (512-d ArcFace preferred, 128-d SFace
/// fallback) instead of the simulated vectors
/// previously fabricated in JS. Used for both enrollment captures and live
/// probe scans; the resulting vector is passed to `register_member` /
/// `process_face_scan` exactly as before, so no changes were needed to the
/// matching engine in `face.rs`.
#[tauri::command]
pub async fn scan_face_frame(image_base64: String, state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    check_license_active(&state)?;

    // Arc-clone the engine handle: heavy ONNX inference (YuNet +
    // ArcFace/SFace on CPU) runs on the blocking pool so concurrent scan
    // ticks never stall the webview IPC.
    let engine_opt = state.face_engine.clone();
    let image = crate::vision::decode_base64_image(&image_base64)?;

    let (model, embedding_dim, detected) = tauri::async_runtime::spawn_blocking(move || {
        let engine = engine_opt
            .as_ref()
            .as_ref()
            .ok_or_else(|| "Face recognition engine unavailable: ONNX models failed to load at startup".to_string())?;
        let out = engine.detect_and_embed(&image)?;
        Ok::<_, String>((engine.recognizer_name().to_string(), engine.embedding_dim(), out))
    })
    .await
    .map_err(|e| format!("Face inference task failed: {}", e))??;
    match detected {
        Some((face, embedding)) => Ok(json!({
            "face_detected": true,
            "confidence": face.score,
            "vector": embedding,
            "embedding_dim": embedding_dim,
            "model": model,
            "box": { "x": face.rect.0, "y": face.rect.1, "w": face.rect.2, "h": face.rect.3 },
            // 5-point landmarks (eyes, nose, mouth corners) in ORIGINAL image
            // pixels — powers the webview liveness check (eye displacement
            // across confirmation frames defeats static photos).
            "landmarks": face.landmarks.iter().map(|p| json!({"x": p[0], "y": p[1]})).collect::<Vec<_>>()
        })),
        None => Ok(json!({
            "face_detected": false,
            "vector": serde_json::Value::Null,
            "message": "No face detected in frame"
        })),
    }
}

/// Counts persons inside the overhead Camera 3 ROI using the bundled
/// `yolov8n.onnx` (`crate::vision::PersonCounter`). Called once per 250ms
/// tick by `armDoorOpenTailgateSurveillance` in the webview during the
/// 3.5s door-open window; `person_count > 1` means tailgating.
#[tauri::command]
pub async fn count_persons_in_frame(
    image_base64: String,
    roi_x: f32,
    roi_y: f32,
    roi_width: f32,
    roi_height: f32,
    state: State<'_, AppContext>,
) -> Result<serde_json::Value, String> {
    check_license_active(&state)?;
    let counter_opt = state.person_counter.clone();
    // MOG sensitivity follows Hardware Settings (Recognition Tuning).
    let mog_sensitivity = state
        .db
        .get_app_settings()
        .ok()
        .and_then(|s| s.camera_config)
        .map(|c| c.mog_sensitivity)
        .unwrap_or(0.5);
    let image = crate::vision::decode_base64_image(&image_base64)?;
    // YOLO inference off the async path — tailgate ticks every 350ms must
    // never queue behind each other on the Tauri worker.
    let (count, boxes, motion_in_roi) = tauri::async_runtime::spawn_blocking(move || {
        let counter = counter_opt
            .as_ref()
            .as_ref()
            .ok_or_else(|| "Person counter unavailable: yolov8n.onnx failed to load".to_string())?;
        counter.set_motion_sensitivity(mog_sensitivity);
        counter.count_and_locate_in_roi(&image, roi_x, roi_y, roi_width, roi_height)
    })
    .await
    .map_err(|e| format!("Person-count task failed: {}", e))??;
    Ok(json!({ "person_count": count, "boxes": boxes, "motion_in_roi": motion_in_roi }))
}

#[tauri::command]
pub fn process_face_scan(
    probe_vector: Vec<f32>,
    direction: String,
    state: State<'_, AppContext>,
) -> Result<serde_json::Value, String> {
    check_license_active(&state)?;

    // Threshold follows the embedding, tunable in Hardware Settings
    // (Recognition Tuning): 512-d ArcFace defaults match >= 0.62 /
    // adapt >= 0.80, legacy 128-d SFace a hair lower. Raised from 0.55/0.50
    // after the accuracy review: genuine matches score ~0.8+, so the higher
    // bar cuts the impostor tail without hurting real members.
    let cfg = state
        .db
        .get_app_settings()
        .ok()
        .and_then(|s| s.camera_config)
        .unwrap_or_default();
    let (match_threshold, adapt_threshold) = if probe_vector.len() >= 512 {
        (cfg.match_threshold, cfg.adapt_threshold)
    } else {
        ((cfg.match_threshold - 0.04).max(0.0), (cfg.adapt_threshold - 0.02).max(0.0))
    };

    let match_result = state.face_store.match_vector(&probe_vector, match_threshold);
    let dim_mismatch = state.face_store.take_dim_mismatch();

    if let Some(m) = match_result {
        // Frozen / expired member accounts are denied at the gate (data retained for records)
        if let Ok(Some(status)) = state.db.get_member_status(&m.member_id) {
            if status != "active" {
                let log = state
                    .db
                    .log_attendance(
                        Some(&m.member_id),
                        Some(&format!("{} ({})", m.member_name, status.to_uppercase())),
                        &direction,
                        Some(m.confidence),
                        false,
                    )
                    .map_err(|e| e.to_string())?;
                return Ok(json!({
                    "matched": true,
                    "account_hold": true,
                    "member_name": m.member_name,
                    "message": format!("Access Denied: {} account is {} (frozen/expired). Renew at front desk.", m.member_name, status),
                    "door_unlocked": false,
                    "log": log
                }));
            }
        }

        // If it's an expired walk-in pass (> 8 hours), deny access immediately
        if m.is_expired {
            let log = state
                .db
                .log_attendance(
                    Some(&m.member_id),
                    Some(&format!("{} (EXPIRED 8H PASS)", m.member_name)),
                    &direction,
                    Some(m.confidence),
                    false,
                )
                .map_err(|e| e.to_string())?;

            return Ok(json!({
                "matched": false,
                "is_expired": true,
                "member_name": m.member_name,
                "message": format!("Access Denied: Walk-In pass for {} expired after 8 hours.", m.member_name),
                "door_unlocked": false,
                "log": log
            }));
        }

        // Anti-Passback Validation:
        // Cannot scan IN if already inside ('in').
        // Egress (direction 'out') is ALWAYS permitted per safety standards and SLS123 access control.
        let last_direction = state.db.get_member_last_direction(&m.member_id).unwrap_or(None);
        if direction == "in" && last_direction.as_deref() == Some("in") {
            return Ok(json!({
                "matched": true,
                "passback_violation": true,
                "member_name": m.member_name,
                "message": format!("Anti-Passback Denied: {} is already inside the gym. Must scan exit before entering again.", m.member_name),
                "door_unlocked": false
            }));
        }

        // Log successful attendance (in or out)
        let log = state
            .db
            .log_attendance(Some(&m.member_id), Some(&m.member_name), &direction, Some(m.confidence), false)
            .map_err(|e| e.to_string())?;

        // Adaptive continuous learning: slightly adapt stored profile on high
        // confidence match (>= 0.80 ArcFace / >= 0.78 legacy SFace).
        // adapt_profile enforces the drift guardrail internally (reverts +
        // warns past cosine 0.90 from enrollment) and never touches walk-ins.
        if m.confidence >= adapt_threshold && !m.is_expired {
            state.face_store.adapt_profile(&m.member_id, &probe_vector, 0.05);
        }

        // Unlock door if hardware is connected & enabled in license
        let mut unlocked = false;
        if let Some(claims) = state.license.current_claims() {
            if claims.hardware_lock_enabled {
                let _ = state.hardware.unlock_door(3000);
                unlocked = true;
            }
        }

        let remaining_mins = m.expires_at.map(|exp| {
            let rem = (exp - chrono::Utc::now()).num_minutes();
            if rem > 0 { rem } else { 0 }
        });

        Ok(json!({
            "matched": true,
            "member_id": m.member_id,
            "member_name": m.member_name,
            "direction": direction,
            "confidence": (m.confidence * 100.0).round() / 100.0,
            "match_margin": (m.match_margin * 100.0).round() / 100.0,
            "door_unlocked": unlocked,
            "remaining_minutes": remaining_mins,
            "log": log
        }))
    } else {
        // Unknown face scan or cooldown-suppressed frame — do NOT spam attendance records!
        // needs_reenroll distinguishes "legacy embedding width" from "stranger".
        Ok(json!({
            "matched": false,
            "is_expired": false,
            "needs_reenroll": dim_mismatch,
            "message": if dim_mismatch {
                "Face gallery needs re-enrollment for the current recognizer".to_string()
            } else {
                "Face not recognized".to_string()
            },
            "door_unlocked": false
        }))
    }
}

#[tauri::command]
pub fn log_tailgate_event(state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    let _ = state.hardware.trigger_alarm(4000);
    let log = state
        .db
        .log_attendance(None, Some("⚠️ Tailgate Intrusion"), "in", None, true)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "alert": "Tailgating violation flagged",
        "log": log
    }))
}

#[tauri::command]
pub fn list_recent_attendance(limit: Option<usize>, state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    let logs = state.db.list_recent_attendance(limit.unwrap_or(20)).map_err(|e| e.to_string())?;
    Ok(json!(logs))
}

// --- POS Store Commands ---

#[tauri::command]
pub fn list_products(state: State<'_, AppContext>) -> Result<Vec<ProductItem>, String> {
    state.db.list_products().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_product(req: CreateProductRequest, state: State<'_, AppContext>) -> Result<ProductItem, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.create_product(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_product(req: UpdateProductRequest, state: State<'_, AppContext>) -> Result<ProductItem, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.update_product(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn adjust_product_stock(id: String, delta: i32, state: State<'_, AppContext>) -> Result<ProductItem, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.adjust_product_stock(&id, delta).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_product(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.delete_product(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn checkout_pos_sale(
    member_id: Option<String>,
    items: Vec<CartItem>,
    payment_method: String,
    discount_type: Option<String>,
    discount_pct: Option<f64>,
    state: State<'_, AppContext>,
) -> Result<SaleTransaction, String> {
    check_license_active(&state)?;
    require_login(&state)?;

    if items.is_empty() {
        return Err("Cart is empty".to_string());
    }
    state
        .db
        .process_sale(
            member_id.as_deref(),
            &items,
            &payment_method,
            discount_type.as_deref().unwrap_or(""),
            discount_pct.unwrap_or(0.0),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn renew_member(id: String, state: State<'_, AppContext>) -> Result<Member, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.renew_member(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn freeze_member(id: String, state: State<'_, AppContext>) -> Result<Member, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.set_member_status(&id, "suspended").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unfreeze_member(id: String, state: State<'_, AppContext>) -> Result<Member, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.set_member_status(&id, "active").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rescan_member_face(
    id: String,
    face_vectors: Vec<Vec<f32>>,
    photo_data_url: Option<String>,
    state: State<'_, AppContext>,
) -> Result<Member, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    let member = state
        .db
        .update_member_vectors(&id, &face_vectors, photo_data_url.as_deref())
        .map_err(|e| e.to_string())?;
    // Refresh the in-memory centroid so the next probe matches the new capture
    let full_name = format!("{} {}", member.first_name, member.last_name);
    if !member.face_vectors.is_empty() {
        state.face_store.upsert(member.id.clone(), full_name, member.face_vectors.clone());
    }
    Ok(member)
}

#[tauri::command]
pub fn get_member_stats(state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    state.db.get_member_stats().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_expense(
    req: CreateExpenseRequest,
    state: State<'_, AppContext>,
) -> Result<ExpenseRecord, String> {
    check_license_active(&state)?;
    require_login(&state)?;
    let by = state
        .session
        .read()
        .clone()
        .map(|s| s.display_name.clone())
        .unwrap_or_else(|| "front-desk".to_string());
    state.db.create_expense(&req, &by).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_expenses(limit: Option<i64>, state: State<'_, AppContext>) -> Result<Vec<ExpenseRecord>, String> {
    state.db.list_expenses(limit.unwrap_or(100)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_expense(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.delete_expense(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_end_of_day(day: Option<String>, state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    require_login(&state)?;
    let day_str = day.unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
    if day_str.len() != 10 || !day_str.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return Err("Day must be YYYY-MM-DD".to_string());
    }
    state.db.get_end_of_day(&day_str).map_err(|e| e.to_string())
}

// --- Coaches Commands ---

#[tauri::command]
pub fn list_coaches(state: State<'_, AppContext>) -> Result<Vec<Coach>, String> {
    state.db.list_coaches().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_coach(req: CreateCoachRequest, state: State<'_, AppContext>) -> Result<Coach, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.create_coach(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_coach(req: UpdateCoachRequest, state: State<'_, AppContext>) -> Result<Coach, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.update_coach(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_coach(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.delete_coach(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn schedule_coach_session(
    coach_id: String,
    coach_name: String,
    member_id: String,
    member_name: String,
    date: String,
    duration: u32,
    state: State<'_, AppContext>,
) -> Result<CoachSession, String> {
    check_license_active(&state)?;
    require_login(&state)?;

    state
        .db
        .schedule_session(&coach_id, &coach_name, &member_id, &member_name, &date, duration)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_coach_sessions(state: State<'_, AppContext>) -> Result<Vec<CoachSession>, String> {
    state.db.list_coach_sessions().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_coach_session(session_id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.cancel_coach_session(&session_id).map_err(|e| e.to_string())
}

// --- Walk-In Extend & Void Commands ---

#[tauri::command]
pub fn extend_walk_in(id: String, extra_hours: i64, state: State<'_, AppContext>) -> Result<WalkInRecord, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    let record = state.db.extend_walk_in(&id, extra_hours).map_err(|e| e.to_string())?;

    // Refresh the live store from the DB vector so renew works even when the
    // memory entry was purged (or never existed for code-only passes that
    // later got a vector). No re-face-scan needed: staff just clicks Renew.
    let temp_id = format!("WALKIN-{}", record.id);
    if let Some(mut entry) = state.face_store.get_entry(&temp_id) {
        entry.expires_at = Some(record.expires_at);
        state.face_store.upsert_with_expiry(entry.member_id, entry.member_name, entry.vectors, entry.expires_at);
    } else if let Some(vec) = record.face_vector.clone() {
        state.face_store.upsert_with_expiry(
            temp_id,
            format!("Walk-In: {}", record.guest_name),
            vec![vec],
            Some(record.expires_at),
        );
    }

    Ok(record)
}

#[tauri::command]
pub fn renew_walk_in(id: String, state: State<'_, AppContext>) -> Result<WalkInRecord, String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    let record = state.db.renew_walk_in(&id).map_err(|e| e.to_string())?;
    // Same re-upsert logic as extend: revive the live entry from the stored
    // vector when the memory entry is gone.
    let temp_id = format!("WALKIN-{}", record.id);
    if let Some(mut entry) = state.face_store.get_entry(&temp_id) {
        entry.expires_at = Some(record.expires_at);
        state.face_store.upsert_with_expiry(entry.member_id, entry.member_name, entry.vectors, entry.expires_at);
    } else if let Some(vec) = record.face_vector.clone() {
        state.face_store.upsert_with_expiry(
            temp_id,
            format!("Walk-In: {}", record.guest_name),
            vec![vec],
            Some(record.expires_at),
        );
    }
    Ok(record)
}

#[tauri::command]
pub fn void_walk_in(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    require_manager(&state)?;
    state.db.void_walk_in(&id).map_err(|e| e.to_string())?;
    let temp_id = format!("WALKIN-{}", id);
    state.face_store.remove(&temp_id);
    Ok(())
}

// --- Auto-Updater Commands ---

#[tauri::command]
pub async fn check_for_updates(
    channel: Option<String>,
    state: State<'_, AppContext>,
) -> Result<gympos_shared::UpdateCheckResponse, String> {
    let cloud_url = std::env::var("CLOUD_URL").unwrap_or_else(|_| "https://gympos-cloud.onrender.com".to_string());
    let updater = crate::updater::AutoUpdater::new(cloud_url);

    let gym_id = state.license.current_claims().map(|c| c.gym_id);
    updater.check_for_updates(gym_id, channel).await
}

#[tauri::command]
pub async fn download_and_install_update(
    download_url: String,
    sha256: String,
    state: State<'_, AppContext>,
) -> Result<String, String> {
    require_owner(&state)?;
    let cloud_url = std::env::var("CLOUD_URL").unwrap_or_else(|_| "https://gympos-cloud.onrender.com".to_string());
    let updater = crate::updater::AutoUpdater::new(cloud_url);

    let tmp_path = updater.download_and_verify(&download_url, &sha256).await?;
    crate::updater::AutoUpdater::apply_update_and_restart(&tmp_path)?;
    Ok("Update applying and restarting...".to_string())
}

#[tauri::command]
pub fn get_app_version() -> Result<String, String> {
    Ok(crate::updater::CURRENT_APP_VERSION.to_string())
}

// --- Terminal Role-Based Access Control (RBAC) Commands ---

#[tauri::command]
pub fn authenticate_staff_pin(
    pin: String,
    state: State<'_, AppContext>,
) -> Result<StaffLoginResponse, String> {
    let pin = pin.trim();
    if pin.is_empty() {
        return Err("PIN code cannot be empty".to_string());
    }

    // Brute-force gate BEFORE touching the database: a 4-digit PIN only has
    // 10,000 combinations, so unlimited silent retries are not acceptable.
    {
        let mut gate = state.pin_gate.lock().expect("pin_gate poisoned");
        gate.check_locked()?;
        let result = state.db.authenticate_staff_pin(pin).map_err(|e| format!("Database error: {}", e));
        match result {
            Ok(Some(staff)) => {
                gate.record_success();
                Ok(staff)
            }
            Ok(None) => {
                gate.record_failure();
                Err("Invalid PIN. Access Denied.".to_string())
            }
            Err(e) => Err(e),
        }
    }
    .map(|staff| {
        let session = TerminalSession {
            is_authenticated: true,
            user_id: staff.id.clone(),
            display_name: staff.full_name.clone(),
            role: staff.role,
            gym_id: staff.gym_id,
            gym_name: staff.gym_name.clone(),
            logged_in_at: chrono::Utc::now(),
            last_activity_at: chrono::Utc::now(),
        };

        *state.session.write() = Some(session);

        StaffLoginResponse {
            authenticated: true,
            staff_id: staff.id,
            full_name: staff.full_name,
            username: staff.username,
            role: staff.role,
            gym_id: staff.gym_id,
            gym_name: staff.gym_name,
        }
    })
}

#[tauri::command]
pub async fn authenticate_owner(
    email: String,
    password: String,
    state: State<'_, AppContext>,
) -> Result<StaffLoginResponse, String> {
    let email = email.trim().to_lowercase();
    let password = password.trim();

    if email.is_empty() || password.is_empty() {
        return Err("Email and password are required".to_string());
    }

    let cloud_url = std::env::var("CLOUD_URL").unwrap_or_else(|_| "https://gympos-cloud.onrender.com".to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.post(format!("{}/api/v1/owner/auth/login", cloud_url))
        .json(&serde_json::json!({
            "email": email,
            "password": password
        }))
        .send()
        .await;

    let mut authenticated = false;
    let mut company_name = "Franchise Owner".to_string();

    match resp {
        Ok(r) if r.status().is_success() => {
            if let Ok(data) = r.json::<serde_json::Value>().await {
                if data["authenticated"].as_bool().unwrap_or(false) {
                    authenticated = true;
                    if let Some(comp) = data["company_name"].as_str() {
                        company_name = comp.to_string();
                    }
                }
            }
        }
        // SECURITY: no offline fallback. The previous build accepted ANY
        // password of length >= 6 when the cloud was unreachable, so anyone
        // at the terminal could gain full Owner privileges by blocking the
        // network and typing the license owner's email (visible on screen).
        // Owner elevation now strictly requires a successful cloud login.
        Ok(r) => {
            return Err(format!(
                "Owner verification failed (cloud returned HTTP {}). Owner login requires a working internet connection.",
                r.status()
            ));
        }
        Err(_) => {
            return Err(
                "Cannot reach the cloud to verify owner credentials. Owner login requires an internet connection — use a staff PIN for offline front-desk work.".to_string(),
            );
        }
    }

    if !authenticated {
        return Err("Invalid owner email or password.".to_string());
    }

    let session = TerminalSession {
        is_authenticated: true,
        user_id: email.clone(),
        display_name: format!("Owner Admin ({})", company_name),
        role: StaffRole::Owner,
        gym_id: None,
        gym_name: None,
        logged_in_at: chrono::Utc::now(),
        last_activity_at: chrono::Utc::now(),
    };

    *state.session.write() = Some(session);

    Ok(StaffLoginResponse {
        authenticated: true,
        staff_id: format!("owner:{}", email),
        full_name: company_name,
        username: email,
        role: StaffRole::Owner,
        gym_id: None,
        gym_name: None,
    })
}

#[tauri::command]
pub fn poll_hardware_buttons(state: State<'_, AppContext>) -> Result<Vec<crate::hardware::HardwareButtonEvent>, String> {
    Ok(state.hardware.drain_button_events())
}

#[tauri::command]
pub fn list_remote_plans(state: State<'_, AppContext>) -> Result<Vec<gympos_shared::MembershipPlanConfig>, String> {
    state.db.list_remote_plans().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_remote_promos(state: State<'_, AppContext>) -> Result<Vec<gympos_shared::PromoVoucherConfig>, String> {
    state.db.list_remote_promos().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_terminal_session(state: State<'_, AppContext>) -> Result<Option<TerminalSession>, String> {
    Ok(state.session.read().clone())
}

/// Diagnostic for "Cryptographic signature verification failed": fetches the
/// cloud's active verification key and compares fingerprints with the exe's
/// embedded key. A MISMATCH means the cloud is signing with a different (or
/// ephemeral) keypair — set RSA_PRIVATE_KEY_PEM on the cloud and re-issue.
#[tauri::command]
pub async fn get_license_key_diagnostics(
    cloud_url: Option<String>,
    state: State<'_, AppContext>,
) -> Result<serde_json::Value, String> {
    let _ = &state;
    let base = cloud_url
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            std::env::var("CLOUD_URL").unwrap_or_else(|_| "https://gympos-cloud.onrender.com".to_string())
        });
    let url = format!("{}/api/v1/licenses/public-key", base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cannot reach cloud at {}: {}", base, e))?;
    if !resp.status().is_success() {
        return Err(format!("Cloud returned {} for public-key endpoint", resp.status()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let cloud_pem = body
        .get("public_key_pem")
        .and_then(|v| v.as_str())
        .ok_or("Cloud response missing public_key_pem")?;
    let embedded_fp = crate::license::embedded_key_fingerprint();
    let cloud_fp = crate::license::public_key_fingerprint(cloud_pem);
    Ok(json!({
        "cloud_url": base,
        "embedded_fingerprint": embedded_fp,
        "cloud_fingerprint": cloud_fp,
        "match": embedded_fp == cloud_fp,
    }))
}

#[tauri::command]
pub fn logout_terminal_session(state: State<'_, AppContext>) -> Result<(), String> {
    *state.session.write() = None;
    Ok(())
}

#[tauri::command]
pub fn list_terminal_staff(state: State<'_, AppContext>) -> Result<Vec<serde_json::Value>, String> {
    // Argon2 PIN hashes stay internal: a 4-digit PIN's hash falls to offline
    // brute-force instantly, so the UI only ever receives sanitized records
    // (the sync layer separately persists hashes it needs for PIN login).
    let staff = state.db.list_local_staff().map_err(|e| e.to_string())?;
    Ok(staff
        .iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).unwrap_or_else(|_| json!({}));
            if let Some(obj) = v.as_object_mut() {
                obj.remove("pin_hash");
            }
            v
        })
        .collect())
}
