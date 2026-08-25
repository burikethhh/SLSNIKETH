use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

/// L2-Normalizes a vector in-place: v = v / ||v||_2
pub fn l2_normalize(vec: &mut [f32]) {
    let sum_sq: f32 = vec.iter().map(|x| x * x).sum();
    let norm = sum_sq.sqrt();
    if norm > 1e-7 {
        for x in vec.iter_mut() {
            *x /= norm;
        }
    }
}

/// Computes the quality/entropy metric of a feature embedding
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
    pub expires_at: Option<DateTime<Utc>>, // None for regular members, Some(timestamp) for 8-hour walk-ins
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub member_id: String,
    pub member_name: String,
    pub confidence: f32,
    pub matched_angle_index: usize,
    pub is_expired: bool,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct FaceVectorStore {
    entries: Arc<RwLock<HashMap<String, FaceEntry>>>,
}

impl FaceVectorStore {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load or upsert a member's face vectors into memory (permanent)
    pub fn upsert(&self, member_id: String, member_name: String, vectors: Vec<Vec<f32>>) {
        self.upsert_with_expiry(member_id, member_name, vectors, None);
    }

    /// Load or upsert face vectors with L2 normalization, centroid clustering, and optional expiry
    pub fn upsert_with_expiry(
        &self,
        member_id: String,
        member_name: String,
        mut vectors: Vec<Vec<f32>>,
        expires_at: Option<DateTime<Utc>>,
    ) {
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
                centroid,
                expires_at,
            },
        );
    }

    /// Adaptively updates the stored embedding when a member unlocks with high confidence (Continuous Learning)
    pub fn adapt_profile(&self, member_id: &str, live_probe: &[f32], alpha: f32) {
        let mut store = self.entries.write();
        if let Some(entry) = store.get_mut(member_id) {
            if !entry.centroid.is_empty() && entry.centroid.len() == live_probe.len() {
                // Exponential Moving Average: centroid_new = (1 - alpha)*centroid_old + alpha*probe
                for (c, p) in entry.centroid.iter_mut().zip(live_probe.iter()) {
                    *c = (1.0 - alpha) * (*c) + alpha * (*p);
                }
                l2_normalize(&mut entry.centroid);
            }
        }
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
    }

    /// Clear all loaded vectors
    pub fn clear(&self) {
        let mut store = self.entries.write();
        store.clear();
    }

    /// Match a probe vector against all registered vectors using Cosine Metric with Centroid + Multi-Angle screening.
    pub fn match_vector(&self, probe: &[f32], threshold: f32) -> Option<MatchResult> {
        if probe.is_empty() {
            return None;
        }

        // Normalize probe vector
        let mut normalized_probe = probe.to_vec();
        l2_normalize(&mut normalized_probe);

        let store = self.entries.read();
        let now = Utc::now();
        let mut best_match: Option<MatchResult> = None;
        let mut highest_score: f32 = threshold;

        for entry in store.values() {
            let is_expired = entry.expires_at.map_or(false, |exp| now > exp);

            // Fast Pre-screening via Centroid Vector
            let centroid_score = if !entry.centroid.is_empty() {
                cosine_similarity(&normalized_probe, &entry.centroid)
            } else {
                0.0
            };

            // If centroid matches well, or check all multi-angle vectors
            if centroid_score > highest_score {
                highest_score = centroid_score;
                best_match = Some(MatchResult {
                    member_id: entry.member_id.clone(),
                    member_name: entry.member_name.clone(),
                    confidence: centroid_score,
                    matched_angle_index: 0,
                    is_expired,
                    expires_at: entry.expires_at,
                });
            }

            for (idx, target_vec) in entry.vectors.iter().enumerate() {
                let score = cosine_similarity(&normalized_probe, target_vec);
                if score > highest_score {
                    highest_score = score;
                    best_match = Some(MatchResult {
                        member_id: entry.member_id.clone(),
                        member_name: entry.member_name.clone(),
                        confidence: score,
                        matched_angle_index: idx,
                        is_expired,
                        expires_at: entry.expires_at,
                    });
                }
            }
        }

        best_match
    }
}

/// Compute cosine similarity between two float vectors.
/// Since vectors are L2-normalized, (A · B) / (||A|| * ||B||) simplifies directly to the Dot Product (A · B)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot_product = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot_product += x * y;
    }

    dot_product.max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![1.0, 2.0, 3.0];
        let v2 = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 0.0).abs() < 1e-5);
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
}
