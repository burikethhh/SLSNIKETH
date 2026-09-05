use chrono::{DateTime, Utc};
use instant_distance::{Builder, HnswMap, Point as HnswPoint, Search};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Number of nearest-by-centroid candidates retrieved from the HNSW index
/// before falling back to full multi-angle vector comparison (Task 5.3).
/// Larger = safer (less chance of missing a match whose centroid ranks
/// outside the top-K but whose specific angle vector would have matched)
/// at a small extra cost; still O(K), not O(N). Raised 8 -> 16 after the
/// accuracy review: drifted/duplicated centroids can push the true member
/// outside a tight top-8 at roster scale.
const HNSW_CANDIDATE_COUNT: usize = 16;

/// Wraps a member's centroid embedding for HNSW indexing. Distance is cosine
/// distance (1 - cosine similarity), reusing the same SIMD-friendly dot
/// product as the rest of the matching pipeline so index results are
/// consistent with `cosine_similarity_fast` used elsewhere.
#[derive(Clone)]
struct CentroidPoint(Vec<f32>);

impl HnswPoint for CentroidPoint {
    fn distance(&self, other: &Self) -> f32 {
        1.0 - cosine_similarity_fast(&self.0, &other.0)
    }
}

/// L2-Normalizes a vector in-place: v = v / ||v||_2
/// Guards against NaN/Inf — invalid vectors are zeroed and left unnormalized.
pub fn l2_normalize(vec: &mut [f32]) {
    if !vec.iter().all(|x| x.is_finite()) {
        // Corrupted embedding (NaN/Inf) — zero out to ensure cosine = 0 (no match)
        for x in vec.iter_mut() {
            *x = 0.0;
        }
        return;
    }
    let sum_sq: f32 = vec.iter().map(|x| x * x).sum();
    if !sum_sq.is_finite() {
        for x in vec.iter_mut() {
            *x = 0.0;
        }
        return;
    }
    let norm = sum_sq.sqrt();
    if norm > 1e-7 && norm.is_finite() {
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }
}

/// Computes the quality/entropy metric of a feature embedding.
/// Returns 0.0-1.0 where higher values indicate richer facial landmark variance.
pub fn calculate_vector_quality(vec: &[f32]) -> f32 {
    if vec.is_empty() {
        return 0.0;
    }
    let mean: f32 = vec.iter().sum::<f32>() / vec.len() as f32;
    let variance: f32 = vec.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / vec.len() as f32;
    let std_dev = variance.sqrt();
    // High standard deviation indicates rich facial landmark feature variance
    (std_dev * 2.5).min(1.0).max(0.0)
}

#[derive(Debug, Clone)]
pub struct FaceEntry {
    pub member_id: String,
    pub member_name: String,
    pub vectors: Vec<Vec<f32>>, // Multi-angle anchors (front, left, right, up, down)
    pub centroid: Vec<f32>,     // Weighted mean embedding for rapid 1-to-N screening
    pub enrolled_centroid: Vec<f32>, // Immutable enrollment snapshot: adapt drift guardrail
    pub adapt_count: u32,       // Successful EMA adapts since enrollment (telemetry)
    pub expires_at: Option<DateTime<Utc>>, // None for regular members, Some(timestamp) for 8-hour walk-ins
}

/// Maximum allowed drift of the live centroid away from the enrollment
/// snapshot, as cosine similarity. Breach reverts the adapt and flags the
/// member for re-scan instead of letting bad lighting/glasses permanently
/// pull the profile.
const ADAPT_DRIFT_MIN_COSINE: f32 = 0.90;

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub member_id: String,
    pub member_name: String,
    pub confidence: f32,
    pub matched_angle_index: usize,
    pub is_expired: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub match_margin: f32, // best score minus runner-up: separation signal for calibration
}

/// Tracks the last successful match to prevent rapid duplicate hardware pulses
#[derive(Debug, Clone)]
struct LastMatchRecord {
    member_id: String,
    timestamp: DateTime<Utc>,
}

