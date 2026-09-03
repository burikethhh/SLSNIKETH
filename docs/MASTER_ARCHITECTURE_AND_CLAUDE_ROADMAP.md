# GymPOS SaaS: Master System Architecture, AAA Framework & Engineering Roadmap for Claude

> **Target Audience:** Engineering Hand-off Document for Claude (and AI Agent Collaborators)  
> **Repository:** `gympos-saas`  
> **Last Verified Git Commit:** `753fd2c` (Branch: `main`)  
> **Status:** Production-Ready Core POS + Cloud Bridge + Centralized CEO License Hierarchy  

---

## Table of Contents
1. [System Architecture Overview](#1-system-architecture-overview)
2. [Complete Feature Inventory](#2-complete-feature-inventory)
3. [AAA Framework Breakdown (Authentication, Authorization, Accounting)](#3-aaa-framework-breakdown)
4. [Biometrics Subsystem: Current Implementation Audit](#4-biometrics-subsystem-current-implementation-audit)
5. [Engineering Blueprint for Claude: Enhancing Face Embedding & Face Scanning](#5-engineering-blueprint-for-claude-enhancing-face-embedding--face-scanning)
6. [Data Schemas & Synchronization Protocol](#6-data-schemas--synchronization-protocol)
7. [Verification Suite & Diagnostic Tooling](#7-verification-suite--diagnostic-tooling)

---

## 1. System Architecture Overview

GymPOS SaaS is a hybrid enterprise platform designed for fitness franchises operating physical turnstiles, point-of-sale registers, and multi-branch gym facilities:

```mermaid
graph TB
    subgraph CEO ["Executive Tier (CEO / SuperAdmin)"]
        CEO_DASH["CEO Master Command Center<br/>(cloud/dashboard/index.html)"]
        RSA_VAULT["Embedded RSA-2048 Signer<br/>Private Key (*.pem)"]
    end

    subgraph CLOUD ["GymPOS Cloud Backend (Axum + SQLite)"]
        API_SERVER["REST API Server (:8080)<br/>Axum + Tokio"]
        CLOUD_DB[(Cloud SQLite DB<br/>gympos_cloud.db)]
        REL_MGR["Fleet Release Controller<br/>Staged Rollouts (1-100%)"]
    end

    subgraph OWNER ["Client Franchise Tier (Gym Owners)"]
        OWNER_PORTAL["Franchise Owner Portal<br/>(cloud/dashboard/portal.html)"]
        STAFF_MGR["Staff & Cashier Provisioning<br/>4-6 Digit PINs"]
        CATALOG_MGR["Branch Pricing Overrides<br/>Catalog & Plans"]
    end

    subgraph DESKTOP ["Branch Hardware Terminal (Tauri + Rust + Vanilla JS)"]
        POS_UI["Front-Desk POS UI<br/>(desktop/webview/index.html)"]
        LOCK_SCREEN["Glassmorphic PIN Lock<br/>Offline Cashier Auth"]
        LOCAL_DB[(Local SQLite DB<br/>gympos.db)]
        BIO_ENGINE["Face Vector Engine (128-d)<br/>Centroid + Angle Matrix"]
        HW_RELAY["ESP32 Relay Controller<br/>Turnstile Solenoids"]
        SYNC_WORKER["Cloud Sync Worker<br/>7-Day Heartbeat + Kill Switch"]
    end

    CEO_DASH -->|Issues Cryptographic RSA Keys| API_SERVER
    RSA_VAULT --> API_SERVER
    OWNER_PORTAL -->|Manages Staff, Prices, Branches| API_SERVER
    API_SERVER <--> CLOUD_DB
    SYNC_WORKER <-->|Heartbeat, Ingest, Attendance, Sales| API_SERVER
    POS_UI --> BIO_ENGINE
    POS_UI --> LOCK_SCREEN
    LOCK_SCREEN --> LOCAL_DB
    BIO_ENGINE --> HW_RELAY
```

### Architectural Tenets
1. **Zero-Trust Offline Independence**: Local POS terminals run turnstile biometrics, sales, and attendance completely offline without internet connectivity. If disconnected, local operations buffer in SQLite.
2. **Centralized CEO License Distribution**: Franchise gym owners cannot self-sign licenses. Only the CEO Master Command Center can sign and issue RSA-2048 keys.
3. **Cryptographic Branch Scoping**: Each issued license token encapsulates the branch UUID (`gym_id`). A key issued to Branch 1 cannot activate Branch 2.
4. **Hierarchical RBAC**: Staff credentials belong to a franchise owner and are filtered to specific branches. Front-desk staff only unlock cashier operations; business revenues and hardware calibrations remain locked.

---

## 2. Complete Feature Inventory

### 2.1 Centralized CEO Master Command Center (`cloud/dashboard/index.html`)
- **Collapsible Franchise Hierarchy**:
  - Replaces flat fleet tables with an interactive accordion grouped by Franchise Owner Account (`cloud_owner_accounts`).
  - Displays Company Name, Owner Email, Registration Timestamp, Total Branch Count, and License Badges (`All Licensed` vs `N Needs Key`).
  - Expanding an owner displays their branch drawer showing Location Names, Gym UUIDs, Tiers, Hardware Terminal IDs, and Action Controls.
- **CEO-Only License Issuance (`/api/v1/admin/branches/:gym_id/issue-key`)**:
  - Cryptographically signs an RSA-2048 license token bound strictly to that branch's UUID.
  - Generates token string: `GPOS-<base64_claims>.<base64_rsa_signature>`.
- **Direct Provisioning (`/api/v1/admin/owners/:email/branches`)**:
  - CEO can add a branch location directly under any franchise owner (with optional automatic key generation).
- **Remote Kill Switch (`/api/v1/remote/disable`)**:
  - Remotely locks out any compromised terminal or delinquent franchise. Polled by desktop sync.
- **Fleet Release & Auto-Updater Controller (`/api/v1/releases`)**:
  - Distributes versioned releases with cryptographic SHA-256 hashes, release notes, channel targeting (`stable`, `beta`, `nightly`), and staged rollout percentages (1–100%).

### 2.2 Franchise Owner Bridge Dashboard (`cloud/dashboard/portal.html`)
- **Self-Service Owner Registration**:
  - "Sign In" vs "Register New Owner" modal tab switcher. Allows new clients to onboard without manual database seeding.
- **Multi-Branch Terminal Management**:
  - Owners can request/register new branch locations. Newly created branches are flagged as `pending_license` awaiting CEO key issuance.
  - Branches with keys display an `Active (Licensed)` badge and a `[Copy Key]` button for the desktop terminal.
- **Staff & Cashier Management**:
  - Owners create staff profiles with 4-6 digit numeric PINs and assign them either to specific branches or as roaming accounts.
- **Branch-Exclusive POS Pricing Overrides**:
  - Owners can set branch-specific price overrides for products and memberships without altering other branches.
- **Consolidated Financial Telemetry**:
  - Aggregates daily gross sales, enrolled member count, active face vectors, turnstile check-in volume, and 30-day revenue charts. Filterable by branch or consolidated across the entire franchise.

### 2.3 Desktop Terminal POS & Turnstile Kiosk (`desktop/`)
- **Glassmorphic PIN Lock Screen (`#terminal-lock-screen`)**:
  - Protects the terminal UI with an on-screen numeric keypad and hardware keyboard support.
  - Cashier logins unlock POS selling, member attendance, and turnstile monitoring. Administrative views (revenue graphs, license vault, COM port relay calibration) are hidden.
- **Master Owner Local Override Modal**:
  - Allows the franchise owner to log in directly on the physical terminal using their cloud credentials to unlock administrative privileges.
- **3-Camera Multi-View Viewfinder Matrix**:
  - Camera 1: Entry Face Scanner.
  - Camera 2: Exit Face Scanner.
  - Camera 3: Overhead Anti-Tailgating ROI Radar.
- **Biometric Face Recognition Engine (`desktop/src-tauri/src/face.rs`)**:
  - 128-dimensional vector memory store with centroid clustering, multi-angle anchor matching, and cosine similarity.
- **ESP32 Serial Turnstile Relay Control (`desktop/src-tauri/src/hardware.rs`)**:
  - Direct hardware pulsing (`PULSE:ENTRY:250\n`) to unlock physical solenoids and turnstile barriers.
- **Walk-in 8-Hour Temporary Biometrics**:
  - Cashiers issue temporary day passes with face capture. Passes automatically expire after 8 hours.

---

## 3. AAA Framework Breakdown

### 3.1 Authentication (AuthN)

```mermaid
graph TD
    subgraph AuthN ["Authentication Mechanisms"]
        A1["CEO Admin Authentication<br/>Bearer Token / Master Secret"]
        A2["Franchise Owner Authentication<br/>bcrypt/PBKDF2 Password Hash + Bearer Session Token"]
        A3["Cashier / Staff Authentication<br/>4-6 Digit PIN -> SHA-256 Hash -> Offline local_staff_accounts"]
        A4["Master Owner Terminal Override<br/>Cloud Email + Password Verification"]
        A5["Cryptographic License Activation<br/>RSA-2048 Public Key Verification + HWID Machine Binding"]
    end
```

1. **CEO Admin Authentication**:
   - Bearer secret configured via the `ADMIN_SECRET_KEY` environment variable (previously a hardcoded fallback string checked into source — rotated; see Section 4.1 audit note below).
   - Verified on all `/api/v1/admin/*`, `/api/v1/licenses/*`, `/api/v1/gyms/*`, and `/api/v1/remote/*` endpoints via `Authorization: Bearer <key>`, compared in constant time.
   - **Audit fix (2026-09-03)**: `POST /api/v1/auth/admin-login`, `POST /api/v1/owner/auth/login`, and `POST /api/v1/owner/auth/register` are now rate-limited (`cloud/src/rate_limit.rs`) to slow down credential brute-forcing — 5 attempts/15min per IP for admin login, 8/10min per (IP, email) plus 30/10min per IP for owner login, 5/hour per IP for registration. Rejected requests get `429` with a `Retry-After` header; successful logins reset the counter for that key.
   - **Audit fix (2026-09-03)**: three CEO-only routes (`admin_list_owners_hierarchy`, `admin_create_branch_for_owner`, `admin_issue_branch_key`) previously discarded the result of `verify_admin_auth(...)` via `let _ = ...`, meaning the auth check ran but its failure was silently ignored — those endpoints were callable with **no credentials at all**. Fixed to propagate the error with `?`.
   - **Audit fix (2026-09-03)**: the RSA private signing key and the admin secret both had hardcoded fallback values committed to source (`DEFAULT_PRODUCTION_PRIVATE_KEY_PEM`, `"gympos_master_ceo_secret_2026"`). Both were removed; the server now requires `RSA_PRIVATE_KEY_PEM` / `ADMIN_SECRET_KEY` env vars for a stable identity, falling back to a random ephemeral value (logged once, with a loud warning) so it can still boot for local development. The keypair was rotated end-to-end (new public key embedded in the desktop client).
2. **Franchise Owner Authentication**:
   - Managed in `cloud_owner_accounts` (`owner_email`, `password_hash`, `company_name`).
   - Authenticated on `/api/v1/owner/auth/login` and `/api/v1/owner/auth/register`.
   - Returns Bearer session token: `owner:<email>`.
   - **Audit fix (2026-09-03)**: `password_hash` migrated from unsalted SHA-256 to Argon2id (`gympos_shared::hash_password`/`verify_password`), with transparent legacy-hash verification for pre-existing accounts.
3. **Front-Desk Cashier / Staff PIN Authentication**:
   - 4-6 digit numeric PIN entered via on-screen keypad.
   - Hashed with Argon2id (`gympos_shared::hash_password`), shared verbatim between cloud and desktop so PINs synced from `cloud_staff_accounts` verify correctly on the terminal.
     - **Audit fix (2026-09-03)**: this previously used unsalted SHA-256, which is trivially rainbow-tabled for a 4-6 digit numeric PIN (10,000-1,000,000 possibilities). Migrated to Argon2id in `gympos-shared` (used by both crates); legacy SHA-256 hashes still verify transparently so existing accounts are not locked out, and get upgraded the next time their PIN is changed. Because Argon2 hashes are salted, `authenticate_staff_pin` changed from an indexed `WHERE pin_hash = ?` lookup to fetch-active-then-verify (fine given branch staff rosters are small).
   - Matched against `local_staff_accounts` in desktop SQLite.
   - Branch Isolation: The cloud heartbeat filters staff syncing:
     ```sql
     SELECT id, owner_email, gym_id, full_name, username, pin_hash, role
     FROM cloud_staff_accounts
     WHERE owner_email = ?1 AND (gym_id = ?2 OR gym_id IS NULL)
     ```
     *Result:* Staff assigned to Branch 1 cannot unlock terminals at Branch 2.
4. **Terminal Master Owner Override**:
   - If an owner visits a branch terminal, they can click "Owner Login Override" to enter their cloud credentials.
   - Successful auth unlocks Manager/Owner privileges on that terminal session.
5. **Cryptographic Terminal License Authentication**:
   - Every terminal stores an active license token in `local_license_cache`.
   - Decoded and verified using the embedded 2048-bit RSA public key:
     - Form: `GPOS-<base64_claims>.<base64_signature>`
     - PKCS#1 v1.5 / PSS signature verification over SHA-256 hash of claims.
     - HWID Binding: Compares `claims.hwid` against the local Windows hardware fingerprint (`MachineGuid` + Disk Serial + Physical MAC + Hostname).

---

### 3.2 Authorization (AuthZ) & RBAC Matrix

| Role | Scope | POS Register | Attendance & Scans | Daily Reports | Financial Analytics | License Key Vault | Hardware Calibration | Add/Edit Staff |
| :--- | :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| **CEO SuperAdmin** | Global Fleet | — | — | Full Fleet | Full Fleet | Full (Sign/Revoke) | Global Overrides | Full Fleet |
| **Franchise Owner** | All Owned Branches | — | Synced Logs | Franchise-wide | Franchise-wide | View/Copy (No Sign) | — | Full for Franchise |
| **Branch Manager** | Assigned Branch | ✅ | ✅ | Branch-only | Branch-only | View-only | Read-only | Local Only |
| **Cashier / Staff** | Assigned Branch | ✅ | ✅ | Shift-only | ❌ (Masked) | ❌ (Hidden) | ❌ (Locked) | ❌ |

---

### 3.3 Accounting (Auditing & Telemetry)

1. **Turnstile Attendance Logs (`cloud_attendance` & `local_attendance`)**:
   - Logged per check-in/check-out with `member_id`, `gym_id`, `direction` (IN/OUT), `method` (FACE / RFID / MANUAL), `confidence_score`, and UTC `timestamp`.
2. **POS Sales Transactions (`cloud_sales` & `local_sales`)**:
   - Stores line items, itemized subtotals, tax, discount, total amount, payment method (`CASH`, `GCASH`, `CARD`), cashier username, and terminal HWID.
3. **Audit Trails (`cloud_audit_logs`)**:
   - Tracks security-sensitive events: `gym_register`, `admin_create_branch`, `admin_issue_branch_key`, `license_revocation`, `kill_switch_toggle`, `staff_created`, and `catalog_override`.
4. **Heartbeat & Telemetry Tracking (`cloud_gyms` & `cloud_licenses`)**:
   - Desktop sync worker pushes local metrics every 5 seconds.
   - Cloud tracks `last_heartbeat_at`, total registered face profiles, and transaction volume.
   - 7-Day Grace Window: Terminals run offline up to 7 consecutive days before locking out if unable to contact the cloud.

---

## 4. Biometrics Subsystem: Current Implementation Audit

The current biometrics implementation resides in [`desktop/src-tauri/src/face.rs`](file:///c:/Users/USER/OneDrive/Desktop/Solo%20Lvel/gympos-saas/desktop/src-tauri/src/face.rs):

```mermaid
graph LR
    subgraph Enrollment ["1. Multi-Angle Enrollment"]
        CAM["Webcam Capture"] --> ANGLES["5 Angles:<br/>Front, Left, Right, Up, Down"]
        ANGLES --> L2_NORM["L2 Normalization<br/>v / ||v||"]
        L2_NORM --> CENTROID["Centroid Clustering<br/>Weighted Mean Vector"]
    end

    subgraph MemoryStore ["2. FaceVectorStore (In-Memory)"]
        CENTROID --> STORE[("RwLock<HashMap<MemberId, FaceEntry>>")]
    end

    subgraph MatchingPipeline ["3. Fast Probe Verification"]
        PROBE["Live Camera Probe"] --> QUAL_GATE["Entropy Quality Gate<br/>StdDev * 2.5 >= 0.15"]
        QUAL_GATE --> NORM_PROBE["L2 Normalize Probe"]
        NORM_PROBE --> SCREEN["Screening 1: Centroid<br/>Cosine Metric"]
        SCREEN --> ANG_SCREEN["Screening 2: Multi-Angle<br/>SIMD 4-Wide Dot Product"]
        ANG_SCREEN --> COOLDOWN{"3-Second Match Cooldown<br/>(Atomic Mutex Check)"}
        COOLDOWN -->|Pass| PULSE["Trigger Turnstile Solenoid<br/>PULSE:ENTRY:250"]
        COOLDOWN -->|Duplicate| REJECT["Suppress Pulse"]
    end
```

### Current Strengths
1. **Centroid Pre-screening**: Rather than comparing against all 5 angles for all $N$ members ($5N$ comparisons), the engine first compares against a single representative centroid vector, discarding non-matching candidates early.
2. **4-Wide Unrolled Dot Product**: `cosine_similarity_fast` breaks the loop into 4-float parallel accumulators (`acc0`, `acc1`, `acc2`, `acc3`), optimizing vector math for 128-d embeddings.
3. **Probe Quality Gating**: `calculate_vector_quality` computes the variance/standard deviation across embedding values. Flat embeddings (often caused by dark frames or occlusion) are rejected before cosine search.
4. **Continuous Learning (EMA Adaptation)**: When a member successfully authenticates with high confidence, `adapt_profile` updates their centroid using Exponential Moving Average:
   $$\mathbf{c}_{\text{new}} = \text{L2\_Norm}\left((1 - \alpha)\mathbf{c}_{\text{old}} + \alpha \mathbf{p}_{\text{live}}\right)$$
5. **Atomic Cooldown Mutex**: An atomic `LastMatchRecord` guards against double-pulsing turnstile relays when a member lingers in front of the camera.

### Current Limitations & Technical Debt
1. **Simulation-Driven Probe Vector Generation**: The webview camera viewfinder currently sends simulated/mocked 128-d vectors via IPC to Tauri, rather than running a neural net inference model directly on the raw video frames.
2. **Linear $O(N)$ Scanning**: As membership scales beyond 2,000 members per gym branch, the linear traversal across memory entries will begin to incur frame drops.
3. **Absence of 2D/3D Anti-Spoofing (Liveness)**: A printed photo or phone screen displaying a member's face can fool an embedding matcher if facial liveness is not evaluated.
4. **Webview/IPC Bandwidth Overhead**: Passing base64 images or large vector arrays across Tauri's webview IPC bridge introduces latency compared to in-process Rust pipeline processing.

---

## 5. Engineering Blueprint for Claude: Enhancing Face Embedding & Face Scanning

This section outlines the exact steps and architecture Claude should follow to elevate the biometric scanning engine to production-grade quality.

```mermaid
graph TD
    subgraph CameraStream ["Camera Stream Pipeline (Rust-Native)"]
        USB_CAM["Direct USB / UVC Video Feed"] --> NOCV["Fast Frame Grabber (OpenCV / rscam / v4l2)"]
        NOCV --> RGB_BUF["Raw RGB Frame Buffer (640x480)"]
    end

    subgraph FacePipeline ["Two-Stage Neural Pipeline (ONNX Runtime)"]
        RGB_BUF --> DETECT["Stage 1: Face Detector<br/>YuNet / UltraFace ONNX"]
        DETECT --> BBOX["Bounding Box + 5 Landmarks<br/>(Eyes, Nose, Mouth Corners)"]
        BBOX --> ALIGN["Affine Alignment & Crop<br/>112x112 Normalized Face"]
        ALIGN --> LIVENESS{"Stage 1.5: Passive Liveness<br/>MiniFASNet ONNX"}
        LIVENESS -->|Spoof Detected| SPOOF_LOG["Log Spoof Attempt & Red Alert"]
        LIVENESS -->|Real Face| RECOG["Stage 2: Feature Extractor<br/>MobileFaceNet / ArcFace 512-d"]
    end

    subgraph FastSearch ["Sub-Millisecond Search Subsystem"]
        RECOG --> PROBE_VEC["512-d Normalized Embedding"]
        PROBE_VEC --> HNSW_INDEX["HNSW Graph Index<br/>instant-distance / hnswlib-rs"]
        HNSW_INDEX --> MATCH_ID["Nearest Neighbor (Cosine Distance <= 0.35)"]
    end

    MATCH_ID --> RELAY_TRIGGER["Trigger Turnstile & Attendance DB"]
```

---

### Task 5.1: Embedded ONNX Runtime Engine in Tauri (`desktop/src-tauri`) — STATUS: DONE (2026-09-03)

**Implemented in `desktop/src-tauri/src/vision.rs`**, using `tract-onnx` (a pure-Rust ONNX inference engine) instead of the `ort` crate suggested below. Rationale: `ort` requires shipping/matching a native `onnxruntime.dll` (plus optional DirectML DLLs) alongside the executable, which is a real deployment/versioning headache for a Windows kiosk installer; `tract-onnx` compiles the inference engine directly into the binary with zero external runtime dependencies, at the cost of DirectML/GPU acceleration (acceptable for these small models — YuNet + SFace — on CPU).

- **Detector decode + NMS** and the **5-point similarity-transform alignment** were transcribed directly from OpenCV's own C++ source (`modules/objdetect/src/face_detect.cpp` and `face_recognize.cpp`), not reverse-engineered, since a subtly wrong anchor-decode formula would silently produce plausible-looking garbage boxes.
- The similarity-transform math is a closed-form least-squares fit (mathematically equivalent to OpenCV's SVD/Umeyama approach for genuine, non-mirrored face landmarks) with a self-verifying unit test (`similarity_transform_recovers_known_transform`) that round-trips a synthetic known transform.
- Verified end-to-end: both bundled ONNX models (`face_detection_yunet_2023mar.onnx`, `face_recognition_sface_2021dec.onnx`) load and run through `tract` without error (`vision::tests::real_onnx_models_load_and_run`), and correctly detect zero faces on a blank frame.
- New Tauri command `scan_face_frame(image_base64)` runs the full detect+align+embed pipeline on a webview-captured camera frame and returns a genuine 128-d embedding — wired into the enrollment Studio flow (`desktop/webview/static/js/app.js: captureCurrentAngleSnapshot`), replacing the previous `generateNormalizedFaceEmbedding()` fabrication.
- Removed a second, fully-fake "Enroll New Member" quick-modal (`submitEnrollMember`) that had no camera at all and always registered members with 100% synthetic vectors; its button now opens the real camera-based Studio flow instead.
- **Not yet verified**: detection/embedding accuracy against an actual live human face (no camera or real face photo available in this sandboxed environment) — recommend a manual smoke test with `cargo tauri dev` and a real webcam before shipping.
- **Original blueprint (kept for reference; superseded by the above):**

**Goal**: Run neural network models directly inside Rust using ONNX Runtime with CPU DirectML / OpenVINO / TensorRT acceleration.

1. **Add Dependencies in `desktop/src-tauri/Cargo.toml`**:
   ```toml
   [dependencies]
   ort = { version = "2.0.0-rc.4", features = ["directml", "copy-dylibs"] } # DirectML for Windows GPU acceleration
   image = "0.24"
   ndarray = "0.15"
   ```
2. **Model Selection**:
   - **Detector**: `yunet.onnx` (Fast, <10ms, predicts bounding boxes + 5 landmarks: left eye, right eye, nose tip, left mouth, right mouth).
   - **Liveness**: `minifasnet.onnx` (Mini-FASNet for 2D passive liveness; outputs real vs spoof probability).
   - **Recognizer**: `mobilefacenet_arcface.onnx` (Generates 512-d feature vectors with high cosine angular separation).
3. **Face Alignment Module**:
   Implement 5-point affine transformation in Rust to rotate and crop faces to standard $112 \times 112$ pixels before embedding extraction. This guarantees pose-invariant vector extraction.

---

### Task 5.2: Upgrading from 128-d to 512-d ArcFace Embeddings — STATUS: DONE (2026-09-03)

**Implemented in `desktop/src-tauri/src/vision.rs` (`FaceEngine.arcface`), `commands.rs` (dim-aware thresholds), `face.rs` (dimension-scaled quality gate), and `desktop/webview/static/js/app.js` (512-d preview mocks)**, standardizing the pipeline on 512-d ArcFace with a safe SFace fallback.

- **Model choice**: InsightFace `buffalo_s` pack's `w600k_mbf.onnx` (MobileFaceNet trained with ArcFace loss on WebFace600K, 13MB) bundled as `desktop/models/face_recognition_arcface_w600k_mbf.onnx` — chosen over `buffalo_l`'s ResNet-50 (`w600k_r50.onnx`, ~170MB) for ~10x smaller disk footprint at the same 512-d output contract, input geometry (112x112), and preprocessing spec. Reference spec (`model_zoo/arcface_onnx.py`, i.e. `blobFromImage(img, 1.0/127.5, (112,112), (127.5,127.5,127.5), swapRB=True)`): RGB order, `(x - 127.5) / 127.5` normalization, input `input.1: [N,3,112,112]`, output `516: [1,512]` (opset 11) — all verified against the real file with `python -c "import onnx; ..."` before writing the decode.
- **Export-artifact workaround (verified, not guessed)**: this InsightFace export carries a degenerate batch dim (`dim_value=0`, no `dim_param`) that makes tract's shape analysis fail (`Failed analyse for node ... ConvHir`, reproduced on tract 0.21.12 *and* 0.23.6 in a scratch crate). Fixed in code via `set_input_fact(0, [1,3,112,112])` before optimizing — no model bytes modified; load + optimize + infer `[1,512]` proven in the scratch probe first, then wired in. Confirmed the bundled file itself is valid (`onnx.checker.check_model` passes).
- **Pipeline**: `FaceEngine` gains an optional `arcface: Option<Plan>` loaded alongside YuNet/SFace; `detect_and_embed` prefers 512-d ArcFace and falls back to 128-d SFace when the file is absent (`embedding_dim()` / `recognizer_name()` report which path served; `scan_face_frame` now returns `embedding_dim` + `model`). Same 5-point similarity-transform + 112x112 crop is reused — ArcFace typically wants 112x112, so alignment is unchanged.
- **Threshold recalibration (per this section's blueprint)**: `process_face_scan` now selects thresholds from the probe itself — 512-d: match `>= 0.68`, adapt `>= 0.82`; legacy 128-d: match `>= 0.60`, adapt `>= 0.88`. The quality gate in `match_vector` scales as `1.7/sqrt(d)` (exactly 0.15 at d=128, ~0.075 at d=512 — required because genuine 512-d embeddings sit at quality ~0.11 and would all be rejected by the old absolute gate), while flat vectors (std ~ 0) are still rejected at any dimension.
- **Dimension-agnostic core untouched**: `cosine_similarity_fast`, HNSW `CentroidPoint(Vec<f32>)`, and centroid logic needed no changes (no hardcoded 128 assumption — the one stale comment was updated). Length mismatch safely yields cosine 0.0 (never a false accept).
- **Migration**: pre-existing 128-d member vectors stay in SQLite and keep matching 128-d probes, but members must be re-enrolled once for 512-d — `upsert_with_expiry` replaces vectors wholesale so one re-enrollment fully converts a member. Preview/test fabrications in `app.js` (`generateNormalizedFaceEmbedding` + inline fallbacks) now emit 512-d via `FACE_EMBEDDING_DIM`.
- **Verified**: `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib` 20/20 (incl. new `real_arcface_model_loads_and_runs` on the real file, `insightface_normalization_maps_pixel_range_to_minus_one_to_one`, `test_quality_gate_scales_to_512d_arcface`), `cargo test --workspace` 7/7, `node --check` clean.
- **Original blueprint (kept for reference; implemented as described above):**

**Goal**: Standardize the system on 512-dimensional ArcFace/CosFace representations.

1. **Update Vector Dimension**:
   - Change feature vector arrays in `desktop/src-tauri/src/face.rs` and `shared/src/lib.rs` from `[f32; 128]` to dynamic or `[f32; 512]`.
2. **SIMD Alignment**:
   - Ensure the vectors are 32-byte aligned (`#[repr(align(32))]`) to allow AVX-256 / AVX-512 vector instructions:
     ```rust
     #[inline]
     pub fn cosine_similarity_512(a: &[f32; 512], b: &[f32; 512]) -> f32 {
         // AVX2 unrolled dot product over 512 floats
         let mut dot = 0.0f32;
         for i in 0..512 {
             dot += a[i] * b[i];
         }
         dot
     }
     ```
3. **Distance Threshold Recalibration**:
   - For 512-d L2-normalized ArcFace vectors:
     - **Match Threshold**: $\text{Cosine Similarity} \ge 0.68$ (Corresponds to False Accept Rate $< 0.001\%$).
     - **High Confidence (Adaptive Learning)**: $\ge 0.82$.

---

### Task 5.3: Sub-Millisecond Search via HNSW Indexing — STATUS: DONE (2026-09-03)

**Implemented in `desktop/src-tauri/src/face.rs`** using `instant-distance` (exactly as suggested below). `FaceVectorStore` now maintains an `HnswMap<CentroidPoint, String>` indexing every member's centroid, alongside the existing `HashMap<String, FaceEntry>` (kept as the source of truth for full multi-angle vectors and metadata).

- **Rebuild strategy**: writes (`upsert_with_expiry`, `adapt_profile`, `remove`, `purge_expired`) set an `AtomicBool` dirty flag rather than rebuilding synchronously; `match_vector` rebuilds the index lazily on the next read if dirty. This amortizes the O(N log N) rebuild cost against the fact that roster writes are rare compared to per-frame match lookups.
- **Search flow**: `match_vector` now retrieves the top-8 nearest-by-centroid candidates from the HNSW index (falling back to a full scan only if the index hasn't been built yet), then runs the exact same precise multi-angle cosine comparison as before on just those candidates — matching the blueprint's "index centroids, then evaluate multi-angle vectors on the top candidates" design.
- **Verified correctness, not just compilation**: added `test_hnsw_finds_correct_match_among_many_members` (50 distinct members, proves the actual HNSW graph search — not the tiny-store fallback path — retrieves the right candidate) and `test_hnsw_index_invalidated_after_removal` (proves a removed member cannot be matched afterward, i.e. the dirty-flag rebuild path actually runs). All prior `face.rs` matching tests still pass unmodified.
- **Not yet done**: no formal large-N (10k-50k members) latency benchmark was run to empirically confirm the roadmap's "<0.5ms at 50,000 members" target — this follows directly from HNSW's known O(log N) query complexity, but hasn't been measured in this environment.

**Original blueprint (kept for reference; implemented as described above):**

**Goal**: Replace linear $O(N)$ scanning with an approximate nearest neighbor (ANN) graph index capable of searching 50,000 members in $< 0.5\text{ ms}$.

1. **Integration**: Use `instant-distance` or `hnswlib-rs`:
   ```rust
   use instant_distance::{Builder, HnswMap, Point};

   #[derive(Clone)]
   pub struct FacePoint([f32; 512]);

   impl Point for FacePoint {
       fn distance(&self, other: &Self) -> f32 {
           // Cosine distance = 1.0 - cosine_similarity
           1.0 - cosine_similarity_512(&self.0, &other.0)
       }
   }
   ```
2. **Centroid Clustering Hierarchy**:
   - Index the centroid of each member in HNSW.
   - Once the top-3 candidate members are returned by HNSW, evaluate their multi-angle vectors to pick the highest match score.

---

### Task 5.4: Multi-Camera Anti-Tailgating Correlation Logic — STATUS: DONE (2026-09-03)

**Implemented in `desktop/src-tauri/src/vision.rs` (`PersonCounter`), `commands.rs` (`count_persons_in_frame`), `lib.rs` (loader + handler registration), and `desktop/webview/static/js/app.js` (`armDoorOpenTailgateSurveillance`)**, replacing the `Math.sin(...)`-based fake "transit density" heuristic that previously stood in for the overhead measurement.

- **Detector**: `PersonCounter` runs the already-bundled `desktop/models/yolov8n.onnx` via `tract-onnx` (same pure-Rust engine as Task 5.1, zero native DLLs). ONNX I/O shape verified against the real file before writing the decode: input `images` is `[1,3,320,320]`, output `output0` is `[1,84,2100]` (channel-major 4 box coords + 80 COCO scores, standard Ultralytics YOLOv8 export). Decode filters COCO class 0 ("person") at confidence 0.45 with IoU-NMS 0.45 via a dedicated `nms_scored()` helper. Preprocessing generalizes `rgb_to_nchw_tensor()` with a `scale` parameter because YOLO expects `[0,1]`-normalized RGB (`1.0/255.0`), unlike the OpenCV face models (`1.0`) — do not revert.
- **ROI contract**: `count_in_roi()` takes percentages matching `gympos_shared::CameraConfig` (`roi_x/roi_y/roi_width/roi_height`, 0-100) — the same values the existing calibration UI (`saveRoiCalibration()`) already tunes, so no new config plumbing was needed. A box counts when its center falls inside the ROI.
- **Wiring**: new Tauri command `count_persons_in_frame(image_base64, roi_x, roi_y, roi_width, roi_height)` (license-gated like `scan_face_frame`) backed by `AppContext.person_counter: Arc<Option<PersonCounter>>`, loaded in `run()` with the same `find_models_dir()` pattern as `FaceEngine` and registered in `generate_handler!`. The webview captures a Camera 3 frame per 250ms tick during the 3.5s door-open window and calls it with the live ROI config; `person_count > 1` increments `suspiciousFrames`, keeping the existing sensitivity-derived `violationThreshold` debounce (default 85 → 2 frames of 14). A `count_persons_in_frame` mock returning `{ person_count: 1 }` was added to the browser-preview fallback so static previews don't alarm.
- **Verified**: `cargo build --manifest-path desktop/src-tauri/Cargo.toml` clean, `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib` 17/17 pass (9 face + 8 vision, incl. `real_yolo_model_loads_and_runs` on the real `yolov8n.onnx` and `nms_scored_suppresses_overlapping_lower_score_boxes`), `cargo test --workspace` 7/7 pass.
- **Not yet verified**: live Camera 3 round-trip against a real overhead feed with two humans (no camera available in this environment) — recommend a manual `cargo tauri dev` pass triggering a scan to confirm the IPC path under real video.
- **Original blueprint (kept for reference; implemented as described above):**

**Goal**: Prevent "two people entering on one scan" by correlating Camera 1 (Face Scan) with Camera 3 (Overhead Anti-Tailgate ROI).

1. **Overhead Person Counting**:
   - Camera 3 runs a lightweight head-and-shoulder detection model (e.g. `yolov8n-head.onnx` or background subtraction ellipse tracker).
2. **Validation Rule**:
   - When Camera 1 validates an enrolled member, a 2.5-second time window opens.
   - Camera 3 measures the count of persons crossing the turnstile entry bounding box.
   - **If count == 1**: Solenoid relay pulses open (`PULSE:ENTRY:250`).
   - **If count > 1**: Sound hardware alarm buzzer (`BUZZER:ON`), flag attendance log with `TAILGATE_SUSPECTED`, capture snapshot, and push alert to cloud.

---

## 6. Data Schemas & Synchronization Protocol

### 6.1 Cloud SQLite Schema (`gympos_cloud.db`)

```sql
-- 1. Owner Accounts (Franchise Credentials)
CREATE TABLE IF NOT EXISTS cloud_owner_accounts (
    owner_email TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    company_name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- 2. Gym Branches
CREATE TABLE IF NOT EXISTS cloud_gyms (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_email TEXT NOT NULL,
    tier TEXT NOT NULL,
    is_active INTEGER DEFAULT 1,
    created_at TEXT NOT NULL,
    FOREIGN KEY(owner_email) REFERENCES cloud_owner_accounts(owner_email)
);

-- 3. Issued RSA Cryptographic Licenses
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
    hardware_lock_enabled INTEGER DEFAULT 1,
    tailgate_detection_enabled INTEGER DEFAULT 1,
    is_revoked INTEGER DEFAULT 0,
    revoked_reason TEXT,
    revoked_at TEXT,
    FOREIGN KEY(gym_id) REFERENCES cloud_gyms(id)
);

-- 4. Staff Accounts (PIN Cashiers)
CREATE TABLE IF NOT EXISTS cloud_staff_accounts (
    id TEXT PRIMARY KEY,
    owner_email TEXT NOT NULL,
    gym_id TEXT, -- NULL for roaming, specific UUID for branch-locked
    full_name TEXT NOT NULL,
    username TEXT NOT NULL,
    pin_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'staff',
    created_at TEXT NOT NULL,
    FOREIGN KEY(owner_email) REFERENCES cloud_owner_accounts(owner_email)
);

-- 5. Branch Pricing Overrides
CREATE TABLE IF NOT EXISTS cloud_branch_product_overrides (
    owner_email TEXT NOT NULL,
    gym_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    custom_price REAL NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(gym_id, product_id)
);
```

### 6.2 Desktop SQLite Schema (`gympos.db`)

```sql
-- Cached License (Offline Activation)
CREATE TABLE IF NOT EXISTS local_license_cache (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    raw_token TEXT NOT NULL,
    verified_at TEXT NOT NULL,
    last_heartbeat_at TEXT NOT NULL
);

-- Local Staff for Offline PIN Verification
CREATE TABLE IF NOT EXISTS local_staff_accounts (
    id TEXT PRIMARY KEY,
    full_name TEXT NOT NULL,
    username TEXT NOT NULL,
    pin_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'staff',
    synced_at TEXT NOT NULL
);

-- Local Biometric Face Vectors
CREATE TABLE IF NOT EXISTS local_face_vectors (
    id TEXT PRIMARY KEY,
    member_id TEXT NOT NULL,
    vector_blob BLOB NOT NULL, -- Serialized IEEE-754 floats
    angle_index INTEGER NOT NULL,
    quality_score REAL NOT NULL,
    expires_at TEXT,
    created_at TEXT NOT NULL
);
```

### 6.3 Synchronization Payload (`SyncPushPayload`)
The terminal periodically sends:
```json
{
  "gym_id": "bb624898-d7d3-4d5c-bd35-e4727069ed12",
  "gym_name": "Titan Fitness - SM Makati Branch",
  "owner_email": "ceo@titan.fitness",
  "timestamp": "2026-09-03T12:00:00Z",
  "attendance_logs": [ ... ],
  "members": [ ... ],
  "face_vectors": [ ... ],
  "sales": [ ... ]
}
```
And receives `SyncResponse`:
```json
{
  "status": "success",
  "remote_disabled": false,
  "sister_branch_members": [ ... ],
  "remote_catalog": [ ... ],
  "remote_plans": [ ... ],
  "remote_promos": [ ... ],
  "staff_accounts": [ ... ]
}
```

---

## 7. Verification Suite & Diagnostic Tooling

All automated test scripts are located in `gympos-saas/tests/`:

### 7.1 Running Automated Backend & Crypto Tests
```powershell
# In gympos-saas directory:
# 1. Rust workspace tests (RSA signature, claims validation)
cargo test --workspace

# 2. CEO License Distribution, Hierarchy, & Owner Registration Test
python tests/test_ceo_license_distribution.py

# 3. RBAC PIN Lock & Branch-Exclusive Pricing Isolation Test
python tests/test_rbac_and_branch_pricing.py
```

### 7.2 Running Browser UI Playwright Tests
```powershell
# 1. Visual Verification of CEO Collapsible Hierarchy & Owner Portal Flow
python tests/test_ui_ceo_hierarchy.py

# 2. Front-Desk Terminal PIN Lockscreen & Cashier/Owner UI RBAC Flow
python tests/test_ui_rbac_flow.py
```

### 7.3 Building Production Release Binaries
```powershell
# Build Cloud Server release
cargo build --release -p gympos-cloud
Copy-Item target/release/gympos-cloud.exe -Destination bin/gympos-cloud.exe -Force

# Build Desktop POS Kiosk release
cargo build --release --manifest-path desktop/src-tauri/Cargo.toml
Copy-Item desktop/src-tauri/target/release/GymPOS.exe -Destination bin/GymPOS.exe -Force
```

---

## 8. Summary Checklist for Incoming Agents (Claude)

When working on GymPOS, adhere to these guidelines:
1. **Do Not Revert CEO-Only Licensing**: Do not allow owners or branches to self-sign license keys. All keys must originate from `/api/v1/admin/branches/:id/issue-key`.
2. **Preserve Branch Scoping**: Whenever new sync entities (promotions, products, staff, hardware configurations) are created, ensure they include `gym_id` filtering so individual branches remain isolated.
3. **Focus on Neural Engine Integration**: Follow the blueprint in **Section 5** to replace mock/simulated vectors with native ONNX inference (`ort` crate), passive liveness validation (`minifasnet`), and HNSW indexing for instant recognition.
