use chrono::{DateTime, Utc};
use gympos_shared::{LicenseTier, UpdateGymRequest};
use parking_lot::Mutex;
use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

use crate::models::GymRecord;

pub struct CloudDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl CloudDatabase {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS cloud_gyms (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                tier TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cloud_disabled_gyms (
                gym_id TEXT PRIMARY KEY,
                disabled_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cloud_licenses (
                license_id TEXT PRIMARY KEY,
                raw_token TEXT NOT NULL,
                gym_id TEXT NOT NULL,
                gym_name TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                tier TEXT NOT NULL,
                issued_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                max_members INTEGER NOT NULL,
                hardware_lock_enabled INTEGER NOT NULL DEFAULT 1,
                tailgate_detection_enabled INTEGER NOT NULL DEFAULT 1,
                is_revoked INTEGER NOT NULL DEFAULT 0,
                revoked_reason TEXT,
                revoked_at TEXT
            );

            CREATE TABLE IF NOT EXISTS cloud_members (
                id TEXT PRIMARY KEY,
                owner_email TEXT NOT NULL,
                home_gym_id TEXT NOT NULL,
                home_gym_name TEXT NOT NULL,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                email TEXT,
                phone TEXT,
                membership_type TEXT NOT NULL,
                status TEXT NOT NULL,
                face_vectors_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                expires_at TEXT
            );

            CREATE TABLE IF NOT EXISTS cloud_attendance (
                id TEXT PRIMARY KEY,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                member_id TEXT,
                member_name TEXT,
                direction TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                confidence REAL,
                tailgate_flag INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn load_all_gyms(&self) -> Result<HashMap<Uuid, GymRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, owner_email, tier, is_active, created_at FROM cloud_gyms")?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
            let name: String = row.get(1)?;
            let owner_email: String = row.get(2)?;
            let tier_str: String = row.get(3)?;
            let is_active: i32 = row.get(4)?;
            let created_str: String = row.get(5)?;

            let tier = match tier_str.to_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Basic,
            };

            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(GymRecord {
                id,
                name,
                owner_email,
                tier,
                is_active: is_active == 1,
                created_at,
            })
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let gym = r?;
            map.insert(gym.id, gym);
        }
        Ok(map)
    }

