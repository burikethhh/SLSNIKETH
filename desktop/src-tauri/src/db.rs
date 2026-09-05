use chrono::{DateTime, Duration, Utc};
use gympos_shared::{
    AppSettings, AttendanceRecord, CartItem, Coach, CoachSession, CreateCoachRequest, CreateExpenseRequest,
    CreateMemberRequest, CreateProductRequest, CreateWalkInRequest, ExpenseRecord, Member, MembershipPlanConfig,
    ProductItem, PromoVoucherConfig, RemoteCatalogProduct, SaleTransaction, StaffAccount, StaffRole,
    UpdateCoachRequest, UpdateMemberRequest, UpdateProductRequest, WalkInRecord,
};
use rusqlite::{params, Connection, Result};
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        db.seed_defaults()?;
        Ok(db)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.init_schema()?;
        db.seed_defaults()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS license_cache (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                raw_token TEXT NOT NULL,
                cached_at TEXT NOT NULL,
                last_verify_unix INTEGER NOT NULL DEFAULT 0,
                last_seen_unix INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                gym_name TEXT NOT NULL,
                logo_data_url TEXT,
                theme_color TEXT NOT NULL,
                walk_in_rate REAL NOT NULL
            );

            CREATE TABLE IF NOT EXISTS members (
                id TEXT PRIMARY KEY,
                home_gym_id TEXT,
                home_gym_name TEXT,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                email TEXT,
                phone TEXT,
                face_vector TEXT, -- JSON array of float arrays
                status TEXT NOT NULL DEFAULT 'active',
                membership_type TEXT NOT NULL DEFAULT 'regular',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                expires_at TEXT,
                is_synced INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS walk_ins (
                id TEXT PRIMARY KEY,
                guest_name TEXT NOT NULL,
                phone TEXT NOT NULL,
                amount_paid REAL NOT NULL,
                payment_method TEXT NOT NULL,
                face_vector TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS attendance_logs (
                id TEXT PRIMARY KEY,
                member_id TEXT,
                member_name TEXT,
                direction TEXT NOT NULL, -- 'in' or 'out'
                timestamp TEXT NOT NULL,
                confidence REAL,
                tailgate_flag INTEGER NOT NULL DEFAULT 0,
                synced_to_cloud INTEGER NOT NULL DEFAULT 0,
                -- Phase A-D tailgate incidents: whose window was piggybacked,
                -- YOLO count snapshot, local acknowledge state.
                linked_member_id TEXT,
                person_count INTEGER,
                acknowledged INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (member_id) REFERENCES members(id)
            );

            CREATE TABLE IF NOT EXISTS products (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                price REAL NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                category TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS remote_plans (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                tag TEXT NOT NULL DEFAULT '',
                billing_period TEXT NOT NULL DEFAULT 'monthly',
                price_monthly REAL NOT NULL DEFAULT 0,
                student_discount_pct REAL NOT NULL DEFAULT 0,
                benefits_json TEXT NOT NULL DEFAULT '[]',
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS remote_promos (
                code TEXT PRIMARY KEY,
                label TEXT NOT NULL DEFAULT '',
                discount_type TEXT NOT NULL DEFAULT 'percent',
                discount_value REAL NOT NULL DEFAULT 0,
                min_spend REAL NOT NULL DEFAULT 0,
                expires_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS transactions (
                id TEXT PRIMARY KEY,
                member_id TEXT,
                total_amount REAL NOT NULL,
                payment_method TEXT NOT NULL,
                items_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                synced_to_cloud INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS coaches (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                specialty TEXT NOT NULL,
                phone TEXT,
                active_students INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS coach_sessions (
                id TEXT PRIMARY KEY,
                coach_id TEXT NOT NULL,
                coach_name TEXT NOT NULL,
                member_id TEXT NOT NULL,
                member_name TEXT NOT NULL,
                session_date TEXT NOT NULL,
                duration_minutes INTEGER NOT NULL DEFAULT 60,
                status TEXT NOT NULL DEFAULT 'scheduled',
                FOREIGN KEY (coach_id) REFERENCES coaches(id) ON DELETE CASCADE,
                FOREIGN KEY (member_id) REFERENCES members(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS local_staff_accounts (
                id TEXT PRIMARY KEY,
                owner_email TEXT NOT NULL,
                gym_id TEXT,
                gym_name TEXT,
                full_name TEXT NOT NULL,
                username TEXT NOT NULL,
                pin_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'staff',
                is_active INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL
            );
            "#,
        )?;

        // Run migrations for existing databases
        let _ = conn.execute("ALTER TABLE members ADD COLUMN home_gym_id TEXT", []);
        let _ = conn.execute("ALTER TABLE members ADD COLUMN home_gym_name TEXT", []);
        let _ = conn.execute("ALTER TABLE members ADD COLUMN is_synced INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE members ADD COLUMN expires_at TEXT", []);
        let _ = conn.execute("ALTER TABLE attendance_logs ADD COLUMN synced_to_cloud INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE license_cache ADD COLUMN last_verify_unix INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE license_cache ADD COLUMN last_seen_unix INTEGER NOT NULL DEFAULT 0", []);
        // Member reference photo (downscaled JPEG data URL, local-first, synced to cloud)
        let _ = conn.execute("ALTER TABLE members ADD COLUMN photo_data_url TEXT", []);
        // POS discount tracking (Senior / Student / PWD ID discounts)
        let _ = conn.execute("ALTER TABLE transactions ADD COLUMN discount_type TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE transactions ADD COLUMN discount_amount REAL NOT NULL DEFAULT 0", []);
        // Phase A-D tailgate incidents (attribution + local acknowledge)
        let _ = conn.execute("ALTER TABLE attendance_logs ADD COLUMN linked_member_id TEXT", []);
        let _ = conn.execute("ALTER TABLE attendance_logs ADD COLUMN person_count INTEGER", []);
        let _ = conn.execute("ALTER TABLE attendance_logs ADD COLUMN acknowledged INTEGER NOT NULL DEFAULT 0", []);
        // Expenses ledger (local bookkeeping for End-of-Day)
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS expenses (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'general',
                amount REAL NOT NULL,
                payment_method TEXT NOT NULL DEFAULT 'cash',
                notes TEXT NOT NULL DEFAULT '',
                spent_at TEXT NOT NULL,
                created_by TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            )",
            [],
        );

        Ok(())
    }

    fn seed_defaults(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Seed default app settings if not exists
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (id, gym_name, logo_data_url, theme_color, walk_in_rate)
             VALUES (1, 'Titan Fitness & Performance', NULL, '#2563eb', 10.0)",
            [],
        )?;

        // Seed default POS inventory if empty
        let product_count: i64 = conn.query_row("SELECT COUNT(*) FROM products", [], |r| r.get(0))?;
        if product_count == 0 {
            conn.execute_batch(
                r#"
                INSERT INTO products (id, name, price, stock, category) VALUES
                    ('prod-1', 'Whey Protein Isolate (2lb)', 45.00, 40, 'supplements'),
                    ('prod-2', 'Pre-Workout Igniter (Blue Raspberry)', 35.00, 30, 'supplements'),
                    ('prod-3', 'Titan Gym Shaker Bottle (750ml)', 15.00, 60, 'merch'),
                    ('prod-4', 'Electrolyte Mineral Sports Drink', 3.50, 120, 'beverages'),
                    ('prod-5', 'Heavy Duty Lifting Straps', 18.00, 25, 'gear'),
                    ('prod-6', 'BCAA Recovery Powder (30 Servings)', 28.00, 35, 'supplements'),
                    ('prod-7', 'Titan Gym Performance T-Shirt', 25.00, 50, 'merch');
                "#,
            )?;
        }

        // Seed coaches if empty
        let coach_count: i64 = conn.query_row("SELECT COUNT(*) FROM coaches", [], |r| r.get(0))?;
        if coach_count == 0 {
            conn.execute_batch(
                r#"
                INSERT INTO coaches (id, name, specialty, phone, active_students) VALUES
                    ('coach-1', 'Marcus Vance', 'Strength & Hypertrophy', '0917-555-0101', 14),
                    ('coach-2', 'Elena Rostova', 'HIIT & Mobility Conditioning', '0917-555-0102', 18),
                    ('coach-3', 'Darius Stone', 'Powerlifting & Athletic Prep', '0917-555-0103', 10);
                "#,
            )?;
        }

        // Seed default staff accounts if empty (Default cashiers for instant out-of-the-box operation)
        let staff_count: i64 = conn.query_row("SELECT COUNT(*) FROM local_staff_accounts", [], |r| r.get(0)).unwrap_or(0);
        if staff_count == 0 {
            let pin_1234 = gympos_shared::hash_password("1234");
            let pin_8888 = gympos_shared::hash_password("8888");

            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO local_staff_accounts (id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, updated_at)
                 VALUES ('staff-default-1', 'system@local', NULL, 'Default Branch', 'Front-Desk Cashier', 'cashier1', ?1, 'staff', 1, ?2)",
                params![pin_1234, now],
            )?;
            conn.execute(
                "INSERT INTO local_staff_accounts (id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, updated_at)
                 VALUES ('staff-default-2', 'system@local', NULL, 'Default Branch', 'Duty Manager', 'manager1', ?1, 'manager', 1, ?2)",
                params![pin_8888, now],
            )?;
        }

        Ok(())
    }

    // --- App Settings (White-Label Branding) ---

    pub fn get_app_settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("ALTER TABLE app_settings ADD COLUMN camera_config_json TEXT", []);
        let mut stmt = conn.prepare("SELECT gym_name, logo_data_url, theme_color, walk_in_rate, camera_config_json FROM app_settings WHERE id = 1")?;
        let res = stmt.query_row([], |row| {
            let config_json: Option<String> = row.get(4).unwrap_or(None);
            let camera_config = config_json.and_then(|s| serde_json::from_str(&s).ok());

            Ok(AppSettings {
                gym_name: row.get(0)?,
                logo_data_url: row.get(1)?,
                theme_color: row.get(2)?,
                walk_in_rate: row.get(3)?,
                camera_config: camera_config.or_else(|| Some(gympos_shared::CameraConfig::default())),
            })
        });

        match res {
            Ok(s) => Ok(s),
            Err(_) => Ok(AppSettings::default()),
        }
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("ALTER TABLE app_settings ADD COLUMN camera_config_json TEXT", []);
        let camera_json = settings.camera_config.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default());

        conn.execute(
            "INSERT INTO app_settings (id, gym_name, logo_data_url, theme_color, walk_in_rate, camera_config_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                gym_name = excluded.gym_name,
                logo_data_url = excluded.logo_data_url,
                theme_color = excluded.theme_color,
                walk_in_rate = excluded.walk_in_rate,
                camera_config_json = excluded.camera_config_json",
            params![
                settings.gym_name,
                settings.logo_data_url,
                settings.theme_color,
                settings.walk_in_rate,
                camera_json
            ],
        )?;
        Ok(())
    }

    // --- License Cache ---

    pub fn get_cached_license(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT raw_token FROM license_cache WHERE id = 1")?;
        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_cached_license(&self, token: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO license_cache (id, raw_token, cached_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET raw_token = excluded.raw_token, cached_at = excluded.cached_at",
            params![token, now],
        )?;
        Ok(())
    }

    pub fn clear_cached_license(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM license_cache", [])?;
        Ok(())
    }

    /// Record a successful cloud verification → refresh the 7-day heartbeat.
    pub fn heartbeat_ok(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "INSERT INTO license_cache (id, raw_token, cached_at, last_verify_unix, last_seen_unix)
             VALUES (1, '', '', ?1, ?1)
             ON CONFLICT(id) DO UPDATE SET last_verify_unix = ?1, last_seen_unix = ?1",
            params![now],
        )?;
        Ok(())
    }

    pub fn last_verify_unix(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let v: i64 = conn
            .query_row("SELECT last_verify_unix FROM license_cache WHERE id = 1", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(v)
    }

    pub fn last_seen_unix(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let v: i64 = conn
            .query_row("SELECT last_seen_unix FROM license_cache WHERE id = 1", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(v)
    }

    pub fn record_last_seen(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp();
        conn.execute(
            "UPDATE license_cache SET last_seen_unix = ?1 WHERE id = 1",
            params![now],
        )?;
        Ok(())
    }

    // --- Walk-In / Day Pass (Strict 8-Hour Validity) ---

    pub fn create_walk_in(&self, req: &CreateWalkInRequest) -> Result<WalkInRecord> {
        let conn = self.conn.lock().unwrap();
        let id = format!("PASS-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let expires_at = now + Duration::hours(8); // Strict 8-hour timed pass
        let vec_json = req
            .face_vector
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        conn.execute(
            "INSERT INTO walk_ins (id, guest_name, phone, amount_paid, payment_method, face_vector, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                req.guest_name,
                req.phone,
                req.amount_paid,
                req.payment_method,
                vec_json,
                now.to_rfc3339(),
                expires_at.to_rfc3339(),
            ],
        )?;

        // Also record transaction in POS
        let tx_id = format!("TX-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let items_json = serde_json::json!([{
            "product_id": "day-pass",
            "product_name": format!("Walk-In Day Pass ({})", req.guest_name),
            "unit_price": req.amount_paid,
            "quantity": 1
        }])
        .to_string();

        conn.execute(
            "INSERT INTO transactions (id, member_id, total_amount, payment_method, items_json, created_at, synced_to_cloud)
             VALUES (?1, NULL, ?2, ?3, ?4, ?5, 0)",
            params![tx_id, req.amount_paid, req.payment_method, items_json, now.to_rfc3339()],
        )?;

        Ok(WalkInRecord {
            id,
            guest_name: req.guest_name.clone(),
            phone: req.phone.clone(),
            amount_paid: req.amount_paid,
            payment_method: req.payment_method.clone(),
            created_at: now,
            expires_at,
            face_vector: req.face_vector.clone(),
        })
    }

    pub fn list_walk_ins(&self) -> Result<Vec<WalkInRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, guest_name, phone, amount_paid, payment_method, created_at, expires_at, face_vector
             FROM walk_ins ORDER BY created_at DESC LIMIT 50",
        )?;

        let rows = stmt.query_map([], |row| {
            let created_str: String = row.get(5)?;
            let expires_str: String = row.get(6)?;
            let vec_json: Option<String> = row.get(7).unwrap_or(None);

            let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let face_vector: Option<Vec<f32>> = vec_json
                .and_then(|s| serde_json::from_str(&s).ok());

            Ok(WalkInRecord {
                id: row.get(0)?,
                guest_name: row.get(1)?,
                phone: row.get(2)?,
                amount_paid: row.get(3)?,
                payment_method: row.get(4)?,
                created_at,
                expires_at,
                face_vector,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn list_active_walk_in_vectors(&self) -> Result<Vec<(String, String, Vec<f32>, DateTime<Utc>)>> {
        let conn = self.conn.lock().unwrap();
        let now_str = Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, guest_name, face_vector, expires_at
             FROM walk_ins WHERE expires_at > ?1 AND face_vector IS NOT NULL",
        )?;

        let rows = stmt.query_map(params![now_str], |row| {
            let id: String = row.get(0)?;
            let guest_name: String = row.get(1)?;
            let vec_str: Option<String> = row.get(2)?;
            let exp_str: String = row.get(3)?;

            let expires_at = chrono::DateTime::parse_from_rfc3339(&exp_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let vector: Vec<f32> = vec_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();

            Ok((id, guest_name, vector, expires_at))
        })?;

        let mut list = Vec::new();
        for r in rows {
            let item = r?;
            if !item.2.is_empty() {
                list.push(item);
            }
        }
        Ok(list)
    }

    // --- Member CRUD ---

    pub fn count_members(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM members WHERE status = 'active'", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    pub fn create_member(&self, req: &CreateMemberRequest) -> Result<Member> {
        let conn = self.conn.lock().unwrap();
        let id = format!("MEM-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let vectors_json = serde_json::to_string(&req.face_vectors).unwrap_or_else(|_| "[]".to_string());

        conn.execute(
            "INSERT INTO members (id, first_name, last_name, email, phone, face_vector, status, membership_type, photo_data_url, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9, ?10)",
            params![
                id,
                req.first_name,
                req.last_name,
                req.email,
                req.phone,
                vectors_json,
                req.membership_type,
                req.photo_data_url,
                now_str,
                now_str
            ],
        )?;

        Ok(Member {
            id,
            first_name: req.first_name.clone(),
            last_name: req.last_name.clone(),
            email: req.email.clone(),
            phone: req.phone.clone(),
            membership_type: req.membership_type.clone(),
            status: "active".to_string(),
            face_vectors: req.face_vectors.clone(),
            photo_data_url: req.photo_data_url.clone(),
            created_at: now,
            expires_at: None,
        })
    }

    pub fn list_members(&self) -> Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, photo_data_url
             FROM members ORDER BY created_at DESC",
        )?;

        let member_rows = stmt.query_map([], |row| {
            let vectors_json: String = row.get(5)?;
            let vectors: Vec<Vec<f32>> = serde_json::from_str(&vectors_json).unwrap_or_default();
            let created_str: String = row.get(8)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Member {
                id: row.get(0)?,
                first_name: row.get(1)?,
                last_name: row.get(2)?,
                email: row.get(3)?,
                phone: row.get(4)?,
                face_vectors: vectors,
                status: row.get(6)?,
                membership_type: row.get(7)?,
                photo_data_url: row.get::<_, Option<String>>(9).unwrap_or(None),
                created_at,
                expires_at: None,
            })
        })?;

        let mut members = Vec::new();
        for m in member_rows {
            members.push(m?);
        }
        Ok(members)
    }

    pub fn get_member_by_id(&self, id: &str) -> Result<Option<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, photo_data_url
             FROM members WHERE id = ?1",
        )?;

        let mut rows = stmt.query_map(params![id], |row| {
            let vectors_json: String = row.get(5)?;
            let vectors: Vec<Vec<f32>> = serde_json::from_str(&vectors_json).unwrap_or_default();
            let created_str: String = row.get(8)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Member {
                id: row.get(0)?,
                first_name: row.get(1)?,
                last_name: row.get(2)?,
                email: row.get(3)?,
                phone: row.get(4)?,
                face_vectors: vectors,
                status: row.get(6)?,
                membership_type: row.get(7)?,
                photo_data_url: row.get::<_, Option<String>>(9).unwrap_or(None),
                created_at,
                expires_at: None,
            })
        })?;

        if let Some(m) = rows.next() {
            Ok(Some(m?))
        } else {
            Ok(None)
        }
    }

    pub fn update_member(&self, req: &UpdateMemberRequest) -> Result<Member> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        conn.execute(
            "UPDATE members SET
                first_name = ?1,
                last_name = ?2,
                email = ?3,
                phone = ?4,
                membership_type = ?5,
                status = ?6,
                photo_data_url = COALESCE(?7, photo_data_url),
                updated_at = ?8
             WHERE id = ?9",
            params![
                req.first_name,
                req.last_name,
                req.email,
                req.phone,
                req.membership_type,
                req.status,
                req.photo_data_url,
                now.to_rfc3339(),
                req.id
            ],
        )?;

        // Fetch and return updated member
        drop(conn);
        self.get_member_by_id(&req.id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn delete_member(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM coach_sessions WHERE member_id = ?1", params![id])?;
        tx.execute("UPDATE attendance_logs SET member_id = NULL WHERE member_id = ?1", params![id])?;
        tx.execute("UPDATE transactions SET member_id = NULL WHERE member_id = ?1", params![id])?;
        tx.execute("DELETE FROM members WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Renew: status back to active + expiry pushed 30 days out (standard membership renewal).
    pub fn renew_member(&self, id: &str) -> Result<Member> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now();
        let new_exp = (now + chrono::Duration::days(30)).to_rfc3339();
        conn.execute(
            "UPDATE members SET status = 'active', expires_at = ?1, updated_at = ?2 WHERE id = ?3",
            params![new_exp, now.to_rfc3339(), id],
        )?;
        drop(conn);
        self.get_member_by_id(id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    /// Freeze/unfreeze: `suspended` members are denied at the gate but keep all data/vectors.
    pub fn set_member_status(&self, id: &str, status: &str) -> Result<Member> {
        let allowed = ["active", "suspended", "expired"];
        if !allowed.contains(&status) {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "Invalid member status '{}' (allowed: active/suspended/expired)",
                status
            )));
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE members SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, Utc::now().to_rfc3339(), id],
        )?;
        drop(conn);
        self.get_member_by_id(id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    /// Re-scan: replace stored face vectors (and optionally the reference photo)
    /// after a fresh Studio capture, refreshing the in-memory centroid caller-side.
    pub fn update_member_vectors(&self, id: &str, vectors: &[Vec<f32>], photo: Option<&str>) -> Result<Member> {
        if vectors.is_empty() {
            return Err(rusqlite::Error::InvalidParameterName(
                "Re-scan requires at least one face vector".to_string(),
            ));
        }
        for v in vectors {
            if v.is_empty() || !v.iter().all(|x| x.is_finite()) {
                return Err(rusqlite::Error::InvalidParameterName(
                    "Re-scan vectors must be finite and non-empty".to_string(),
                ));
            }
        }
        let conn = self.conn.lock().unwrap();
        let vectors_json = serde_json::to_string(vectors).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "UPDATE members SET face_vector = ?1, photo_data_url = COALESCE(?2, photo_data_url), is_synced = 0, updated_at = ?3 WHERE id = ?4",
            params![vectors_json, photo, Utc::now().to_rfc3339(), id],
        )?;
        drop(conn);
        self.get_member_by_id(id)?
            .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    /// Fast status lookup for the gate (denies non-active members before unlocking).
    pub fn get_member_status(&self, id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT status FROM members WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |r| r.get::<_, String>(0))?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    /// Active / expired / suspended / total counts for the dashboard stat boxes.
    pub fn get_member_stats(&self) -> Result<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let count = |status: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM members WHERE status = ?1",
                params![status],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM members", [], |r| r.get(0))
            .unwrap_or(0);
        Ok(serde_json::json!({
            "active": count("active"),
            "expired": count("expired"),
            "suspended": count("suspended"),
            "total": total,
        }))
    }

    pub fn list_interbranch_members(&self) -> Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, expires_at, photo_data_url FROM members WHERE home_gym_id IS NOT NULL AND home_gym_id != '' ORDER BY home_gym_name, last_name",
        )?;
        let rows = stmt.query_map([], |row| {
            let face_json: String = row.get::<_, Option<String>>(5).unwrap_or(None).unwrap_or_else(|| "[]".to_string());
            let face_vectors: Vec<Vec<f32>> = serde_json::from_str(&face_json).unwrap_or_default();
            let created_str: String = row.get(8)?;
            let expires_str: Option<String> = row.get::<_, Option<String>>(9).unwrap_or(None);
            let created_at = DateTime::parse_from_rfc3339(&created_str).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now());
            Ok(Member {
                id: row.get(0)?,
                first_name: row.get(1)?,
                last_name: row.get(2)?,
                email: row.get::<_, Option<String>>(3).unwrap_or(None).unwrap_or_default(),
                phone: row.get::<_, Option<String>>(4).unwrap_or(None).unwrap_or_default(),
                membership_type: row.get(7)?,
                status: row.get(6)?,
                face_vectors,
                photo_data_url: row.get::<_, Option<String>>(10).unwrap_or(None),
                created_at,
                expires_at: expires_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
            })
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    pub fn list_interbranch_members_detailed(&self) -> Result<Vec<serde_json::Value>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, expires_at, home_gym_id, home_gym_name, photo_data_url FROM members WHERE home_gym_id IS NOT NULL AND home_gym_id != '' ORDER BY home_gym_name, last_name",
        )?;
        let rows = stmt.query_map([], |row| {
            let face_json: String = row.get::<_, Option<String>>(5).unwrap_or(None).unwrap_or_else(|| "[]".to_string());
            let vectors: Vec<Vec<f32>> = serde_json::from_str(&face_json).unwrap_or_default();
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "first_name": row.get::<_, String>(1)?,
                "last_name": row.get::<_, String>(2)?,
                "email": row.get::<_, Option<String>>(3).unwrap_or(None).unwrap_or_default(),
                "phone": row.get::<_, Option<String>>(4).unwrap_or(None).unwrap_or_default(),
                "status": row.get::<_, String>(6)?,
                "membership_type": row.get::<_, String>(7)?,
                "created_at": row.get::<_, String>(8)?,
                "expires_at": row.get::<_, Option<String>>(9).unwrap_or(None),
                "home_gym_id": row.get::<_, Option<String>>(10).unwrap_or(None).unwrap_or_default(),
                "home_gym_name": row.get::<_, Option<String>>(11).unwrap_or(None).unwrap_or_default(),
                "photo_data_url": row.get::<_, Option<String>>(12).unwrap_or(None),
                "vector_count": vectors.len(),
            }))
        })?;
        let mut out = Vec::new();
        for r in rows { out.push(r?); }
        Ok(out)
    }

    // --- Attendance & Gate Logs ---

    pub fn log_attendance(
        &self,
        member_id: Option<&str>,
        member_name: Option<&str>,
        direction: &str,
        confidence: Option<f32>,
        tailgate_flag: bool,
    ) -> Result<AttendanceRecord> {
        let conn = self.conn.lock().unwrap();
        let id = format!("ATT-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO attendance_logs (id, member_id, member_name, direction, timestamp, confidence, tailgate_flag, synced_to_cloud)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
            params![
                id,
                member_id,
                member_name,
                direction,
                now_str,
                confidence,
                if tailgate_flag { 1 } else { 0 }
            ],
        )?;

        Ok(AttendanceRecord {
            id,
            member_id: member_id.map(|s| s.to_string()),
            member_name: member_name.map(|s| s.to_string()),
            direction: direction.to_string(),
            confidence,
            tailgate_flag,
            timestamp: now,
            sync_status: "pending".to_string(),
            linked_member_id: None,
            person_count: None,
        })
    }

    /// Phase A-D: logs a tailgate incident with attribution — whose admitted
    /// window was piggybacked (`linked_member_id`) plus the YOLO count
    /// snapshot. `member_id` stays NULL (the intruder is unknown by design).
    pub fn log_tailgate_incident(
        &self,
        linked_member_id: Option<&str>,
        display_name: &str,
        person_count: Option<i32>,
    ) -> Result<AttendanceRecord> {
        let conn = self.conn.lock().unwrap();
        let id = format!("ATT-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        conn.execute(
            "INSERT INTO attendance_logs (id, member_id, member_name, direction, timestamp, confidence, tailgate_flag, synced_to_cloud, linked_member_id, person_count, acknowledged)
             VALUES (?1, NULL, ?2, 'in', ?3, NULL, 1, 0, ?4, ?5, 0)",
            params![id, display_name, now_str, linked_member_id, person_count],
        )?;

        Ok(AttendanceRecord {
            id,
            member_id: None,
            member_name: Some(display_name.to_string()),
            direction: "in".to_string(),
            confidence: None,
            tailgate_flag: true,
            timestamp: now,
            sync_status: "pending".to_string(),
            linked_member_id: linked_member_id.map(|s| s.to_string()),
            person_count,
        })
    }

    pub fn list_recent_attendance(&self, limit: usize) -> Result<Vec<AttendanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, member_name, direction, timestamp, confidence, tailgate_flag, linked_member_id, person_count
             FROM attendance_logs ORDER BY timestamp DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            let time_str: String = row.get(4)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(AttendanceRecord {
                id: row.get(0)?,
                member_id: row.get(1)?,
                member_name: row.get(2)?,
                direction: row.get(3)?,
                timestamp,
                confidence: row.get(5)?,
                tailgate_flag: row.get::<_, i32>(6)? == 1,
                sync_status: "synced".to_string(),
                linked_member_id: row.get(7).unwrap_or(None),
                person_count: row.get(8).unwrap_or(None),
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Phase D: tailgate incident history for the exe resolve-view (newest
    /// first). Uses `unwrap_or` on the Phase-A columns so pre-migration
    /// databases that somehow missed the ALTER still read instead of erroring.
    pub fn list_tailgate_incidents(&self, limit: usize) -> Result<Vec<AttendanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, member_name, direction, timestamp, confidence, tailgate_flag, linked_member_id, person_count
             FROM attendance_logs WHERE tailgate_flag = 1 ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let time_str: String = row.get(4)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(AttendanceRecord {
                id: row.get(0)?,
                member_id: row.get(1)?,
                member_name: row.get(2)?,
                direction: row.get(3)?,
                timestamp,
                confidence: row.get(5)?,
                tailgate_flag: true,
                sync_status: "synced".to_string(),
                linked_member_id: row.get(7).unwrap_or(None),
                person_count: row.get(8).unwrap_or(None),
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Phase D: marks a local tailgate incident reviewed. Returns true when a
    /// row was actually updated. Cloud acknowledgement is separate (owner/CEO
    /// ack via the dashboards); this only clears the local queue badge.
    pub fn resolve_tailgate_incident(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE attendance_logs SET acknowledged = 1 WHERE id = ?1 AND tailgate_flag = 1",
            params![id],
        )?;
        Ok(n > 0)
    }

    /// Phase D: unreviewed local tailgate incidents (drives the exe badge).
    pub fn count_unacked_tailgates(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM attendance_logs WHERE tailgate_flag = 1 AND acknowledged = 0",
            [],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    /// Queries the last recorded direction ('in' or 'out') for Anti-Passback validation
    pub fn get_member_last_direction(&self, member_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT direction FROM attendance_logs
             WHERE member_id = ?1
             ORDER BY timestamp DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map(params![member_id], |row| row.get::<_, String>(0))?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    // --- Inter-Branch Multi-Gym Sync Helpers ---

    pub fn get_unsynced_members(&self, owner_email: &str, home_gym_id: &Uuid, home_gym_name: &str) -> Result<Vec<gympos_shared::CloudMemberSyncItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, updated_at, expires_at, photo_data_url
             FROM members WHERE is_synced = 0"
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let first_name: String = row.get(1)?;
            let last_name: String = row.get(2)?;
            let email: String = row.get(3).unwrap_or_default();
            let phone: String = row.get(4).unwrap_or_default();
            let vectors_json: String = row.get(5).unwrap_or_else(|_| "[]".to_string());
            let status: String = row.get(6)?;
            let membership_type: String = row.get(7)?;
            let created_str: String = row.get(8)?;
            let updated_str: String = row.get(9)?;
            let expires_str: Option<String> = row.get(10).unwrap_or(None);
            let photo_data_url: Option<String> = row.get(11).unwrap_or(None);

            let face_vectors: Vec<Vec<f32>> = serde_json::from_str(&vectors_json).unwrap_or_default();
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = expires_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

            Ok(gympos_shared::CloudMemberSyncItem {
                id,
                home_gym_id: *home_gym_id,
                home_gym_name: home_gym_name.to_string(),
                owner_email: owner_email.to_string(),
                first_name,
                last_name,
                email,
                phone,
                membership_type,
                status,
                face_vectors,
                photo_data_url,
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

    pub fn mark_members_synced(&self, ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute("UPDATE members SET is_synced = 1 WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    pub fn upsert_interbranch_members(&self, members: &[gympos_shared::CloudMemberSyncItem]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut count = 0;
        for m in members {
            let vectors_json = serde_json::to_string(&m.face_vectors).unwrap_or_else(|_| "[]".to_string());
            let expires_str = m.expires_at.map(|e| e.to_rfc3339());

            conn.execute(
                "INSERT INTO members (id, home_gym_id, home_gym_name, first_name, last_name, email, phone, face_vector, status, membership_type, photo_data_url, created_at, updated_at, expires_at, is_synced)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 1)
                 ON CONFLICT(id) DO UPDATE SET
                    home_gym_name = excluded.home_gym_name,
                    first_name = excluded.first_name,
                    last_name = excluded.last_name,
                    email = excluded.email,
                    phone = excluded.phone,
                    face_vector = excluded.face_vector,
                    status = excluded.status,
                    membership_type = excluded.membership_type,
                    photo_data_url = excluded.photo_data_url,
                    updated_at = excluded.updated_at,
                    expires_at = excluded.expires_at,
                    is_synced = 1",
                params![
                    m.id,
                    m.home_gym_id.to_string(),
                    m.home_gym_name,
                    m.first_name,
                    m.last_name,
                    m.email,
                    m.phone,
                    vectors_json,
                    m.status,
                    m.membership_type,
                    m.photo_data_url,
                    m.created_at.to_rfc3339(),
                    m.updated_at.to_rfc3339(),
                    expires_str,
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn get_unsynced_attendance(&self) -> Result<Vec<gympos_shared::AttendanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, member_name, direction, timestamp, confidence, tailgate_flag, linked_member_id, person_count
             FROM attendance_logs WHERE synced_to_cloud = 0 ORDER BY timestamp ASC LIMIT 50"
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let member_id: Option<String> = row.get(1)?;
            let member_name: Option<String> = row.get(2)?;
            let direction: String = row.get(3)?;
            let time_str: String = row.get(4)?;
            let confidence: Option<f32> = row.get(5)?;
            let tailgate_flag: i32 = row.get(6)?;
            let linked_member_id: Option<String> = row.get(7).unwrap_or(None);
            let person_count: Option<i32> = row.get(8).unwrap_or(None);

            let timestamp = chrono::DateTime::parse_from_rfc3339(&time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(gympos_shared::AttendanceRecord {
                id,
                member_id,
                member_name,
                direction,
                confidence,
                tailgate_flag: tailgate_flag == 1,
                timestamp,
                sync_status: "pending".to_string(),
                linked_member_id,
                person_count,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn mark_attendance_synced(&self, ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute("UPDATE attendance_logs SET synced_to_cloud = 1 WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    pub fn count_today_checkins(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM attendance_logs WHERE direction = 'in' AND date(timestamp) = date('now')",
            [],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    pub fn count_tailgates(&self) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM attendance_logs WHERE tailgate_flag = 1 AND date(timestamp) = date('now')",
            [],
            |r| r.get(0),
        )?;
        Ok(count as usize)
    }

    // --- POS Products & Transactions ---

    pub fn list_products(&self) -> Result<Vec<ProductItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, price, stock, category FROM products")?;
        let rows = stmt.query_map([], |row| {
            Ok(ProductItem {
                id: row.get(0)?,
                name: row.get(1)?,
                price: row.get(2)?,
                stock: row.get(3)?,
                category: row.get(4)?,
            })
        })?;

        let mut products = Vec::new();
        for p in rows {
            products.push(p?);
        }
        Ok(products)
    }

    pub fn get_product_by_id(&self, id: &str) -> Result<Option<ProductItem>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, price, stock, category FROM products WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(ProductItem {
                id: row.get(0)?,
                name: row.get(1)?,
                price: row.get(2)?,
                stock: row.get(3)?,
                category: row.get(4)?,
            })
        })?;

        if let Some(p) = rows.next() {
            Ok(Some(p?))
        } else {
            Ok(None)
        }
    }

    pub fn create_product(&self, req: &CreateProductRequest) -> Result<ProductItem> {
        let conn = self.conn.lock().unwrap();
        let id = format!("prod-{}", Uuid::new_v4().to_string()[..8].to_lowercase());
        conn.execute(
            "INSERT INTO products (id, name, price, stock, category) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, req.name, req.price, req.stock, req.category],
        )?;

        Ok(ProductItem {
            id,
            name: req.name.clone(),
            price: req.price,
            stock: req.stock,
            category: req.category.clone(),
        })
    }

    pub fn update_product(&self, req: &UpdateProductRequest) -> Result<ProductItem> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE products SET name = ?1, price = ?2, stock = ?3, category = ?4 WHERE id = ?5",
            params![req.name, req.price, req.stock, req.category, req.id],
        )?;

        Ok(ProductItem {
            id: req.id.clone(),
            name: req.name.clone(),
            price: req.price,
            stock: req.stock,
            category: req.category.clone(),
        })
    }

    pub fn adjust_product_stock(&self, id: &str, delta: i32) -> Result<ProductItem> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE products SET stock = MAX(0, stock + ?1) WHERE id = ?2",
            params![delta, id],
        )?;
        drop(conn);
        self.get_product_by_id(id)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn delete_product(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM products WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn process_sale(
        &self,
        member_id: Option<&str>,
        items: &[CartItem],
        payment_method: &str,
        discount_type: &str,
        discount_pct: f64,
    ) -> Result<SaleTransaction> {
        let mut conn = self.conn.lock().unwrap();
        if items.is_empty() {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }

        let tx_id = format!("TX-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let gross: f64 = items.iter().map(|item| item.unit_price * (item.quantity as f64)).sum();
        // Clamp discount to 0-100%; Senior/PWD statutory 20% is applied client-side via discount_pct
        let pct = discount_pct.clamp(0.0, 100.0);
        let discount_amount = (gross * pct / 100.0 * 100.0).round() / 100.0;
        let total_amount = ((gross - discount_amount) * 100.0).round() / 100.0;
        let dtype = if pct > 0.0 && !discount_type.is_empty() {
            discount_type.to_string()
        } else {
            String::new()
        };
        let items_json = serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string());

        let tx = conn.transaction()?;

        // Deduct inventory stock — atomic check: fail if requested qty exceeds available stock
        for item in items {
            if item.quantity > 0 {
                // Verify sufficient stock first, then deduct atomically with guard `stock >= qty`
                let current_stock: i32 = tx
                    .query_row("SELECT stock FROM products WHERE id = ?1", params![item.product_id], |r| r.get(0))
                    .unwrap_or(0);
                if current_stock < item.quantity as i32 {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "Insufficient stock for {}: have {}, requested {}",
                        item.product_id, current_stock, item.quantity
                    )));
                }
                let changed = tx.execute(
                    "UPDATE products SET stock = stock - ?1 WHERE id = ?2 AND stock >= ?1",
                    params![item.quantity, item.product_id],
                )?;
                if changed == 0 {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "Concurrent stock depletion for {} during checkout",
                        item.product_id
                    )));
                }
            }
        }

        // Record transaction
        tx.execute(
            "INSERT INTO transactions (id, member_id, total_amount, payment_method, items_json, created_at, synced_to_cloud, discount_type, discount_amount)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            params![tx_id, member_id, total_amount, payment_method, items_json, now.to_rfc3339(), dtype, discount_amount],
        )?;

        tx.commit()?;

        Ok(SaleTransaction {
            id: tx_id,
            member_id: member_id.map(|s| s.to_string()),
            total_amount,
            payment_method: payment_method.to_string(),
            items: items.to_vec(),
            timestamp: now,
            discount_type: dtype,
            discount_amount,
        })
    }

    pub fn get_unsynced_sales(&self) -> Result<Vec<SaleTransaction>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, total_amount, payment_method, items_json, created_at, discount_type, discount_amount
             FROM transactions WHERE synced_to_cloud = 0 ORDER BY created_at ASC LIMIT 50",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let member_id: Option<String> = row.get(1)?;
            let total_amount: f64 = row.get(2)?;
            let payment_method: String = row.get(3)?;
            let items_json: String = row.get(4)?;
            let items: Vec<CartItem> = serde_json::from_str(&items_json).unwrap_or_default();
            let created_at_str: String = row.get(5)?;
            let timestamp = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(SaleTransaction {
                id,
                member_id,
                total_amount,
                payment_method,
                items,
                timestamp,
                discount_type: row.get::<_, Option<String>>(6).unwrap_or(None).unwrap_or_default(),
                discount_amount: row.get::<_, Option<f64>>(7).unwrap_or(None).unwrap_or(0.0),
            })
        })?;

        let mut list = Vec::new();
        for s in rows {
            list.push(s?);
        }
        Ok(list)
    }

    pub fn mark_sales_synced(&self, ids: &[String]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute("UPDATE transactions SET synced_to_cloud = 1 WHERE id = ?1", params![id])?;
        }
        Ok(())
    }

    // --- Expenses Ledger ---

    pub fn create_expense(&self, req: &CreateExpenseRequest, created_by: &str) -> Result<ExpenseRecord> {
        let conn = self.conn.lock().unwrap();
        let id = format!("EXP-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let spent = req.spent_at.unwrap_or(now);
        if req.title.trim().is_empty() {
            return Err(rusqlite::Error::InvalidParameterName("Expense title is required".to_string()));
        }
        if req.amount < 0.0 {
            return Err(rusqlite::Error::InvalidParameterName("Expense amount cannot be negative".to_string()));
        }
        conn.execute(
            "INSERT INTO expenses (id, title, category, amount, payment_method, notes, spent_at, created_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, req.title.trim(), req.category, req.amount, req.payment_method, req.notes, spent.to_rfc3339(), created_by, now.to_rfc3339()],
        )?;
        Ok(ExpenseRecord {
            id,
            title: req.title.trim().to_string(),
            category: req.category.clone(),
            amount: req.amount,
            payment_method: req.payment_method.clone(),
            notes: req.notes.clone(),
            spent_at: spent,
            created_by: created_by.to_string(),
        })
    }

    pub fn list_expenses(&self, limit: i64) -> Result<Vec<ExpenseRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, category, amount, payment_method, notes, spent_at, created_by
             FROM expenses ORDER BY spent_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit.max(1).min(500)], |row| {
            let spent_str: String = row.get(6)?;
            let spent_at = DateTime::parse_from_rfc3339(&spent_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(ExpenseRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                category: row.get(2)?,
                amount: row.get(3)?,
                payment_method: row.get(4)?,
                notes: row.get::<_, Option<String>>(5).unwrap_or(None).unwrap_or_default(),
                spent_at,
                created_by: row.get::<_, Option<String>>(7).unwrap_or(None).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_expense(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM expenses WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// End-of-Day Z-report: sales (gross/discount/net, by payment method),
    /// walk-in revenue, attendance counts, and expenses for a calendar day (UTC).
    pub fn get_end_of_day(&self, day: &str) -> Result<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let like = format!("{}%", day);
        // Sales by payment method
        let mut stmt = conn.prepare(
            "SELECT payment_method, COUNT(*), COALESCE(SUM(total_amount), 0), COALESCE(SUM(discount_amount), 0)
             FROM transactions WHERE created_at LIKE ?1 GROUP BY payment_method",
        )?;
        let mut by_method = Vec::new();
        let mut tx_count = 0i64;
        let mut net = 0.0f64;
        let mut discounts = 0.0f64;
        for row in stmt.query_map(params![like], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?))
        })? {
            let (method, n, sum, disc) = row?;
            tx_count += n;
            net += sum;
            discounts += disc;
            by_method.push(serde_json::json!({"payment_method": method, "count": n, "net": sum, "discounts": disc}));
        }
        // Discounted transaction count
        let disc_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE created_at LIKE ?1 AND discount_amount > 0",
                params![like],
                |r| r.get(0),
            )
            .unwrap_or(0);
        // Walk-ins
        let (walk_count, walk_rev): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(amount_paid), 0) FROM walk_ins WHERE created_at LIKE ?1",
                params![like],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0.0));
        // Attendance
        let checkins: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attendance_logs WHERE timestamp LIKE ?1 AND direction = 'in'",
                params![like],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let tailgates: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM attendance_logs WHERE timestamp LIKE ?1 AND tailgate_flag = 1",
                params![like],
                |r| r.get(0),
            )
            .unwrap_or(0);
        // Expenses
        let (exp_count, exp_total): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(amount), 0) FROM expenses WHERE spent_at LIKE ?1",
                params![like],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0.0));
        Ok(serde_json::json!({
            "day": day,
            "transactions": tx_count,
            "gross": net + discounts,
            "discounts": discounts,
            "discounted_transactions": disc_count,
            "net_sales": net,
            "by_payment_method": by_method,
            "walk_ins": walk_count,
            "walk_in_revenue": walk_rev,
            "check_ins": checkins,
            "tailgate_flags": tailgates,
            "expense_count": exp_count,
            "expense_total": exp_total,
            "net_cash_flow": net + walk_rev - exp_total,
        }))
    }

    pub fn ingest_remote_catalog(
        &self,
        products: &[RemoteCatalogProduct],
        plans: &[MembershipPlanConfig],
        promos: &[PromoVoucherConfig],
    ) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut count = 0;
        for p in products {
            conn.execute(
                "INSERT INTO products (id, name, price, stock, category)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, price = ?3, stock = ?4, category = ?5",
                params![p.id, p.name, p.price, p.stock, p.category],
            )?;
            count += 1;
        }
        for p in plans {
            let benefits_json = serde_json::to_string(&p.benefits).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT INTO remote_plans (id, name, tag, billing_period, price_monthly, student_discount_pct, benefits_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, tag = ?3, billing_period = ?4, price_monthly = ?5, student_discount_pct = ?6, benefits_json = ?7, updated_at = ?8",
                params![p.id, p.name, p.tag, p.billing_period, p.price_monthly, p.student_discount_pct, benefits_json, p.updated_at.to_rfc3339()],
            )?;
            count += 1;
        }
        for pr in promos {
            let exp = pr.expires_at.map(|e| e.to_rfc3339());
            conn.execute(
                "INSERT INTO remote_promos (code, label, discount_type, discount_value, min_spend, expires_at, is_active)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(code) DO UPDATE SET label = ?2, discount_type = ?3, discount_value = ?4, min_spend = ?5, expires_at = ?6, is_active = ?7",
                params![pr.code, pr.label, pr.discount_type, pr.discount_value, pr.min_spend, exp, if pr.is_active { 1 } else { 0 }],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn list_remote_plans(&self) -> Result<Vec<MembershipPlanConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, tag, billing_period, price_monthly, student_discount_pct, benefits_json, updated_at FROM remote_plans ORDER BY price_monthly",
        )?;
        let rows = stmt.query_map([], |row| {
            let benefits_json: String = row.get(6)?;
            let benefits = serde_json::from_str(&benefits_json).unwrap_or_default();
            let updated_str: String = row.get(7)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(MembershipPlanConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                tag: row.get::<_, Option<String>>(2).unwrap_or(None).unwrap_or_default(),
                billing_period: row.get::<_, Option<String>>(3).unwrap_or(None).unwrap_or_else(|| "monthly".to_string()),
                price_monthly: row.get(4)?,
                student_discount_pct: row.get(5)?,
                target_gym_id: None,
                benefits,
                updated_at,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn list_remote_promos(&self) -> Result<Vec<PromoVoucherConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT code, label, discount_type, discount_value, min_spend, expires_at, is_active FROM remote_promos WHERE is_active = 1",
        )?;
        let rows = stmt.query_map([], |row| {
            let exp_str: Option<String> = row.get(5)?;
            let expires_at = exp_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc)));
            let active_int: i32 = row.get(6)?;
            Ok(PromoVoucherConfig {
                code: row.get(0)?,
                label: row.get::<_, Option<String>>(1).unwrap_or(None).unwrap_or_default(),
                discount_type: row.get(2)?,
                discount_value: row.get(3)?,
                min_spend: row.get(4)?,
                expires_at,
                is_active: active_int == 1,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // --- Coaches CRUD Operations ---

    pub fn list_coaches(&self) -> Result<Vec<Coach>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, specialty, phone, active_students FROM coaches")?;
        let rows = stmt.query_map([], |row| {
            let students: i32 = row.get(4)?;
            Ok(Coach {
                id: row.get(0)?,
                name: row.get(1)?,
                specialty: row.get(2)?,
                phone: row.get(3)?,
                active_students: students as usize,
            })
        })?;

        let mut coaches = Vec::new();
        for c in rows {
            coaches.push(c?);
        }
        Ok(coaches)
    }

    pub fn create_coach(&self, req: &CreateCoachRequest) -> Result<Coach> {
        let conn = self.conn.lock().unwrap();
        let id = format!("coach-{}", Uuid::new_v4().to_string()[..8].to_lowercase());
        conn.execute(
            "INSERT INTO coaches (id, name, specialty, phone, active_students) VALUES (?1, ?2, ?3, ?4, 0)",
            params![id, req.name, req.specialty, req.phone],
        )?;

        Ok(Coach {
            id,
            name: req.name.clone(),
            specialty: req.specialty.clone(),
            phone: req.phone.clone(),
            active_students: 0,
        })
    }

    pub fn update_coach(&self, req: &UpdateCoachRequest) -> Result<Coach> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE coaches SET name = ?1, specialty = ?2, phone = ?3 WHERE id = ?4",
            params![req.name, req.specialty, req.phone, req.id],
        )?;

        let students: i32 = conn.query_row("SELECT active_students FROM coaches WHERE id = ?1", params![req.id], |r| r.get(0)).unwrap_or(0);

        Ok(Coach {
            id: req.id.clone(),
            name: req.name.clone(),
            specialty: req.specialty.clone(),
            phone: req.phone.clone(),
            active_students: students as usize,
        })
    }

    pub fn delete_coach(&self, id: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM coach_sessions WHERE coach_id = ?1", params![id])?;
        tx.execute("DELETE FROM coaches WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn schedule_session(&self, coach_id: &str, coach_name: &str, member_id: &str, member_name: &str, date: &str, duration: u32) -> Result<CoachSession> {
        let conn = self.conn.lock().unwrap();
        let id = format!("SES-{}", Uuid::new_v4().to_string()[..8].to_uppercase());

        conn.execute(
            "INSERT INTO coach_sessions (id, coach_id, coach_name, member_id, member_name, session_date, duration_minutes, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'scheduled')",
            params![id, coach_id, coach_name, member_id, member_name, date, duration],
        )?;

        // Increment coach active students
        let _ = conn.execute("UPDATE coaches SET active_students = active_students + 1 WHERE id = ?1", params![coach_id]);

        Ok(CoachSession {
            id,
            coach_id: coach_id.to_string(),
            coach_name: coach_name.to_string(),
            member_id: member_id.to_string(),
            member_name: member_name.to_string(),
            scheduled_at: date.to_string(),
            duration_minutes: duration,
        })
    }

    pub fn list_coach_sessions(&self) -> Result<Vec<CoachSession>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, coach_id, coach_name, member_id, member_name, session_date, duration_minutes FROM coach_sessions WHERE status != 'cancelled' ORDER BY session_date DESC")?;
        let rows = stmt.query_map([], |row| {
            let dur: i32 = row.get(6)?;
            Ok(CoachSession {
                id: row.get(0)?,
                coach_id: row.get(1)?,
                coach_name: row.get(2)?,
                member_id: row.get(3)?,
                member_name: row.get(4)?,
                scheduled_at: row.get(5)?,
                duration_minutes: dur as u32,
            })
        })?;

        let mut sessions = Vec::new();
        for s in rows {
            sessions.push(s?);
        }
        Ok(sessions)
    }

    pub fn cancel_coach_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let coach_id: Option<String> = conn.query_row("SELECT coach_id FROM coach_sessions WHERE id = ?1", params![session_id], |r| r.get(0)).ok();
        conn.execute("UPDATE coach_sessions SET status = 'cancelled' WHERE id = ?1", params![session_id])?;
        if let Some(cid) = coach_id {
            let _ = conn.execute("UPDATE coaches SET active_students = MAX(0, active_students - 1) WHERE id = ?1", params![cid]);
        }
        Ok(())
    }

    // --- Walk-In Extend & Void ---

    pub fn extend_walk_in(&self, id: &str, extra_hours: i64) -> Result<WalkInRecord> {
        let conn = self.conn.lock().unwrap();
        let cur_expires: String = conn.query_row("SELECT expires_at FROM walk_ins WHERE id = ?1", params![id], |r| r.get(0))?;
        let cur_dt = chrono::DateTime::parse_from_rfc3339(&cur_expires)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        // Renew semantics: an already-expired pass restarts from NOW instead
        // of stacking hours onto a long-dead expiry.
        let base = cur_dt.max(Utc::now());
        let new_dt = base + Duration::hours(extra_hours);

        conn.execute("UPDATE walk_ins SET expires_at = ?1 WHERE id = ?2", params![new_dt.to_rfc3339(), id])?;
        drop(conn);

        let walkins = self.list_walk_ins()?;
        walkins.into_iter().find(|w| w.id == id).ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    /// Staff/manager Renew: fresh 8-hour pass from NOW reusing the stored
    /// enrollment vector — no re-face-scan needed.
    pub fn renew_walk_in(&self, id: &str) -> Result<WalkInRecord> {
        let conn = self.conn.lock().unwrap();
        let new_dt = Utc::now() + Duration::hours(8);
        let changed = conn.execute(
            "UPDATE walk_ins SET expires_at = ?1 WHERE id = ?2",
            params![new_dt.to_rfc3339(), id],
        )?;
        if changed == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        drop(conn);
        let walkins = self.list_walk_ins()?;
        walkins.into_iter().find(|w| w.id == id).ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn void_walk_in(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now() - Duration::hours(1);
        conn.execute("UPDATE walk_ins SET expires_at = ?1 WHERE id = ?2", params![now.to_rfc3339(), id])?;
        Ok(())
    }

    // --- Local Staff & Cashier RBAC Methods ---

    pub fn upsert_synced_staff(&self, staff_list: &[StaffAccount]) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let mut count = 0;
        for s in staff_list {
            let gym_id_str = s.gym_id.map(|u| u.to_string());
            conn.execute(
                "INSERT INTO local_staff_accounts (id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    full_name = ?5,
                    username = ?6,
                    pin_hash = ?7,
                    role = ?8,
                    gym_id = ?3,
                    gym_name = ?4,
                    is_active = ?9,
                    updated_at = ?10",
                params![
                    s.id,
                    s.owner_email,
                    gym_id_str,
                    s.gym_name,
                    s.full_name,
                    s.username,
                    s.pin_hash,
                    match s.role {
                        StaffRole::Manager => "manager",
                        StaffRole::Owner => "owner",
                        StaffRole::Staff => "staff",
                    },
                    if s.is_active { 1 } else { 0 },
                    s.updated_at.to_rfc3339(),
                ],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Authenticates a cashier/manager PIN. Argon2id hashes are salted, so a
    /// direct `WHERE pin_hash = ?` lookup (the previous SHA-256 approach) is no
    /// longer possible — instead we scan the (small, per-branch) active staff
    /// list and verify the PIN against each stored hash. A successful login
    /// against a legacy unsalted-SHA-256 hash transparently re-hashes the PIN
    /// with Argon2id so pre-migration accounts stop being rainbow-tableable.
    pub fn authenticate_staff_pin(&self, pin: &str) -> Result<Option<StaffAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, updated_at
             FROM local_staff_accounts
             WHERE is_active = 1",
        )?;

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let pin_hash: String = row.get(6)?;
            if !gympos_shared::verify_password(pin, &pin_hash) {
                continue;
            }

            let id: String = row.get(0)?;
            let owner_email: String = row.get(1)?;
            let gym_id_str: Option<String> = row.get(2)?;
            let gym_id = gym_id_str.and_then(|s| Uuid::parse_str(&s).ok());
            let gym_name: Option<String> = row.get(3)?;
            let full_name: String = row.get(4)?;
            let username: String = row.get(5)?;
            let role_str: String = row.get(7)?;
            let role = match role_str.as_str() {
                "manager" => StaffRole::Manager,
                "owner" => StaffRole::Owner,
                _ => StaffRole::Staff,
            };
            let is_active_int: i32 = row.get(8)?;
            let updated_at_str: String = row.get(9)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            drop(rows);
            drop(stmt);

            if gympos_shared::password_is_legacy(&pin_hash) {
                let upgraded = gympos_shared::hash_password(pin);
                if let Err(e) = conn.execute(
                    "UPDATE local_staff_accounts SET pin_hash = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![upgraded, Utc::now().to_rfc3339(), id],
                ) {
                    tracing::warn!("Failed to upgrade legacy PIN hash for staff {}: {}", id, e);
                }
            }

            return Ok(Some(StaffAccount {
                id,
                owner_email,
                gym_id,
                gym_name,
                full_name,
                username,
                pin_hash,
                role,
                is_active: is_active_int > 0,
                created_at: updated_at,
                updated_at,
            }));
        }
        Ok(None)
    }

    pub fn list_local_staff(&self) -> Result<Vec<StaffAccount>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, updated_at
             FROM local_staff_accounts
             ORDER BY full_name",
        )?;

        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let owner_email: String = row.get(1)?;
            let gym_id_str: Option<String> = row.get(2)?;
            let gym_id = gym_id_str.and_then(|s| Uuid::parse_str(&s).ok());
            let gym_name: Option<String> = row.get(3)?;
            let full_name: String = row.get(4)?;
            let username: String = row.get(5)?;
            let pin_hash: String = row.get(6)?;
            let role_str: String = row.get(7)?;
            let role = match role_str.as_str() {
                "manager" => StaffRole::Manager,
                "owner" => StaffRole::Owner,
                _ => StaffRole::Staff,
            };
            let is_active_int: i32 = row.get(8)?;
            let updated_at_str: String = row.get(9)?;
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
                created_at: updated_at,
                updated_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delete_member_cascades_foreign_keys() {
        let db = Database::in_memory().unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        }

        let req = CreateMemberRequest {
            first_name: "Test".to_string(),
            last_name: "Member".to_string(),
            email: "test@example.com".to_string(),
            phone: "1234567890".to_string(),
            membership_type: "regular".to_string(),
            face_vectors: vec![vec![0.1; 128]],
            photo_data_url: None,
        };

        let member = db.create_member(&req).unwrap();
        let member_id = member.id.clone();

        // 1. Add attendance log referencing member
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO attendance_logs (id, member_id, member_name, direction, timestamp, confidence)
                 VALUES ('ATT-1', ?1, 'Test Member', 'in', '2026-09-05T00:00:00Z', 0.95)",
                params![member_id],
            ).unwrap();
        }

        // 2. Add coach and session referencing member
        let coach_req = CreateCoachRequest {
            name: "Coach John".to_string(),
            specialty: "Fitness".to_string(),
            phone: "09171234567".to_string(),
        };
        let coach = db.create_coach(&coach_req).unwrap();
        db.schedule_session(&coach.id, &coach.name, &member_id, "Test Member", "2026-09-06T10:00:00Z", 60).unwrap();

        // 3. Test re-scan: update_member_vectors succeeds
        let rescanned = db.update_member_vectors(&member_id, &[vec![0.2; 128]], None).unwrap();
        assert_eq!(rescanned.face_vectors.len(), 1);
        assert!((rescanned.face_vectors[0][0] - 0.2).abs() < 1e-5);

        // 4. Test delete_member: must not fail with FOREIGN KEY constraint failed
        db.delete_member(&member_id).unwrap();

        // Verify member is gone
        assert!(db.get_member_by_id(&member_id).unwrap().is_none());

        // Verify attendance log remains for auditing but with member_id NULL
        {
            let conn = db.conn.lock().unwrap();
            let att_member_id: Option<String> = conn.query_row(
                "SELECT member_id FROM attendance_logs WHERE id = 'ATT-1'",
                [],
                |r| r.get(0),
            ).unwrap();
            assert_eq!(att_member_id, None);

            // Verify coach session was deleted
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM coach_sessions WHERE member_id = ?1",
                params![member_id],
                |r| r.get(0),
            ).unwrap();
            assert_eq!(count, 0);
        }

        // 5. Test delete_coach: must not fail with FOREIGN KEY constraint
        let other_req = CreateMemberRequest {
            first_name: "Other".to_string(),
            last_name: "Member".to_string(),
            email: "other@example.com".to_string(),
            phone: "9876543210".to_string(),
            membership_type: "regular".to_string(),
            face_vectors: vec![vec![0.1; 128]],
            photo_data_url: None,
        };
        let other = db.create_member(&other_req).unwrap();
        db.schedule_session(&coach.id, &coach.name, &other.id, "Other Member", "2026-09-07T10:00:00Z", 60).unwrap();
        db.delete_coach(&coach.id).unwrap();
        assert!(!db.list_coaches().unwrap().iter().any(|c| c.id == coach.id));
    }
}
