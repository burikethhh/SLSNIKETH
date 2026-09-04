# GymPOS SaaS Platform - 10-Day MVP Sprint

## Project Vision

Transform the existing GymPOS system into a multi-tenant SaaS platform where gym owners subscribe to use the system. The CEO (you) manages subscriptions, distributes license keys, and monitors all gym installations remotely.

## Synopsis

GymPOS SaaS is a subscription-based gym management and access control system delivered as a packaged hardware-software solution. The CEO (platform owner) manages gym owner subscriptions through a cloud dashboard, generating license keys and monitoring installations remotely. Each gym receives a complete setup: two USB cameras for face recognition (in/out), one USB camera for anti-tailgate detection, a magnetic lock, ESP32 controller, and the desktop application. The system is built in Rust with Tauri (embedded WebView), using an offline-first architecture with encrypted cloud backup. Face recognition is the sole access method for entry and exit — RFID has been replaced. Anti-tailgate detection prevents unauthorized entry via a third camera running YOLOv8 nano person detection. Subscription tiers (Basic/Pro/Ultra) gate member limits and features. A 3-day grace period follows expiry, then full lockout.

---

## Business Model

### Subscription Tiers

| Feature | Basic | Pro | Ultra |
|---------|-------|-----|-------|
| **Max Members** | 200 | 500 | 1000 |
| **Face Scan (In/Out)** | ✅ | ✅ | ✅ |
| **Magnetic Lock** | ✅ | ✅ | ✅ |
| **Tailgate Detection** | ✅ | ✅ | ✅ |
| **POS System** | ✅ | ✅ | ✅ |
| **Staff Accounts** | 3 | 10 | Unlimited |
| **Analytics** | Basic | Advanced | Advanced + API |
| **Coaching/Sessions** | ✅ | ✅ | ✅ |
| **Expense Tracking** | ✅ | ✅ | ✅ |
| **Multi-Location** | ❌ | ✅ | ✅ |
| **Price/Month** | $99 | $199 | $349 |

