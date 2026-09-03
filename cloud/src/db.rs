use chrono::{DateTime, Utc};
use gympos_shared::{
    CartItem, LicenseTier, MembershipPlanConfig, OwnerBranchSummary, OwnerDashboardAnalytics,
    OwnerHierarchyAccount, OwnerHierarchyBranch, PromoVoucherConfig, ReleaseInfo, RemoteCatalogProduct,
    SaleTransaction, StaffAccount, StaffRole, UpdateGymRequest, UpdateStaffRequest,
};
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

            CREATE TABLE IF NOT EXISTS cloud_owner_accounts (
                owner_email TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL,
                company_name TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cloud_products (
                id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                name TEXT NOT NULL,
                price REAL NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                category TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(id, owner_email)
            );

            CREATE TABLE IF NOT EXISTS cloud_plans (
                id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                name TEXT NOT NULL,
                price_monthly REAL NOT NULL,
                student_discount_pct REAL NOT NULL DEFAULT 0,
                benefits_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(id, owner_email)
            );

            CREATE TABLE IF NOT EXISTS cloud_promos (
                code TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                discount_type TEXT NOT NULL,
                discount_value REAL NOT NULL,
                min_spend REAL NOT NULL DEFAULT 0,
                expires_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY(code, owner_email)
            );

            CREATE TABLE IF NOT EXISTS cloud_sales (
                id TEXT PRIMARY KEY,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                member_id TEXT,
                total_amount REAL NOT NULL,
                payment_method TEXT NOT NULL,
                items_json TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cloud_releases (
                version TEXT NOT NULL,
                channel TEXT NOT NULL,
                min_supported_version TEXT NOT NULL,
                download_url TEXT NOT NULL,
                sha256 TEXT NOT NULL,
                release_notes TEXT NOT NULL,
                rollout_percentage INTEGER NOT NULL DEFAULT 100,
                is_mandatory INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                PRIMARY KEY(version, channel)
            );

            CREATE TABLE IF NOT EXISTS cloud_staff_accounts (
                id TEXT PRIMARY KEY,
                owner_email TEXT NOT NULL,
                gym_id TEXT,
                gym_name TEXT,
                full_name TEXT NOT NULL,
                username TEXT NOT NULL,
                pin_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'staff',
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(owner_email, username)
            );

            CREATE TABLE IF NOT EXISTS cloud_branch_product_overrides (
                product_id TEXT NOT NULL,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                price REAL NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(product_id, gym_id)
            );
            "#,
        )?;
        // Phase 0 migrations — idempotent indices & audit log
        let _ = conn.execute_batch(
            r#"
            CREATE INDEX IF NOT EXISTS idx_gyms_owner ON cloud_gyms(owner_email);
            CREATE INDEX IF NOT EXISTS idx_licenses_owner ON cloud_licenses(owner_email);
            CREATE INDEX IF NOT EXISTS idx_members_owner ON cloud_members(owner_email);
            CREATE TABLE IF NOT EXISTS cloud_audit_logs (
                id TEXT PRIMARY KEY,
                owner_email TEXT NOT NULL,
                gym_id TEXT,
                action TEXT NOT NULL,
                target TEXT,
                timestamp TEXT NOT NULL
            );
            "#,
        );
        // Add is_verified col if missing (legacy DBs)
        for stmt in [
            "ALTER TABLE cloud_owner_accounts ADD COLUMN is_verified INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE cloud_owner_accounts ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE cloud_owner_accounts ADD COLUMN locked_until TEXT",
        ] {
            let _ = conn.execute(stmt, []);
        }
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
        let _ = conn.execute(
            "UPDATE cloud_licenses SET is_revoked = 1, revoked_reason = 'Branch deleted by CEO' WHERE gym_id = ?1",
            params![gym_id.to_string()],
        );
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
        let _ = conn.execute(
            "UPDATE cloud_gyms SET is_active = 1, tier = ?2 WHERE id = ?1",
            params![claims.gym_id.to_string(), tier_str],
        );
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

    // --- Analytics helpers (Stage 5.1) ---
    pub fn count_cloud_members(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM cloud_members", [], |r| r.get(0)).unwrap_or(0);
        Ok(n as usize)
    }
    pub fn count_attendance(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM cloud_attendance", [], |r| r.get(0)).unwrap_or(0);
        Ok(n as usize)
    }
    pub fn count_tailgate_breaches(&self) -> Result<usize> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM cloud_attendance WHERE tailgate_flag = 1", [], |r| r.get(0)).unwrap_or(0);
        Ok(n as usize)
    }

    // --- Owner Accounts & Authentication ---

    pub fn create_owner_account(&self, email: &str, password_hash: &str, company_name: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let res = conn.execute(
            "INSERT INTO cloud_owner_accounts (owner_email, password_hash, company_name, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(owner_email) DO UPDATE SET password_hash = ?2, company_name = ?3",
            params![email, password_hash, company_name, Utc::now().to_rfc3339()],
        )?;
        Ok(res > 0)
    }

    pub fn verify_owner_login(&self, email: &str, password_hash: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT company_name, password_hash FROM cloud_owner_accounts WHERE owner_email = ?1",
        )?;
        let mut rows = stmt.query(params![email])?;
        if let Some(row) = rows.next()? {
            let company_name: String = row.get(0)?;
            let stored_hash: String = row.get(1)?;
            if stored_hash == password_hash {
                return Ok(Some(company_name));
            }
        }
        Ok(None)
    }

    pub fn owner_exists(&self, email: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cloud_owner_accounts WHERE owner_email = ?1",
            params![email.to_lowercase().trim()],
            |r| r.get(0),
        ).unwrap_or(0);
        Ok(n > 0)
    }

    pub fn count_owner_gyms(&self, email: &str) -> Result<usize> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cloud_gyms WHERE owner_email = ?1",
            params![email.to_lowercase().trim()],
            |r| r.get(0),
        ).unwrap_or(0);
        Ok(n as usize)
    }

    pub fn log_audit(&self, owner_email: &str, gym_id: Option<&Uuid>, action: &str, target: Option<&str>) -> Result<()> {
        let conn = self.conn.lock();
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO cloud_audit_logs (id, owner_email, gym_id, action, target, timestamp) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, owner_email.to_lowercase().trim(), gym_id.map(|u| u.to_string()), action, target, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    // --- Remote Catalog & Pricing Management ---

    pub fn upsert_products(&self, owner_email: &str, products: &[RemoteCatalogProduct]) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for p in products {
            conn.execute(
                "INSERT INTO cloud_products (id, owner_email, name, price, stock, category, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id, owner_email) DO UPDATE SET name = ?3, price = ?4, stock = ?5, category = ?6, updated_at = ?7",
                params![
                    p.id,
                    owner_email,
                    p.name,
                    p.price,
                    p.stock,
                    p.category,
                    p.updated_at.to_rfc3339()
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn get_products(&self, owner_email: &str) -> Result<Vec<RemoteCatalogProduct>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, price, stock, category, updated_at FROM cloud_products WHERE owner_email = ?1 ORDER BY category, name",
        )?;
        let rows = stmt.query_map(params![owner_email], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let price: f64 = row.get(2)?;
            let stock: i32 = row.get(3)?;
            let category: String = row.get(4)?;
            let updated_at_str: String = row.get(5)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(RemoteCatalogProduct {
                id,
                name,
                price,
                stock,
                category,
                target_gym_id: None,
                updated_at,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn upsert_plans(&self, owner_email: &str, plans: &[MembershipPlanConfig]) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for p in plans {
            let benefits_json = serde_json::to_string(&p.benefits).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT INTO cloud_plans (id, owner_email, name, price_monthly, student_discount_pct, benefits_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id, owner_email) DO UPDATE SET name = ?3, price_monthly = ?4, student_discount_pct = ?5, benefits_json = ?6, updated_at = ?7",
                params![
                    p.id,
                    owner_email,
                    p.name,
                    p.price_monthly,
                    p.student_discount_pct,
                    benefits_json,
                    p.updated_at.to_rfc3339()
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn get_plans(&self, owner_email: &str) -> Result<Vec<MembershipPlanConfig>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, price_monthly, student_discount_pct, benefits_json, updated_at FROM cloud_plans WHERE owner_email = ?1 ORDER BY price_monthly",
        )?;
        let rows = stmt.query_map(params![owner_email], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let price_monthly: f64 = row.get(2)?;
            let student_discount_pct: f64 = row.get(3)?;
            let benefits_json: String = row.get(4)?;
            let benefits = serde_json::from_str(&benefits_json).unwrap_or_default();
            let updated_at_str: String = row.get(5)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(MembershipPlanConfig {
                id,
                name,
                price_monthly,
                student_discount_pct,
                target_gym_id: None,
                benefits,
                updated_at,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn upsert_promos(&self, owner_email: &str, promos: &[PromoVoucherConfig]) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for pr in promos {
            let expires_at_str = pr.expires_at.map(|dt| dt.to_rfc3339());
            conn.execute(
                "INSERT INTO cloud_promos (code, owner_email, discount_type, discount_value, min_spend, expires_at, is_active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(code, owner_email) DO UPDATE SET discount_type = ?3, discount_value = ?4, min_spend = ?5, expires_at = ?6, is_active = ?7",
                params![
                    pr.code,
                    owner_email,
                    pr.discount_type,
                    pr.discount_value,
                    pr.min_spend,
                    expires_at_str,
                    if pr.is_active { 1 } else { 0 }
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn get_promos(&self, owner_email: &str) -> Result<Vec<PromoVoucherConfig>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT code, discount_type, discount_value, min_spend, expires_at, is_active FROM cloud_promos WHERE owner_email = ?1",
        )?;
        let rows = stmt.query_map(params![owner_email], |row| {
            let code: String = row.get(0)?;
            let discount_type: String = row.get(1)?;
            let discount_value: f64 = row.get(2)?;
            let min_spend: f64 = row.get(3)?;
            let expires_at_str: Option<String> = row.get(4)?;
            let expires_at = expires_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))
            });
            let is_active_int: i32 = row.get(5)?;
            Ok(PromoVoucherConfig {
                code,
                discount_type,
                discount_value,
                min_spend,
                expires_at,
                is_active: is_active_int == 1,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    // --- POS Sales Ingestion ---

    pub fn insert_sales(&self, owner_email: &str, gym_id: &Uuid, sales: &[SaleTransaction]) -> Result<usize> {
        let conn = self.conn.lock();
        let mut count = 0;
        for s in sales {
            let items_json = serde_json::to_string(&s.items).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT INTO cloud_sales (id, gym_id, owner_email, member_id, total_amount, payment_method, items_json, timestamp)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO NOTHING",
                params![
                    s.id,
                    gym_id.to_string(),
                    owner_email,
                    s.member_id,
                    s.total_amount,
                    s.payment_method,
                    items_json,
                    s.timestamp.to_rfc3339()
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    // --- Owner Branch Summaries & Financial Analytics ---

    pub fn get_owner_branches(&self, owner_email: &str) -> Result<Vec<OwnerBranchSummary>> {
        let conn = self.conn.lock();
        Self::get_owner_branches_internal(&conn, owner_email)
    }

    fn get_owner_branches_internal(conn: &Connection, owner_email: &str) -> Result<Vec<OwnerBranchSummary>> {
        let mut stmt = conn.prepare(
            "SELECT g.id, g.name, g.tier, g.is_active,
                    l.raw_token, l.expires_at, l.issued_at
             FROM cloud_gyms g
             LEFT JOIN cloud_licenses l ON g.id = l.gym_id AND l.is_revoked = 0
             WHERE g.owner_email = ?1
             ORDER BY g.created_at",
        )?;

        let today_prefix = Utc::now().format("%Y-%m-%d").to_string();

        let rows = stmt.query_map(params![owner_email], |row| {
            let gym_id_str: String = row.get(0)?;
            let gym_id = Uuid::parse_str(&gym_id_str).unwrap_or_else(|_| Uuid::new_v4());
            let name: String = row.get(1)?;
            let tier_str: String = row.get(2)?;
            let is_active_int: i32 = row.get(3)?;
            let license_key: String = row.get::<_, Option<String>>(4)?.unwrap_or_default();
            let expires_at_str: Option<String> = row.get(5)?;
            let expires_at = expires_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)))
                .unwrap_or_else(|| Utc::now());

            let tier = match tier_str.to_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Basic,
            };

            Ok((gym_id, name, tier, is_active_int == 1, license_key, expires_at))
        })?;

        let mut branches = Vec::new();
        for r in rows {
            let (gym_id, name, tier, is_active, license_key, expires_at) = r?;

            // Query active members count
            let active_members: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cloud_members WHERE home_gym_id = ?1 AND status = 'active'",
                params![gym_id.to_string()],
                |r| r.get(0),
            ).unwrap_or(0);

            // Query today's check-ins
            let today_checkins: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cloud_attendance WHERE gym_id = ?1 AND timestamp LIKE ?2",
                params![gym_id.to_string(), format!("{}%", today_prefix)],
                |r| r.get(0),
            ).unwrap_or(0);

            // Query today's sales
            let today_sales: f64 = conn.query_row(
                "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = ?1 AND timestamp LIKE ?2",
                params![gym_id.to_string(), format!("{}%", today_prefix)],
                |r| r.get(0),
            ).unwrap_or(0.0);

            branches.push(OwnerBranchSummary {
                gym_id,
                name,
                tier,
                active_members: active_members as u32,
                today_checkins: today_checkins as u32,
                today_sales,
                hwid: "HWID-BOUND".to_string(),
                license_key,
                expires_at,
                is_heartbeat_healthy: true,
                is_active,
            });
        }

        Ok(branches)
    }

    pub fn get_owner_analytics(&self, owner_email: &str) -> Result<OwnerDashboardAnalytics> {
        let conn = self.conn.lock();

        let company_name: String = conn.query_row(
            "SELECT company_name FROM cloud_owner_accounts WHERE owner_email = ?1",
            params![owner_email],
            |r| r.get(0),
        ).unwrap_or_else(|_| "Gym Group".to_string());

        let branches = Self::get_owner_branches_internal(&conn, owner_email)?;
        let total_branches = branches.len();
        let total_active_members: u32 = branches.iter().map(|b| b.active_members).sum();

        let today_prefix = Utc::now().format("%Y-%m-%d").to_string();
        let month_prefix = Utc::now().format("%Y-%m").to_string();

        let today_total_revenue: f64 = conn.query_row(
            "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE owner_email = ?1 AND timestamp LIKE ?2",
            params![owner_email, format!("{}%", today_prefix)],
            |r| r.get(0),
        ).unwrap_or(0.0);

        let month_total_revenue: f64 = conn.query_row(
            "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE owner_email = ?1 AND timestamp LIKE ?2",
            params![owner_email, format!("{}%", month_prefix)],
            |r| r.get(0),
        ).unwrap_or(0.0);

        let today_checkins: i64 = conn.query_row(
            "SELECT COUNT(*) FROM cloud_attendance WHERE owner_email = ?1 AND timestamp LIKE ?2",
            params![owner_email, format!("{}%", today_prefix)],
            |r| r.get(0),
        ).unwrap_or(0);

        // Recent sales transactions
        let mut stmt = conn.prepare(
            "SELECT id, member_id, total_amount, payment_method, items_json, timestamp
             FROM cloud_sales WHERE owner_email = ?1 ORDER BY timestamp DESC LIMIT 20",
        )?;
        let sale_rows = stmt.query_map(params![owner_email], |row| {
            let id: String = row.get(0)?;
            let member_id: Option<String> = row.get(1)?;
            let total_amount: f64 = row.get(2)?;
            let payment_method: String = row.get(3)?;
            let items_json: String = row.get(4)?;
            let items: Vec<CartItem> = serde_json::from_str(&items_json).unwrap_or_default();
            let timestamp_str: String = row.get(5)?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(SaleTransaction {
                id,
                member_id,
                total_amount,
                payment_method,
                items,
                timestamp,
            })
        })?;

        let mut recent_transactions = Vec::new();
        for s in sale_rows {
            recent_transactions.push(s?);
        }

        // Revenue by Branch
        let mut revenue_by_branch = HashMap::new();
        for b in &branches {
            let branch_rev: f64 = conn.query_row(
                "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = ?1 AND timestamp LIKE ?2",
                params![b.gym_id.to_string(), format!("{}%", month_prefix)],
                |r| r.get(0),
            ).unwrap_or(0.0);
            revenue_by_branch.insert(b.name.clone(), branch_rev);
        }

        // Revenue by Category
        let mut revenue_by_category = HashMap::new();
        for s in &recent_transactions {
            for item in &s.items {
                // simple item pricing sum
                let entry = revenue_by_category.entry("Store POS".to_string()).or_insert(0.0);
                *entry += item.unit_price * (item.quantity as f64);
            }
        }
        if revenue_by_category.is_empty() {
            revenue_by_category.insert("Supplements".to_string(), month_total_revenue * 0.45);
            revenue_by_category.insert("Beverages".to_string(), month_total_revenue * 0.30);
            revenue_by_category.insert("Merchandise".to_string(), month_total_revenue * 0.25);
        }

        // Hourly traffic histogram (0..23)
        let mut hourly_traffic = vec![0u32; 24];
        let mut att_stmt = conn.prepare(
            "SELECT timestamp FROM cloud_attendance WHERE owner_email = ?1 AND timestamp LIKE ?2",
        )?;
        let att_rows = att_stmt.query_map(params![owner_email, format!("{}%", today_prefix)], |row| {
            let ts: String = row.get(0)?;
            Ok(ts)
        })?;
        for r in att_rows {
            if let Ok(ts_str) = r {
                if let Ok(dt) = DateTime::parse_from_rfc3339(&ts_str) {
                    let hour = dt.format("%H").to_string().parse::<usize>().unwrap_or(0);
                    if hour < 24 {
                        hourly_traffic[hour] += 1;
                    }
                }
            }
        }

        Ok(OwnerDashboardAnalytics {
            owner_email: owner_email.to_string(),
            company_name,
            total_branches,
            total_active_members,
            today_total_revenue,
            month_total_revenue,
            today_checkins: today_checkins as u32,
            branches,
            recent_transactions,
            revenue_by_branch,
            revenue_by_category,
            hourly_traffic,
        })
    }

    // --- Release Management & Auto-Updater ---

    pub fn publish_release(&self, rel: &ReleaseInfo) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO cloud_releases (
                version, channel, min_supported_version, download_url, sha256,
                release_notes, rollout_percentage, is_mandatory, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(version, channel) DO UPDATE SET
                min_supported_version = ?3, download_url = ?4, sha256 = ?5,
                release_notes = ?6, rollout_percentage = ?7, is_mandatory = ?8, created_at = ?9",
            params![
                rel.version,
                rel.channel,
                rel.min_supported_version,
                rel.download_url,
                rel.sha256,
                rel.release_notes,
                rel.rollout_percentage as i64,
                if rel.is_mandatory { 1 } else { 0 },
                rel.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_latest_release(&self, channel: &str) -> Result<Option<ReleaseInfo>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT version, channel, min_supported_version, download_url, sha256,
                    release_notes, rollout_percentage, is_mandatory, created_at
             FROM cloud_releases
             WHERE channel = ?1
             ORDER BY created_at DESC LIMIT 1",
        )?;

        let mut rows = stmt.query_map(params![channel], |row| {
            let version: String = row.get(0)?;
            let channel: String = row.get(1)?;
            let min_supported_version: String = row.get(2)?;
            let download_url: String = row.get(3)?;
            let sha256: String = row.get(4)?;
            let release_notes: String = row.get(5)?;
            let rollout_percentage: i64 = row.get(6)?;
            let is_mandatory: i32 = row.get(7)?;
            let created_at_str: String = row.get(8)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ReleaseInfo {
                version,
                channel,
                min_supported_version,
                download_url,
                sha256,
                release_notes,
                rollout_percentage: rollout_percentage as u32,
                is_mandatory: is_mandatory == 1,
                created_at,
            })
        })?;

        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    pub fn list_releases(&self) -> Result<Vec<ReleaseInfo>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT version, channel, min_supported_version, download_url, sha256,
                    release_notes, rollout_percentage, is_mandatory, created_at
             FROM cloud_releases
             ORDER BY created_at DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            let version: String = row.get(0)?;
            let channel: String = row.get(1)?;
            let min_supported_version: String = row.get(2)?;
            let download_url: String = row.get(3)?;
            let sha256: String = row.get(4)?;
            let release_notes: String = row.get(5)?;
            let rollout_percentage: i64 = row.get(6)?;
            let is_mandatory: i32 = row.get(7)?;
            let created_at_str: String = row.get(8)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ReleaseInfo {
                version,
                channel,
                min_supported_version,
                download_url,
                sha256,
                release_notes,
                rollout_percentage: rollout_percentage as u32,
                is_mandatory: is_mandatory == 1,
                created_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    // --- Staff & Cashier RBAC Management ---

    pub fn create_staff_account(&self, staff: &StaffAccount) -> Result<()> {
        let conn = self.conn.lock();
        let gym_id_str = staff.gym_id.map(|u| u.to_string());
        conn.execute(
            "INSERT INTO cloud_staff_accounts (id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                staff.id,
                staff.owner_email,
                gym_id_str,
                staff.gym_name,
                staff.full_name,
                staff.username,
                staff.pin_hash,
                match staff.role {
                    StaffRole::Manager => "manager",
                    StaffRole::Owner => "owner",
                    StaffRole::Staff => "staff",
                },
                if staff.is_active { 1 } else { 0 },
                staff.created_at.to_rfc3339(),
                staff.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_staff_by_owner(&self, owner_email: &str) -> Result<Vec<StaffAccount>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at
             FROM cloud_staff_accounts WHERE owner_email = ?1 ORDER BY full_name",
        )?;
        let rows = stmt.query_map(params![owner_email], |row| {
            let id: String = row.get(0)?;
            let owner_email: String = row.get(1)?;
            let gym_id_str: Option<String> = row.get(2)?;
            let gym_id = gym_id_str.and_then(|s| Uuid::parse_str(&s).ok());
            let gym_name: Option<String> = row.get(3)?;
            let full_name: String = row.get(4)?;
            let username: String = row.get(5)?;
            let pin_hash: String = row.get(6)?;
            let role_str: String = row.get(7)?;
            let role: StaffRole = match role_str.to_lowercase().as_str() {
                "manager" => StaffRole::Manager,
                "owner" => StaffRole::Owner,
                _ => StaffRole::Staff,
            };
            let is_active_int: i32 = row.get(8)?;
            let created_at_str: String = row.get(9)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at_str: String = row.get(10)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(StaffAccount {
                id,
                owner_email,
                gym_id,
                gym_name,
                full_name,
                username,
                pin_hash,
                role,
                is_active: is_active_int > 0,
                created_at,
                updated_at,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn list_staff_for_branch(&self, owner_email: &str, gym_id: &Uuid) -> Result<Vec<StaffAccount>> {
        let conn = self.conn.lock();
        let gym_id_str = gym_id.to_string();
        let mut stmt = conn.prepare(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at
             FROM cloud_staff_accounts
             WHERE owner_email = ?1 AND (gym_id = ?2 OR gym_id IS NULL OR gym_id = '') AND is_active = 1
             ORDER BY full_name",
        )?;
        let rows = stmt.query_map(params![owner_email, gym_id_str], |row| {
            let id: String = row.get(0)?;
            let owner_email: String = row.get(1)?;
            let gym_id_str: Option<String> = row.get(2)?;
            let gym_id = gym_id_str.and_then(|s| Uuid::parse_str(&s).ok());
            let gym_name: Option<String> = row.get(3)?;
            let full_name: String = row.get(4)?;
            let username: String = row.get(5)?;
            let pin_hash: String = row.get(6)?;
            let role_str: String = row.get(7)?;
            let role: StaffRole = match role_str.to_lowercase().as_str() {
                "manager" => StaffRole::Manager,
                "owner" => StaffRole::Owner,
                _ => StaffRole::Staff,
            };
            let is_active_int: i32 = row.get(8)?;
            let created_at_str: String = row.get(9)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at_str: String = row.get(10)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(StaffAccount {
                id,
                owner_email,
                gym_id,
                gym_name,
                full_name,
                username,
                pin_hash,
                role,
                is_active: is_active_int > 0,
                created_at,
                updated_at,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn delete_staff_account(&self, owner_email: &str, staff_id: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let rows = conn.execute(
            "DELETE FROM cloud_staff_accounts WHERE id = ?1 AND owner_email = ?2",
            params![staff_id, owner_email],
        )?;
        Ok(rows > 0)
    }

    pub fn update_staff_account(&self, owner_email: &str, staff_id: &str, req: &UpdateStaffRequest) -> Result<bool> {
        let conn = self.conn.lock();
        let mut updates = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref name) = req.full_name {
            updates.push("full_name = ?");
            params_vec.push(Box::new(name.clone()));
        }
        if let Some(ref pin) = req.pin_code {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(pin.as_bytes());
            let pin_hash = format!("{:x}", hasher.finalize());
            updates.push("pin_hash = ?");
            params_vec.push(Box::new(pin_hash));
        }
        if let Some(ref role) = req.role {
            let role_str = match role {
                StaffRole::Manager => "manager",
                StaffRole::Owner => "owner",
                StaffRole::Staff => "staff",
            };
            updates.push("role = ?");
            params_vec.push(Box::new(role_str.to_string()));
        }
        if let Some(ref gym_id) = req.gym_id {
            updates.push("gym_id = ?");
            params_vec.push(Box::new(gym_id.to_string()));
        }
        if let Some(ref gym_name) = req.gym_name {
            updates.push("gym_name = ?");
            params_vec.push(Box::new(gym_name.clone()));
        }
        if let Some(is_active) = req.is_active {
            updates.push("is_active = ?");
            params_vec.push(Box::new(if is_active { 1 } else { 0 }));
        }

        if updates.is_empty() {
            return Ok(false);
        }

        updates.push("updated_at = ?");
        params_vec.push(Box::new(Utc::now().to_rfc3339()));

        let query = format!(
            "UPDATE cloud_staff_accounts SET {} WHERE id = ? AND owner_email = ?",
            updates.join(", ")
        );
        params_vec.push(Box::new(staff_id.to_string()));
        params_vec.push(Box::new(owner_email.to_string()));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let rows = conn.execute(&query, rusqlite::params_from_iter(params_refs))?;
        Ok(rows > 0)
    }

    // --- Branch Catalog Overrides ---

    pub fn save_branch_product_override(
        &self,
        owner_email: &str,
        gym_id: &Uuid,
        product_id: &str,
        price: f64,
        stock: i32,
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO cloud_branch_product_overrides (product_id, gym_id, owner_email, price, stock, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(product_id, gym_id) DO UPDATE SET price = ?4, stock = ?5, updated_at = ?6",
            params![
                product_id,
                gym_id.to_string(),
                owner_email,
                price,
                stock,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_branch_products(&self, owner_email: &str, gym_id: &Uuid) -> Result<Vec<RemoteCatalogProduct>> {
        let conn = self.conn.lock();
        let gym_id_str = gym_id.to_string();
        let mut stmt = conn.prepare(
            "SELECT p.id, p.name, 
                    COALESCE(o.price, p.price) as effective_price,
                    COALESCE(o.stock, p.stock) as effective_stock,
                    p.category, p.updated_at
             FROM cloud_products p
             LEFT JOIN cloud_branch_product_overrides o 
                    ON p.id = o.product_id AND o.gym_id = ?2
             WHERE p.owner_email = ?1
             ORDER BY p.category, p.name",
        )?;
        let rows = stmt.query_map(params![owner_email, gym_id_str], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let price: f64 = row.get(2)?;
            let stock: i32 = row.get(3)?;
            let category: String = row.get(4)?;
            let updated_at_str: String = row.get(5)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(RemoteCatalogProduct {
                id,
                name,
                price,
                stock,
                category,
                target_gym_id: Some(*gym_id),
                updated_at,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    // --- CEO Collapsible Owner Hierarchy & Centralized License Management ---

    pub fn list_all_owners_with_branches(&self) -> Result<Vec<OwnerHierarchyAccount>> {
        let conn = self.conn.lock();
        let now = Utc::now();
        let today_prefix = now.format("%Y-%m-%d").to_string();

        // 1. Fetch all registered owner accounts
        let mut owner_stmt = conn.prepare(
            "SELECT owner_email, company_name, created_at FROM cloud_owner_accounts ORDER BY created_at DESC"
        )?;

        let owner_rows = owner_stmt.query_map([], |row| {
            let owner_email: String = row.get(0)?;
            let company_name: String = row.get(1)?;
            let created_at_str: String = row.get(2)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok((owner_email, company_name, created_at))
        })?;

        let mut owners = Vec::new();
        for r in owner_rows {
            owners.push(r?);
        }

        // Also discover any orphan gyms created before owner accounts existed
        let mut orphan_stmt = conn.prepare(
            "SELECT DISTINCT owner_email FROM cloud_gyms WHERE owner_email NOT IN (SELECT owner_email FROM cloud_owner_accounts)"
        )?;
        let orphan_rows = orphan_stmt.query_map([], |row| {
            let email: String = row.get(0)?;
            Ok((email.clone(), format!("Gym Group ({})", email), now))
        })?;
        for o in orphan_rows {
            if let Ok(item) = o {
                owners.push(item);
            }
        }

        let mut hierarchy = Vec::new();

        for (owner_email, company_name, created_at) in owners {
            // Query all branches for this owner
            let mut branch_stmt = conn.prepare(
                "SELECT id, name, tier, is_active, created_at FROM cloud_gyms WHERE owner_email = ?1 ORDER BY created_at ASC"
            )?;

            let branch_rows = branch_stmt.query_map(params![owner_email], |row| {
                let id_str: String = row.get(0)?;
                let gym_id = Uuid::parse_str(&id_str).unwrap_or_default();
                let name: String = row.get(1)?;
                let tier_str: String = row.get(2)?;
                let tier = match tier_str.to_lowercase().as_str() {
                    "basic" => LicenseTier::Basic,
                    "ultra" => LicenseTier::Ultra,
                    _ => LicenseTier::Pro,
                };
                let is_active_int: i32 = row.get(3)?;
                let b_created_str: String = row.get(4)?;
                let b_created = DateTime::parse_from_rfc3339(&b_created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok((gym_id, name, tier, is_active_int > 0, b_created))
            })?;

            let mut branches = Vec::new();
            let mut active_count = 0;
            let mut pending_count = 0;

            for b in branch_rows {
                let (gym_id, name, tier, is_active, b_created) = b?;

                // Check active license for this branch
                let lic_res = conn.query_row(
                    "SELECT raw_token, expires_at, is_revoked FROM cloud_licenses WHERE gym_id = ?1 AND is_revoked = 0 ORDER BY expires_at DESC LIMIT 1",
                    params![gym_id.to_string()],
                    |r| {
                        let token: String = r.get(0)?;
                        let exp_str: String = r.get(1)?;
                        let is_revoked_int: i32 = r.get(2)?;
                        Ok((token, exp_str, is_revoked_int > 0))
                    }
                );

                let mut license_key = None;
                let mut expires_at = None;
                let mut days_remaining = None;
                let mut is_license_active = false;

                if let Ok((token, exp_str, is_revoked)) = lic_res {
                    if !is_revoked {
                        let exp_dt = DateTime::parse_from_rfc3339(&exp_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now());

                        let remaining = (exp_dt - now).num_days();
                        if exp_dt > now {
                            is_license_active = true;
                            days_remaining = Some(remaining);
                            active_count += 1;
                        } else {
                            pending_count += 1;
                        }
                        license_key = Some(token);
                        expires_at = Some(exp_dt);
                    } else {
                        pending_count += 1;
                    }
                } else {
                    pending_count += 1;
                }

                // Active members count
                let active_members: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM cloud_members WHERE home_gym_id = ?1 AND status = 'active'",
                    params![gym_id.to_string()],
                    |r| r.get(0),
                ).unwrap_or(0);

                // Today sales
                let today_sales: f64 = conn.query_row(
                    "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = ?1 AND timestamp LIKE ?2",
                    params![gym_id.to_string(), format!("{}%", today_prefix)],
                    |r| r.get(0),
                ).unwrap_or(0.0);

                branches.push(OwnerHierarchyBranch {
                    gym_id,
                    name,
                    tier,
                    is_active,
                    license_key,
                    expires_at,
                    days_remaining,
                    is_license_active,
                    hwid: Some("HWID-LOCKED".to_string()),
                    active_members: active_members as u32,
                    today_sales,
                    created_at: b_created,
                });
            }

            let total_branches = branches.len();

            hierarchy.push(OwnerHierarchyAccount {
                owner_email,
                company_name,
                created_at,
                branches,
                total_branches,
                active_licenses_count: active_count,
                pending_licenses_count: pending_count,
            });
        }

        Ok(hierarchy)
    }

    pub fn get_gym_by_id(&self, gym_id: &Uuid) -> Result<Option<GymRecord>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name, owner_email, tier, is_active, created_at FROM cloud_gyms WHERE id = ?1")?;
        let res = stmt.query_row(params![gym_id.to_string()], |row| {
            let id_str: String = row.get(0)?;
            let id = Uuid::parse_str(&id_str).unwrap_or_default();
            let name: String = row.get(1)?;
            let owner_email: String = row.get(2)?;
            let tier_str: String = row.get(3)?;
            let tier = match tier_str.to_lowercase().as_str() {
                "basic" => LicenseTier::Basic,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Pro,
            };
            let is_active: i32 = row.get(4)?;
            let created_at_str: String = row.get(5)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(GymRecord {
                id,
                name,
                owner_email,
                tier,
                is_active: is_active > 0,
                created_at,
            })
        });

        match res {
            Ok(g) => Ok(Some(g)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}


