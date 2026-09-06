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

    // Legacy gallery migration: members enrolled before the ArcFace upgrade
    // carry 128-d SFace vectors the 512-d recognizer can never match — the
    // only manual fix was re-scanning every member. The stored reference
    // photo is re-embedded through the CURRENT recognizer in a background
    // thread (one 512-d anchor beats five dead 128-d ones), replacing the
    // gallery in SQLite + the in-memory store. Runs once per launch; the
    // migration is self-terminating because upgraded vectors match dim.
    let engine = std::sync::Arc::new(face_engine);
    {
        let db = db_arc.clone();
        let store = face_store.clone();
        let engine_for_thread = engine.clone();
        std::thread::spawn(move || {
            let Some(engine) = engine_for_thread.as_ref() else { return };
            let dim = engine.embedding_dim();
            std::thread::sleep(std::time::Duration::from_secs(6)); // let boot settle
            let members = match db.list_members() {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!("Legacy gallery migration skipped: {}", e);
                    return;
                }
            };
            let legacy: Vec<_> = members
                .into_iter()
                .filter(|m| !m.face_vectors.is_empty())
                .filter(|m| m.face_vectors.iter().any(|v| v.len() != dim))
                .filter(|m| {
                    m.photo_data_url
                        .as_deref()
                        .map(|p| !p.trim().is_empty())
                        .unwrap_or(false)
                })
                .collect();
            // Seal any remaining plaintext legacy vector rows (at-rest hygiene)
            match db.reseal_plain_face_vectors(dim) {
                Ok(n) if n > 0 => tracing::info!("Re-sealed {} plaintext vector row(s) at rest", n),
                _ => {}
            }

            if legacy.is_empty() {
                return;
            }
            let old_dim = legacy[0]
                .face_vectors
                .first()
                .map(|v| v.len())
                .unwrap_or(0);
            tracing::info!(
                "Legacy gallery migration: {} member(s) ({}-d vectors) -> re-embedding reference photos at {}-d",
                legacy.len(),
                old_dim,
                dim
            );
            let mut migrated = 0usize;
            for m in &legacy {
                let Some(photo) = m.photo_data_url.as_deref() else { continue };
                let image = match crate::vision::decode_base64_image(photo) {
                    Ok(i) => i,
                    Err(e) => {
                        tracing::debug!("Migration {}: undecodable photo ({})", m.id, e);
                        continue;
                    }
                };
                match engine.detect_and_embed(&image) {
                    Ok(Some((_, vec))) if vec.len() == dim => {
                        let vectors = vec![vec];
                        match db.update_member_vectors(&m.id, &vectors, Some(photo)) {
                            Ok(updated) => {
                                store.upsert(
                                    updated.id.clone(),
                                    format!("{} {}", updated.first_name, updated.last_name),
                                    updated.face_vectors.clone(),
                                );
                                migrated += 1;
                                tracing::info!(
                                    "Migrated {} {} to {}-d gallery",
                                    updated.first_name, updated.last_name, dim
                                );
                            }
                            Err(e) => tracing::warn!("Migration {}: DB update failed: {}", m.id, e),
                        }
                    }
                    Ok(_) => tracing::debug!("Migration {}: no face in stored photo — skipped", m.id),
                    Err(e) => tracing::warn!("Migration {}: embed failed: {}", m.id, e),
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            tracing::info!("Legacy gallery migration complete: {migrated}/{} upgraded", legacy.len());
        });
    }

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
        face_engine: engine,
        person_counter: Arc::new(person_counter),
        pin_gate: Arc::new(std::sync::Mutex::new(commands::PinGate::default())),
        tailgate_policy,
        last_tailgate_alarm: Arc::new(std::sync::Mutex::new(None)),
    };

    // ESP32 auto-detect: every 3s, try EVERY candidate USB serial port (VID
    // whitelist: FTDI/CP210x/CH34x/Espressif). connect() PING-verifies each,
    // so with any number of other USB devices plugged in, only the real
    // controller is kept; the branded idle screen is pushed on success, and
    // write failures auto-clear so unplugging self-heals on the next pass.
    {
        let hw = hardware.clone();
        let db = db_arc;
        std::thread::spawn(move || loop {
            if !hw.is_connected() {
                let mut connected_port: Option<String> = None;
                for port in hw.find_esp_ports() {
                    match hw.connect(&port, 115200) {
                        Ok(_) => {
                            connected_port = Some(port);
                            break;
                        }
                        Err(e) => {
                            tracing::debug!("ESP32 candidate {} rejected: {}", port, e)
                        }
                    }
                }
                if let Some(port) = connected_port {
                    tracing::info!("ESP32 auto-connected on {}", port);
                    let brand = db
                        .get_app_settings()
                        .map(|s| s.gym_name)
                        .unwrap_or_else(|_| "GymPOS".to_string());
                    let _ = hw.set_idle_screen(&brand);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
    }

    tauri::Builder::default()
        // Single-instance guard: a second launch focuses the existing window
        // instead of fighting over the cameras and the SQLite database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
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
