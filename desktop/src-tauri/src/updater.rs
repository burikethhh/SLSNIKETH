use gympos_shared::UpdateCheckResponse;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use uuid::Uuid;

pub const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_UPDATE_CHANNEL: &str = "stable";

/// GitHub Releases channel: every tag push `v*` builds the installer via
/// `.github/workflows/release.yml` and publishes `latest.json` + signed
/// archives. This is the PRIMARY update source; the cloud channel below is
/// kept as a silent fallback (and still serves the CEO dashboard).
pub const GITHUB_LATEST_JSON: &str =
    "https://github.com/burikethhh/SLSNIKETH/releases/latest/download/latest.json";

pub struct AutoUpdater {
    client: Client,
    cloud_url: String,
}

impl AutoUpdater {
    pub fn new(cloud_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client, cloud_url }
    }

    pub async fn check_for_updates(
        &self,
        gym_id: Option<Uuid>,
        channel: Option<String>,
    ) -> Result<UpdateCheckResponse, String> {
        let ch = channel.unwrap_or_else(|| DEFAULT_UPDATE_CHANNEL.to_string());
        // Omit gym_id when unlicensed: an empty `gym_id=` breaks Axum's
        // Option<Uuid> parsing (whole request 422s) and would wrongly bypass
        // staged-rollout bucketing on the server.
        let mut url = format!(
            "{}/api/v1/updates/check?current_version={}&channel={}",
            self.cloud_url.trim_end_matches('/'),
            CURRENT_APP_VERSION,
            ch
        );
        if let Some(g) = gym_id {
            url.push_str(&format!("&gym_id={}", g));
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Update check failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Update check server error: HTTP {}", resp.status()));
        }

        let update_info = resp
            .json::<UpdateCheckResponse>()
            .await
            .map_err(|e| format!("Failed to parse update payload: {}", e))?;

        Ok(update_info)
    }

    pub async fn download_and_verify(
        &self,
        download_url: &str,
        expected_sha256: &str,
    ) -> Result<PathBuf, String> {
        let current_exe = env::current_exe().map_err(|e| format!("Failed to get exe path: {}", e))?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| "Failed to get exe directory".to_string())?;

        let temp_update_path = exe_dir.join(".GymPOS_update.tmp");

        let response = self
            .client
            .get(download_url)
            .send()
            .await
            .map_err(|e| format!("Download request error: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Download failed with HTTP {}", response.status()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read binary stream: {}", e))?;

        // SHA-256 is MANDATORY: an update with no recorded hash is refused
        // outright (previously an empty hash silently skipped verification,
        // letting any bytes at the URL execute as the new binary).
        let expected = expected_sha256.trim();
        if expected.is_empty() {
            return Err("Refusing update with no recorded SHA-256 hash".to_string());
        }
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("Refusing update: recorded SHA-256 hash is malformed".to_string());
        }
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let calculated_hash = format!("{:x}", hasher.finalize());

        if !calculated_hash.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "Integrity check mismatch! Expected SHA-256: {}, Calculated: {}",
                expected, calculated_hash
            ));
        }

        let mut file = File::create(&temp_update_path)
            .map_err(|e| format!("Failed to create temporary update file: {}", e))?;
        file.write_all(&bytes)
            .map_err(|e| format!("Failed to write update binary: {}", e))?;

        Ok(temp_update_path)
    }

    pub fn apply_update_and_restart(temp_update_path: &Path) -> Result<(), String> {
        let current_exe = env::current_exe().map_err(|e| format!("Current exe error: {}", e))?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| "Exe parent dir error".to_string())?;
        let exe_name = current_exe
            .file_name()
            .ok_or_else(|| "Exe filename error".to_string())?
            .to_string_lossy()
            .to_string();

        let old_backup = exe_dir.join(format!("{}.old", exe_name));
        let pid = std::process::id();

        // Write atomic Windows update batch helper
        let bat_script = format!(
            r#"@echo off
timeout /t 2 /nobreak >nul
taskkill /PID {pid} /F >nul 2>&1
if exist "{old}" del /f /q "{old}"
move /y "{current}" "{old}" >nul
move /y "{tmp}" "{current}" >nul
start "" "{current}"
del "%~f0"
exit
"#,
            pid = pid,
            current = current_exe.to_string_lossy(),
            old = old_backup.to_string_lossy(),
            tmp = temp_update_path.to_string_lossy(),
        );

        let bat_path = exe_dir.join(".apply_update.bat");
        fs::write(&bat_path, bat_script)
            .map_err(|e| format!("Failed to write update launcher: {}", e))?;

        #[cfg(target_os = "windows")]
        {
            Command::new("cmd")
                .args(["/C", &bat_path.to_string_lossy()])
                .spawn()
                .map_err(|e| format!("Failed to launch update batch: {}", e))?;
        }

        std::process::exit(0);
    }
}

// --- GitHub Releases channel (Tauri updater plugin, minisign-verified) ---

/// Checks `latest.json` on GitHub Releases. Returns `Ok(None)` when already
/// current. Signature verification against the embedded pubkey happens inside
/// `download_and_install` — a forged/unsigned payload never executes.
pub async fn check_github(
    app: &tauri::AppHandle,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| format!("GitHub updater unavailable: {}", e))?;
    updater
        .check()
        .await
        .map_err(|e| format!("GitHub update check failed: {}", e))
}

/// Maps a plugin `Update` onto the existing `UpdateCheckResponse` shape so
/// the UI needs zero changes. `sha256` is empty by design here: trust comes
/// from the minisign signature (verified at install), not a hash string.
pub fn github_to_response(
    update: &tauri_plugin_updater::Update,
    channel: &str,
) -> gympos_shared::UpdateCheckResponse {
    gympos_shared::UpdateCheckResponse {
        update_available: true,
        current_version: update.current_version.clone(),
        latest_version: update.version.clone(),
        channel: channel.to_string(),
        download_url: update.download_url.to_string(),
        sha256: String::new(),
        release_notes: update.body.clone().unwrap_or_default(),
        is_mandatory: false,
        rollout_percentage: 100,
        server_time: chrono::Utc::now(),
    }
}

/// Downloads + signature-verifies + installs the GitHub update, then
/// restarts into it. Diverges (process restarts) on success.
pub async fn download_install_restart(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app
        .updater()
        .map_err(|e| format!("GitHub updater unavailable: {}", e))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("GitHub update check failed: {}", e))?
        .ok_or_else(|| "No GitHub update available".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("GitHub update install failed: {}", e))?;
    app.restart();
}
