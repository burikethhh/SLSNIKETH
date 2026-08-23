pub mod commands;
pub mod db;
pub mod face;
pub mod hardware;
pub mod license;
pub mod sync;

use commands::AppContext;
use db::Database;
use face::FaceVectorStore;
use hardware::HardwareManager;
use license::LicenseManager;
use std::sync::Arc;
use sync::CloudSyncWorker;

pub fn run() {
    let db_path = "gympos_local.sqlite";
    let db = Database::new(db_path).expect("Failed to initialize SQLite database");
    let license = LicenseManager::new(None);
    let face_store = FaceVectorStore::new();

    // If a license was previously cached in SQLite, attempt to restore it
    if let Ok(Some(cached_key)) = db.get_cached_license() {
        let _ = license.verify_and_apply(&cached_key);
    }

    // Pre-populate in-memory FaceVectorStore from SQLite members
    if let Ok(members) = db.list_members() {
        for m in members {
            if !m.face_vectors.is_empty() {
                let full_name = format!("{} {}", m.first_name, m.last_name);
                face_store.upsert(m.id, full_name, m.face_vectors);
            }
        }
    }

    // Pre-populate in-memory FaceVectorStore with active unexpired walk-in passes
    if let Ok(walk_ins) = db.list_active_walk_in_vectors() {
        for (id, guest_name, vector, expires_at) in walk_ins {
            let label = format!("Walk-In: {}", guest_name);
            face_store.upsert_with_expiry(id, label, vec![vector], Some(expires_at));
        }
    }

    let db_arc = Arc::new(db);
    let license_arc = Arc::new(license);

    // Start background cloud sync loop
    let sync_worker = CloudSyncWorker::new(db_arc.clone(), license_arc.clone(), None);
    sync_worker.start_background_sync();

    let app_context = AppContext {
        db: db_arc,
        license: license_arc,
        hardware: HardwareManager::new(),
        face_store,
    };

    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        })
        .manage(app_context)
        .invoke_handler(tauri::generate_handler![
            commands::get_app_settings,
            commands::save_app_settings,
            commands::get_license_status,
            commands::apply_license_key,
            commands::list_com_ports,
            commands::connect_com_port,
            commands::unlock_magnetic_lock,
            commands::trigger_tailgate_alarm,
            commands::get_dashboard_summary,
            commands::list_members,
            commands::get_member,
            commands::register_member,
            commands::update_member,
            commands::delete_member,
            commands::process_walk_in,
            commands::list_walk_ins,
            commands::extend_walk_in,
            commands::void_walk_in,
            commands::process_face_scan,
            commands::log_tailgate_event,
            commands::list_recent_attendance,
            commands::list_products,
            commands::create_product,
            commands::update_product,
            commands::adjust_product_stock,
            commands::delete_product,
            commands::checkout_pos_sale,
            commands::list_coaches,
            commands::create_coach,
            commands::update_coach,
            commands::delete_coach,
            commands::schedule_coach_session,
            commands::list_coach_sessions,
            commands::cancel_coach_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GymPOS tauri application");
}
