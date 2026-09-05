# GymPOS SaaS Platform — 10-Day MVP Production Suite

A high-performance, offline-first SaaS management, biometric access control, and Point-of-Sale (POS) platform designed for modern fitness centers.

---

## 🏛️ System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      CLOUD SAAS BACKEND                         │
│                    Rust Axum + PostgreSQL                       │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌─────────┐ │
│  │ Subscription │ │ RSA License  │ │ Face Vector  │ │ Remote  │ │
│  │   Manager    │ │  Generator   │ │ Sync Backup  │ │ Lockout │ │
│  └──────────────┘ └──────────────┘ └──────────────┘ └─────────┘ │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │            CEO Command Center Web Dashboard                │ │
│  └────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                               │
                               │ HTTPS / JSON (TLS 1.3)
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   GYM LOCAL CLIENT INSTANCE                     │
│         ┌─────────────────────────────────────┐                 │
│         │        Tauri v2 Desktop App         │                 │
│         │  ┌───────────────────────────────┐  │                 │
│         │  │       Rust Core Engine        │  │                 │
│         │  │ ┌───────────┐ ┌─────────────┐ │  │                 │
│         │  │ │ Hardware  │ │ RSA License │ │  │                 │
│         │  │ │  Manager  │ │  Validator  │ │  │                 │
│         │  │ └───────────┘ └─────────────┘ │  │                 │
│         │  │ ┌───────────┐ ┌─────────────┐ │  │                 │
│         │  │ │SFace / ONNX│ │   SQLite    │ │  │                 │
│         │  │ │VectorStore│ │  Local DB   │ │  │                 │
│         │  │ └───────────┘ └─────────────┘ │  │                 │
│         │  └───────────────────────────────┘  │                 │
│         │  ┌───────────────────────────────┐  │                 │
│         │  │ WebView (Solo Leveling Theme) │  │                 │
│         │  └───────────────────────────────┘  │                 │
│         └─────────────────────────────────────┘                 │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────────┐ │
│  │  3x Cameras  │   │Magnetic Lock │   │  ESP32 Serial Bridge │ │
│  │ In/Out/Tail  │   │ Relay Switch │   │  (115200 Baud COM)   │ │
│  └──────────────┘   └──────────────┘   └──────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

---

## ⚡ 10-Day Sprint Deliverables Summary

| Milestone | Deliverables | Status |
| :--- | :--- | :--- |
| **Day 1: Foundation** | Cargo multi-crate workspace (`shared`, `cloud`, `desktop`), RSA cryptographic signing engine, basic Tauri v2 skeleton. | ✅ Complete |
| **Day 2: Core Backend** | Full SQLite migrations, multi-channel attendance logging, store products, and COM port bridge. | ✅ Complete |
| **Day 3: Hardware & Lock** | ESP32 serial communication protocol (`UNLOCK:3000`), emergency gate triggers, and hardware manager. | ✅ Complete |
| **Day 4: Biometric Pipeline** | In-memory `FaceVectorStore` with sub-millisecond Cosine Similarity matching over 128-d SFace embeddings and 3-angle capture. | ✅ Complete |
| **Day 5: Integration Layer** | License tier gating (Basic: 200, Pro: 500, Ultra: 1000 members), 3-day grace period evaluator, and offline retry queue. | ✅ Complete |
| **Day 6: Converted Views** | Converted UI for Live Gate / Kiosk, Member Directory, Store POS, and Coaches. | ✅ Complete |
| **Day 7: POS & Coaches** | Real-time shopping cart, automatic stock decrement, multi-payment methods (Cash, Card, GCash), and PT session booking. | ✅ Complete |
| **Day 8: Cloud Dashboard** | CEO Command Center web application served directly from Axum with real-time MRR estimator and fleet management. | ✅ Complete |
| **Day 9: Sync & Security** | Background cloud sync worker, instant remote kill switch, and automated test suite. | ✅ Complete |
| **Day 10: Final Handoff** | Production documentation, Render configuration, and installer setup. | ✅ Complete |

---

## 🚀 Quickstart & Development

### Prerequisites
- **Rust Toolchain**: `rustup default stable` (v1.80+)
- **Node.js**: v18+ & npm
- **Tauri CLI**: `cargo install tauri-cli --version "^2.0"`

### 1. Launch the Cloud Backend (CEO Dashboard)
```bash
cd gympos-saas/cloud
cargo run
```
*Access the CEO Command Center at `http://localhost:8080` to onboard gyms and issue RSA license keys.*

### 2. Launch the Desktop Client
```bash
cd gympos-saas/desktop
cargo tauri dev
```

---

## 🔑 CEO Account Authentication (Cloud Backend)

There is no shared master admin key. The platform super-admin is a **CEO account**
(validated email + min-8-char Argon2id password) stored in `cloud_ceo_accounts`:

1. On a fresh database, open the Command Center (`/`) → **First-Time Setup** tab
   (or `POST /api/v1/auth/ceo-register`) to create the first CEO. Registration
   locks down to CEO-authenticated creation once an account exists.
2. Sign in (`POST /api/v1/auth/ceo-login`) for a `ceo:<email>` session token.
3. Pass it as `Authorization: Bearer <token>` on all CEO endpoints
   (`/api/v1/admin/*`, `/api/v1/licenses/*`, `/api/v1/gyms/*`, `/api/v1/remote/*`).

