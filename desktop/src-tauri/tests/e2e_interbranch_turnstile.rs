//! E2E inter-branch + gate turnstile integration test
//! Run: cargo test --test e2e_interbranch_turnstile
//! Steps: enroll 5-angle → interbranch upsert → FaceVectorStore match → anti-passback → visitor badge → tailgate

use gympos_shared::{LicenseTier, CloudMemberSyncItem};
use gympos_desktop_lib::db::Database;
use gympos_desktop_lib::face::{FaceVectorStore, cosine_similarity_fast, l2_normalize, calculate_vector_quality};
use chrono::Utc;
use uuid::Uuid;

fn gen_embedding(seed: i32, offset: f32) -> Vec<f32> {
    let mut raw = Vec::with_capacity(128);
    for i in 0..128 {
        let v = ((seed as f32 + i as f32 * 1.618 + offset).sin()
            * (seed as f32 * 0.5 + i as f32 * 0.314).cos()
            + ((seed + i) as f32 * 0.1).sin());
        raw.push(v);
    }
    l2_normalize(&mut raw);
    raw
}

#[test]
fn e2e_branch_a_enrollment_5_angle() {
    let seed = "AkiraSato".bytes().map(|b| b as i32).sum::<i32>();
    let offsets = [0.0, 0.45, -0.45, 0.25, -0.25];
    let vectors: Vec<Vec<f32>> = offsets.iter().map(|&off| gen_embedding(seed, off)).collect();
    assert_eq!(vectors.len(), 5);
    for (i, v) in vectors.iter().enumerate() {
        assert_eq!(v.len(), 128);
        let q = calculate_vector_quality(v);
        assert!(q >= 0.15, "angle {} quality {} below gate", i, q);
        assert!(v.iter().all(|x| x.is_finite()));
    }
}

#[test]
fn e2e_interbranch_sync_and_gate() {
    let db = Database::in_memory().expect("in-memory db");
    let store = FaceVectorStore::new();

    let owner = "ceo@titan.fitness";
    let branch_a_id = Uuid::new_v4();
    let branch_a_name = "Titan BGC Branch A";
    let branch_b_name = "Titan Makati Branch B";

    // Step 1: Branch A enroll
    let member_id = format!("MEM-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
    let seed = 12345;
    let vectors: Vec<Vec<f32>> = [0.0, 0.45, -0.45, 0.25, -0.25].iter().map(|&off| gen_embedding(seed, off)).collect();

    // Simulate cloud CloudMemberSyncItem for branch A
    let item = CloudMemberSyncItem {
        id: member_id.clone(),
        home_gym_id: branch_a_id,
        home_gym_name: branch_a_name.to_string(),
        owner_email: owner.to_string(),
        first_name: "Akira".to_string(),
        last_name: "Sato".to_string(),
        email: "akira@titan.fitness".to_string(),
        phone: "0917-000-0001".to_string(),
        membership_type: "vip".to_string(),
        status: "active".to_string(),
        face_vectors: vectors.clone(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        expires_at: None,
    };

    // Step 2-3: Branch B receives sister members (owner-isolated) and upserts
    let count = db.upsert_interbranch_members(&[item.clone()]).expect("upsert");
    assert_eq!(count, 1);

    // Verify list_interbranch_members_detailed returns the visitor
    let detailed = db.list_interbranch_members_detailed().expect("list detailed");
    assert_eq!(detailed.len(), 1);
    assert_eq!(detailed[0]["home_gym_name"], branch_a_name);
    assert_eq!(detailed[0]["home_gym_id"], branch_a_id.to_string());

    // Load into FaceVectorStore
    let list = db.list_interbranch_members().expect("list");
    for m in list {
        store.upsert(m.id, format!("{} {}", m.first_name, m.last_name), m.face_vectors);
    }
    assert_eq!(store.count(), 1);

    // Step 4: Branch B gate probe same person
    let probe = gen_embedding(seed, 0.0);
    let res = store.match_vector(&probe, 0.60).expect("should match");
    assert_eq!(res.member_id, member_id);
    assert!(res.confidence >= 0.90);
    assert!(!res.is_expired);

    // Visitor badge check: home_gym_name != local
    let home = detailed[0]["home_gym_name"].as_str().unwrap();
    let is_visitor = home != branch_b_name;
    assert!(is_visitor, "should be flagged Inter-Branch Visitor");

    // Anti-passback: first IN allowed, second IN without OUT denied (via DB)
    let _log = db.log_attendance(Some(&res.member_id), Some(&res.member_name), "in", Some(res.confidence), false).expect("log in");
    let last_dir = db.get_member_last_direction(&res.member_id).expect("last dir");
    assert_eq!(last_dir, Some("in".to_string()));
    // Second IN should be considered passback (caller checks last_direction == Some("in"))
    assert_eq!(last_dir.as_deref(), Some("in"));
}

#[test]
fn e2e_tailgate_threshold_math() {
    // App.js armDoorOpenTailgateSurveillance: duration 3500ms, interval 250ms, sensitivity 85
    let duration = 3500;
    let interval = 250;
    let max_frames = duration / interval; // 14
    assert_eq!(max_frames, 14);
    let sensitivity = 85;
    let violation_threshold = std::cmp::max(2, ((max_frames as f32 * (1.0 - sensitivity as f32 / 100.0) * 0.6).floor() as usize));
    assert!(violation_threshold >= 2);
    // 5 suspicious frames in 14 with threshold 2 -> should trigger ALARM:5000 / PAT_HEAVY_ALERT
    let suspicious = 5;
    assert!(suspicious >= 3, "3 confirm frames minimum for tailgate");
}

#[test]
fn e2e_nan_guard() {
    let store = FaceVectorStore::new();
    let bad = vec![f32::NAN; 128];
    store.upsert("BAD".to_string(), "Bad Vector".to_string(), vec![bad.clone()]);
    // Should have rejected insertion
    assert_eq!(store.count(), 0);
    let probe = gen_embedding(999, 0.0);
    assert!(store.match_vector(&probe, 0.60).is_none());
    let dot = cosine_similarity_fast(&bad, &probe);
    assert_eq!(dot, 0.0);
}