#[derive(Clone)]
pub struct FaceVectorStore {
    entries: Arc<RwLock<HashMap<String, FaceEntry>>>,
    last_match: Arc<Mutex<Option<LastMatchRecord>>>,
    /// HNSW index over member centroids (Task 5.3: sub-millisecond search at
    /// scale). Rebuilt lazily on the next `match_vector` call after any write
    /// (upsert/remove/clear/adapt) sets `index_dirty` — writes to a gym's
    /// roster are rare compared to match lookups, so a rebuild-on-next-read
    /// strategy amortizes the (still cheap, O(N log N)) rebuild cost far
    /// better than rebuilding on every single write.
    index: Arc<RwLock<Option<HnswMap<CentroidPoint, String>>>>,
    index_dirty: Arc<AtomicBool>,
    /// Set when the last `match_vector` skipped a candidate purely on
    /// embedding-dimension mismatch. Drained via `take_dim_mismatch()`.
    dim_mismatch_flag: Arc<AtomicBool>,
}

impl FaceVectorStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            last_match: Arc::new(Mutex::new(None)),
            index: Arc::new(RwLock::new(None)),
            index_dirty: Arc::new(AtomicBool::new(false)),
            dim_mismatch_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Drains the dimension-mismatch signal set by the last `match_vector`
    /// call. True means at least one candidate was skipped only because its
    /// stored embedding width differs from the probe width.
    pub fn take_dim_mismatch(&self) -> bool {
        self.dim_mismatch_flag.swap(false, Ordering::AcqRel)
    }

    /// Rebuilds the HNSW centroid index from the current entries if it has
    /// been marked dirty by a write since the last rebuild. No-op otherwise.
    fn rebuild_index_if_dirty(&self) {
        if !self.index_dirty.swap(false, Ordering::AcqRel) {
            return;
        }

        let (points, ids): (Vec<CentroidPoint>, Vec<String>) = {
            let store = self.entries.read();
            store
                .values()
                .filter(|e| !e.centroid.is_empty())
                .map(|e| (CentroidPoint(e.centroid.clone()), e.member_id.clone()))
                .unzip()
        };

        let mut index = self.index.write();
        *index = if points.is_empty() {
            None
        } else {
            Some(Builder::default().build(points, ids))
        };
    }

    /// Removes expired walk-in entries from memory (8-hour purge janitor).
    /// Call periodically (e.g., on each match or via 60s timer) to prevent memory leak & scan slowdown.
    pub fn purge_expired(&self) -> usize {
        let now = Utc::now();
        let mut store = self.entries.write();
        let before = store.len();
        store.retain(|_, entry| {
            if let Some(exp) = entry.expires_at {
                now <= exp
            } else {
                true
            }
        });
        let removed = before.saturating_sub(store.len());
        if removed > 0 {
            drop(store);
            self.index_dirty.store(true, Ordering::Release);
        }
        removed
    }

    /// Load or upsert a member's face vectors into memory (permanent)
    pub fn upsert(&self, member_id: String, member_name: String, vectors: Vec<Vec<f32>>) {
        self.upsert_with_expiry(member_id, member_name, vectors, None);
    }

    /// Load or upsert face vectors with L2 normalization, centroid clustering, and optional expiry
    /// Rejects NaN/Inf vectors — returns without inserting if any vector is corrupted.
    pub fn upsert_with_expiry(
        &self,
        member_id: String,
        member_name: String,
        mut vectors: Vec<Vec<f32>>,
        expires_at: Option<DateTime<Utc>>,
    ) {
        // Sanitize: reject any vector containing NaN/Inf or absurd magnitude
        for vec in &vectors {
            if !vec.iter().all(|x| x.is_finite() && x.abs() < 1e4) || vec.is_empty() {
                tracing::warn!(
                    "Rejecting upsert for {}: vector contains NaN/Inf or empty",
                    member_id
                );
                return;
            }
        }
        // 1. L2 Normalize all input angle vectors
        for vec in vectors.iter_mut() {
            l2_normalize(vec);
        }

        // 2. Compute Centroid Vector (Average Representation)
        let centroid = if !vectors.is_empty() {
            let dim = vectors[0].len();
            let mut mean_vec = vec![0.0f32; dim];
            for v in &vectors {
                for (i, val) in v.iter().enumerate() {
                    if i < dim {
                        mean_vec[i] += val;
                    }
                }
            }
            l2_normalize(&mut mean_vec);
            mean_vec
        } else {
            vec![]
        };

        let mut store = self.entries.write();
        store.insert(
            member_id.clone(),
            FaceEntry {
                member_id,
                member_name,
                vectors,
                centroid: centroid.clone(),
                enrolled_centroid: centroid,
                adapt_count: 0,
                expires_at,
            },
        );
        drop(store);
        self.index_dirty.store(true, Ordering::Release);
    }

    /// Adaptively updates the stored embedding when a member unlocks with high confidence (Continuous Learning).
    /// Guardrailed: after the EMA step the new centroid must still be within
    /// ADAPT_DRIFT_MIN_COSINE of the enrollment snapshot, otherwise the adapt
    /// is reverted (returns false) so bad lighting/glasses/beards cannot
    /// permanently pull the profile. Returns true when the adapt stuck.
    pub fn adapt_profile(&self, member_id: &str, live_probe: &[f32], alpha: f32) -> bool {
        let mut store = self.entries.write();
        if let Some(entry) = store.get_mut(member_id) {
            if !entry.centroid.is_empty() && entry.centroid.len() == live_probe.len() {
                // Exponential Moving Average: centroid_new = (1 - alpha)*centroid_old + alpha*probe
                let mut candidate = entry.centroid.clone();
                for (c, p) in candidate.iter_mut().zip(live_probe.iter()) {
                    *c = (1.0 - alpha) * (*c) + alpha * (*p);
                }
                l2_normalize(&mut candidate);
                let drift = if entry.enrolled_centroid.len() == candidate.len() && !entry.enrolled_centroid.is_empty() {
                    cosine_similarity_fast(&candidate, &entry.enrolled_centroid)
                } else {
                    1.0
                };
                if drift < ADAPT_DRIFT_MIN_COSINE {
                    tracing::warn!(
                        "Adapt reverted for {}: drift cosine {:.3} below {:.2} — flag for re-scan",
                        entry.member_name, drift, ADAPT_DRIFT_MIN_COSINE
                    );
                    return false;
                }
                entry.centroid = candidate;
                entry.adapt_count = entry.adapt_count.saturating_add(1);
                drop(store);
                self.index_dirty.store(true, Ordering::Release);
                return true;
            }
        }
        false
    }

    /// Get a cloned entry by member_id
    pub fn get_entry(&self, member_id: &str) -> Option<FaceEntry> {
        let store = self.entries.read();
        store.get(member_id).cloned()
    }

    /// Remove a member's face vectors from memory
    pub fn remove(&self, member_id: &str) {
        let mut store = self.entries.write();
        store.remove(member_id);
        drop(store);
        self.index_dirty.store(true, Ordering::Release);
    }

    /// Clear all loaded vectors
    pub fn clear(&self) {
        let mut store = self.entries.write();
        store.clear();
        drop(store);
        *self.index.write() = None;
        self.index_dirty.store(false, Ordering::Release);
    }

    /// Returns the number of enrolled face profiles currently in memory
    pub fn count(&self) -> usize {
        self.entries.read().len()
    }

    /// Match a probe vector against all registered vectors using Cosine Metric
    /// with Centroid + Multi-Angle screening, probe quality gating, and duplicate-scan cooldown.
    pub fn match_vector(&self, probe: &[f32], threshold: f32) -> Option<MatchResult> {
        if probe.is_empty() {
            return None;
        }

        // Quality gate: reject flat/corrupted probe embeddings for full feature vectors.
        // Threshold scales with dimension: a uniform-on-sphere embedding has
        // std ~ 1/sqrt(d), so 1.7/sqrt(d) reproduces the legacy 0.15 gate at
        // d=128 exactly (1.7/11.31 = 0.150) while admitting genuine 512-d
        // ArcFace embeddings (typical quality ~0.11, gate ~0.075). Flat
        // vectors (std ~ 0) are still rejected at any dimension.
        if probe.len() >= 16 {
            let probe_quality = calculate_vector_quality(probe);
            let min_quality = 1.7 / (probe.len() as f32).sqrt();
            if probe_quality < min_quality {
                tracing::debug!(
                    "Probe rejected: quality {:.3} below minimum threshold {:.3} (dim {})",
                    probe_quality,
                    min_quality,
                    probe.len()
                );
                return None;
            }
        }

        // Normalize probe vector
        let mut normalized_probe = probe.to_vec();
        l2_normalize(&mut normalized_probe);

        let now = Utc::now();
        self.rebuild_index_if_dirty();

        // Note: expired walk-ins are retained briefly to allow `is_expired` reporting
        // (see test_walk_in_8_hour_expiry). Purge is explicit via `purge_expired()` called
        // periodically by a 60s janitor timer (not on every match) to avoid breaking that contract.
        let store = self.entries.read();
        let mut best_match: Option<MatchResult> = None;
        let mut highest_score: f32 = threshold;
        let mut runner_up: f32 = 0.0;
        let mut best_member_id: Option<String> = None;
        // Set when a candidate was skipped purely on embedding-dimension
        // mismatch (e.g. legacy 128-d gallery vs 512-d probe). Surfaced via
        // take_dim_mismatch() so the UI can say "needs re-enrollment"
        // instead of the misleading "Face not recognized".
        let mut dim_mismatch_seen = false;

        // Task 5.3: instead of a full O(N) scan over every enrolled member,
        // retrieve only the top-K nearest-by-centroid candidates from the
        // HNSW index, then run the same precise multi-angle comparison as
        // before on just those candidates. Falls back to scanning every
        // entry when the index hasn't been built yet (e.g. right after the
        // very first insert, before any match has triggered a rebuild).
        let candidate_ids: Vec<String> = {
            let index_guard = self.index.read();
            match index_guard.as_ref() {
                Some(index) => {
                    let query = CentroidPoint(normalized_probe.clone());
                    let mut search = Search::default();
                    index
                        .search(&query, &mut search)
                        .take(HNSW_CANDIDATE_COUNT)
                        .map(|item| item.value.clone())
                        .collect()
                }
                None => store.keys().cloned().collect(),
            }
        };

        for member_id in &candidate_ids {
            let Some(entry) = store.get(member_id) else {
                continue;
            };
            let is_expired = entry.expires_at.map_or(false, |exp| now > exp);

            // Dimension-mismatch signal: entry stores a different embedding
            // width than the probe (e.g. legacy 128-d gallery vs 512-d
            // probe). cosine returns 0.0 for these, so flag it for the
            // "needs re-enrollment" UX instead of silent non-match.
            let entry_dim = entry.vectors.first().map(|v| v.len()).unwrap_or(0);
            if entry_dim != 0 && entry_dim != probe.len() {
                dim_mismatch_seen = true;
            }

            // Runner-up tracks the best score from a DIFFERENT member so the
            // margin measures identification separation, not the winner's
            // own angle gallery.
            let mut note_score = |score: f32,
                                  member_id: &str,
                                  highest_score: &mut f32,
                                  runner_up: &mut f32,
                                  best_member_id: &mut Option<String>|
             -> bool {
                if score > *highest_score {
                    if best_member_id.as_deref() != Some(member_id) {
                        *runner_up = runner_up.max(*highest_score);
                    }
                    *highest_score = score;
                    *best_member_id = Some(member_id.to_string());
                    true
                } else {
                    if best_member_id.as_deref() != Some(member_id) {
                        *runner_up = runner_up.max(score);
                    }
                    false
                }
            };

            // Fast Pre-screening via Centroid Vector
            let centroid_score = if !entry.centroid.is_empty() {
                cosine_similarity_fast(&normalized_probe, &entry.centroid)
            } else {
                0.0
            };

            // If centroid matches well, update best match
            if note_score(
                centroid_score,
                &entry.member_id,
                &mut highest_score,
                &mut runner_up,
                &mut best_member_id,
            ) {
                best_match = Some(MatchResult {
                    member_id: entry.member_id.clone(),
                    member_name: entry.member_name.clone(),
                    confidence: centroid_score,
                    matched_angle_index: 0,
                    is_expired,
                    expires_at: entry.expires_at,
                    match_margin: 0.0, // filled in below
                });
            }

            // Full multi-angle vector comparison
            for (idx, target_vec) in entry.vectors.iter().enumerate() {
                let score = cosine_similarity_fast(&normalized_probe, target_vec);
                if note_score(
                    score,
                    &entry.member_id,
                    &mut highest_score,
                    &mut runner_up,
                    &mut best_member_id,
                ) {
                    best_match = Some(MatchResult {
                        member_id: entry.member_id.clone(),
                        member_name: entry.member_name.clone(),
                        confidence: score,
                        matched_angle_index: idx,
                        is_expired,
                        expires_at: entry.expires_at,
                        match_margin: 0.0, // filled in below
                    });
                }
            }
        }

        if dim_mismatch_seen {
            self.dim_mismatch_flag.store(true, Ordering::Release);
        }
        if let Some(ref mut m) = best_match {
            m.match_margin = (m.confidence - runner_up).max(0.0);
        }

        // Duplicate-scan cooldown (atomic): if the same member was matched within 3 seconds, suppress
        // Uses Mutex for atomic check-and-set, eliminating race under concurrent frame evaluation.
        if let Some(ref matched) = best_match {
            let mut last = self.last_match.lock();
            if let Some(ref record) = *last {
                if record.member_id == matched.member_id {
                    let elapsed = (now - record.timestamp).num_milliseconds();
                    if elapsed < 3000 {
                        tracing::debug!(
                            "Duplicate scan suppressed for {} ({}ms since last match)",
                            matched.member_name,
                            elapsed
                        );
                        return None;
                    }
                }
            }
            // Record this match atomically
            *last = Some(LastMatchRecord {
                member_id: matched.member_id.clone(),
                timestamp: now,
            });
        }

        best_match
    }
}

