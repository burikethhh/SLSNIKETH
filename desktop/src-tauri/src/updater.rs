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

pub const CURRENT_APP_VERSION: &str = "0.1.0";
pub const DEFAULT_UPDATE_CHANNEL: &str = "stable";

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
        let gym_id_str = gym_id.map(|g| g.to_string()).unwrap_or_default();

        let url = format!(
            "{}/api/v1/updates/check?current_version={}&gym_id={}&channel={}",
            self.cloud_url.trim_end_matches('/'),
            CURRENT_APP_VERSION,
            gym_id_str,
            ch
        );

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

        // Verify SHA256 if provided
        if !expected_sha256.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let calculated_hash = format!("{:x}", hasher.finalize());

            if !calculated_hash.eq_ignore_ascii_case(expected_sha256.trim()) {
                return Err(format!(
                    "Integrity check mismatch! Expected SHA-256: {}, Calculated: {}",
                    expected_sha256, calculated_hash
                ));
            }
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
