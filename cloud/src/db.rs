use chrono::{DateTime, Utc};
use gympos_shared::{
    CartItem, LicenseTier, MembershipPlanConfig, OwnerBranchSummary, OwnerDashboardAnalytics,
    OwnerHierarchyAccount, OwnerHierarchyBranch, PromoVoucherConfig, ReleaseInfo, RemoteCatalogProduct,
    SaleTransaction, StaffAccount, StaffRole, UpdateGymRequest, UpdateStaffRequest,
};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::models::GymRecord;

/// Cloud database backed by Postgres (`DATABASE_URL`).
/// Single permanent backend for every environment — local runs point at the
/// same Render `gympos-db` so CEOs/owners can never be split-brained or wiped
/// by ephemeral container filesystems. REAL columns are DOUBLE PRECISION
/// (Postgres REAL is float4 and would not decode into f64).
#[derive(Clone)]
pub struct CloudDatabase {
    pool: PgPool,
}

impl CloudDatabase {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(database_url).await?;
        let db = Self { pool };
        db.init_schema().await?;
        Ok(db)
    }

    async fn init_schema(&self) -> Result<(), sqlx::Error> {
        // NOTE: sqlx uses the extended protocol (one statement per query), so
        // each DDL statement is executed individually rather than as a batch.
        const TABLES: &[&str] = &[
            r#"CREATE TABLE IF NOT EXISTS cloud_gyms (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                tier TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_disabled_gyms (
                gym_id TEXT PRIMARY KEY,
                disabled_at TEXT NOT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_licenses (
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
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_members (
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
                expires_at TEXT,
                photo_data_url TEXT
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_attendance (
                id TEXT PRIMARY KEY,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                member_id TEXT,
                member_name TEXT,
                direction TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                confidence DOUBLE PRECISION,
                tailgate_flag INTEGER NOT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_owner_accounts (
                owner_email TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL,
                company_name TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_ceo_accounts (
                ceo_email TEXT PRIMARY KEY,
                password_hash TEXT NOT NULL,
                display_name TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_products (
                id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                name TEXT NOT NULL,
                price DOUBLE PRECISION NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                category TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(id, owner_email)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_plans (
                id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                name TEXT NOT NULL,
                tag TEXT NOT NULL DEFAULT '',
                billing_period TEXT NOT NULL DEFAULT 'monthly',
                price_monthly DOUBLE PRECISION NOT NULL,
                student_discount_pct DOUBLE PRECISION NOT NULL DEFAULT 0,
                benefits_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(id, owner_email)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_promos (
                code TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                label TEXT NOT NULL DEFAULT '',
                discount_type TEXT NOT NULL,
                discount_value DOUBLE PRECISION NOT NULL,
                min_spend DOUBLE PRECISION NOT NULL DEFAULT 0,
                expires_at TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY(code, owner_email)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_sales (
                id TEXT PRIMARY KEY,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                member_id TEXT,
                total_amount DOUBLE PRECISION NOT NULL,
                payment_method TEXT NOT NULL,
                items_json TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                discount_type TEXT NOT NULL DEFAULT '',
                discount_amount DOUBLE PRECISION NOT NULL DEFAULT 0
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_releases (
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
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_staff_accounts (
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
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_branch_product_overrides (
                product_id TEXT NOT NULL,
                gym_id TEXT NOT NULL,
                owner_email TEXT NOT NULL,
                price DOUBLE PRECISION NOT NULL,
                stock INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                PRIMARY KEY(product_id, gym_id)
            )"#,
            r#"CREATE TABLE IF NOT EXISTS cloud_audit_logs (
                id TEXT PRIMARY KEY,
                owner_email TEXT NOT NULL,
                gym_id TEXT,
                action TEXT NOT NULL,
                target TEXT,
                timestamp TEXT NOT NULL
            )"#,
        ];
        for ddl in TABLES {
            sqlx::raw_sql(*ddl).execute(&self.pool).await?;
        }
        // Indices: pre-existing shapes plus the hot WHERE shapes from the
        // scale audit (members/attendance/sales by gym+time, licenses by gym,
        // staff/catalog by owner, releases by channel).
        const INDICES: &[&str] = &[
            "CREATE INDEX IF NOT EXISTS idx_gyms_owner ON cloud_gyms(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_licenses_owner ON cloud_licenses(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_licenses_gym ON cloud_licenses(gym_id)",
            "CREATE INDEX IF NOT EXISTS idx_members_owner ON cloud_members(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_members_gym_status ON cloud_members(home_gym_id, status)",
            "CREATE INDEX IF NOT EXISTS idx_attendance_gym_ts ON cloud_attendance(gym_id, timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_attendance_owner_ts ON cloud_attendance(owner_email, timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_sales_gym_ts ON cloud_sales(gym_id, timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_sales_owner_ts ON cloud_sales(owner_email, timestamp)",
            "CREATE INDEX IF NOT EXISTS idx_staff_owner_gym ON cloud_staff_accounts(owner_email, gym_id)",
            "CREATE INDEX IF NOT EXISTS idx_products_owner ON cloud_products(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_plans_owner ON cloud_plans(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_promos_owner ON cloud_promos(owner_email)",
            "CREATE INDEX IF NOT EXISTS idx_releases_channel_ts ON cloud_releases(channel, created_at)",
            "CREATE INDEX IF NOT EXISTS idx_audit_owner_ts ON cloud_audit_logs(owner_email, timestamp)",
        ];
        for idx in INDICES {
            sqlx::raw_sql(*idx).execute(&self.pool).await?;
        }
        // Legacy-alignment migrations. Postgres has no ADD COLUMN IF NOT
        // EXISTS, so probe information_schema first and add only when missing.
        // Errors are ignored: concurrent boots may race the same ALTER.
        // `const` (not an inline array) so the DDL keeps its 'static lifetime
        // for sqlx::raw_sql's SqlSafeStr bound.
        const MIGRATIONS: &[(&str, &str, &str)] = &[
            ("cloud_owner_accounts", "is_verified", "ALTER TABLE cloud_owner_accounts ADD COLUMN is_verified INTEGER NOT NULL DEFAULT 0"),
            ("cloud_owner_accounts", "failed_attempts", "ALTER TABLE cloud_owner_accounts ADD COLUMN failed_attempts INTEGER NOT NULL DEFAULT 0"),
            ("cloud_owner_accounts", "locked_until", "ALTER TABLE cloud_owner_accounts ADD COLUMN locked_until TEXT"),
            // Member reference photos (local-first capture, cloud-synced for sister-branch reference)
            ("cloud_members", "photo_data_url", "ALTER TABLE cloud_members ADD COLUMN photo_data_url TEXT"),
            // Customizable rate-card fields (owner-defined names/tags)
            ("cloud_plans", "tag", "ALTER TABLE cloud_plans ADD COLUMN tag TEXT NOT NULL DEFAULT ''"),
            ("cloud_plans", "billing_period", "ALTER TABLE cloud_plans ADD COLUMN billing_period TEXT NOT NULL DEFAULT 'monthly'"),
            ("cloud_promos", "label", "ALTER TABLE cloud_promos ADD COLUMN label TEXT NOT NULL DEFAULT ''"),
            ("cloud_sales", "discount_type", "ALTER TABLE cloud_sales ADD COLUMN discount_type TEXT NOT NULL DEFAULT ''"),
            ("cloud_sales", "discount_amount", "ALTER TABLE cloud_sales ADD COLUMN discount_amount DOUBLE PRECISION NOT NULL DEFAULT 0"),
        ];
        for (table, column, ddl) in MIGRATIONS {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_name = $1 AND column_name = $2)",
            )
            .bind(table)
            .bind(column)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(true);
            if !exists {
                let _ = sqlx::raw_sql(*ddl).execute(&self.pool).await;
            }
        }
        Ok(())
    }

    pub async fn load_all_gyms(&self) -> sqlx::Result<HashMap<Uuid, GymRecord>> {
        let rows = sqlx::query("SELECT id, name, owner_email, tier, is_active, created_at FROM cloud_gyms")
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for row in rows {
            let id_str: String = row.try_get(0)?;
            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
            let name: String = row.try_get(1)?;
            let owner_email: String = row.try_get(2)?;
            let tier_str: String = row.try_get(3)?;
            let is_active: i32 = row.try_get(4)?;
            let created_str: String = row.try_get(5)?;

            let tier = match tier_str.to_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Basic,
            };

            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let gym = GymRecord {
                id,
                name,
                owner_email,
                tier,
                is_active: is_active == 1,
                created_at,
            };
            map.insert(gym.id, gym);
        }
        Ok(map)
    }

    pub async fn load_disabled_gyms(&self) -> sqlx::Result<HashSet<Uuid>> {
        let rows = sqlx::query("SELECT gym_id FROM cloud_disabled_gyms")
            .fetch_all(&self.pool)
            .await?;
        let mut set = HashSet::new();
        for row in rows {
            let id_str: String = row.try_get(0)?;
            set.insert(Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4()));
        }
        Ok(set)
    }

    pub async fn upsert_gym(&self, gym: &GymRecord) -> sqlx::Result<()> {
        let tier_str = format!("{:?}", gym.tier).to_lowercase();
        sqlx::query(
            "INSERT INTO cloud_gyms (id, name, owner_email, tier, is_active, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(id) DO UPDATE SET name = $2, owner_email = $3, tier = $4, is_active = $5",
        )
        .bind(gym.id.to_string())
        .bind(&gym.name)
        .bind(&gym.owner_email)
        .bind(&tier_str)
        .bind(if gym.is_active { 1 } else { 0 })
        .bind(gym.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_gym(&self, req: &UpdateGymRequest) -> sqlx::Result<()> {
        let tier_str = format!("{:?}", req.tier).to_lowercase();
        sqlx::query("UPDATE cloud_gyms SET name = $1, owner_email = $2, tier = $3 WHERE id = $4")
            .bind(&req.name)
            .bind(&req.contact_email)
            .bind(&tier_str)
            .bind(req.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_gym(&self, gym_id: &Uuid) -> sqlx::Result<()> {
        let gid = gym_id.to_string();
        sqlx::query("DELETE FROM cloud_gyms WHERE id = $1").bind(&gid).execute(&self.pool).await?;
        sqlx::query("DELETE FROM cloud_disabled_gyms WHERE gym_id = $1").bind(&gid).execute(&self.pool).await?;
        let _ = sqlx::query(
            "UPDATE cloud_licenses SET is_revoked = 1, revoked_reason = 'Branch deleted by CEO' WHERE gym_id = $1",
        )
        .bind(&gid)
        .execute(&self.pool)
        .await;
        Ok(())
    }

    pub async fn set_disabled(&self, gym_id: &Uuid, disable: bool) -> sqlx::Result<()> {
        if disable {
            sqlx::query(
                "INSERT INTO cloud_disabled_gyms (gym_id, disabled_at) VALUES ($1, $2)
                 ON CONFLICT(gym_id) DO UPDATE SET disabled_at = EXCLUDED.disabled_at",
            )
            .bind(gym_id.to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM cloud_disabled_gyms WHERE gym_id = $1")
                .bind(gym_id.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    // --- License Persistence & Revocation ---

    pub async fn insert_license(&self, claims: &gympos_shared::LicenseClaims, raw_token: &str) -> sqlx::Result<()> {
        let tier_str = format!("{:?}", claims.tier).to_lowercase();
        sqlx::query(
            "INSERT INTO cloud_licenses (
                license_id, raw_token, gym_id, gym_name, owner_email, tier,
                issued_at, expires_at, max_members, hardware_lock_enabled,
                tailgate_detection_enabled, is_revoked
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 0)
             ON CONFLICT(license_id) DO UPDATE SET
                raw_token = $2, gym_name = $4, owner_email = $5, tier = $6,
                expires_at = $8, max_members = $9, hardware_lock_enabled = $10,
                tailgate_detection_enabled = $11",
        )
        .bind(claims.license_id.to_string())
        .bind(raw_token)
        .bind(claims.gym_id.to_string())
        .bind(&claims.gym_name)
        .bind(&claims.owner_email)
        .bind(&tier_str)
        .bind(claims.issued_at.to_rfc3339())
        .bind(claims.expires_at.to_rfc3339())
        .bind(claims.max_members as i64)
        .bind(if claims.hardware_lock_enabled { 1 } else { 0 })
        .bind(if claims.tailgate_detection_enabled { 1 } else { 0 })
        .execute(&self.pool)
        .await?;
        let _ = sqlx::query("UPDATE cloud_gyms SET is_active = 1, tier = $2 WHERE id = $1")
            .bind(claims.gym_id.to_string())
            .bind(&tier_str)
            .execute(&self.pool)
            .await;
        Ok(())
    }

    pub async fn list_licenses(&self) -> sqlx::Result<Vec<crate::models::CloudLicenseRecord>> {
        let rows = sqlx::query(
            "SELECT license_id, raw_token, gym_id, gym_name, owner_email, tier,
                    issued_at, expires_at, max_members, hardware_lock_enabled,
                    tailgate_detection_enabled, is_revoked, revoked_reason, revoked_at
             FROM cloud_licenses
             ORDER BY issued_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut list = Vec::new();
        for row in rows {
            let lic_id_str: String = row.try_get(0)?;
            let raw_token: String = row.try_get(1)?;
            let gym_id_str: String = row.try_get(2)?;
            let gym_name: String = row.try_get(3)?;
            let owner_email: String = row.try_get(4)?;
            let tier_str: String = row.try_get(5)?;
            let issued_str: String = row.try_get(6)?;
            let expires_str: String = row.try_get(7)?;
            let max_members: i32 = row.try_get(8)?;
            let hw_lock: i32 = row.try_get(9)?;
            let tailgate: i32 = row.try_get(10)?;
            let is_revoked: i32 = row.try_get(11)?;
            let revoked_reason: Option<String> = row.try_get(12).ok().flatten();
            let revoked_at_str: Option<String> = row.try_get(13).ok().flatten();

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

            list.push(crate::models::CloudLicenseRecord {
                license_id,
                raw_token,
                gym_id,
                gym_name,
                owner_email,
                tier,
                issued_at,
                expires_at,
                max_members: max_members as u32,
                hardware_lock_enabled: hw_lock == 1,
                tailgate_detection_enabled: tailgate == 1,
                is_revoked: is_revoked == 1,
                revoked_reason,
                revoked_at,
            });
        }
        Ok(list)
    }

    pub async fn revoke_license(&self, license_id: &Uuid, reason: &str) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE cloud_licenses
             SET is_revoked = 1, revoked_reason = $1, revoked_at = $2
             WHERE license_id = $3",
        )
        .bind(reason)
        .bind(Utc::now().to_rfc3339())
        .bind(license_id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn is_license_revoked(&self, license_id: &Uuid) -> sqlx::Result<bool> {
        let row: Option<(i32,)> = sqlx::query_as("SELECT is_revoked FROM cloud_licenses WHERE license_id = $1")
            .bind(license_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|(v,)| v == 1).unwrap_or(false))
    }

    pub async fn load_revoked_license_ids(&self) -> sqlx::Result<HashSet<Uuid>> {
        let rows = sqlx::query("SELECT license_id FROM cloud_licenses WHERE is_revoked = 1")
            .fetch_all(&self.pool)
            .await?;
        let mut set = HashSet::new();
        for row in rows {
            let id_str: String = row.try_get(0)?;
            set.insert(Uuid::parse_str(&id_str).unwrap_or_default());
        }
        Ok(set)
    }

    // --- Inter-Branch Multi-Gym Sync ---

    pub async fn upsert_cloud_members(&self, owner_email: &str, members: &[gympos_shared::CloudMemberSyncItem]) -> sqlx::Result<usize> {
        let mut count = 0;
        for m in members {
            let vectors_json = serde_json::to_string(&m.face_vectors).unwrap_or_else(|_| "[]".to_string());
            let expires_str = m.expires_at.map(|e| e.to_rfc3339());

            sqlx::query(
                "INSERT INTO cloud_members (id, owner_email, home_gym_id, home_gym_name, first_name, last_name, email, phone, membership_type, status, face_vectors_json, created_at, updated_at, expires_at, photo_data_url)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                 ON CONFLICT(id) DO UPDATE SET
                    home_gym_name = $4,
                    first_name = $5,
                    last_name = $6,
                    email = $7,
                    phone = $8,
                    membership_type = $9,
                    status = $10,
                    face_vectors_json = $11,
                    updated_at = $13,
                    expires_at = $14,
                    photo_data_url = COALESCE($15, cloud_members.photo_data_url)",
            )
            .bind(&m.id)
            .bind(owner_email)
            .bind(m.home_gym_id.to_string())
            .bind(&m.home_gym_name)
            .bind(&m.first_name)
            .bind(&m.last_name)
            .bind(&m.email)
            .bind(&m.phone)
            .bind(&m.membership_type)
            .bind(&m.status)
            .bind(&vectors_json)
            .bind(m.created_at.to_rfc3339())
            .bind(m.updated_at.to_rfc3339())
            .bind(expires_str)
            .bind(&m.photo_data_url)
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn get_sister_branch_members(&self, owner_email: &str, exclude_gym_id: &Uuid) -> sqlx::Result<Vec<gympos_shared::CloudMemberSyncItem>> {
        let rows = sqlx::query(
            "SELECT id, home_gym_id, home_gym_name, owner_email, first_name, last_name, email, phone, membership_type, status, face_vectors_json, created_at, updated_at, expires_at, photo_data_url
             FROM cloud_members
             WHERE owner_email = $1 AND home_gym_id != $2",
        )
        .bind(owner_email)
        .bind(exclude_gym_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let mut list = Vec::new();
        for row in rows {
            let id: String = row.try_get(0)?;
            let home_gym_id_str: String = row.try_get(1)?;
            let home_gym_id = Uuid::parse_str(&home_gym_id_str).unwrap_or_default();
            let home_gym_name: String = row.try_get(2)?;
            let owner_email: String = row.try_get(3)?;
            let first_name: String = row.try_get(4)?;
            let last_name: String = row.try_get(5)?;
            let email: Option<String> = row.try_get(6).ok().flatten();
            let phone: Option<String> = row.try_get(7).ok().flatten();
            let membership_type: String = row.try_get(8)?;
            let status: String = row.try_get(9)?;
            let vectors_json: String = row.try_get(10)?;
            let created_str: String = row.try_get(11)?;
            let updated_str: String = row.try_get(12)?;
            let expires_str: Option<String> = row.try_get(13).ok().flatten();
            let photo_data_url: Option<String> = row.try_get(14).ok().flatten();

            let face_vectors: Vec<Vec<f32>> = serde_json::from_str(&vectors_json).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let expires_at = expires_str.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)));

            list.push(gympos_shared::CloudMemberSyncItem {
                id,
                home_gym_id,
                home_gym_name,
                owner_email,
                first_name,
                last_name,
                email: email.unwrap_or_default(),
                phone: phone.unwrap_or_default(),
                membership_type,
                status,
                face_vectors,
                photo_data_url,
                created_at,
                updated_at,
                expires_at,
            });
        }
        Ok(list)
    }

    pub async fn insert_attendance_logs(&self, owner_email: &str, logs: &[gympos_shared::AttendanceRecord], gym_id: &Uuid) -> sqlx::Result<usize> {
        let mut count = 0;
        for l in logs {
            sqlx::query(
                "INSERT INTO cloud_attendance (id, gym_id, owner_email, member_id, member_name, direction, timestamp, confidence, tailgate_flag)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT(id) DO NOTHING",
            )
            .bind(&l.id)
            .bind(gym_id.to_string())
            .bind(owner_email)
            .bind(&l.member_id)
            .bind(&l.member_name)
            .bind(&l.direction)
            .bind(l.timestamp.to_rfc3339())
            .bind(l.confidence)
            .bind(if l.tailgate_flag { 1 } else { 0 })
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    // --- Analytics helpers (Stage 5.1) ---
    pub async fn count_cloud_members(&self) -> sqlx::Result<usize> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_members")
            .fetch_one(&self.pool)
            .await?;
        Ok(n as usize)
    }
    pub async fn count_attendance(&self) -> sqlx::Result<usize> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_attendance")
            .fetch_one(&self.pool)
            .await?;
        Ok(n as usize)
    }
    pub async fn count_tailgate_breaches(&self) -> sqlx::Result<usize> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_attendance WHERE tailgate_flag = 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(n as usize)
    }

    // --- Owner Accounts & Authentication ---

    pub async fn create_owner_account(&self, email: &str, password_hash: &str, company_name: &str) -> sqlx::Result<bool> {
        let res = sqlx::query(
            "INSERT INTO cloud_owner_accounts (owner_email, password_hash, company_name, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT(owner_email) DO UPDATE SET password_hash = $2, company_name = $3",
        )
        .bind(email)
        .bind(password_hash)
        .bind(company_name)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Verifies a plaintext password against the stored Argon2id hash (or a
    /// legacy unsalted SHA-256 hash for accounts created before the migration).
    /// On a successful legacy match the stored hash is transparently upgraded
    /// to Argon2id so pre-migration accounts stop being rainbow-tableable.
    pub async fn verify_owner_login(&self, email: &str, password: &str) -> sqlx::Result<Option<String>> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT company_name, password_hash FROM cloud_owner_accounts WHERE owner_email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;
        if let Some((company_name, stored_hash)) = row {
            if gympos_shared::verify_password(password, &stored_hash) {
                if gympos_shared::password_is_legacy(&stored_hash) {
                    let upgraded = gympos_shared::hash_password(password);
                    if let Err(e) = sqlx::query("UPDATE cloud_owner_accounts SET password_hash = $1 WHERE owner_email = $2")
                        .bind(&upgraded)
                        .bind(email)
                        .execute(&self.pool)
                        .await
                    {
                        tracing::warn!("Failed to upgrade legacy owner password hash for {}: {}", email, e);
                    }
                }
                return Ok(Some(company_name));
            }
        }
        Ok(None)
    }

    pub async fn owner_exists(&self, email: &str) -> sqlx::Result<bool> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_owner_accounts WHERE owner_email = $1")
            .bind(email.to_lowercase().trim().to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(n > 0)
    }

    // --- CEO Accounts (platform super-admins; replaces the shared master key) ---

    pub async fn count_ceos(&self) -> sqlx::Result<usize> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_ceo_accounts")
            .fetch_one(&self.pool)
            .await?;
        Ok(n as usize)
    }

    pub async fn ceo_exists(&self, email: &str) -> sqlx::Result<bool> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_ceo_accounts WHERE ceo_email = $1")
            .bind(email.to_lowercase().trim().to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(n > 0)
    }

    /// Creates a CEO account. Returns `false` when the email is already taken
    /// (never overwrites — use password reset flow instead of silent replace).
    pub async fn create_ceo_account(&self, email: &str, password_hash: &str, display_name: &str) -> sqlx::Result<bool> {
        let res = sqlx::query(
            "INSERT INTO cloud_ceo_accounts (ceo_email, password_hash, display_name, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT(ceo_email) DO NOTHING",
        )
        .bind(email.to_lowercase().trim().to_string())
        .bind(password_hash)
        .bind(display_name)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Verifies a CEO plaintext password against the stored Argon2id hash.
    /// Returns the display name on success. Legacy unsalted-SHA-256 hashes are
    /// transparently upgraded to Argon2id on the successful login itself.
    pub async fn verify_ceo_login(&self, email: &str, password: &str) -> sqlx::Result<Option<String>> {
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT display_name, password_hash FROM cloud_ceo_accounts WHERE ceo_email = $1",
        )
        .bind(email.to_lowercase().trim().to_string())
        .fetch_optional(&self.pool)
        .await?;
        if let Some((display_name, stored_hash)) = row {
            if gympos_shared::verify_password(password, &stored_hash) {
                if gympos_shared::password_is_legacy(&stored_hash) {
                    let upgraded = gympos_shared::hash_password(password);
                    if let Err(e) = sqlx::query("UPDATE cloud_ceo_accounts SET password_hash = $1 WHERE ceo_email = $2")
                        .bind(&upgraded)
                        .bind(email.to_lowercase().trim().to_string())
                        .execute(&self.pool)
                        .await
                    {
                        tracing::warn!("Failed to upgrade legacy CEO password hash for {}: {}", email, e);
                    }
                }
                return Ok(Some(display_name));
            }
        }
        Ok(None)
    }

    pub async fn count_owner_gyms(&self, email: &str) -> sqlx::Result<usize> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cloud_gyms WHERE owner_email = $1")
            .bind(email.to_lowercase().trim().to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(n as usize)
    }

    pub async fn log_audit(&self, owner_email: &str, gym_id: Option<&Uuid>, action: &str, target: Option<&str>) -> sqlx::Result<()> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO cloud_audit_logs (id, owner_email, gym_id, action, target, timestamp) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(owner_email.to_lowercase().trim().to_string())
        .bind(gym_id.map(|u| u.to_string()))
        .bind(action)
        .bind(target)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // --- Remote Catalog & Pricing Management ---

    pub async fn upsert_products(&self, owner_email: &str, products: &[RemoteCatalogProduct]) -> sqlx::Result<usize> {
        let mut count = 0;
        for p in products {
            sqlx::query(
                "INSERT INTO cloud_products (id, owner_email, name, price, stock, category, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT(id, owner_email) DO UPDATE SET name = $3, price = $4, stock = $5, category = $6, updated_at = $7",
            )
            .bind(&p.id)
            .bind(owner_email)
            .bind(&p.name)
            .bind(p.price)
            .bind(p.stock)
            .bind(&p.category)
            .bind(p.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn get_products(&self, owner_email: &str) -> sqlx::Result<Vec<RemoteCatalogProduct>> {
        let rows = sqlx::query(
            "SELECT id, name, price, stock, category, updated_at FROM cloud_products WHERE owner_email = $1 ORDER BY category, name",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for row in rows {
            let id: String = row.try_get(0)?;
            let name: String = row.try_get(1)?;
            let price: f64 = row.try_get(2)?;
            let stock: i32 = row.try_get(3)?;
            let category: String = row.try_get(4)?;
            let updated_at_str: String = row.try_get(5)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            list.push(RemoteCatalogProduct {
                id,
                name,
                price,
                stock,
                category,
                target_gym_id: None,
                updated_at,
            });
        }
        Ok(list)
    }

    pub async fn upsert_plans(&self, owner_email: &str, plans: &[MembershipPlanConfig]) -> sqlx::Result<usize> {
        let mut count = 0;
        for p in plans {
            let benefits_json = serde_json::to_string(&p.benefits).unwrap_or_else(|_| "[]".to_string());
            sqlx::query(
                "INSERT INTO cloud_plans (id, owner_email, name, tag, billing_period, price_monthly, student_discount_pct, benefits_json, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT(id, owner_email) DO UPDATE SET name = $3, tag = $4, billing_period = $5, price_monthly = $6, student_discount_pct = $7, benefits_json = $8, updated_at = $9",
            )
            .bind(&p.id)
            .bind(owner_email)
            .bind(&p.name)
            .bind(&p.tag)
            .bind(&p.billing_period)
            .bind(p.price_monthly)
            .bind(p.student_discount_pct)
            .bind(&benefits_json)
            .bind(p.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn get_plans(&self, owner_email: &str) -> sqlx::Result<Vec<MembershipPlanConfig>> {
        let rows = sqlx::query(
            "SELECT id, name, tag, billing_period, price_monthly, student_discount_pct, benefits_json, updated_at FROM cloud_plans WHERE owner_email = $1 ORDER BY price_monthly",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for row in rows {
            let id: String = row.try_get(0)?;
            let name: String = row.try_get(1)?;
            let tag: Option<String> = row.try_get(2).ok().flatten();
            let billing_period: Option<String> = row.try_get(3).ok().flatten();
            let price_monthly: f64 = row.try_get(4)?;
            let student_discount_pct: f64 = row.try_get(5)?;
            let benefits_json: String = row.try_get(6)?;
            let benefits = serde_json::from_str(&benefits_json).unwrap_or_default();
            let updated_at_str: String = row.try_get(7)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            list.push(MembershipPlanConfig {
                id,
                name,
                tag: tag.unwrap_or_default(),
                billing_period: billing_period.unwrap_or_else(|| "monthly".to_string()),
                price_monthly,
                student_discount_pct,
                target_gym_id: None,
                benefits,
                updated_at,
            });
        }
        Ok(list)
    }

    pub async fn upsert_promos(&self, owner_email: &str, promos: &[PromoVoucherConfig]) -> sqlx::Result<usize> {
        let mut count = 0;
        for pr in promos {
            let expires_at_str = pr.expires_at.map(|dt| dt.to_rfc3339());
            sqlx::query(
                "INSERT INTO cloud_promos (code, owner_email, label, discount_type, discount_value, min_spend, expires_at, is_active)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT(code, owner_email) DO UPDATE SET label = $3, discount_type = $4, discount_value = $5, min_spend = $6, expires_at = $7, is_active = $8",
            )
            .bind(&pr.code)
            .bind(owner_email)
            .bind(&pr.label)
            .bind(&pr.discount_type)
            .bind(pr.discount_value)
            .bind(pr.min_spend)
            .bind(expires_at_str)
            .bind(if pr.is_active { 1 } else { 0 })
            .execute(&self.pool)
            .await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn get_promos(&self, owner_email: &str) -> sqlx::Result<Vec<PromoVoucherConfig>> {
        let rows = sqlx::query(
            "SELECT code, label, discount_type, discount_value, min_spend, expires_at, is_active FROM cloud_promos WHERE owner_email = $1",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for row in rows {
            let code: String = row.try_get(0)?;
            let label: Option<String> = row.try_get(1).ok().flatten();
            let discount_type: String = row.try_get(2)?;
            let discount_value: f64 = row.try_get(3)?;
            let min_spend: f64 = row.try_get(4)?;
            let expires_at_str: Option<String> = row.try_get(5).ok().flatten();
            let expires_at = expires_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc))
            });
            let is_active_int: i32 = row.try_get(6)?;
            list.push(PromoVoucherConfig {
                code,
                label: label.unwrap_or_default(),
                discount_type,
                discount_value,
                min_spend,
                expires_at,
                is_active: is_active_int == 1,
            });
        }
        Ok(list)
    }

    // --- POS Sales Ingestion ---

    pub async fn insert_sales(&self, owner_email: &str, gym_id: &Uuid, sales: &[SaleTransaction]) -> sqlx::Result<usize> {
        let mut count = 0;
        for s in sales {
            let items_json = serde_json::to_string(&s.items).unwrap_or_else(|_| "[]".to_string());
            let inserted = sqlx::query(
                "INSERT INTO cloud_sales (id, gym_id, owner_email, member_id, total_amount, payment_method, items_json, timestamp, discount_type, discount_amount)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT(id) DO NOTHING",
            )
            .bind(&s.id)
            .bind(gym_id.to_string())
            .bind(owner_email)
            .bind(&s.member_id)
            .bind(s.total_amount)
            .bind(&s.payment_method)
            .bind(&items_json)
            .bind(s.timestamp.to_rfc3339())
            .bind(&s.discount_type)
            .bind(s.discount_amount)
            .execute(&self.pool)
            .await?;
            // Decrement base catalog stock so the next down-sync carries the
            // true remaining quantity (prevents sold stock resurrecting on the
            // terminal). Branch overrides are manual adjustments — untouched.
            // Only on first insert: retried pushes must not double-decrement.
            if inserted.rows_affected() > 0 {
                for item in &s.items {
                    if item.quantity > 0 {
                        let _ = sqlx::query(
                            "UPDATE cloud_products SET stock = GREATEST(0, stock - $1) WHERE id = $2 AND owner_email = $3",
                        )
                        .bind(item.quantity as i32)
                        .bind(&item.product_id)
                        .bind(owner_email)
                        .execute(&self.pool)
                        .await;
                    }
                }
            }
            count += 1;
        }
        Ok(count)
    }

    // --- Owner Branch Summaries & Financial Analytics ---

    pub async fn get_owner_branches(&self, owner_email: &str) -> sqlx::Result<Vec<OwnerBranchSummary>> {
        self.get_owner_branches_internal(owner_email).await
    }

    async fn get_owner_branches_internal(&self, owner_email: &str) -> sqlx::Result<Vec<OwnerBranchSummary>> {
        let rows = sqlx::query(
            "SELECT g.id, g.name, g.tier, g.is_active,
                    l.raw_token, l.expires_at, l.issued_at
             FROM cloud_gyms g
             LEFT JOIN cloud_licenses l ON g.id = l.gym_id AND l.is_revoked = 0
             WHERE g.owner_email = $1
             ORDER BY g.created_at",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;

        let today_prefix = Utc::now().format("%Y-%m-%d").to_string();

        let mut branches = Vec::new();
        for row in rows {
            let gym_id_str: String = row.try_get(0)?;
            let gym_id = Uuid::parse_str(&gym_id_str).unwrap_or_else(|_| Uuid::new_v4());
            let name: String = row.try_get(1)?;
            let tier_str: String = row.try_get(2)?;
            let is_active_int: i32 = row.try_get(3)?;
            let license_key: Option<String> = row.try_get(4).ok().flatten();
            let expires_at_str: Option<String> = row.try_get(5).ok().flatten();
            let expires_at = expires_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&Utc)))
                .unwrap_or_else(|| Utc::now());

            let tier = match tier_str.to_lowercase().as_str() {
                "pro" => LicenseTier::Pro,
                "ultra" => LicenseTier::Ultra,
                _ => LicenseTier::Basic,
            };
            let is_active = is_active_int == 1;

            // Query active members count
            let active_members: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cloud_members WHERE home_gym_id = $1 AND status = 'active'",
            )
            .bind(gym_id.to_string())
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            // Query today's check-ins
            let today_checkins: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cloud_attendance WHERE gym_id = $1 AND timestamp LIKE $2",
            )
            .bind(gym_id.to_string())
            .bind(format!("{}%", today_prefix))
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            // Query today's sales
            let today_sales: f64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = $1 AND timestamp LIKE $2",
            )
            .bind(gym_id.to_string())
            .bind(format!("{}%", today_prefix))
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0.0);

            branches.push(OwnerBranchSummary {
                gym_id,
                name,
                tier,
                active_members: active_members as u32,
                today_checkins: today_checkins as u32,
                today_sales,
                hwid: "HWID-BOUND".to_string(),
                license_key: license_key.unwrap_or_default(),
                expires_at,
                is_heartbeat_healthy: true,
                is_active,
                is_disabled: sqlx::query_scalar(
                    "SELECT COUNT(*) FROM cloud_disabled_gyms WHERE gym_id = $1",
                )
                .bind(gym_id.to_string())
                .fetch_one(&self.pool)
                .await
                .map(|n: i64| n > 0)
                .unwrap_or(false),
            });
        }

        Ok(branches)
    }

    pub async fn get_owner_analytics(&self, owner_email: &str) -> sqlx::Result<OwnerDashboardAnalytics> {
        let company_name: Option<String> = sqlx::query_scalar(
            "SELECT company_name FROM cloud_owner_accounts WHERE owner_email = $1",
        )
        .bind(owner_email)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        let company_name = company_name.unwrap_or_else(|| "Gym Group".to_string());

        let branches = self.get_owner_branches_internal(owner_email).await?;
        let total_branches = branches.len();
        let total_active_members: u32 = branches.iter().map(|b| b.active_members).sum();

        let today_prefix = Utc::now().format("%Y-%m-%d").to_string();
        let month_prefix = Utc::now().format("%Y-%m").to_string();

        let today_total_revenue: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE owner_email = $1 AND timestamp LIKE $2",
        )
        .bind(owner_email)
        .bind(format!("{}%", today_prefix))
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        let month_total_revenue: f64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE owner_email = $1 AND timestamp LIKE $2",
        )
        .bind(owner_email)
        .bind(format!("{}%", month_prefix))
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        let today_checkins: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM cloud_attendance WHERE owner_email = $1 AND timestamp LIKE $2",
        )
        .bind(owner_email)
        .bind(format!("{}%", today_prefix))
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        // Recent sales transactions
        let sale_rows = sqlx::query(
            "SELECT id, member_id, total_amount, payment_method, items_json, timestamp, discount_type, discount_amount
             FROM cloud_sales WHERE owner_email = $1 ORDER BY timestamp DESC LIMIT 20",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;
        let mut recent_transactions = Vec::new();
        for row in sale_rows {
            let id: String = row.try_get(0)?;
            let member_id: Option<String> = row.try_get(1).ok().flatten();
            let total_amount: f64 = row.try_get(2)?;
            let payment_method: String = row.try_get(3)?;
            let items_json: String = row.try_get(4)?;
            let items: Vec<CartItem> = serde_json::from_str(&items_json).unwrap_or_default();
            let created_at_str: String = row.try_get(5)?;
            let timestamp = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            recent_transactions.push(SaleTransaction {
                id,
                member_id,
                total_amount,
                payment_method,
                items,
                timestamp,
                discount_type: row.try_get::<Option<String>, _>(6).ok().flatten().unwrap_or_default(),
                discount_amount: row.try_get::<Option<f64>, _>(7).ok().flatten().unwrap_or(0.0),
            });
        }

        // Revenue by Branch
        let mut revenue_by_branch = HashMap::new();
        for b in &branches {
            let branch_rev: f64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = $1 AND timestamp LIKE $2",
            )
            .bind(b.gym_id.to_string())
            .bind(format!("{}%", month_prefix))
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0.0);
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
        let att_rows = sqlx::query(
            "SELECT timestamp FROM cloud_attendance WHERE owner_email = $1 AND timestamp LIKE $2",
        )
        .bind(owner_email)
        .bind(format!("{}%", today_prefix))
        .fetch_all(&self.pool)
        .await?;
        for row in att_rows {
            let ts_str: String = match row.try_get(0) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if let Ok(dt) = DateTime::parse_from_rfc3339(&ts_str) {
                let hour = dt.format("%H").to_string().parse::<usize>().unwrap_or(0);
                if hour < 24 {
                    hourly_traffic[hour] += 1;
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

    pub async fn publish_release(&self, rel: &ReleaseInfo) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO cloud_releases (
                version, channel, min_supported_version, download_url, sha256,
                release_notes, rollout_percentage, is_mandatory, created_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
             ON CONFLICT(version, channel) DO UPDATE SET
                min_supported_version = $3, download_url = $4, sha256 = $5,
                release_notes = $6, rollout_percentage = $7, is_mandatory = $8, created_at = $9",
        )
        .bind(&rel.version)
        .bind(&rel.channel)
        .bind(&rel.min_supported_version)
        .bind(&rel.download_url)
        .bind(&rel.sha256)
        .bind(&rel.release_notes)
        .bind(rel.rollout_percentage as i64)
        .bind(if rel.is_mandatory { 1 } else { 0 })
        .bind(rel.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn release_from_row(row: &sqlx::postgres::PgRow) -> sqlx::Result<ReleaseInfo> {
        let version: String = row.try_get(0)?;
        let channel: String = row.try_get(1)?;
        let min_supported_version: String = row.try_get(2)?;
        let download_url: String = row.try_get(3)?;
        let sha256: String = row.try_get(4)?;
        let release_notes: String = row.try_get(5)?;
        let rollout_percentage: i64 = row.try_get(6)?;
        let is_mandatory: i32 = row.try_get(7)?;
        let created_at_str: String = row.try_get(8)?;
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
    }

    pub async fn get_latest_release(&self, channel: &str) -> sqlx::Result<Option<ReleaseInfo>> {
        let row = sqlx::query(
            "SELECT version, channel, min_supported_version, download_url, sha256,
                    release_notes, rollout_percentage, is_mandatory, created_at
             FROM cloud_releases
             WHERE channel = $1
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(channel)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(Some(Self::release_from_row(&r)?)),
            None => Ok(None),
        }
    }

    pub async fn list_releases(&self) -> sqlx::Result<Vec<ReleaseInfo>> {
        let rows = sqlx::query(
            "SELECT version, channel, min_supported_version, download_url, sha256,
                    release_notes, rollout_percentage, is_mandatory, created_at
             FROM cloud_releases
             ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut list = Vec::new();
        for r in rows {
            list.push(Self::release_from_row(&r)?);
        }
        Ok(list)
    }

    // --- Staff & Cashier RBAC Management ---

    pub async fn create_staff_account(&self, staff: &StaffAccount) -> sqlx::Result<()> {
        let gym_id_str = staff.gym_id.map(|u| u.to_string());
        sqlx::query(
            "INSERT INTO cloud_staff_accounts (id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(&staff.id)
        .bind(&staff.owner_email)
        .bind(gym_id_str)
        .bind(&staff.gym_name)
        .bind(&staff.full_name)
        .bind(&staff.username)
        .bind(&staff.pin_hash)
        .bind(match staff.role {
            StaffRole::Manager => "manager",
            StaffRole::Owner => "owner",
            StaffRole::Staff => "staff",
        })
        .bind(if staff.is_active { 1 } else { 0 })
        .bind(staff.created_at.to_rfc3339())
        .bind(staff.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn staff_from_row(row: &sqlx::postgres::PgRow) -> sqlx::Result<StaffAccount> {
        let id: String = row.try_get(0)?;
        let owner_email: String = row.try_get(1)?;
        let gym_id_str: Option<String> = row.try_get(2).ok().flatten();
        let gym_id = gym_id_str.and_then(|s| Uuid::parse_str(&s).ok());
        let gym_name: Option<String> = row.try_get(3).ok().flatten();
        let full_name: String = row.try_get(4)?;
        let username: String = row.try_get(5)?;
        let pin_hash: String = row.try_get(6)?;
        let role_str: String = row.try_get(7)?;
        let role: StaffRole = match role_str.to_lowercase().as_str() {
            "manager" => StaffRole::Manager,
            "owner" => StaffRole::Owner,
            _ => StaffRole::Staff,
        };
        let is_active_int: i32 = row.try_get(8)?;
        let created_at_str: String = row.try_get(9)?;
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at_str: String = row.try_get(10)?;
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
    }

    pub async fn list_staff_by_owner(&self, owner_email: &str) -> sqlx::Result<Vec<StaffAccount>> {
        let rows = sqlx::query(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at
             FROM cloud_staff_accounts WHERE owner_email = $1 ORDER BY full_name",
        )
        .bind(owner_email)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for r in rows {
            list.push(Self::staff_from_row(&r)?);
        }
        Ok(list)
    }

    pub async fn list_staff_for_branch(&self, owner_email: &str, gym_id: &Uuid) -> sqlx::Result<Vec<StaffAccount>> {
        let gym_id_str = gym_id.to_string();
        let rows = sqlx::query(
            "SELECT id, owner_email, gym_id, gym_name, full_name, username, pin_hash, role, is_active, created_at, updated_at
             FROM cloud_staff_accounts
             WHERE owner_email = $1 AND (gym_id = $2 OR gym_id IS NULL OR gym_id = '') AND is_active = 1
             ORDER BY full_name",
        )
        .bind(owner_email)
        .bind(&gym_id_str)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for r in rows {
            list.push(Self::staff_from_row(&r)?);
        }
        Ok(list)
    }

    pub async fn delete_staff_account(&self, owner_email: &str, staff_id: &str) -> sqlx::Result<bool> {
        let res = sqlx::query("DELETE FROM cloud_staff_accounts WHERE id = $1 AND owner_email = $2")
            .bind(staff_id)
            .bind(owner_email)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn update_staff_account(&self, owner_email: &str, staff_id: &str, req: &UpdateStaffRequest) -> sqlx::Result<bool> {
        // Positional $n placeholders are numbered as values are pushed.
        let mut sets: Vec<String> = Vec::new();
        let mut query = sqlx::QueryBuilder::new("UPDATE cloud_staff_accounts SET ");

        if let Some(ref name) = req.full_name {
            sets.push("full_name".to_string());
            query.push("full_name = ");
            query.push_bind(name.clone());
        }
        if let Some(ref pin) = req.pin_code {
            let pin_hash = gympos_shared::hash_password(pin);
            if !sets.is_empty() {
                query.push(", ");
            }
            sets.push("pin_hash".to_string());
            query.push("pin_hash = ");
            query.push_bind(pin_hash);
        }
        if let Some(ref role) = req.role {
            let role_str = match role {
                StaffRole::Manager => "manager",
                StaffRole::Owner => "owner",
                StaffRole::Staff => "staff",
            };
            if !sets.is_empty() {
                query.push(", ");
            }
            sets.push("role".to_string());
            query.push("role = ");
            query.push_bind(role_str.to_string());
        }
        if let Some(ref gym_id) = req.gym_id {
            if !sets.is_empty() {
                query.push(", ");
            }
            sets.push("gym_id".to_string());
            query.push("gym_id = ");
            query.push_bind(gym_id.to_string());
        }
        if let Some(ref gym_name) = req.gym_name {
            if !sets.is_empty() {
                query.push(", ");
            }
            sets.push("gym_name".to_string());
            query.push("gym_name = ");
            query.push_bind(gym_name.clone());
        }
        if let Some(is_active) = req.is_active {
            if !sets.is_empty() {
                query.push(", ");
            }
            sets.push("is_active".to_string());
            query.push("is_active = ");
            query.push_bind(if is_active { 1 } else { 0 });
        }

        if sets.is_empty() {
            return Ok(false);
        }

        query.push(", updated_at = ");
        query.push_bind(Utc::now().to_rfc3339());
        query.push(" WHERE id = ");
        query.push_bind(staff_id.to_string());
        query.push(" AND owner_email = ");
        query.push_bind(owner_email.to_string());

        let res = query.build().execute(&self.pool).await?;
        Ok(res.rows_affected() > 0)
    }

    // --- Branch Catalog Overrides ---

    pub async fn save_branch_product_override(
        &self,
        owner_email: &str,
        gym_id: &Uuid,
        product_id: &str,
        price: f64,
        stock: i32,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO cloud_branch_product_overrides (product_id, gym_id, owner_email, price, stock, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT(product_id, gym_id) DO UPDATE SET price = $4, stock = $5, updated_at = $6",
        )
        .bind(product_id)
        .bind(gym_id.to_string())
        .bind(owner_email)
        .bind(price)
        .bind(stock)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_branch_products(&self, owner_email: &str, gym_id: &Uuid) -> sqlx::Result<Vec<RemoteCatalogProduct>> {
        let gym_id_str = gym_id.to_string();
        let rows = sqlx::query(
            "SELECT p.id, p.name,
                    COALESCE(o.price, p.price) as effective_price,
                    COALESCE(o.stock, p.stock) as effective_stock,
                    p.category, p.updated_at
             FROM cloud_products p
             LEFT JOIN cloud_branch_product_overrides o
                    ON p.id = o.product_id AND o.gym_id = $2
             WHERE p.owner_email = $1
             ORDER BY p.category, p.name",
        )
        .bind(owner_email)
        .bind(&gym_id_str)
        .fetch_all(&self.pool)
        .await?;
        let mut list = Vec::new();
        for row in rows {
            let id: String = row.try_get(0)?;
            let name: String = row.try_get(1)?;
            let price: f64 = row.try_get(2)?;
            let stock: i32 = row.try_get(3)?;
            let category: String = row.try_get(4)?;
            let updated_at_str: String = row.try_get(5)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            list.push(RemoteCatalogProduct {
                id,
                name,
                price,
                stock,
                category,
                target_gym_id: Some(*gym_id),
                updated_at,
            });
        }
        Ok(list)
    }

    // --- CEO Collapsible Owner Hierarchy & Centralized License Management ---

    pub async fn list_all_owners_with_branches(&self) -> sqlx::Result<Vec<OwnerHierarchyAccount>> {
        let now = Utc::now();
        let today_prefix = now.format("%Y-%m-%d").to_string();

        // 1. Fetch all registered owner accounts
        let owner_rows = sqlx::query(
            "SELECT owner_email, company_name, created_at FROM cloud_owner_accounts ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut owners = Vec::new();
        for row in owner_rows {
            let owner_email: String = row.try_get(0)?;
            let company_name: String = row.try_get(1)?;
            let created_at_str: String = row.try_get(2)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            owners.push((owner_email, company_name, created_at));
        }

        // Also discover any orphan gyms created before owner accounts existed
        let orphan_rows = sqlx::query(
            "SELECT DISTINCT owner_email FROM cloud_gyms WHERE owner_email NOT IN (SELECT owner_email FROM cloud_owner_accounts)",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in orphan_rows {
            let email: String = row.try_get(0)?;
            owners.push((email.clone(), format!("Gym Group ({})", email), now));
        }

        let mut hierarchy = Vec::new();

        for (owner_email, company_name, created_at) in owners {
            // Query all branches for this owner
            let branch_rows = sqlx::query(
                "SELECT id, name, tier, is_active, created_at FROM cloud_gyms WHERE owner_email = $1 ORDER BY created_at ASC",
            )
            .bind(&owner_email)
            .fetch_all(&self.pool)
            .await?;

            let mut branches = Vec::new();
            let mut active_count = 0;
            let mut pending_count = 0;

            for brow in branch_rows {
                let id_str: String = brow.try_get(0)?;
                let gym_id = Uuid::parse_str(&id_str).unwrap_or_default();
                let name: String = brow.try_get(1)?;
                let tier_str: String = brow.try_get(2)?;
                let tier = match tier_str.to_lowercase().as_str() {
                    "basic" => LicenseTier::Basic,
                    "ultra" => LicenseTier::Ultra,
                    _ => LicenseTier::Pro,
                };
                let is_active_int: i32 = brow.try_get(3)?;
                let b_created_str: String = brow.try_get(4)?;
                let b_created = DateTime::parse_from_rfc3339(&b_created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                let is_active = is_active_int > 0;

                // Check active license for this branch
                let lic_row: Option<(String, String, i32)> = sqlx::query_as(
                    "SELECT raw_token, expires_at, is_revoked FROM cloud_licenses WHERE gym_id = $1 AND is_revoked = 0 ORDER BY expires_at DESC LIMIT 1",
                )
                .bind(gym_id.to_string())
                .fetch_optional(&self.pool)
                .await?;

                let mut license_key = None;
                let mut expires_at = None;
                let mut days_remaining = None;
                let mut is_license_active = false;

                if let Some((token, exp_str, is_revoked_int)) = lic_row {
                    if is_revoked_int == 0 {
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
                let active_members: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM cloud_members WHERE home_gym_id = $1 AND status = 'active'",
                )
                .bind(gym_id.to_string())
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);

                // Today sales
                let today_sales: f64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(total_amount), 0.0) FROM cloud_sales WHERE gym_id = $1 AND timestamp LIKE $2",
                )
                .bind(gym_id.to_string())
                .bind(format!("{}%", today_prefix))
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0.0);

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
                    is_disabled: sqlx::query_scalar(
                        "SELECT COUNT(*) FROM cloud_disabled_gyms WHERE gym_id = $1",
                    )
                    .bind(gym_id.to_string())
                    .fetch_one(&self.pool)
                    .await
                    .map(|n: i64| n > 0)
                    .unwrap_or(false),
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

    pub async fn get_gym_by_id(&self, gym_id: &Uuid) -> sqlx::Result<Option<GymRecord>> {
        let row = sqlx::query("SELECT id, name, owner_email, tier, is_active, created_at FROM cloud_gyms WHERE id = $1")
            .bind(gym_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => {
                let id_str: String = r.try_get(0)?;
                let id = Uuid::parse_str(&id_str).unwrap_or_default();
                let name: String = r.try_get(1)?;
                let owner_email: String = r.try_get(2)?;
                let tier_str: String = r.try_get(3)?;
                let tier = match tier_str.to_lowercase().as_str() {
                    "basic" => LicenseTier::Basic,
                    "ultra" => LicenseTier::Ultra,
                    _ => LicenseTier::Pro,
                };
                let is_active: i32 = r.try_get(4)?;
                let created_at_str: String = r.try_get(5)?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                Ok(Some(GymRecord {
                    id,
                    name,
                    owner_email,
                    tier,
                    is_active: is_active > 0,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    }
}
