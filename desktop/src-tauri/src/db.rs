use chrono::{DateTime, Duration, Utc};
use gympos_shared::{
    AppSettings, AttendanceRecord, CartItem, Coach, CoachSession, CreateCoachRequest, CreateMemberRequest,
    CreateProductRequest, CreateWalkInRequest, Member, ProductItem, SaleTransaction, UpdateCoachRequest,
    UpdateMemberRequest, UpdateProductRequest, WalkInRecord,
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
                cached_at TEXT NOT NULL
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
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                email TEXT,
                phone TEXT,
                face_vector TEXT, -- JSON array of float arrays
                status TEXT NOT NULL DEFAULT 'active',
                membership_type TEXT NOT NULL DEFAULT 'regular',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
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
                FOREIGN KEY (member_id) REFERENCES members(id)
            );

            CREATE TABLE IF NOT EXISTS products (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                price REAL NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                category TEXT NOT NULL
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
                duration_minutes INTEGER NOT NULL,
                status TEXT NOT NULL DEFAULT 'scheduled',
                FOREIGN KEY (coach_id) REFERENCES coaches(id),
                FOREIGN KEY (member_id) REFERENCES members(id)
            );
            "#,
        )?;

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

        Ok(())
    }

    // --- App Settings (White-Label Branding) ---

    pub fn get_app_settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT gym_name, logo_data_url, theme_color, walk_in_rate FROM app_settings WHERE id = 1")?;
        let res = stmt.query_row([], |row| {
            Ok(AppSettings {
                gym_name: row.get(0)?,
                logo_data_url: row.get(1)?,
                theme_color: row.get(2)?,
                walk_in_rate: row.get(3)?,
            })
        });

        match res {
            Ok(s) => Ok(s),
            Err(_) => Ok(AppSettings::default()),
        }
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_settings (id, gym_name, logo_data_url, theme_color, walk_in_rate)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                gym_name = excluded.gym_name,
                logo_data_url = excluded.logo_data_url,
                theme_color = excluded.theme_color,
                walk_in_rate = excluded.walk_in_rate",
            params![
                settings.gym_name,
                settings.logo_data_url,
                settings.theme_color,
                settings.walk_in_rate
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
        })
    }

    pub fn list_walk_ins(&self) -> Result<Vec<WalkInRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, guest_name, phone, amount_paid, payment_method, created_at, expires_at 
             FROM walk_ins ORDER BY created_at DESC LIMIT 50",
        )?;

        let rows = stmt.query_map([], |row| {
            let created_str: String = row.get(5)?;
            let expires_str: String = row.get(6)?;

            let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(WalkInRecord {
                id: row.get(0)?,
                guest_name: row.get(1)?,
                phone: row.get(2)?,
                amount_paid: row.get(3)?,
                payment_method: row.get(4)?,
                created_at,
                expires_at,
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
            "INSERT INTO members (id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9)",
            params![
                id,
                req.first_name,
                req.last_name,
                req.email,
                req.phone,
                vectors_json,
                req.membership_type,
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
            created_at: now,
            expires_at: None,
        })
    }

    pub fn list_members(&self) -> Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at 
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
            "SELECT id, first_name, last_name, email, phone, face_vector, status, membership_type, created_at 
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
                updated_at = ?7
             WHERE id = ?8",
            params![
                req.first_name,
                req.last_name,
                req.email,
                req.phone,
                req.membership_type,
                req.status,
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
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM members WHERE id = ?1", params![id])?;
        Ok(())
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
        })
    }

    pub fn list_recent_attendance(&self, limit: usize) -> Result<Vec<AttendanceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, member_name, direction, timestamp, confidence, tailgate_flag 
             FROM attendance_logs ORDER BY timestamp DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            let time_str: String = row.get(4)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&time_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let tg: i32 = row.get(6)?;

            Ok(AttendanceRecord {
                id: row.get(0)?,
                member_id: row.get(1)?,
                member_name: row.get(2)?,
                direction: row.get(3)?,
                timestamp,
                confidence: row.get(5)?,
                tailgate_flag: tg == 1,
                sync_status: "local".to_string(),
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
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

    pub fn process_sale(&self, member_id: Option<&str>, items: &[CartItem], payment_method: &str) -> Result<SaleTransaction> {
        let mut conn = self.conn.lock().unwrap();
        if items.is_empty() {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }

        let tx_id = format!("TX-{}", Uuid::new_v4().to_string()[..8].to_uppercase());
        let now = Utc::now();
        let total_amount: f64 = items.iter().map(|item| item.unit_price * (item.quantity as f64)).sum();
        let items_json = serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string());

        let tx = conn.transaction()?;

        // Deduct inventory stock
        for item in items {
            if item.quantity > 0 {
                tx.execute(
                    "UPDATE products SET stock = MAX(0, stock - ?1) WHERE id = ?2",
                    params![item.quantity, item.product_id],
                )?;
            }
        }

        // Record transaction
        tx.execute(
            "INSERT INTO transactions (id, member_id, total_amount, payment_method, items_json, created_at, synced_to_cloud)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![tx_id, member_id, total_amount, payment_method, items_json, now.to_rfc3339()],
        )?;

        tx.commit()?;

        Ok(SaleTransaction {
            id: tx_id,
            member_id: member_id.map(|s| s.to_string()),
            total_amount,
            payment_method: payment_method.to_string(),
            items: items.to_vec(),
            timestamp: now,
        })
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
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM coaches WHERE id = ?1", params![id])?;
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
        let new_dt = cur_dt + Duration::hours(extra_hours);

        conn.execute("UPDATE walk_ins SET expires_at = ?1 WHERE id = ?2", params![new_dt.to_rfc3339(), id])?;
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
}
