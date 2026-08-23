use gympos_shared::{SyncPushPayload, SyncResponse};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::db::Database;
use crate::license::LicenseManager;

pub struct CloudSyncWorker {
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

    /// Spawns the background synchronization loop with real-time kill-switch polling
    pub fn start_background_sync(self) {
        tauri::async_runtime::spawn(async move {
            info!("Cloud sync background worker active targeting: {}", self.cloud_url);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default();

            loop {
                // Poll every 5 seconds for responsive real-time fleet commands
                sleep(Duration::from_secs(5)).await;

                if let Some(claims) = self.license.current_claims() {
                    let sync_url = format!("{}/api/v1/sync/push", self.cloud_url.trim_end_matches('/'));
                    let payload = SyncPushPayload {
                        gym_id: claims.gym_id,
                        timestamp: chrono::Utc::now(),
                        attendance_logs: vec![],
                        face_vectors: vec![],
                        sales: vec![],
                    };

                    match client.post(&sync_url).json(&payload).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                if let Ok(body) = resp.json::<SyncResponse>().await {
                                    if body.remote_disabled {
                                        warn!("🛑 REMOTE KILL SWITCH: CEO Command Center disabled gym {}. Revoking local license.", claims.gym_name);
                                        self.license.revoke();
                                        let _ = self.db.clear_cached_license();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Cloud sync heartbeat pending: {}", e);
                        }
                    }
                }
            }
        });
    }
}