    pub fn load_disabled_gyms(&self) -> Result<HashSet<Uuid>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT gym_id FROM cloud_disabled_gyms")?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            Ok(Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()))
        })?;

        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    pub fn upsert_gym(&self, gym: &GymRecord) -> Result<()> {
        let conn = self.conn.lock();
        let tier_str = format!("{:?}", gym.tier).to_lowercase();
        conn.execute(
            "INSERT INTO cloud_gyms (id, name, owner_email, tier, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name = ?2, owner_email = ?3, tier = ?4, is_active = ?5",
            params![
                gym.id.to_string(),
                gym.name,
                gym.owner_email,
                tier_str,
                if gym.is_active { 1 } else { 0 },
                gym.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn update_gym(&self, req: &UpdateGymRequest) -> Result<()> {
        let conn = self.conn.lock();
        let tier_str = format!("{:?}", req.tier).to_lowercase();
        conn.execute(
            "UPDATE cloud_gyms SET name = ?1, owner_email = ?2, tier = ?3 WHERE id = ?4",
            params![req.name, req.contact_email, tier_str, req.id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete_gym(&self, gym_id: &Uuid) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM cloud_gyms WHERE id = ?1", params![gym_id.to_string()])?;
        conn.execute("DELETE FROM cloud_disabled_gyms WHERE gym_id = ?1", params![gym_id.to_string()])?;
        Ok(())
    }

    pub fn set_disabled(&self, gym_id: &Uuid, disable: bool) -> Result<()> {
        let conn = self.conn.lock();
        if disable {
            conn.execute(
                "INSERT OR REPLACE INTO cloud_disabled_gyms (gym_id, disabled_at) VALUES (?1, ?2)",
                params![gym_id.to_string(), Utc::now().to_rfc3339()],
            )?;
        } else {
            conn.execute(
                "DELETE FROM cloud_disabled_gyms WHERE gym_id = ?1",
                params![gym_id.to_string()],
            )?;
        }
        Ok(())
    }

    // --- License Persistence & Revocation ---

    pub fn insert_license(&self, claims: &gympos_shared::LicenseClaims, raw_token: &str) -> Result<()> {
        let conn = self.conn.lock();
        let tier_str = format!("{:?}", claims.tier).to_lowercase();
        conn.execute(
            "INSERT INTO cloud_licenses (
                license_id, raw_token, gym_id, gym_name, owner_email, tier,
                issued_at, expires_at, max_members, hardware_lock_enabled,
                tailgate_detection_enabled, is_revoked
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)
             ON CONFLICT(license_id) DO UPDATE SET
                raw_token = ?2, gym_name = ?4, owner_email = ?5, tier = ?6,
                expires_at = ?8, max_members = ?9, hardware_lock_enabled = ?10,
                tailgate_detection_enabled = ?11",
            params![
                claims.license_id.to_string(),
                raw_token,
                claims.gym_id.to_string(),
                claims.gym_name,
                claims.owner_email,
                tier_str,
                claims.issued_at.to_rfc3339(),
                claims.expires_at.to_rfc3339(),
                claims.max_members,
                if claims.hardware_lock_enabled { 1 } else { 0 },
                if claims.tailgate_detection_enabled { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn list_licenses(&self) -> Result<Vec<crate::models::CloudLicenseRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT license_id, raw_token, gym_id, gym_name, owner_email, tier,
                    issued_at, expires_at, max_members, hardware_lock_enabled,
                    tailgate_detection_enabled, is_revoked, revoked_reason, revoked_at
             FROM cloud_licenses
             ORDER BY issued_at DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            let lic_id_str: String = row.get(0)?;
            let raw_token: String = row.get(1)?;
            let gym_id_str: String = row.get(2)?;
            let gym_name: String = row.get(3)?;
            let owner_email: String = row.get(4)?;
            let tier_str: String = row.get(5)?;
            let issued_str: String = row.get(6)?;
            let expires_str: String = row.get(7)?;
            let max_members: u32 = row.get(8)?;
            let hw_lock: i32 = row.get(9)?;
            let tailgate: i32 = row.get(10)?;
            let is_revoked: i32 = row.get(11)?;
            let revoked_reason: Option<String> = row.get(12).unwrap_or(None);
            let revoked_at_str: Option<String> = row.get(13).unwrap_or(None);

            let tier = match tier_str.to_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Basic,
            };

            let license_id = Uuid::parse_str(&lic_id_str).unwrap_or_default();
            let gym_id = Uuid::parse_str(&gym_id_str).unwrap_or_default();
            let issued_at = DateTime::parse_from_rfc3339(&issued_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = DateTime::parse_from_rfc3339(&expires_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let revoked_at = revoked_at_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

            Ok(crate::models::CloudLicenseRecord {
                license_id,
                raw_token,
                gym_id,
                gym_name,
                owner_email,
                tier,
                issued_at,
                expires_at,
                max_members,
                hardware_lock_enabled: hw_lock == 1,
                tailgate_detection_enabled: tailgate == 1,
                is_revoked: is_revoked == 1,
                revoked_reason,
                revoked_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn revoke_license(&self, license_id: &Uuid, reason: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE cloud_licenses
             SET is_revoked = 1, revoked_reason = ?1, revoked_at = ?2
             WHERE license_id = ?3",
            params![reason, Utc::now().to_rfc3339(), license_id.to_string()],
        )?;
        Ok(())
    }

    pub fn is_license_revoked(&self, license_id: &Uuid) -> Result<bool> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT is_revoked FROM cloud_licenses WHERE license_id = ?1")?;
        let mut rows = stmt.query(params![license_id.to_string()])?;
        if let Some(row) = rows.next()? {
            let is_revoked: i32 = row.get(0)?;
            Ok(is_revoked == 1)
        } else {
            Ok(false)
        }
    }

    pub fn load_revoked_license_ids(&self) -> Result<HashSet<Uuid>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT license_id FROM cloud_licenses WHERE is_revoked = 1")?;
        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            Ok(Uuid::parse_str(&id_str).unwrap_or_default())
        })?;

        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    // --- Inter-Branch Multi-Gym Sync ---

    pub fn upsert_cloud_members(&self, owner_email: &str, members: &[gympos_shared::CloudMemberSyncItem]) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for m in members {
            let vectors_json = serde_json::to_string(&m.face_vectors).unwrap_or_else(|_| "[]".to_string());
            let expires_str = m.expires_at.map(|e| e.to_rfc3339());

            conn.execute(
                "INSERT INTO cloud_members (id, owner_email, home_gym_id, home_gym_name, first_name, last_name, email, phone, membership_type, status, face_vectors_json, created_at, updated_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(id) DO UPDATE SET
                    home_gym_name = ?4,
                    first_name = ?5,
                    last_name = ?6,
                    email = ?7,
                    phone = ?8,
                    membership_type = ?9,
                    status = ?10,
                    face_vectors_json = ?11,
                    updated_at = ?13,
                    expires_at = ?14",
                params![
                    m.id,
                    owner_email,
                    m.home_gym_id.to_string(),
                    m.home_gym_name,
                    m.first_name,
                    m.last_name,
                    m.email,
                    m.phone,
                    m.membership_type,
                    m.status,
                    vectors_json,
                    m.created_at.to_rfc3339(),
                    m.updated_at.to_rfc3339(),
                    expires_str,
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn get_sister_branch_members(&self, owner_email: &str, exclude_gym_id: &Uuid) -> Result<Vec<gympos_shared::CloudMemberSyncItem>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, home_gym_id, home_gym_name, owner_email, first_name, last_name, email, phone, membership_type, status, face_vectors_json, created_at, updated_at, expires_at
             FROM cloud_members
             WHERE owner_email = ?1 AND home_gym_id != ?2"
        )?;

        let rows = stmt.query_map(params![owner_email, exclude_gym_id.to_string()], |row| {
            let id: String = row.get(0)?;
            let home_gym_id_str: String = row.get(1)?;
            let home_gym_id = Uuid::parse_str(&home_gym_id_str).unwrap_or_default();
            let home_gym_name: String = row.get(2)?;
            let owner_email: String = row.get(3)?;
            let first_name: String = row.get(4)?;
            let last_name: String = row.get(5)?;
            let email: String = row.get(6).unwrap_or_default();
            let phone: String = row.get(7).unwrap_or_default();
            let membership_type: String = row.get(8)?;
            let status: String = row.get(9)?;
            let vectors_json: String = row.get(10)?;
            let created_str: String = row.get(11)?;
            let updated_str: String = row.get(12)?;
            let expires_str: Option<String> = row.get(13).unwrap_or(None);

            let face_vectors: Vec<Vec<f32>> = serde_json::from_str(&vectors_json).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = expires_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

            Ok(gympos_shared::CloudMemberSyncItem {
                id,
                home_gym_id,
                home_gym_name,
                owner_email,
                first_name,
                last_name,
                email,
                phone,
                membership_type,
                status,
                face_vectors,
                created_at,
                updated_at,
                expires_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn insert_attendance_logs(&self, owner_email: &str, logs: &[gympos_shared::AttendanceRecord], gym_id: &Uuid) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for l in logs {
            conn.execute(
                "INSERT INTO cloud_attendance (id, gym_id, owner_email, member_id, member_name, direction, timestamp, confidence, tailgate_flag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    l.id,
                    gym_id.to_string(),
                    owner_email,
                    l.member_id,
                    l.member_name,
                    l.direction,
                    l.timestamp.to_rfc3339(),
                    l.confidence,
                    if l.tailgate_flag { 1 } else { 0 }
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }
}