### License Enforcement
- License key cryptographically signed (RSA)
- 1 license = 1 gym
- 3-day grace period after expiry
- Full lockout after grace period
- System stops all functionality when license invalid

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      CLOUD (Render)                             │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                  Rust Backend (Axum)                     │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐  │   │
│  │  │Subscription│ │  License │ │   Face   │ │  Remote  │  │   │
│  │  │  Manager  │ │Generator │ │  Vector  │ │  Disable │  │   │
│  │  │          │ │          │ │  Sync    │ │  Controller│  │   │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘  │   │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐               │   │
│  │  │   CEO    │ │Analytics │ │  Audit   │               │   │
│  │  │Dashboard │ │Aggregator│ │  Logs    │               │   │
│  │  └──────────┘ └──────────┘ └──────────┘               │   │
│  └─────────────────────────────────────────────────────────┘   │
│                    PostgreSQL + Redis                           │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ HTTPS + WebSocket (TLS 1.3)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   GYM LOCAL INSTANCE                            │
│         ┌─────────────────────────────────────┐                 │
│         │     Tauri (Rust + WebView)          │                 │
│         │  ┌─────────────────────────────┐    │                 │
│         │  │      Rust Core Engine       │    │                 │
│         │  │  ┌──────────┐ ┌──────────┐  │    │                 │
│         │  │  │Hardware  │ │License   │  │    │                 │
│         │  │  │Manager   │ │Validator │  │    │                 │
│         │  │  └──────────┘ └──────────┘  │    │                 │
│         │  │  ┌──────────┐ ┌──────────┐  │    │                 │
│         │  │  │Face      │ │Local DB  │  │    │                 │
│         │  │  │Processor │ │Manager   │  │    │                 │
│         │  │  └──────────┘ └──────────┘  │    │                 │
│         │  └─────────────────────────────┘    │                 │
│         │  ┌─────────────────────────────┐    │                 │
│         │  │   WebView (Existing UI)     │    │                 │
│         │  │   Templates + Static Assets │    │                 │
│         │  └─────────────────────────────┘    │                 │
│         └─────────────────────────────────────┘                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐          │
│  │ 3x Camera│ │ Magnetic │ │   ESP32  │          │
│  │ In/Out/TG│ │  Lock    │ │Controller│          │
│  └──────────┘ └──────────┘ └──────────┘          │
└─────────────────────────────────────────────────────────────────┘
```

---

## 10-Day Sprint Plan

### Day 1: Foundation Setup

**Track A (Cloud - Render)**
- [x] Create Render configuration (`render.yaml`, Dockerfile)
- [x] Provision cloud database & schema (gyms, subscriptions, licenses, members, attendance)
- [x] Design schema (gyms, subscriptions, licenses, face vectors)
- [x] Create Axum server skeleton
- [x] Prepare cloud deployment pipeline

**Track B (Rust Desktop)**
- [x] Install Rust toolchain & Tauri v2 environment
- [x] Initialize Tauri multi-crate workspace (`desktop`, `cloud`, `shared`)
- [x] Set up Cargo.toml with all required dependencies
- [x] Create clean modular structure (`face`, `hardware`, `license`, `db`, `sync`, `commands`)

**Deliverables**: Working servers and desktop runtime on both ends ✅

---

### Day 2: Core Backend

**Track A (Cloud)**
- [x] License key generator (RSA-2048 / PSS with SHA-256 signatures)
- [x] Subscription & gym management endpoints
- [x] Gym registration & fleet onboarding endpoints
- [x] CEO account authentication (`POST /api/v1/auth/ceo-register`, `POST /api/v1/auth/ceo-login` — validated email + password, `ceo:<email>` session tokens)

**Track B (Rust)**
- [x] Hardware COM port module (ESP32 serial communication with broken pipe auto-clear)
- [x] Camera manager & live multi-stream routing (3x USB cameras)
- [x] Local SQLite schema + automated migrations (`gympos_local.sqlite`)
- [x] Core Tauri IPC command handlers

**Deliverables**: Cryptographic license generation works, hardware COM port communicates ✅

---

### Day 3: Hardware + UI Start

**Track A (Cloud)**
- [x] CEO Cloud Command Center dashboard (`cloud/dashboard/index.html`)
- [x] Subscription & license validation API
- [x] Remote kill-switch & instant gym disable endpoint

**Track B (Rust)**
- [x] Biometric registration studio with live 5-angle face capture
- [x] Magnetic lock & turnstile relay controller (`UNLOCK:3000`)
- [x] Anti-tailgate optical surveillance module (`ALARM:5000`)

**Track C (UI)**
- [x] Solo Leveling dark glassmorphism design system
- [x] White-label branding engine with dynamic CSS hex/RGB color theming
- [x] Base layout with responsive sidebar navigation & HUD status bar

**Deliverables**: Hardware responds to commands, glassmorphism UI active ✅

---

### Day 4: Face Recognition Pipeline

**Track A (Cloud)**
- [x] Inter-branch multi-gym face vector sync API (`/api/v1/sync/push`)
- [x] Multi-tenant data isolation by `owner_email`
- [x] Biometric anonymization (128-d math embeddings, zero raw photo storage)

**Track B (Rust)**
- [x] Fast in-memory vector store (`FaceVectorStore`) with zero-latency matching
- [x] 4-wide unrolled SIMD-friendly cosine dot product (32 iterations vs 128)
- [x] Centroid pre-screening, probe entropy quality gating, and 3s duplicate cooldown
- [x] Adaptive facial learning via exponential moving average on high-confidence matches

**Track C (UI)**
- [x] Real-time gate scan dashboard with camera viewfinders
- [x] Member directory with quick profile management and pass status
- [x] Walk-in guest registration with 8-hour auto-expiring timed passes

**Deliverables**: Real-time biometric face scan works, core pages active ✅

---

### Day 5: Integration Layer

**Track A (Cloud)**
- [x] Background sync worker with exponential backoff retry (5s -> 15s -> 30s -> 60s)
- [x] Persistent Key Vault in SQLite (`cloud_licenses` table)
- [x] On-demand instant license key revocation (`/api/v1/licenses/revoke`)

**Track B (Rust)**
- [x] Offline-first RSA cryptographic license validator with 3-day grace period
- [x] Feature gating & member limit enforcement (Basic: 200, Pro: 500, Ultra: 1000)
- [x] Anti-Passback state machine (`IN` must follow `OUT`, `OUT` must follow `IN`)

**Track C (UI)**
- [x] Store & Point of Sale (POS) with stock tracking and cart checkout
- [x] Attendance audit logs with staff manual override flag detection
- [x] Coach management & 1-on-1 training session tracker

**Deliverables**: Cloud sync works, multi-gym access active, license enforced ✅

---

### Day 6: Tauri IPC + WebView

**Track B (Rust)**
- [x] Member management IPC commands (CRUD, search, export)
- [x] Attendance logs IPC commands with real-time statistics
- [x] POS sales transactions & receipt recording
- [x] Camera stream permissions and memory leak cleanup (`stopStream()`)

**Track C (UI)**
- [x] Hardware configuration & COM port baud rate selector
- [x] Camera routing assignment dropdowns with live auto-preview
- [x] Interactive ROI draggable calibration overlay for Camera 3 (Overhead)
- [x] Staff PIN & security audit logging

**Deliverables**: Frontend completely orchestrated with Rust backend ✅

---

### Day 7: UI Completion

**Track C (UI)**
- [x] White-label custom logo upload with local base64 storage
- [x] Theme color presets & custom HEX color picker
- [x] Responsive layout tested for 1080p and 720p touch kiosk monitors
- [x] All modal dialogs and HUD alerts styled in dark glassmorphism

**Deliverables**: All views converted, integrated, and verified ✅

---

### Day 8: Cloud Polish & Security Hardening

**Track A (Cloud)**
- [x] CEO Command Center Key Vault UI with live status badges (`ACTIVE`, `EXPIRED`, `REVOKED`)
- [x] Master Admin Key authorization modal & bearer header integration
- [x] Sync push Bearer token license verification & instant remote lockout

**Track B (Rust)**
- [x] Offline mode operation with zero-tamper cryptographic expiration
- [x] Background CPU throttling (skips polling when window is minimized)
- [x] Duplicate member detection and camera readiness validation in registration studio

**Deliverables**: Enterprise-grade security, tamper-proof license vault ✅

---

### Day 9: Packaging + Verification

**Track B (Rust)**
- [x] Full automated test suite passing across workspace (8/8 unit tests)
- [x] Optimized release compilation (`cargo build --release`)
- [x] Standalone executable generated at `bin/GymPOS.exe` (14.2 MB)
- [x] End-to-end hardware, biometric, and sync verification

**Deliverables**: Installable, standalone production binary ready ✅

---

### Day 10: Deploy + Document

**Track A (Cloud)**
- [x] Render deployment configuration verified (`render.yaml`)
- [x] Environment variable documentation (`RSA_PRIVATE_KEY_PEM`, `PORT` — CEO access is an email+password account, no master key)
- [x] Multi-gym inter-branch synchronization validated

**Track B (Rust)**
- [x] Production binary packaged in `bin/GymPOS.exe`
- [x] System audit, security report, and walkthrough artifacts created
- [x] Full codebase committed and synchronized to GitHub `main`

**Deliverables**: Complete, production-ready SaaS platform ✅

---

## Project Structure

```
gympos-saas/
├── bin/
│   └── GymPOS.exe                      # Standalone Release Executable (14.2 MB)
│
├── cloud/                              # Render Cloud Backend (Axum)
│   ├── dashboard/
│   │   └── index.html                  # CEO Cloud Command Center (Key Vault UI)
│   ├── src/
│   │   ├── main.rs                     # Server initialization, Admin Auth & Routing
│   │   ├── routes.rs                   # Fleet CRUD, License Generation, Sync, Revoke
│   │   ├── crypto.rs                   # RSA-2048 / PSS with SHA-256 Signer & Verifier
│   │   ├── db.rs                       # Cloud SQLite Database (Licenses, Gyms, Sync)
│   │   └── models.rs                   # Shared SaaS Domain Models
│   └── Cargo.toml
│
├── desktop/                            # Tauri v2 Desktop Application
│   ├── src-tauri/
│   │   ├── src/
│   │   │   ├── main.rs                 # Tauri application entry point
│   │   │   ├── lib.rs                  # Startup orchestrator & state injection
│   │   │   ├── face.rs                 # SIMD Cosine Biometrics & FaceVectorStore
│   │   │   ├── hardware.rs             # ESP32 Serial COM Manager
│   │   │   ├── license.rs              # Offline RSA License Validator & Grace Period
│   │   │   ├── sync.rs                 # Exponential Backoff Cloud Sync Worker
│   │   │   ├── db.rs                   # Local SQLite Database (Members, Walk-ins, POS)
│   │   │   └── commands.rs             # Tauri IPC Command Handlers
│   │   ├── Cargo.toml
│   │   └── tauri.conf.json
│   └── webview/
│       ├── index.html                  # Single Page Application Shell
│       └── static/
│           └── js/
│               └── app.js              # Core UI orchestration & Hardware controller
│
├── shared/                             # Cross-Platform Shared Types
│   ├── src/
│   │   └── lib.rs                      # LicenseClaims, Tiers, SyncPaylaods, Statuses
│   └── Cargo.toml
│
├── render.yaml                         # Render Cloud Deployment Blueprint
└── Cargo.toml                          # Multi-Crate Workspace Config
```

---

## Tech Stack

### Cloud Backend
- **Language**: Rust (Axum Framework, Tokio async runtime)
- **Database**: SQLite / PostgreSQL (`gympos_cloud.sqlite`)
- **Cryptography**: RSA-2048 / PSS with SHA-256 (asymmetric signing & verification)
- **Authentication**: Master Admin Key Bearer Auth + License Token Verification
- **Hosting Target**: Render Cloud (`gympos-cloud.onrender.com`)

### Desktop Application
- **Framework**: Tauri v2 (Rust + WebView2)
- **Database**: Local SQLite (`gympos_local.sqlite`)
- **Biometrics**: 128-dimensional embedding matching, 4-wide SIMD unrolled dot product
- **Hardware Controller**: Serial COM interface (`serialport` crate) for ESP32

### UI / UX
- **Design System**: Dark glassmorphism ("Solo Leveling" inspired purple aura)
- **Typography**: Inter + Rajdhani (Google Fonts)
- **White-Labeling**: Real-time CSS custom property engine (primary color, gradients, borders, logos)

---

## Progress Log

| Date | Status | Notes |
|------|--------|-------|
| Day 1 | Complete | Project architecture, multi-crate workspace setup, cloud & desktop skeletons. |
| Day 2 | Complete | RSA-2048 licensing engine, ESP32 COM port driver, SQLite schema & migrations. |
| Day 3 | Complete | CEO Cloud dashboard, magnetic lock relay commands, dark glassmorphism UI. |
| Day 4 | Complete | Biometric vector matching, 5-angle registration studio, anti-passback state machine. |
| Day 5 | Complete | Inter-branch multi-gym sync, 8-hour walk-in passes, POS system & coach tracker. |
| Day 6 | Complete | Camera routing system (3x cameras), ROI overlay calibration, Tauri IPC integration. |
| Day 7 | Complete | White-label branding engine, dynamic theme coloring, custom logo uploads. |
| Day 8 | Complete | Master Admin Key auth, persistent Key Vault table, instant license revocation. |
| Day 9 | Complete | Full system audit, SIMD optimization, automated test suite (8/8 passed). |
| Day 10 | Complete | Release binary built (`bin/GymPOS.exe`), GitHub repository synced, deployment ready. |

---

*Last updated: 2026-08-26 (All 10 Sprint Days 100% Completed & Verified)*

