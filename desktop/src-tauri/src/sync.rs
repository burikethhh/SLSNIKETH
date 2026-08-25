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

    /// Spawns the background synchronization loop with real-time kill-switch polling & inter-branch sync.
    /// Uses exponential backoff on consecutive failures (5s -> 15s -> 30s -> 60s cap).
    pub fn start_background_sync(self) {
        tauri::async_runtime::spawn(async move {
            info!("Cloud sync background worker active targeting: {}", self.cloud_url);
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_default();

            let mut consecutive_failures: u32 = 0;
            let base_interval_secs: u64 = 5;

            loop {
                // Exponential backoff: 5s -> 15s -> 30s -> 60s cap
                let backoff_secs = match consecutive_failures {
                    0 => base_interval_secs,
                    1 => 15,
                    2 => 30,
                    _ => 60,
                };
                sleep(Duration::from_secs(backoff_secs)).await;

                if let Some(claims) = self.license.current_claims() {
                    // 1. Gather unsynced local members (registered at this branch)
                    let unsynced_members = self
                        .db
                        .get_unsynced_members(&claims.owner_email, &claims.gym_id, &claims.gym_name)
                        .unwrap_or_default();
                    let unsynced_member_ids: Vec<String> = unsynced_members.iter().map(|m| m.id.clone()).collect();

                    // 2. Gather unsynced attendance records
                    let unsynced_att = self.db.get_unsynced_attendance().unwrap_or_default();
                    let unsynced_att_ids: Vec<String> = unsynced_att.iter().map(|a| a.id.clone()).collect();

                    // 3. Skip empty payloads — no need to POST when nothing changed
                    if unsynced_members.is_empty() && unsynced_att.is_empty() {
                        // Still reset backoff on idle cycles (network is reachable, just nothing to send)
                        if consecutive_failures > 0 {
                            consecutive_failures = 0;
                        }
                        continue;
                    }

                    let sync_url = format!("{}/api/v1/sync/push", self.cloud_url.trim_end_matches('/'));

                    let payload = SyncPushPayload {
                        gym_id: claims.gym_id,
                        gym_name: claims.gym_name.clone(),
                        owner_email: claims.owner_email.clone(),
                        timestamp: chrono::Utc::now(),
                        attendance_logs: unsynced_att,
                        members: unsynced_members,
                        face_vectors: vec![],
                        sales: vec![],
                    };

                    match client.post(&sync_url).json(&payload).send().await {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                // Reset backoff on success
                                consecutive_failures = 0;

                                if let Ok(body) = resp.json::<SyncResponse>().await {
                                    // A. Mark local items as synced
                                    if !unsynced_member_ids.is_empty() {
                                        let _ = self.db.mark_members_synced(&unsynced_member_ids);
                                    }
                                    if !unsynced_att_ids.is_empty() {
                                        let _ = self.db.mark_attendance_synced(&unsynced_att_ids);
                                    }

                                    // B. Ingest sister branch members and face vectors
                                    if !body.sister_branch_members.is_empty() {
                                        let ingested = self
                                            .db
                                            .upsert_interbranch_members(&body.sister_branch_members)
                                            .unwrap_or(0);
                                        if ingested > 0 {
                                            info!(
                                                "Inter-branch sync: Ingested {} members/vectors from sister branches for owner {}",
                                                ingested, claims.owner_email
                                            );
                                        }
                                    }

                                    // C. Check kill-switch
                                    if body.remote_disabled {
                                        warn!("REMOTE KILL SWITCH ACTIVATED: CEO Command Center disabled gym {}. Revoking local license.", claims.gym_name);
                                        self.license.revoke();
                                        let _ = self.db.clear_cached_license();
                                    }
                                }
                            } else {
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                tracing::debug!(
                                    "Cloud sync HTTP error (status {}), backoff stage: {}",
                                    resp.status(), consecutive_failures
                                );
                            }
                        }
                        Err(e) => {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                            tracing::debug!(
                                "Cloud sync heartbeat pending (attempt {}): {}",
                                consecutive_failures, e
                            );
                        }
                    }
                }
            }
        });
    }
}

