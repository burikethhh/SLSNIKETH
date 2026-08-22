use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct FaceEntry {
    pub member_id: String,
    pub member_name: String,
    pub vectors: Vec<Vec<f32>>, // 1-3 angles (front, left, right)
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

    /// Load or upsert face vectors with an optional expiration timestamp (e.g. 8-hour walk-in pass)
    pub fn upsert_with_expiry(
        &self,
        member_id: String,
        member_name: String,
        vectors: Vec<Vec<f32>>,
        expires_at: Option<DateTime<Utc>>,
    ) {
        let mut store = self.entries.write();
        store.insert(
            member_id.clone(),
            FaceEntry {
                member_id,
                member_name,
                vectors,
                expires_at,
            },
        );
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

    /// Match a probe vector against all registered vectors using Cosine Similarity.
    pub fn match_vector(&self, probe: &[f32], threshold: f32) -> Option<MatchResult> {
        let store = self.entries.read();
        let now = Utc::now();
        let mut best_match: Option<MatchResult> = None;
        let mut highest_score: f32 = threshold;

        for entry in store.values() {
            let is_expired = entry.expires_at.map_or(false, |exp| now > exp);

            for (idx, target_vec) in entry.vectors.iter().enumerate() {
                let score = cosine_similarity(probe, target_vec);
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
/// Cosine Similarity = (A · B) / (||A|| * ||B||)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot_product = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot_product += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denominator = norm_a.sqrt() * norm_b.sqrt();
    if denominator == 0.0 {
        0.0
    } else {
        dot_product / denominator
    }
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
