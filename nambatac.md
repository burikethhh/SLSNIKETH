# GymPOS — Solo Leveling Gym (Standalone Edition)

> Complete system documentation: architecture, system flow, features, and vitae.
> Source: `C:\Users\USER\Desktop\STANDALONE` · Repo: `github.com/heckeristttt/SLS123` (branch `main`)

---

## 1. System Overview

A self-contained **gym management + automated access-control POS** that runs as a
**FastAPI (Python 3.14) backend** wrapped in an **Electron desktop shell**. It is a
"source-mode" build: a modern extension layer (`main.py`) bootstraps an older
compiled application (`main.pyc`) and registers feature routes on top of it.

- **Timezone:** Philippine Standard Time (UTC+8), stored as naive datetime so SQLite `date()` comparisons agree.
- **Data store:** SQLite (`gym.db`). Packaged builds use a portable `GymPOS_Data/gym.db`.
- **Backend:** FastAPI on `127.0.0.1:8000`. Electron (`electron/main.js`) spawns it, waits for the port, then loads the UI in a `BrowserWindow`. On quit it kills the whole process tree (`taskkill /T /F`).
- **Auth:** bcrypt-hashed passwords + session cookies; two roles — `admin` and `staff`.
- **Default login (fresh DB):** `admin / admin`.

---

## 2. Tech Stack

| Layer | Technology |
|---|---|
| Backend | Python 3.14, FastAPI, SQLAlchemy, uvicorn |
| Auth / Sessions | bcrypt, starlette sessions |
| Vision | OpenCV (cv2, DShow), MediaPipe, numpy 2.4.4 |
| Face recognition | SFace embedding (`face_recognition_sface_2021dec.onnx`) + YuNet detection (`face_detection_yunet_2023mar.onnx`); cosine distance threshold 0.36; 3-angle enrollment |
| Object detection | YOLOv8n (`yolov8n.onnx`) for tailgate/person counting |
| Templates | Jinja2 + Tailwind (dark purple theme), htmx, Chart.js |
| Desktop shell | Electron (spawn/kill backend, injects renderer error logger) |
| Firmware | ESP32 (PlatformIO / Arduino) — relay door control, RFID beep, LCD messages |
| Hardware | 2× EMEET SmartCam C60E 4K (USB), USB HID RFID reader, ESP32 on COM3 (Silicon Labs CP210x) |

---

## 3. System Flow

### 3.1 Startup Flow

```
GymPOS.bat / GymPOS.exe (Electron)
   │
   ├─ resolveBackendExe() → finds packaged GymPOS.exe OR source backend (.venv python + main.py)
   ├─ pickPort() → default 8000 (falls back if busy)
   ├─ spawn python main.py with env: SOLO_HEADLESS=1, SOLO_PORT, SOLO_DATA_DIR, PYTHONUNBUFFERED=1
   │     └─ stdout/stderr → %APPDATA%\gympos-shell\gympos-shell.log
   └─ waitForBackend() polls 127.0.0.1:8000 until listening → loads BrowserWindow
```

### 3.2 Backend Startup Sequence (`main.py`)

1. **Timezone setup** — PHT helpers (`_now_utc`, `_to_local*`).
2. **Jinja filter injection** — `to_local`, `photo_url`, `student_id` injected into `jinja2.defaults` for *every* environment.
3. **Bootstrap compiled app** — `importlib` loads `main.pyc` as `gympos_main` (avoids `__main__` guard), grabs its `app`.
4. **Schema & migration** — create extension tables (familiars, store_products, store_sales, attendance_daily, coaching_plans, coaching_sessions, vouchers, admin_settings…), `ALTER TABLE` for missing columns (`commission_pct`, `gym_share_type`), seed defaults + admin account.
5. **Hardware / services startup**
   - Camera manager (DShow-first, index correction thread fires after 8s).
   - Access-control patches: `TAILGATE_MONITOR_SECONDS=7.0`, attendance-driven `_face_cooldown` gate, UNLOCK rate-limiter (1 per 2.5s), robust attendance logging.
   - RFID listener, serial bridge (ESP32), face-recognition loops.
6. **Background threads**
   - Attendance cleanup/archive loop (every 5 min; archives previous day at midnight, retention `_RETENTION_DAYS`).
   - Membership expiry check loop (marks expired, blocks entry).
   - Walk-in auto-logout loop.
7. **DB integrity** — dedup trigger management (`trg_attendance_no_dup_in`), no-dup IN protection.

### 3.3 Access-Control Flow (the core loop)

