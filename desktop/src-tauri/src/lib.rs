pub mod commands;
pub mod db;
pub mod face;
pub mod hardware;
pub mod license;
pub mod sync;
pub mod updater;
pub mod vision;

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

    // Real 60s janitor for expired walk-in vectors (purge_expired was
    // previously documented but never scheduled anywhere).
    {
        let janitor_store = face_store.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                let removed = janitor_store.purge_expired();
                if removed > 0 {
                    tracing::info!("Face janitor purged {} expired walk-in entries", removed);
                }
            }
        });
    }

    let face_engine = match vision::find_models_dir() {
        Some(dir) => match vision::FaceEngine::load(&dir) {
            Ok(engine) => {
                tracing::info!("Loaded ONNX face detection/recognition models from {:?}", dir);
                Some(engine)
            }
            Err(e) => {
                tracing::error!("Failed to load ONNX face models from {:?}: {}", dir, e);
                None
            }
        },
        None => {
            tracing::warn!("Could not locate desktop/models/*.onnx — real face scanning (scan_face_frame) will be unavailable.");
            None
        }
    };

    let person_counter = match vision::find_models_dir() {
        Some(dir) => match vision::PersonCounter::load(&dir) {
            Ok(c) => {
                tracing::info!("Loaded yolov8n.onnx person counter from {:?}", dir);
                Some(c)
            }
            Err(e) => {
                tracing::error!("Failed to load yolov8n.onnx from {:?}: {}", dir, e);
                None
            }
        },
        None => {
            tracing::warn!("Could not locate desktop/models/yolov8n.onnx — tailgate person counting will be unavailable.");
            None
        }
    };

    let db_arc = Arc::new(db);
    let license_arc = Arc::new(license);

    // Start background cloud sync loop
    let sync_worker = CloudSyncWorker::new(db_arc.clone(), license_arc.clone(), None);
    sync_worker.start_background_sync();

    // Background 60s janitor: purge expired walk-in profiles from memory
    let face_store_janitor = face_store.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let purged = face_store_janitor.purge_expired();
            if purged > 0 {
                tracing::info!("Janitor: purged {} expired walk-in profile(s) from memory", purged);
            }
        }
    });

    let app_context = AppContext {
        db: db_arc,
        license: license_arc,
        hardware: HardwareManager::new(),
        face_store,
        session: Arc::new(parking_lot::RwLock::new(None)),
        face_engine: Arc::new(face_engine),
        person_counter: Arc::new(person_counter),
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
            commands::list_interbranch_members,
            commands::get_member,
            commands::register_member,
            commands::update_member,
            commands::delete_member,
            commands::renew_member,
            commands::freeze_member,
            commands::unfreeze_member,
            commands::rescan_member_face,
            commands::get_member_stats,
            commands::create_expense,
            commands::list_expenses,
            commands::delete_expense,
            commands::get_end_of_day,
            commands::process_walk_in,
            commands::list_walk_ins,
            commands::extend_walk_in,
            commands::renew_walk_in,
            commands::void_walk_in,
            commands::scan_face_frame,
            commands::count_persons_in_frame,
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
            commands::check_for_updates,
            commands::download_and_install_update,
            commands::get_app_version,
            commands::authenticate_staff_pin,
            commands::authenticate_owner,
            commands::poll_hardware_buttons,
            commands::list_remote_plans,
            commands::list_remote_promos,
            commands::get_terminal_session,
            commands::get_license_key_diagnostics,
            commands::logout_terminal_session,
            commands::list_terminal_staff,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GymPOS tauri application");
}
