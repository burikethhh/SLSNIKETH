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
