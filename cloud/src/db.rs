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
}