| Variable | Required in Production | Purpose |
| :--- | :--- | :--- |
| `DATABASE_URL` | ✅ Yes | Postgres connection string — the **one permanent backend** for CEOs, owners, gyms, licenses, members, sales and catalog. Local runs must point at the same database (e.g. the Render `gympos-db` external URL with `?sslmode=require`) so there is exactly one source of truth. The server refuses to boot without it. SQLite files are not used anymore (Render wipes them on every restart; relative paths split-brained local runs by CWD). |
| `RSA_PRIVATE_KEY_PEM` | ✅ Yes | PKCS#8 private key used to sign all license tokens. If unset, an ephemeral key is generated per-process — any license issued will stop verifying after a restart. Generate a real pair with `cargo run --bin gen_keys -p gympos-cloud`, keep the private half secret, and update `EMBEDDED_PUBLIC_KEY_PEM` in `desktop/src-tauri/src/license.rs` with the matching public half before shipping desktop builds. |

For local development/testing, the automated suites bootstrap their own CEO
(`ceo@test.local` / `TestCEO123`) via `ceo-register` — no env secrets needed
beyond `DATABASE_URL`. Note: the Render free Postgres expires after 30 days
(14-day grace); upgrade to a paid tier for truly permanent storage.

### Login rate limiting

`cloud/src/rate_limit.rs` implements a small in-memory, per-key fixed-window
limiter applied to the three credential-guessing surfaces:

| Endpoint | Limit | Key |
| :--- | :--- | :--- |
| `POST /api/v1/auth/ceo-login` | 5 / 15 min, and 30 / 15 min | (IP + email), and IP alone |
| `POST /api/v1/auth/ceo-register` | 5 / hour | client IP |
| `POST /api/v1/owner/auth/login` | 8 / 10 min, and 30 / 10 min | (IP + email), and IP alone |
| `POST /api/v1/owner/auth/register` | 5 / hour | client IP |

Exceeding a limit returns `429 Too Many Requests` with a `Retry-After` header
and a JSON body (`{"code": "RATE_LIMITED", "retry_after_seconds": ...}`).
Successful CEO/owner logins reset that key's counter so legitimate users
aren't penalized by earlier typos. Client IP is read from `X-Forwarded-For`
first (set by Render's proxy) and falls back to the raw TCP peer address.

---

## 🔐 Cryptographic License Format

License keys are cryptographically signed using **RSA-2048 with PSS padding and SHA-256**:
```
GPOS-<base64_url_claims>.<base64_url_signature>
```

### Claims Payload
```json
{
  "license_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "gym_id": "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed",
  "gym_name": "Shadow Monarch Fitness",
  "tier": "pro",
  "issued_at": "2026-08-20T00:00:00Z",
  "expires_at": "2026-09-20T00:00:00Z",
  "max_members": 500,
  "hardware_lock_enabled": true,
  "tailgate_detection_enabled": true
}
```

---

## 🔌 ESP32 Serial Protocol

- **Baud Rate**: `115200`
- **Unlock Command**: `UNLOCK:<duration_ms>\n` (e.g. `UNLOCK:3000\n`)
- **Firmware Location**: `gympos-saas/hardware/`

---

## 🤖 Computer Vision Models

Located in `gympos-saas/desktop/models/`:
- **Face Detection**: `face_detection_yunet_2023mar.onnx`
- **Face Recognition**: `face_recognition_sface_2021dec.onnx` (128-d normalized embeddings)
- **Anti-Tailgating**: `yolov8n.onnx` (Person tracking & multi-occupancy trigger)

---

## 📦 Standalone Installer

The GymPOS installer is a **single `.exe`** that bundles:
- GymPOS desktop application (Tauri v2 / Rust)
- All ONNX AI models (~65 MB)
- Seed SQLite database with default staff accounts
- WebView2 bootstrapper (auto-installs if missing)

Installs to `C:\Program Files\GymPOS\` with Desktop + Start Menu shortcuts.

### Building Locally

```powershell
.\scripts\build-installer.ps1
# Output: bin\GymPOS-Setup-<version>-x64.exe
```

Requirements: Rust stable, `cargo install tauri-cli --version "^2"`, NSIS 3.x

---

## 🚀 Shipping an Update

Updates are fully automated via GitHub Actions + Tauri's signed updater.

### Steps

**1. Bump the version** (must match in BOTH files):

```toml
# desktop/src-tauri/Cargo.toml
version = "0.2.0"
```

```json
// desktop/src-tauri/tauri.conf.json
"version": "0.2.0"
```

**2. Commit and push:**

```bash
git add -A
git commit -m "release: v0.2.0 — <describe what changed>"
git push origin main
```

**3. Tag and push the release tag:**

```bash
git tag v0.2.0
git push origin v0.2.0
```

**4. GitHub Actions runs automatically** (`.github/workflows/release.yml`):
- Compiles the Rust binary (release mode)
- Bundles NSIS installer with all models + seed DB
- Signs the installer with `TAURI_SIGNING_PRIVATE_KEY`
- Creates a GitHub Release containing:
  - `GymPOS_0.2.0_x64-setup.exe` — the standalone installer
  - `GymPOS_0.2.0_x64-setup.exe.sig` — updater signature
  - `latest.json` — version manifest for auto-updater

**5. Installed terminals auto-update:**
- On next launch, the app checks `latest.json` from GitHub Releases
- If a newer version is found, it downloads + installs silently (signature-verified)
- No manual action needed at the gym

### Required GitHub Secret

In your repo → **Settings → Secrets → Actions**, add:

| Secret Name | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Your minisign private key (generate once with `cargo tauri signer generate`) |

The matching public key is already baked into `tauri.conf.json` → `plugins.updater.pubkey`.
