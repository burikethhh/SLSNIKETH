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
    // Logging: the crate emits tracing::info/warn everywhere (sync worker,
    // license verification, activation) but never initialized a subscriber,
    // so every log was a silent no-op. Init a simple fmt subscriber writing
    // to stdout — when launched with `GymPOS.exe > out.log 2>&1` this gives a
    // real diagnostic trail; INFO covers all operational paths.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Resolve SQLite database path: in development or user mode, use exe directory;
    // if exe directory is read-only (e.g. Program Files), fall back to %LOCALAPPDATA%\GymPOS.
    let db_path = {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let candidate = exe_dir.join("gympos_local.sqlite");

        let is_writable = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(true)
            .open(&candidate)
            .is_ok();

        if is_writable {
            candidate
        } else if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let app_data_dir = std::path::PathBuf::from(local_app_data).join("GymPOS");
            let _ = std::fs::create_dir_all(&app_data_dir);
            app_data_dir.join("gympos_local.sqlite")
        } else {
            candidate
        }
    };
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

    // Face janitor (60s interval) is scheduled once below after the sync
    // worker starts — keep a single scheduler so expiry purges don't double.

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

    // Start background cloud sync loop (shares the tailgate policy handle so
    // Phase-D remote enable/cooldown lands without a restart).
    let tailgate_policy = Arc::new(parking_lot::RwLock::new(gympos_shared::TailgatePolicy::default()));
    let sync_worker = CloudSyncWorker::new(db_arc.clone(), license_arc.clone(), None)
        .with_policy_sink(tailgate_policy.clone());
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

    let initial_session = if license_arc.current_status().is_operable() {
        db_arc.get_saved_terminal_session().unwrap_or(None)
    } else {
        None
    };

    let hardware = HardwareManager::new();
    let app_context = AppContext {
        db: db_arc.clone(),
        license: license_arc,
        hardware: hardware.clone(),
        face_store,
        session: Arc::new(parking_lot::RwLock::new(initial_session)),
        face_engine: Arc::new(face_engine),
        person_counter: Arc::new(person_counter),
        pin_gate: Arc::new(std::sync::Mutex::new(commands::PinGate::default())),
        tailgate_policy,
        last_tailgate_alarm: Arc::new(std::sync::Mutex::new(None)),
    };

    // ESP32 auto-detect: every 3s, connect to the controller automatically
    // when it appears on a USB serial port (VID whitelist: FTDI/CP210x/CH34x/
    // Espressif), and push the owner-branded idle screen. Write failures
    // auto-clear the connection, so unplugging self-heals on the next pass.
    {
        let hw = hardware.clone();
        let db = db_arc;
        std::thread::spawn(move || loop {
            if !hw.is_connected() {
                if let Some(port) = hw.find_esp_port() {
                    match hw.connect(&port, 115200) {
                        Ok(_) => {
                            tracing::info!("ESP32 auto-connected on {}", port);
                            let brand = db
                                .get_app_settings()
                                .map(|s| s.gym_name)
                                .unwrap_or_else(|_| "GymPOS".to_string());
                            let _ = hw.set_idle_screen(&brand);
                        }
                        Err(e) => {
                            tracing::debug!("ESP32 auto-connect on {} failed: {}", port, e)
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        });
    }

    tauri::Builder::default()
        // GitHub-Releases auto-updater (signed; see updater.rs + tauri.conf
        // plugins.updater). Registered before manage() so commands can use it.
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            commands::list_tailgate_incidents,
            commands::resolve_tailgate_incident,
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
            commands::activate_terminal_owner,
            commands::authenticate_owner,
            commands::owner_login_preview,
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
