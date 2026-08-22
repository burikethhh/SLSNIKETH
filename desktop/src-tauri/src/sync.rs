use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

use crate::db::Database;
use crate::license::LicenseManager;

pub struct CloudSyncWorker {
    #[allow(dead_code)]
    pub db: Arc<Database>,
    pub license: Arc<LicenseManager>,
    pub cloud_url: String,
}

impl CloudSyncWorker {
    pub fn new(db: Arc<Database>, license: Arc<LicenseManager>, cloud_url: Option<String>) -> Self {
        Self {
            db,
            license,
            cloud_url: cloud_url.unwrap_or_else(|| "https://gympos-cloud.onrender.com".to_string()),
        }
    }

    /// Spawns the background synchronization loop
    pub fn start_background_sync(self) {
        tauri::async_runtime::spawn(async move {
            info!("Cloud sync background worker started targeting: {}", self.cloud_url);
            loop {
                sleep(Duration::from_secs(60)).await;

                if let Some(claims) = self.license.current_claims() {
                    info!("Running scheduled sync cycle for gym: {}", claims.gym_name);
                    // Sync cycle:
                    // 1. In production, serialize unsynced attendance_logs from self.db
                    // 2. Post to /api/v1/sync/push
                    // 3. If response indicates remote_disabled == true, invalidate local license
                } else {
                    // Gym is currently unlicensed, sleep and retry
                }
            }
        });
    }
}