```
[Camera cam1]  continuous face detect
   │
   ├─ match against roster (members, staff 10000+, familiars -id)
   │
   ├─ BLOCKED if: already inside today (last attendance = FACE-IN)
   │            │ member expired (auto-marks status='expired')
   │            │ in 5s re-arm grace after RFID-OUT
   │
   ├─ ALLOWED → _face_cooldown[member_id] = now  (gate intercepts)
   │            ├─ logs attendance IN  (direct sqlite3 + retry; dedup trigger guards duplicates)
   │            ├─ arms tailgate monitor (7s window)
   │            └─ sends UNLOCK to ESP32 (rate-limited 1 per 2.5s)

[RFID reader / cam2]
   ├─ RFID OUT → attendance OUT (method=RFID) → re-arm face grace (+5s)
   └─ Tailgate cam watches for a second person entering during door-open window
```

Attendance cycle (per member per day): `FACE-IN → RFID-OUT → FACE-IN → …`
The cycle gate + cooldown gate make the compiled loop 100% attendance-driven.

### 3.4 Registration Flow (member / staff / familiar)

1. **Step 1** — personal info (name, plan, discount type: student/voucher/PWD).
2. **Step 2** — **3-angle face capture** (Front, Left, Right). Cam1 auto-selected. Averaged SFace embedding stored as a pickled vector blob in DB.
3. **Step 3** — summary / confirmation. Roster cache invalidated automatically → recognition active immediately.

### 3.5 Store POS Flow

```
Admin → Store Products  (add item, price, stock, low-stock threshold)
   │
Staff → Store POS       (search → pick product → qty → sell)
   ├─ stock auto-deducts
   ├─ store_sales row written (product, qty, unit price, total, payment)
   └─ low-stock alerts when stock ≤ threshold
Reports: Store Reports (CSV/PDF export) · Store Analytics (revenue, top products, stock health)
```

### 3.6 Coaching Flow

```
Admin → Gym Plans
   ├─ Coaching Plans (name, duration, price, commission %)
   └─ Coaching Session Revenue settings
        ├─ Default Session Price (₱)
        └─ Gym Share TYPE: Percentage (%)  OR  Fixed Price (₱)   ← toggle
             ├─ pct:  gym amount = session_price × ratio / 100
             └─ peso: gym amount = fixed gym_share_peso (per session)

Coach → Coaches page → Record Session
   ├─ coaching_sessions row (price, gym_commission_pct, gym_share_type audit)
   └─ sales row created for the gym amount (payment_method='coaching', receipt R<date>-COA<time>)
```

### 3.7 Walk-in Flow

- Walk-in members (day pass) tracked with manual IN/OUT toggles (max 3 IN + 3 OUT per day).
- Walk-ins can be upgraded/renewed, assigned RFID, or logged out automatically by the auto-logout loop.

---

## 4. Features (full route map)

### Access & Attendance
- Face-recognition entry (members, staff, familiars), RFID exit, manual toggle
- Attendance page + `/api/live-feed` + `/api/face-detect` (staff/familiar face override)
- RFID scan endpoint `/api/rfid-scan` (manual UID entry supported)
- Tailgate monitoring, UNLOCK rate limiting, membership expiry enforcement

### Members
- Register (3-step incl. face), update, delete, re-scan face, renew, freeze
- Profile modal (student ID / voucher extraction), walk-in list, assign RFID, walk-in logout/delete

### Staff & Familiars
- Staff CRUD (admin), role-based access, password reset
- Familiars CRUD (free access people — face + RFID, entry logged)

### Coaching
- Coaches page (status, students, assignments), add/toggle/delete coaches
- Coach–student assignment (enroll new/existing, renew, edit, toggle, delete)
- Coaching sessions record/delete + gym-share revenue (percent or peso)

### Pricing & Plans
- Subscription + walk-in plans CRUD with `commission_pct`
- Coaching plans CRUD
- Coaching session settings (price + gym share type/value)
- Vouchers: create (batch codes), toggle active, delete-batch + usage tracking

### Store POS
- Products CRUD, restock, active toggle, `/api/store/products`
- Sell (cash/GCash), store history, delete sale
- Store analytics + reports (CSV/PDF export)

### Sales & Finance
- End-of-day report (income, expenses, cash/GCash breakdown, net revenue)
- Sales history, walk-in sales, renewals, expense tracking page
- Gym analytics (revenue today/week/month, peak hours, demographics)

### Admin / Ops
- Admin dashboard (combined gym + store gross), overrides, activity log
- Hardware page (verify password → camera/serial control)
- Maintenance: change password, **reset data** (backup first → drop tables → recreate schema → reseed)
- Cameras page, incidents (security), discrepancy report, freeze management

---

## 5. Database (SQLite — 19 tables)

