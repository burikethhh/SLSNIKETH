use gympos_shared::{
    AppSettings, CartItem, Coach, CoachSession, CreateCoachRequest, CreateMemberRequest, CreateProductRequest,
    CreateWalkInRequest, LicenseStatus, Member, ProductItem, SaleTransaction, UpdateCoachRequest, UpdateMemberRequest,
    UpdateProductRequest, WalkInRecord,
};
use serde_json::json;
use std::sync::Arc;
use tauri::State;

use crate::db::Database;
use crate::face::FaceVectorStore;
use crate::hardware::HardwareManager;
use crate::license::LicenseManager;

pub struct AppContext {
    pub db: Arc<Database>,
    pub license: Arc<LicenseManager>,
    pub hardware: HardwareManager,
    pub face_store: FaceVectorStore,
}

fn check_license_active(state: &AppContext) -> Result<(), String> {
    match state.license.current_status() {
        LicenseStatus::Valid { .. } | LicenseStatus::GracePeriod { .. } => Ok(()),
        LicenseStatus::Expired { expired_at } => Err(format!(
            "Access Denied: Gym subscription expired on {}. System is locked out.",
            expired_at.format("%Y-%m-%d")
        )),
        LicenseStatus::Invalid { reason } => Err(format!("Access Denied: {}", reason)),
        LicenseStatus::Unlicensed => {
            Err("Access Denied: Gym is unlicensed. Please activate a license key to operate.".to_string())
        }
    }
}

// --- App Settings (White-Label Branding) ---

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppContext>) -> Result<AppSettings, String> {
    state.db.get_app_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_settings(settings: AppSettings, state: State<'_, AppContext>) -> Result<AppSettings, String> {
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
    let status = state.license.verify_and_apply(&key)?;
    state.db.set_cached_license(&key).map_err(|e| e.to_string())?;
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
    state.hardware.connect(&port, baud.unwrap_or(115200))
}

#[tauri::command]
pub fn unlock_magnetic_lock(duration_ms: Option<u32>, state: State<'_, AppContext>) -> Result<String, String> {
    check_license_active(&state)?;
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

    state.hardware.unlock_door(duration_ms.unwrap_or(3000))
}

#[tauri::command]
pub fn trigger_tailgate_alarm(reason: Option<String>, state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    // 1. Fire ESP32 hardware buzzer/strobe relay for 5 seconds
    let _ = state.hardware.trigger_alarm(5000);

    // 2. Log high-priority security violation
    let log = state
        .db
        .log_attendance(None, None, "in", None, true)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "status": "ALARM_TRIGGERED",
        "reason": reason.unwrap_or_else(|| "Turnstile ROI multi-occupancy violation".to_string()),
        "log": log
    }))
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
    state.db.delete_member(&id).map_err(|e| e.to_string())?;
    state.face_store.remove(&id);
    Ok(())
}

// --- Walk-In / Day Pass Commands ---

#[tauri::command]
pub fn process_walk_in(req: CreateWalkInRequest, state: State<'_, AppContext>) -> Result<WalkInRecord, String> {
    check_license_active(&state)?;

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

#[tauri::command]
pub fn process_face_scan(
    probe_vector: Vec<f32>,
    direction: String,
    state: State<'_, AppContext>,
) -> Result<serde_json::Value, String> {
    check_license_active(&state)?;

    let match_result = state.face_store.match_vector(&probe_vector, 0.60);

    if let Some(m) = match_result {
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

        // Log successful attendance (in or out)
        let log = state
            .db
            .log_attendance(Some(&m.member_id), Some(&m.member_name), &direction, Some(m.confidence), false)
            .map_err(|e| e.to_string())?;

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
            "door_unlocked": unlocked,
            "remaining_minutes": remaining_mins,
            "log": log
        }))
    } else {
        // Unknown face scan
        let log = state
            .db
            .log_attendance(None, None, &direction, None, false)
            .map_err(|e| e.to_string())?;

        Ok(json!({
            "matched": false,
            "is_expired": false,
            "message": "Face not recognized",
            "door_unlocked": false,
            "log": log
        }))
    }
}

#[tauri::command]
pub fn log_tailgate_event(state: State<'_, AppContext>) -> Result<serde_json::Value, String> {
    let _ = state.hardware.trigger_alarm(4000);
    let log = state
        .db
        .log_attendance(None, None, "in", None, true)
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
    state.db.create_product(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_product(req: UpdateProductRequest, state: State<'_, AppContext>) -> Result<ProductItem, String> {
    check_license_active(&state)?;
    state.db.update_product(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn adjust_product_stock(id: String, delta: i32, state: State<'_, AppContext>) -> Result<ProductItem, String> {
    check_license_active(&state)?;
    state.db.adjust_product_stock(&id, delta).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_product(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    state.db.delete_product(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn checkout_pos_sale(
    member_id: Option<String>,
    items: Vec<CartItem>,
    payment_method: String,
    state: State<'_, AppContext>,
) -> Result<SaleTransaction, String> {
    check_license_active(&state)?;

    if items.is_empty() {
        return Err("Cart is empty".to_string());
    }
    state.db.process_sale(member_id.as_deref(), &items, &payment_method).map_err(|e| e.to_string())
}

// --- Coaches Commands ---

#[tauri::command]
pub fn list_coaches(state: State<'_, AppContext>) -> Result<Vec<Coach>, String> {
    state.db.list_coaches().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_coach(req: CreateCoachRequest, state: State<'_, AppContext>) -> Result<Coach, String> {
    check_license_active(&state)?;
    state.db.create_coach(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_coach(req: UpdateCoachRequest, state: State<'_, AppContext>) -> Result<Coach, String> {
    check_license_active(&state)?;
    state.db.update_coach(&req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_coach(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
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
    state.db.cancel_coach_session(&session_id).map_err(|e| e.to_string())
}

// --- Walk-In Extend & Void Commands ---

#[tauri::command]
pub fn extend_walk_in(id: String, extra_hours: i64, state: State<'_, AppContext>) -> Result<WalkInRecord, String> {
    check_license_active(&state)?;
    let record = state.db.extend_walk_in(&id, extra_hours).map_err(|e| e.to_string())?;

    // Update in-memory biometric expiry if present
    let temp_id = format!("WALKIN-{}", record.id);
    if let Some(mut entry) = state.face_store.get_entry(&temp_id) {
        entry.expires_at = Some(record.expires_at);
        state.face_store.upsert_with_expiry(entry.member_id, entry.member_name, entry.vectors, entry.expires_at);
    }

    Ok(record)
}

#[tauri::command]
pub fn void_walk_in(id: String, state: State<'_, AppContext>) -> Result<(), String> {
    check_license_active(&state)?;
    state.db.void_walk_in(&id).map_err(|e| e.to_string())?;
    let temp_id = format!("WALKIN-{}", id);
    state.face_store.remove(&temp_id);
    Ok(())
}