/// Compute cosine similarity between two L2-normalized float vectors.
/// Uses 4-wide unrolled accumulation for SIMD-friendly throughput on
/// arbitrary-length embeddings (128-d SFace: 128 -> 32 iterations;
/// 512-d ArcFace: 512 -> 128 iterations).
pub fn cosine_similarity_fast(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let len = a.len();
    let chunks = len / 4;
    let remainder = len % 4;

    let mut acc0 = 0.0f32;
    let mut acc1 = 0.0f32;
    let mut acc2 = 0.0f32;
    let mut acc3 = 0.0f32;

    // 4-wide unrolled dot product
    for i in 0..chunks {
        let base = i * 4;
        acc0 += a[base] * b[base];
        acc1 += a[base + 1] * b[base + 1];
        acc2 += a[base + 2] * b[base + 2];
        acc3 += a[base + 3] * b[base + 3];
    }

    // Handle remaining elements
    let base = chunks * 4;
    for j in 0..remainder {
        acc0 += a[base + j] * b[base + j];
    }

    let dot_product = acc0 + acc1 + acc2 + acc3;
    if !dot_product.is_finite() {
        return 0.0;
    }
    dot_product.max(0.0).min(1.0)
}

/// Legacy compatibility wrapper
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    cosine_similarity_fast(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![1.0, 2.0, 3.0];
        let v2 = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity_fast(&v1, &v2);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity_fast(&v1, &v2);
        assert!((sim - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_128d_unrolled() {
        // Verify 4-wide unrolled accumulation gives correct result for 128-d vectors
        let mut v1 = vec![0.0f32; 128];
        let mut v2 = vec![0.0f32; 128];
        for i in 0..128 {
            v1[i] = (i as f32 * 0.1).sin();
            v2[i] = (i as f32 * 0.1).sin();
        }
        l2_normalize(&mut v1);
        l2_normalize(&mut v2);
        let sim = cosine_similarity_fast(&v1, &v2);
        assert!((sim - 1.0).abs() < 1e-4, "Expected ~1.0, got {}", sim);
    }

    #[test]
    fn test_probe_quality_gating() {
        let store = FaceVectorStore::new();
        store.upsert(
            "MEM-001".to_string(),
            "Test User".to_string(),
            vec![vec![0.5, 0.5, 0.5, 0.5]],
        );

        // A flat/degenerate probe should be rejected by quality gate
        let flat_probe = vec![0.1; 128];
        let match_res = store.match_vector(&flat_probe, 0.60);
        // flat_probe has near-zero variance, quality gate should reject it
        assert!(
            match_res.is_none(),
            "Flat probe should be rejected by quality gate"
        );
    }

    #[test]
    fn test_quality_gate_scales_to_512d_arcface() {
        // Genuine 512-d embeddings (L2-normalized, std ~0.044, quality ~0.11)
        // must PASS the dimension-scaled gate (~0.075), while flat 512-d
        // probes (std ~ 0) must still be REJECTED.
        let store = FaceVectorStore::new();
        let mut genuine: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
        l2_normalize(&mut genuine);
        let q = calculate_vector_quality(&genuine);
        assert!(
            q >= 1.7 / (512.0f32).sqrt(),
            "genuine 512-d probe quality {:.3} should pass scaled gate",
            q
        );
        store.upsert(
            "MEM-512".to_string(),
            "ArcFace User".to_string(),
            vec![genuine.clone()],
        );
        let hit = store.match_vector(&genuine, 0.68);
        assert!(hit.is_some(), "512-d genuine probe should match at 0.68");

        let flat_512 = vec![0.1; 512];
        assert!(
            store.match_vector(&flat_512, 0.68).is_none(),
            "Flat 512-d probe should be rejected by scaled quality gate"
        );
    }

    #[test]
    fn test_vector_store_matching() {
        let store = FaceVectorStore::new();
        let vector_front = vec![0.5, 0.5, 0.5, 0.5];
        let vector_left = vec![0.6, 0.4, 0.5, 0.5];

        store.upsert(
            "MEM-001".to_string(),
            "John Doe".to_string(),
            vec![vector_front.clone(), vector_left],
        );

        let probe = vec![0.5, 0.5, 0.5, 0.5];
        let match_res = store.match_vector(&probe, 0.60);

        assert!(match_res.is_some());
        let result = match_res.unwrap();
        assert_eq!(result.member_id, "MEM-001");
        assert_eq!(result.member_name, "John Doe");
        assert!((result.confidence - 1.0).abs() < 1e-5);
        assert!(!result.is_expired);
    }

    #[test]
    fn test_walk_in_8_hour_expiry() {
        let store = FaceVectorStore::new();
        let vector = vec![0.8, 0.6];
        let expired_time = Utc::now() - chrono::Duration::hours(1);

        store.upsert_with_expiry(
            "WALKIN-001".to_string(),
            "Guest John".to_string(),
            vec![vector.clone()],
            Some(expired_time),
        );

        let match_res = store.match_vector(&vector, 0.60);
        assert!(match_res.is_some());
        assert!(match_res.unwrap().is_expired);
    }

    #[test]
    fn test_duplicate_scan_cooldown() {
        let store = FaceVectorStore::new();
        let vector = vec![0.5, 0.5, 0.5, 0.5];
        store.upsert(
            "MEM-002".to_string(),
            "Jane Smith".to_string(),
            vec![vector.clone()],
        );

        // First match should succeed
        let first = store.match_vector(&vector, 0.60);
        assert!(first.is_some(), "First match should succeed");

        // Immediate second match should be suppressed by 3s cooldown
        let second = store.match_vector(&vector, 0.60);
        assert!(
            second.is_none(),
            "Duplicate scan within 3s should be suppressed"
        );
    }

    #[test]
    fn test_hnsw_index_invalidated_after_removal() {
        let store = FaceVectorStore::new();
        let v1 = vec![0.9, 0.1, 0.1, 0.1];
        let v2 = vec![0.1, 0.9, 0.1, 0.1];
        store.upsert("MEM-A".to_string(), "Alice".to_string(), vec![v1.clone()]);
        store.upsert("MEM-B".to_string(), "Bob".to_string(), vec![v2.clone()]);

        // Triggers the first index build.
        let hit = store.match_vector(&v1, 0.60);
        assert_eq!(hit.unwrap().member_id, "MEM-A");

        // Remove Alice, then immediately probe with her exact vector again.
        // If the index were stale (not rebuilt), a naive implementation could
        // still surface a removed member from a cached index; this proves
        // the dirty-flag rebuild-on-next-read path actually runs.
        store.remove("MEM-A");
        let after_removal = store.match_vector(&v1, 0.60);
        assert!(
            after_removal.is_none(),
            "Removed member must not be matchable after removal, got {:?}",
            after_removal
        );

        // Bob should still be findable.
        let bob = store.match_vector(&v2, 0.60);
        assert_eq!(bob.unwrap().member_id, "MEM-B");
    }

    #[test]
    fn test_hnsw_finds_correct_match_among_many_members() {
        // With HNSW_CANDIDATE_COUNT == 16, a store with 50 members means most
        // entries are NOT in every top-K candidate set by construction — this
        // proves the HNSW search itself (not just the fallback full-scan
        // path for tiny stores) is retrieving the right candidate.
        let store = FaceVectorStore::new();
        for i in 0..50 {
            // Spread points around the unit hypersphere in a 16-d space so
            // each member has a distinct, well-separated centroid.
            let mut v = vec![0.0f32; 16];
            v[i % 16] = 1.0;
            v[(i + 1) % 16] = 0.5 + (i as f32 * 0.01);
            store.upsert(format!("MEM-{:03}", i), format!("Member {}", i), vec![v]);
        }

        // Probe with member #37's exact (post-normalization) vector.
        let target_id = "MEM-037";
        let target_entry = store.get_entry(target_id).expect("seeded above");
        let probe = target_entry.vectors[0].clone();

        let result = store.match_vector(&probe, 0.60);
        let matched = result.expect("should find an exact match among 50 members");
        assert_eq!(matched.member_id, target_id);
        assert!(
            matched.confidence > 0.99,
            "exact match should score near 1.0, got {}",
            matched.confidence
        );
    }

    #[test]
    fn test_match_reports_margin_against_runner_up() {
        // Two members: exact self-match must win with margin = best - second.
        let store = FaceVectorStore::new();
        let mut v1 = vec![0.0f32; 16];
        v1[0] = 1.0;
        let mut v2 = vec![0.0f32; 16];
        v2[0] = 0.9;
        v2[1] = 0.1;
        store.upsert("MEM-A".to_string(), "Alice".to_string(), vec![v1.clone()]);
        store.upsert("MEM-B".to_string(), "Bob".to_string(), vec![v2]);
        let probe = store.get_entry("MEM-A").unwrap().vectors[0].clone();
        let m = store.match_vector(&probe, 0.60).expect("exact match");
        assert_eq!(m.member_id, "MEM-A");
        assert!(
            m.match_margin > 0.0,
            "margin must be positive when a runner-up exists, got {}",
            m.match_margin
        );
    }

    #[test]
    fn test_adapt_reverts_on_excessive_drift() {
        // Enroll at one pole of the space, then try to drag the centroid to
        // the opposite pole: drift guard must revert and return false.
        let store = FaceVectorStore::new();
        let mut v = vec![0.0f32; 16];
        v[0] = 1.0;
        store.upsert("MEM-A".to_string(), "Alice".to_string(), vec![v]);
        let mut hostile = vec![0.0f32; 16];
        hostile[5] = 1.0;
        // Small alpha steps toward hostile stay within drift: allowed.
        assert!(store.adapt_profile("MEM-A", &hostile, 0.01));
        // A full jump to the hostile pole breaches cosine 0.90: reverted.
        assert!(!store.adapt_profile("MEM-A", &hostile, 1.0));
        // Centroid must still match the enrollment neighborhood.
        let probe = store.get_entry("MEM-A").unwrap().vectors[0].clone();
        let m = store.match_vector(&probe, 0.60).expect("still matches self");
        assert_eq!(m.member_id, "MEM-A");
    }

    #[test]
    fn test_dim_mismatch_flags_reenroll() {
        // Legacy 128-d gallery vs 512-d probe: no match, but the mismatch
        // flag must trip so the UI can say "needs re-enrollment".
        let store = FaceVectorStore::new();
        store.upsert(
            "MEM-OLD".to_string(),
            "Old".to_string(),
            vec![vec![0.1f32; 128]],
        );
        // Non-flat probe (flat vectors are rejected by the quality gate
        // before dimension checks run).
        let probe: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
        assert!(store.match_vector(&probe, 0.60).is_none());
        assert!(
            store.take_dim_mismatch(),
            "dimension mismatch should have been flagged"
        );
        assert!(
            !store.take_dim_mismatch(),
            "flag must drain after one read"
        );
    }
}