`admin_settings` · `attendance` · `coach_assignments` · `coaching_plans` · `coaching_sessions` · `expenses` · `familiars` · `freezes` · `manual_overrides` · `members` · `plans` · `sales` · `security_incidents` · `staff` · `staff_activities` · `store_products` · `store_sales` · `voucher_usage` · `vouchers`

Key design points:
- `attendance` has `member_id` / `staff_id` / `familiar_id`; directions `IN`/`OUT`; methods `FACE`/`RFID`/`MANUAL`.
- `sales` records gym-share coaching revenue (`payment_method='coaching'`), plus normal memberships/store.
- `admin_settings` key/value store (session price, gym ratio, gym share type, gym share peso…).
- DB trigger `trg_attendance_no_dup_in` prevents duplicate IN records per member per day.
- Timezone migrated from UTC → PHT (+8h) on 2026-05-12.

**Current live settings:**
```
coaching_session_price = 150.0
coaching_gym_ratio     = 40.0
coaching_gym_share_type = peso
coaching_gym_share_peso = 50.0
```

---

## 6. Vitae — System History & Evolution

### 6.1 Timeline

| Date (approx.) | Milestone |
|---|---|
| Pre-2026 | Original compiled GymPOS build (`main.pyc`, `routers/*.pyc`, `services/*.pyc`) authored by upstream developer. Bundled as `GymPOS.exe` (PyInstaller). No `.py` source retained. |
| 2026-05-04 | **Source-mode edition** published. `main.py` extension layer + Electron shell (`electron/main.js`) added. `GymPOS.bat` launcher. |
| 2026-05-12 | **Timezone migration** — DB timestamps shifted UTC → PHT (+8h). |
| 2026-05-10 | DB reset/backup workflow established (`static/backups/pre_reset_*.db`). |
| 2026-05-29 | `GymPOS.exe.bak` (158 MB) preserved in `electron/` as original-compiled-backup. |
| Ongoing | Hardware stabilization: DShow camera engine, 8s index-correction thread, camera watchdog labels. |

### 6.2 Feature & Fix Log (this session's engineering)

1. **Attendance IN records lost (fix).**
   - Root cause: compiled ORM INSERT silently failed under SQLite lock contention (`except: pass` swallowed errors); dedup check compared DATETIME vs date string.
   - Fix: direct-sqlite3 fallback with WAL mode + 5 retries; DB-level `trg_attendance_no_dup_in` trigger; `_face_cooldown` gate made attendance-driven; UNLOCK rate-limiter.

2. **GYM SHARE peso option (feature).**
   - Added `coaching_gym_share_type` (`pct`/`peso`) + `coaching_gym_share_peso` settings; `gym_share_type` audit column on `coaching_sessions`; admin UI toggle (Percentage vs Fixed ₱); `add-session` computes gym amount by mode.

3. **Backend crash — `ModuleNotFoundError: numpy` (fix).**
   - Root cause: `.venv\Lib\site-packages\numpy` corrupted (WinError 1392) → backend died <1s after launch → Electron "backend process exited unexpectedly".
   - Fix: installed numpy 2.4.4 into isolated `.venv\numpy_fix` + `.pth` pointer; verified import chain (`services.face_recognition_ml`, `routers.members`) and full boot (cameras, port 8000). *Recommend `chkdsk C: /f` on next reboot to clear the stale NTFS entry.*

4. **GitHub migration.**
   - Init repo, `main` branch, `.gitignore` (data/privacy/binary exclusions), 3 commits pushed:
     - `4c5684c` — Initial push (89 files)
     - `3fde9c7` — Compiled app modules (`database/`, `routers/`, `services/`, root `.pyc`) — repo was incomplete without them
     - `bf0f5e4` — `.gitignore` exceptions to keep essential `.pyc` tracked
   - 113 tracked files total. PAT embedded in remote URL was scrubbed afterwards.

### 6.3 Known Notes / Caveats

- Core app logic ships as **compiled `.pyc` (Python 3.14 bytecode)** — no source exists. A fresh clone on another Python version requires recompile/reinstall.
- `cam1` may display "degraded" even when streaming (watchdog label artifact — confirm via `/api/camera-snapshot?cam=cam1`).
- "No match: roster=0" in logs is normal until ≥1 face is registered.
- `.env` holds local hardware config (cam1_index=1, cam2_index=0, face_tolerance=0.36) — machine-specific, intentionally not committed.
- Excluded from repo: `gym.db*`, `.env`, `static/photos`, `backups`, `.venv`, `node_modules`, `GymPOS.exe` (>GitHub 100 MB limit), Electron `.bak`.
