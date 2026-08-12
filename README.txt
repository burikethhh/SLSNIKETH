============================================================
  GymPOS — Standalone Edition
  Solo Leveling Gym
============================================================

VERSION: Source-mode build (Python 3.14 + Electron)
DATE:    2026-05-04

------------------------------------------------------------
  REQUIREMENTS
------------------------------------------------------------
  - Windows 10/11 (64-bit)
  - Python 3.14 installed at:
    C:\Users\USER\AppData\Local\Programs\Python\Python314\
  - Virtual environment (.venv) already configured
  - 2x EMEET SmartCam C60E 4K (USB)
  - ESP32 via USB (Silicon Labs CP210x driver installed)
  - USB HID RFID reader

------------------------------------------------------------
  HOW TO LAUNCH
------------------------------------------------------------
  Double-click:  GymPOS.bat

  The app loads in ~30–35 seconds (Python startup + cameras).
  A spinning GymPOS loading screen appears while services start.

  Default login credentials (first time):
    Admin   →  username: admin   password: admin
    Staff   →  username: admin   password: admin

  CHANGE THE ADMIN PASSWORD immediately after first login:
    Admin → Staff Accounts → Reset PW on the admin row

------------------------------------------------------------
  CAMERA SETUP (this PC)
------------------------------------------------------------
  cam1 — Face scan / member recognition  → device index 1
  cam2 — Tailgate / overhead monitor     → device index 0

  To change: edit STANDALONE\.env
    cam1_index=1
    cam2_index=0

  Camera assignments auto-correct 8 s after startup.
  Face recognition threshold: 0.36 (stricter = fewer false positives)

------------------------------------------------------------
  FACE REGISTRATION — IMPORTANT
------------------------------------------------------------
  All face registrations use 3-angle capture (Front, Left, Right).
  This produces a more accurate averaged SFace embedding.

  For Members: Registration → Step 2 (Face Capture)
    • Start Camera (cam1 auto-selected)
    • Click Front, then Left, then Right buttons
    • 3/3 captured → Next

  For Staff & Familiars: same 3-button capture in their form.

  After registering a face, recognition activates immediately
  (roster cache is invalidated automatically).

------------------------------------------------------------
  FAMILIARS
------------------------------------------------------------
  Familiars are people the admin allows free gym access to
  (no membership fee required). Their entry/exit is still logged.

  Admin → Familiars → Add Familiar
    • Name, Phone, Notes, RFID UID, Face (3-angle)
    • Can scan RFID via hardware reader or type manually
    • Familiar RFID scan works exactly like member scan

------------------------------------------------------------
  STORE POS
------------------------------------------------------------
  Admin setup:   Admin → Store Products (add items, set prices, stocks)
  Staff selling: Staff → Store POS (search, click item, enter qty, sell)
  Reports:       Admin → Store Reports / Store Analytics

  Stock deducts automatically on each sale.
  Low-stock alerts appear in Store Analytics when stock ≤ threshold.

------------------------------------------------------------
  DASHBOARDS
------------------------------------------------------------
  Admin Dashboard  → Combined Gym + Store gross sales overview
  Gym Analytics    → Revenue chart, peak hours, member demographics
  Store Analytics  → Store revenue, top products, stock health

------------------------------------------------------------
  HARDWARE STATUS
------------------------------------------------------------
  ESP32   : COM3 (Silicon Labs CP210x)  — gate control, RFID beep/LCD
  RFID    : USB HID reader — type UID into any screen
  Cameras : Running after ~45s startup (startup fix thread corrects indices)

------------------------------------------------------------
  FOLDER STRUCTURE
------------------------------------------------------------
  GymPOS.bat              — Launch the app
  main.py                 — Source-mode entry point + all extensions
  .env                    — Local hardware configuration
  services/
    camera_manager.py     — Fixed camera engine (DShow-first, config sort)
    (*.pyc)               — Compiled services from original build
  templates/              — All HTML templates
    admin/                — Admin pages (dashboard, store, familiars, staff…)
  static/                 — CSS, JS, fonts, photos
  electron/               — Electron shell
    GymPOS.exe.bak        — Original compiled backend (preserved as backup)
  .venv/                  — Python 3.14 virtual environment

  GymPOS_Data/ (inside electron/node_modules/electron/dist/)
    gym.db                — SQLite database (14 tables)
    .env                  — Runtime hardware config (same as root .env)
    static/photos/        — Member, staff, familiar face photos
    backups/              — Automatic DB backups

------------------------------------------------------------
  DATABASE
------------------------------------------------------------
  14 tables:
    plans, staff, members, familiars, attendance, sales,
    expenses, freezes, coach_assignments, manual_overrides,
    staff_activities, security_incidents, store_products, store_sales

  Default seed data:
    Plans: Day Pass (₱100), 1 Month (₱500), 3 Months (₱1300),
           6 Months (₱2400), 1 Year (₱4500)
    Staff: admin / admin (Administrator, admin role)

------------------------------------------------------------
  KNOWN NOTES
------------------------------------------------------------
  • cam1 may show "degraded" status in the camera info panel
    even when producing frames — this is a watchdog label artifact.
    Check /api/camera-snapshot?cam=cam1 to confirm frames are live.

  • The startup camera-index correction thread fires after 8 s.
    If cameras show both at index 0 immediately after boot,
    wait 15–20 s for the correction to complete.

  • Face recognition requires at least 1 registered face vector.
    "No match: roster=0" in logs is expected until a member
    or familiar is registered with face capture.

------------------------------------------------------------
  FIRST-TIME SETUP (already done on this PC)
------------------------------------------------------------
  If setting up on a NEW PC:
    1. Run _setup\run_setup.vbs  (installs VC++ runtime, drivers)
    2. Restart PC
    3. Install Python 3.14 from python.org
    4. Open STANDALONE folder in terminal:
         python -m venv .venv
         .venv\Scripts\pip install -r requirements.txt  (if present)
         OR install manually — see main.py imports
    5. Double-click GymPOS.bat

============================================================
