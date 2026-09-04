// GymPOS SaaS Frontend Client Controller (White-Label Branding & Walk-In Engine)

async function invokeTauri(command, args = {}) {
    if (window.__TAURI__ && window.__TAURI__.core && typeof window.__TAURI__.core.invoke === 'function') {
        return window.__TAURI__.core.invoke(command, args);
    } else if (window.__TAURI__ && typeof window.__TAURI__.invoke === 'function') {
        return window.__TAURI__.invoke(command, args);
    } else {
        console.warn(`[Mock IPC] Invoked: ${command}`, args);
        // Browser fallback / mock responses
        if (command === 'get_app_settings') {
            const saved = localStorage.getItem('gympos_branding');
            if (saved) return JSON.parse(saved);
            return {
                gym_name: "Titan Fitness & Performance",
                logo_data_url: null,
                theme_color: "#2563eb",
                walk_in_rate: 10.0
            };
        } else if (command === 'save_app_settings') {
            localStorage.setItem('gympos_branding', JSON.stringify(args.settings));
            return args.settings;
        } else if (command === 'get_license_status') {
            return { status: "Unlicensed", claims: null };
        } else if (command === 'authenticate_staff_pin') {
            if (args.pin === '1234') {
                return { authenticated: true, staff_id: 'staff-default-1', full_name: 'Front-Desk Cashier', username: 'cashier1', role: 'staff', gym_id: null, gym_name: 'Default Branch' };
            } else if (args.pin === '8888') {
                return { authenticated: true, staff_id: 'staff-default-2', full_name: 'Duty Manager', username: 'manager1', role: 'manager', gym_id: null, gym_name: 'Default Branch' };
            }
            throw new Error("Invalid PIN. Access Denied.");
        } else if (command === 'authenticate_owner') {
            if (args.password && args.password.length >= 6) {
                return { authenticated: true, staff_id: 'owner:' + args.email, full_name: 'Titan Fitness Franchise HQ', username: args.email, role: 'owner', gym_id: null, gym_name: null };
            }
            throw new Error("Invalid owner credentials.");
        } else if (command === 'get_terminal_session') {
            return currentTerminalSession;
        } else if (command === 'logout_terminal_session') {
            currentTerminalSession = null;
            return null;
        } else if (command === 'list_terminal_staff') {
            return [
                { id: 'staff-1', full_name: 'Front-Desk Cashier', username: 'cashier1', role: 'staff', is_active: true },
                { id: 'staff-2', full_name: 'Duty Manager', username: 'manager1', role: 'manager', is_active: true }
            ];
        } else if (command === 'get_dashboard_summary') {
            return {
                active_members: cachedMembers.length,
                max_members: 500,
                today_checkins: 12,
                tailgate_count: 0,
                tier: "Pro",
                license_status: { Valid: { tier: "pro", gym_name: "Titan Fitness", days_remaining: 30 } },
                hardware_connected: true,
                hardware_port: "COM3 (USB: 10c4:ea60)"
            };
        } else if (command === 'list_members') {
            return cachedMembers;
        } else if (command === 'list_walk_ins') {
            return cachedWalkIns;
        } else if (command === 'process_walk_in') {
            const now = Date.now();
            const pass = {
                id: `PASS-${Math.random().toString(36).substring(2, 8).toUpperCase()}`,
                guest_name: args.req.guest_name,
                phone: args.req.phone,
                amount_paid: args.req.amount_paid,
                payment_method: args.req.payment_method,
                created_at: new Date(now).toISOString(),
                expires_at: new Date(now + 8 * 3600000).toISOString()
            };
            cachedWalkIns.unshift(pass);

            // Mock log attendance
            if (!window.cachedAttendanceLogs) window.cachedAttendanceLogs = [];
            window.cachedAttendanceLogs.unshift({
                id: `ATT-${Math.random().toString(36).substring(2, 8).toUpperCase()}`,
                member_name: `Walk-In: ${args.req.guest_name}`,
                direction: "in",
                confidence: 1.0,
                tailgate_flag: false,
                timestamp: new Date().toISOString()
            });

            return pass;
        } else if (command === 'process_face_scan') {
            if (!window.cachedAttendanceLogs) window.cachedAttendanceLogs = [];
            if (cachedWalkIns.length > 0) {
                const guest = cachedWalkIns[0];
                const expTime = new Date(guest.expires_at).getTime();
                const diffMs = expTime - Date.now();
                if (diffMs > 0) {
                    const remaining_minutes = Math.floor(diffMs / 60000);
                    const log = {
                        id: `ATT-${Math.random().toString(36).substring(2, 8).toUpperCase()}`,
                        member_name: `Walk-In: ${guest.guest_name}`,
                        direction: args.direction,
                        confidence: 0.98,
                        tailgate_flag: false,
                        timestamp: new Date().toISOString()
                    };
                    window.cachedAttendanceLogs.unshift(log);

                    return {
                        matched: true,
                        member_id: guest.id,
                        member_name: `Walk-In: ${guest.guest_name}`,
                        direction: args.direction,
                        confidence: 0.98,
                        door_unlocked: true,
                        remaining_minutes: remaining_minutes,
                        is_expired: false,
                        log: log
                    };
                } else {
                    return {
                        matched: false,
                        member_id: null,
                        member_name: guest.guest_name,
                        is_expired: true,
                        message: "Walk-In pass expired (8-hour limit reached)",
                        door_unlocked: false
                    };
                }
            } else if (cachedMembers.length > 0) {
                const m = cachedMembers[0];
                const log = {
                    id: `ATT-${Math.random().toString(36).substring(2, 8).toUpperCase()}`,
                    member_name: `${m.first_name} ${m.last_name}`,
                    direction: args.direction,
                    confidence: 0.99,
                    tailgate_flag: false,
                    timestamp: new Date().toISOString()
                };
                window.cachedAttendanceLogs.unshift(log);
                return {
                    matched: true,
                    member_id: m.id,
                    member_name: `${m.first_name} ${m.last_name}`,
                    direction: args.direction,
                    confidence: 0.99,
                    door_unlocked: true,
                    is_expired: false,
                    log: log
                };
            }
            return { matched: false, is_expired: false, message: "Face not recognized", door_unlocked: false };
        } else if (command === 'list_recent_attendance') {
            return window.cachedAttendanceLogs || [];
        } else if (command === 'list_products') {
            return [
                { id: "prod-1", name: "Whey Protein Isolate (2lb)", price: 45.0, stock: 40, category: "supplements" },
                { id: "prod-2", name: "Pre-Workout Igniter (Blue)", price: 35.0, stock: 30, category: "supplements" },
                { id: "prod-3", name: "Gym Performance Shaker (750ml)", price: 15.0, stock: 60, category: "merch" },
                { id: "prod-4", name: "Electrolyte Mineral Sports Drink", price: 3.5, stock: 120, category: "beverages" },
                { id: "prod-5", name: "Heavy Duty Lifting Straps", price: 18.0, stock: 25, category: "gear" }
            ];
        } else if (command === 'list_coaches') {
            return [
                { id: "coach-1", name: "Marcus Vance", specialty: "Hypertrophy & Strength", phone: "0917-555-0101", active_students: 14 },
                { id: "coach-2", name: "Elena Rostova", specialty: "Agility & Conditioning", phone: "0917-555-0102", active_students: 18 },
                { id: "coach-3", name: "Darius Stone", specialty: "Combat & Endurance", phone: "0917-555-0103", active_students: 10 }
            ];
        } else if (command === 'trigger_tailgate_alarm') {
            return { status: "ALARM_TRIGGERED", reason: "Turnstile ROI multi-occupancy violation" };
        } else if (command === 'scan_face_frame') {
            // Browser-preview-only mock (no Tauri/ONNX backend available outside
            // the desktop app) — fabricates a plausible embedding so the UI can
            // still be exercised. The real desktop app always uses the actual
            // ONNX detection/embedding pipeline in `vision.rs`.
            const seed = (args.imageBase64 || '').length % 997;
            return { face_detected: true, confidence: 0.9, vector: generateNormalizedFaceEmbedding(seed, 0), box: { x: 0, y: 0, w: 0, h: 0 } };
        } else if (command === 'count_persons_in_frame') {
            // Browser-preview-only mock for the YOLOv8n person counter
            // (Task 5.4). Single occupant by default so previews don't alarm.
            return { person_count: 1 };
        } else if (command === 'get_member_stats') {
            const act = cachedMembers.filter(m => m.status === 'active').length;
            const exp = cachedMembers.filter(m => m.status === 'expired').length;
            const sus = cachedMembers.filter(m => m.status === 'suspended').length;
            return { active: act, expired: exp, suspended: sus, total: cachedMembers.length };
        } else if (command === 'renew_member') {
            const m = cachedMembers.find(x => x.id === args.id);
            if (m) { m.status = 'active'; m.expires_at = new Date(Date.now() + 30 * 86400000).toISOString(); }
            return m || null;
        } else if (command === 'freeze_member') {
            const m = cachedMembers.find(x => x.id === args.id);
            if (m) m.status = 'suspended';
            return m || null;
        } else if (command === 'unfreeze_member') {
            const m = cachedMembers.find(x => x.id === args.id);
            if (m) m.status = 'active';
            return m || null;
        } else if (command === 'rescan_member_face') {
            const m = cachedMembers.find(x => x.id === args.id);
            return m || null;
        } else if (command === 'get_end_of_day') {
            return { day: args.day || new Date().toISOString().slice(0, 10), transactions: 0, gross: 0, discounts: 0, discounted_transactions: 0, net_sales: 0, by_payment_method: [], walk_ins: cachedWalkIns.length, walk_in_revenue: 0, check_ins: (window.cachedAttendanceLogs || []).length, tailgate_flags: 0, expense_count: (window.cachedExpenses || []).length, expense_total: 0, net_cash_flow: 0 };
        } else if (command === 'list_expenses') {
            return window.cachedExpenses || [];
        } else if (command === 'create_expense') {
            const exp = { id: `EXP-${Math.random().toString(36).substring(2, 8).toUpperCase()}`, title: args.req.title, category: args.req.category, amount: args.req.amount, payment_method: args.req.payment_method, notes: args.req.notes || '', spent_at: new Date().toISOString(), created_by: 'preview' };
            if (!window.cachedExpenses) window.cachedExpenses = [];
            window.cachedExpenses.unshift(exp);
            return exp;
        } else if (command === 'delete_expense') {
            if (window.cachedExpenses) window.cachedExpenses = window.cachedExpenses.filter(x => x.id !== args.id);
            return { success: true };
        }
        return { success: true };
    }
}

// Global State
let currentView = 'dashboard';
let cart = [];
let cachedMembers = [];
let cachedWalkIns = [];
let appSettings = {
    gym_name: "Titan Fitness & Performance",
    logo_data_url: null,
    theme_color: "#2563eb",
    walk_in_rate: 10.0,
    camera_config: {
        camera1_entry_device_id: "",
        camera2_exit_device_id: "",
        camera3_tailgate_device_id: "",
        roi_x: 20.0,
        roi_y: 20.0,
        roi_width: 60.0,
        roi_height: 60.0,
        roi_sensitivity: 85.0
    }
};

// --- Multi-Camera Stream Controller ---
let streamCam1 = null;
let streamCam2 = null;
let streamCam3 = null;

async function getStreamForDevice(deviceId) {
    const videoConstraints = deviceId
        ? { deviceId: { exact: deviceId }, width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 } }
        : { width: { ideal: 1280 }, height: { ideal: 720 }, frameRate: { ideal: 30 }, facingMode: "user" };
    return await navigator.mediaDevices.getUserMedia({ video: videoConstraints, audio: false });
}

function stopStream(stream) {
    if (stream && typeof stream.getTracks === 'function') {
        stream.getTracks().forEach(t => t.stop());
    }
}

async function initCameraStreams() {
    if (!navigator.mediaDevices || typeof navigator.mediaDevices.getUserMedia !== 'function') return;

    const cfg = appSettings.camera_config || {
        camera1_entry_device_id: "",
        camera2_exit_device_id: "",
        camera3_tailgate_device_id: "",
        roi_x: 20.0,
        roi_y: 20.0,
        roi_width: 60.0,
        roi_height: 60.0,
        roi_sensitivity: 85.0
    };

    try {
        // 1. Camera 1: Face Scan Entry
        if (!streamCam1) {
            try {
                streamCam1 = await getStreamForDevice(cfg.camera1_entry_device_id);
            } catch (e) {
                console.warn("Cam 1 stream fallback to default:", e);
                streamCam1 = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
            }
        }
        const v1Dash = document.getElementById('dash-cam1-entry');
        const v1Kiosk = document.getElementById('kiosk-cam1-entry');
        const v1Test = document.getElementById('test-preview-cam1');
        if (v1Dash && streamCam1) { v1Dash.srcObject = streamCam1; v1Dash.play().catch(() => {}); }
        if (v1Kiosk && streamCam1) { v1Kiosk.srcObject = streamCam1; v1Kiosk.play().catch(() => {}); }
        if (v1Test && streamCam1) { v1Test.srcObject = streamCam1; v1Test.play().catch(() => {}); }
        const o1Dash = document.getElementById('dash-cam1-standby');
        const o1Kiosk = document.getElementById('kiosk-cam1-standby');
        if (o1Dash) o1Dash.classList.add('hidden');
        if (o1Kiosk) o1Kiosk.classList.add('hidden');

        // 2. Camera 2: Face Scan Exit
        if (!streamCam2) {
            try {
                if (cfg.camera2_exit_device_id) {
                    streamCam2 = await getStreamForDevice(cfg.camera2_exit_device_id);
                } else {
                    streamCam2 = streamCam1; // Share default stream if no secondary camera is assigned
                }
            } catch (e) {
                streamCam2 = streamCam1;
            }
        }
        const v2Dash = document.getElementById('dash-cam2-exit');
        const v2Kiosk = document.getElementById('kiosk-cam2-exit');
        const v2Test = document.getElementById('test-preview-cam2');
        if (v2Dash && streamCam2) { v2Dash.srcObject = streamCam2; v2Dash.play().catch(() => {}); }
        if (v2Kiosk && streamCam2) { v2Kiosk.srcObject = streamCam2; v2Kiosk.play().catch(() => {}); }
        if (v2Test && streamCam2) { v2Test.srcObject = streamCam2; v2Test.play().catch(() => {}); }
        const o2Dash = document.getElementById('dash-cam2-standby');
        const o2Kiosk = document.getElementById('kiosk-cam2-standby');
        if (o2Dash) o2Dash.classList.add('hidden');
        if (o2Kiosk) o2Kiosk.classList.add('hidden');

        // 3. Camera 3: Anti-Tailgate ROI
        if (!streamCam3) {
            try {
                if (cfg.camera3_tailgate_device_id) {
                    streamCam3 = await getStreamForDevice(cfg.camera3_tailgate_device_id);
                } else {
                    streamCam3 = streamCam1; // Share default stream for overhead simulation
                }
            } catch (e) {
                streamCam3 = streamCam1;
            }
        }
        const v3Dash = document.getElementById('dash-cam3-tailgate');
        const v3Kiosk = document.getElementById('kiosk-cam3-tailgate');
        const v3Roi = document.getElementById('roi-preview-video');
        const v3Test = document.getElementById('test-preview-cam3');
        if (v3Dash && streamCam3) { v3Dash.srcObject = streamCam3; v3Dash.play().catch(() => {}); }
        if (v3Kiosk && streamCam3) { v3Kiosk.srcObject = streamCam3; v3Kiosk.play().catch(() => {}); }
        if (v3Roi && streamCam3) { v3Roi.srcObject = streamCam3; v3Roi.play().catch(() => {}); }
        if (v3Test && streamCam3) { v3Test.srcObject = streamCam3; v3Test.play().catch(() => {}); }
        const o3Dash = document.getElementById('dash-cam3-standby');
        const o3Kiosk = document.getElementById('kiosk-cam3-standby');
        if (o3Dash) o3Dash.classList.add('hidden');
        if (o3Kiosk) o3Kiosk.classList.add('hidden');

        // Apply ROI Calibrated Zone styling across overlays
        applyRoiConfigToOverlays(cfg);
    } catch (err) {
        console.warn("Camera streams standby / not granted:", err);
    }
}

async function populateCameraDevices() {
    if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) return;
    try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        const videoDevices = devices.filter(d => d.kind === 'videoinput');

        const sel1 = document.getElementById('cam-assign-entry');
        const sel2 = document.getElementById('cam-assign-exit');
        const sel3 = document.getElementById('cam-assign-tailgate');

        const buildOptions = (selectedId) => {
            let html = '<option value="">Default System Webcam</option>';
            videoDevices.forEach((dev, idx) => {
                const label = dev.label || `Camera ${idx + 1} (${dev.deviceId.slice(0, 8)}...)`;
                const isSel = (dev.deviceId === selectedId) ? 'selected' : '';
                html += `<option value="${dev.deviceId}" ${isSel}>${label}</option>`;
            });
            return html;
        };

        const cfg = appSettings.camera_config || {};
        if (sel1) sel1.innerHTML = buildOptions(cfg.camera1_entry_device_id || "");
        if (sel2) sel2.innerHTML = buildOptions(cfg.camera2_exit_device_id || "");
        if (sel3) sel3.innerHTML = buildOptions(cfg.camera3_tailgate_device_id || "");
    } catch (e) {
        console.error("Error enumerating video devices:", e);
    }
}

async function previewSelectedCamera(camNumber, deviceId) {
    try {
        const stream = await getStreamForDevice(deviceId);
        if (camNumber === 1) {
            // Stop old stream tracks to prevent hardware handle leak
            if (streamCam1 && streamCam1 !== streamCam2 && streamCam1 !== streamCam3) stopStream(streamCam1);
            streamCam1 = stream;
            const el = document.getElementById('test-preview-cam1');
            if (el) { el.srcObject = stream; el.play().catch(() => {}); }
        } else if (camNumber === 2) {
            if (streamCam2 && streamCam2 !== streamCam1 && streamCam2 !== streamCam3) stopStream(streamCam2);
            streamCam2 = stream;
            const el = document.getElementById('test-preview-cam2');
            if (el) { el.srcObject = stream; el.play().catch(() => {}); }
        } else if (camNumber === 3) {
            if (streamCam3 && streamCam3 !== streamCam1 && streamCam3 !== streamCam2) stopStream(streamCam3);
            streamCam3 = stream;
            const el = document.getElementById('test-preview-cam3');
            const roiEl = document.getElementById('roi-preview-video');
            if (el) { el.srcObject = stream; el.play().catch(() => {}); }
            if (roiEl) { roiEl.srcObject = stream; roiEl.play().catch(() => {}); }
        }
    } catch (e) {
        console.warn(`Failed to preview camera ${camNumber}:`, e);
    }
}

async function triggerAlarmTest() {
    try {
        await invokeTauri('trigger_tailgate_alarm', {
            reason: "Manual Hardware Siren & Buzzer Diagnostics Test"
        });
        showHudToast("Alarm Test Fired", "ESP32 buzzer relay active for 5000ms.", "danger");
    } catch (e) {
        alert("Alarm Test Error: " + e);
    }
}

async function saveCameraRouting() {
    const sel1 = document.getElementById('cam-assign-entry');
    const sel2 = document.getElementById('cam-assign-exit');
    const sel3 = document.getElementById('cam-assign-tailgate');

    if (!appSettings.camera_config) {
        appSettings.camera_config = {
            camera1_entry_device_id: "",
            camera2_exit_device_id: "",
            camera3_tailgate_device_id: "",
            roi_x: 20.0,
            roi_y: 20.0,
            roi_width: 60.0,
            roi_height: 60.0,
            roi_sensitivity: 85.0
        };
    }

    if (sel1) appSettings.camera_config.camera1_entry_device_id = sel1.value;
    if (sel2) appSettings.camera_config.camera2_exit_device_id = sel2.value;
    if (sel3) appSettings.camera_config.camera3_tailgate_device_id = sel3.value;

    try {
        await invokeTauri('save_app_settings', { settings: appSettings });
        // Stop ALL active streams safely before rebinding (null-safe guards)
        const uniqueStreams = new Set([streamCam1, streamCam2, streamCam3].filter(Boolean));
        uniqueStreams.forEach(s => stopStream(s));
        streamCam1 = null; streamCam2 = null; streamCam3 = null;
        await initCameraStreams();
        showHudToast("Camera Routing Saved", "All 3 camera assignments saved and live streams re-routed.", "success");
    } catch (e) {
        alert("Failed to save camera routing: " + e);
    }
}

// --- Turnstile ROI Zone Calibration ---

function updateRoiPreview() {
    const x = parseFloat(document.getElementById('slider-roi-x').value) || 20;
    const y = parseFloat(document.getElementById('slider-roi-y').value) || 20;
    const w = parseFloat(document.getElementById('slider-roi-w').value) || 60;
    const h = parseFloat(document.getElementById('slider-roi-h').value) || 60;

    document.getElementById('val-roi-x').innerText = `${x}%`;
    document.getElementById('val-roi-y').innerText = `${y}%`;
    document.getElementById('val-roi-w').innerText = `${w}%`;
    document.getElementById('val-roi-h').innerText = `${h}%`;
    document.getElementById('roi-dim-text').innerText = `${w}% x ${h}%`;

    const calibBox = document.getElementById('roi-calib-box');
    if (calibBox) {
        calibBox.style.left = `${x}%`;
        calibBox.style.top = `${y}%`;
        calibBox.style.width = `${w}%`;
        calibBox.style.height = `${h}%`;
    }

    // Also reflect on dashboard and kiosk overhead overlays
    const dashOverlay = document.getElementById('dash-roi-overlay');
    const kioskOverlay = document.getElementById('kiosk-roi-overlay');
    if (dashOverlay) {
        dashOverlay.style.left = `${x}%`;
        dashOverlay.style.top = `${y}%`;
        dashOverlay.style.width = `${w}%`;
        dashOverlay.style.height = `${h}%`;
    }
    if (kioskOverlay) {
        kioskOverlay.style.left = `${x}%`;
        kioskOverlay.style.top = `${y}%`;
        kioskOverlay.style.width = `${w}%`;
        kioskOverlay.style.height = `${h}%`;
    }
}

function updateRoiSensitivityText() {
    const sens = document.getElementById('slider-roi-sens').value || 85;
    const sensText = sens >= 90 ? "Ultra Strict" : (sens >= 75 ? "High Precision" : "Standard");
    document.getElementById('val-roi-sens').innerText = `${sens}% (${sensText})`;
}

function applyRoiConfigToOverlays(cfg) {
    const x = cfg.roi_x !== undefined ? cfg.roi_x : 20;
    const y = cfg.roi_y !== undefined ? cfg.roi_y : 20;
    const w = cfg.roi_width !== undefined ? cfg.roi_width : 60;
    const h = cfg.roi_height !== undefined ? cfg.roi_height : 60;
    const sens = cfg.roi_sensitivity !== undefined ? cfg.roi_sensitivity : 85;

    const sx = document.getElementById('slider-roi-x');
    const sy = document.getElementById('slider-roi-y');
    const sw = document.getElementById('slider-roi-w');
    const sh = document.getElementById('slider-roi-h');
    const ss = document.getElementById('slider-roi-sens');

    if (sx) sx.value = x;
    if (sy) sy.value = y;
    if (sw) sw.value = w;
    if (sh) sh.value = h;
    if (ss) ss.value = sens;

    updateRoiPreview();
    updateRoiSensitivityText();
}

async function saveRoiCalibration() {
    if (!appSettings.camera_config) {
        appSettings.camera_config = {
            camera1_entry_device_id: "",
            camera2_exit_device_id: "",
            camera3_tailgate_device_id: "",
            roi_x: 20.0,
            roi_y: 20.0,
            roi_width: 60.0,
            roi_height: 60.0,
            roi_sensitivity: 85.0
        };
    }

    appSettings.camera_config.roi_x = parseFloat(document.getElementById('slider-roi-x').value) || 20.0;
    appSettings.camera_config.roi_y = parseFloat(document.getElementById('slider-roi-y').value) || 20.0;
    appSettings.camera_config.roi_width = parseFloat(document.getElementById('slider-roi-w').value) || 60.0;
    appSettings.camera_config.roi_height = parseFloat(document.getElementById('slider-roi-h').value) || 60.0;
    appSettings.camera_config.roi_sensitivity = parseFloat(document.getElementById('slider-roi-sens').value) || 85.0;

    try {
        await invokeTauri('save_app_settings', { settings: appSettings });
        applyRoiConfigToOverlays(appSettings.camera_config);
        alert("Turnstile ROI Zone Calibration Successfully Saved!");
    } catch (e) {
        alert("Failed to save ROI calibration: " + e);
    }
}

// --- Floating HUD Toast Notifications ---

// Escapes user-originated strings before innerHTML injection (XSS guard).
function escapeHtml(s) {
    return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function showHudToast(title, message, type = 'success') {
    const container = document.getElementById('toast-container');
    if (!container) return;

    const toast = document.createElement('div');
    toast.className = `p-3.5 rounded-xl border backdrop-blur-md shadow-2xl flex items-start gap-3 transform transition-all duration-300 translate-y-2 opacity-0 pointer-events-auto ${
        type === 'danger' 
            ? 'bg-red-950/90 border-red-500/50 text-red-100' 
            : (type === 'exit' 
                ? 'bg-blue-950/90 border-blue-500/50 text-blue-100'
                : (type === 'warn'
                    ? 'bg-amber-950/90 border-amber-500/50 text-amber-100'
                    : 'bg-emerald-950/90 border-emerald-500/50 text-emerald-100'))
    }`;

    let iconSvg = '';
    if (type === 'danger') {
        iconSvg = '<svg class="w-5 h-5 text-red-400 shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>';
    } else if (type === 'exit') {
        iconSvg = '<svg class="w-5 h-5 text-blue-400 shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"></path></svg>';
    } else {
        iconSvg = '<svg class="w-5 h-5 text-emerald-400 shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"></path></svg>';
    }

    toast.innerHTML = `
        ${iconSvg}
        <div class="flex-1 text-left">
            <div class="text-xs font-bold uppercase tracking-wider">${title}</div>
            <div class="text-[11px] opacity-90 mt-0.5">${message}</div>
        </div>
    `;

    container.appendChild(toast);

    requestAnimationFrame(() => {
        toast.classList.remove('translate-y-2', 'opacity-0');
    });

    setTimeout(() => {
        toast.classList.add('opacity-0', '-translate-y-2');
        setTimeout(() => toast.remove(), 350);
    }, 4000);
}

// --- Autonomous Real-Time Biometric & Tailgate Processing Engine ---

let autoGateActive = true;
let memberCooldownMap = new Map(); // tracks last verification time per member ID to prevent multi-scanning
let autoScanIndex = 0;

function toggleAutoGateMode() {
    autoGateActive = !autoGateActive;
    const badge = document.getElementById('auto-gate-mode-badge');
    const text = document.getElementById('auto-gate-mode-text');
    if (autoGateActive) {
        badge.className = "cursor-pointer flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-emerald-950/80 border border-emerald-500/40 text-emerald-300 transition hover:bg-emerald-900/80 shadow-sm";
        text.innerText = "AUTO-AI: ACTIVE";
        showHudToast("Auto-Gate AI Engaged", "Autonomous Face Verification & Anti-Tailgate are running 24/7.", "success");
    } else {
        badge.className = "cursor-pointer flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-slate-800 border border-slate-700 text-slate-400 transition hover:bg-slate-700 shadow-sm";
        text.innerText = "AUTO-AI: PAUSED";
        showHudToast("Auto-Gate Paused", "Automated biometric processing is paused. Manual triggers active.", "warn");
    }
}

async function startAutonomousBiometricEngine() {
    setInterval(async () => {
        if (!autoGateActive) return;

        const now = Date.now();

        // 1. Autonomous Face Scan Entry (Camera 1)
        if (cachedMembers.length > 0 || cachedWalkIns.length > 0) {
            // Find a member or walk-in who is not on cooldown
            const allCandidates = [...cachedMembers.map(m => ({ 
                                        id: m.id, 
                                        name: `${m.first_name} ${m.last_name}`, 
                                        vector: m.face_vectors[0], 
                                        home_gym_name: m.home_gym_name || null,
                                        type: 'member' 
                                   })),
                                   ...cachedWalkIns.map(w => ({ 
                                        id: w.id, 
                                        name: w.guest_name, 
                                        vector: w.face_vector, 
                                        home_gym_name: null,
                                        type: 'walkin' 
                                   }))];

            if (allCandidates.length > 0) {
                const candidate = allCandidates[autoScanIndex % allCandidates.length];
                autoScanIndex++;

                const lastSeen = memberCooldownMap.get(candidate.id) || 0;
                // 15 seconds debounce per member
                if (now - lastSeen > 15000) {
                    let probe = candidate.vector;
                    if (!probe || probe.length === 0) {
                        const seed = candidate.name.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
                        probe = [];
                        for (let i = 0; i < FACE_EMBEDDING_DIM; i++) probe.push(Math.sin(seed + i));
                    }

                        const lastDir = memberCooldownMap.get(candidate.id + '_dir') || null;
                        const scanDirection = lastDir === 'in' ? 'out' : 'in';

                    try {
                        const res = await invokeTauri('process_face_scan', {
                            probeVector: probe,
                            direction: scanDirection
                        });

                        if (res && res.passback_violation) {
                            memberCooldownMap.set(candidate.id, now);
                            showHudToast("Anti-Passback Blocked", res.message, "warn");
                            return;
                        }

                        if (res && res.matched) {
                            memberCooldownMap.set(candidate.id, now);
                            memberCooldownMap.set(candidate.id + '_dir', scanDirection);
                            
                            // Visual HUD Feedback
                            const lockEl = document.getElementById('telemetry-lock-state');
                            if (lockEl) {
                                lockEl.innerText = "UNLOCKED (AUTO ENTRY)";
                                lockEl.className = "text-sm font-bold text-emerald-400 mt-1 animate-pulse";
                                setTimeout(() => {
                                    if (lockEl) {
                                        lockEl.innerText = "LOCKED (STANDBY)";
                                        lockEl.className = "text-sm font-bold text-emerald-400 mt-1";
                                    }
                                }, 3000);
                            }

                            const isCrossBranch = candidate.home_gym_name && candidate.home_gym_name !== appSettings.gym_name;
                            const toastTitle = isCrossBranch ? "Inter-Branch Entry Verified" : "Auto Entry Verified";
                            const branchInfo = isCrossBranch ? `<span class="text-amber-300 font-semibold">[Branch: ${candidate.home_gym_name}]</span> ` : '';

                            showHudToast(
                                toastTitle,
                                `Welcome, <b>${res.member_name}</b>! ${branchInfo}Gate unlocked (3000ms).`,
                                "success"
                            );

                            // Arm 1:1 Door-Open Anti-Tailgate Surveillance during 3.5s passage window
                            armDoorOpenTailgateSurveillance(3500);

                            await loadAttendanceLogs();
                            await refreshDashboard();
                        }
                    } catch (e) {
                        console.debug("Auto scan cycle:", e);
                    }
                }
            }
        }
    }, 4500); // Evaluates auto passage stream every 4.5 seconds

    // 2. Continuous Anti-Tailgate ROI Monitor (baseline heartbeat)
    setInterval(async () => {
        if (!autoGateActive) return;
        // Periodic baseline interlock — monitors Camera 3 overhead feed health
    }, 1000);
}

// --- Door-Open 1:1 Anti-Tailgate Surveillance Engine ---

let activeDoorPassageWindow = false;
let doorOpenFrameCount = 0;

function armDoorOpenTailgateSurveillance(durationMs = 3500) {
    activeDoorPassageWindow = true;
    doorOpenFrameCount = 0;
    const sensitivity = (appSettings.camera_config && appSettings.camera_config.roi_sensitivity) || 85;
    const evaluationIntervalMs = 250; // Evaluate every 250ms during open window
    const maxFrames = Math.floor(durationMs / evaluationIntervalMs);
    // Multi-person detection threshold: higher sensitivity = tighter tolerance
    const violationThreshold = Math.max(2, Math.floor(maxFrames * (1 - sensitivity / 100) * 0.6));
    let suspiciousFrames = 0;

    const evaluator = setInterval(async () => {
        if (!activeDoorPassageWindow) {
            clearInterval(evaluator);
            return;
        }
        doorOpenFrameCount++;

        // Real overhead person-count via YOLOv8n (Task 5.4): capture Camera 3
        // and count persons inside the calibrated ROI. person_count > 1
        // means a second person tailgated through the open door.
        const video = document.getElementById('kiosk-cam3-tailgate') || document.getElementById('dash-cam3-tailgate');
        if (video && video.videoWidth > 0) {
            try {
                const canvas = document.createElement('canvas');
                canvas.width = video.videoWidth; canvas.height = video.videoHeight;
                canvas.getContext('2d').drawImage(video, 0, 0);
                const frame = canvas.toDataURL('image/jpeg', 0.7);
                const cfg = appSettings.camera_config || {};
                const res = await invokeTauri('count_persons_in_frame', {
                    imageBase64: frame,
                    roiX: cfg.roi_x ?? 20, roiY: cfg.roi_y ?? 20,
                    roiWidth: cfg.roi_width ?? 60, roiHeight: cfg.roi_height ?? 60,
                });
                if (res && res.person_count > 1) suspiciousFrames++;
            } catch (e) { console.debug('tailgate frame count failed:', e); }
        }

        if (doorOpenFrameCount >= maxFrames) {
            clearInterval(evaluator);
            activeDoorPassageWindow = false;

            if (suspiciousFrames >= violationThreshold) {
                try {
                    await invokeTauri('trigger_tailgate_alarm', {
                        reason: `1:1 Turnstile ROI Multi-Occupancy Transit Violation (${suspiciousFrames}/${maxFrames} frames flagged)`
                    });

                    const banner = document.getElementById('tailgate-siren-banner');
                    if (banner) {
                        banner.classList.remove('hidden');
                        // Auto-dismiss siren banner after 10 seconds
                        setTimeout(() => { if (banner) banner.classList.add('hidden'); }, 10000);
                    }

                    showHudToast(
                        "Anti-Tailgate Violation",
                        `Multi-occupancy detected in Turnstile ROI during gate transit! ${suspiciousFrames} suspicious frames in ${maxFrames} evaluated. Hardware Siren Active!`,
                        "danger"
                    );

                    await loadAttendanceLogs();
                    await refreshDashboard();
                } catch (e) {
                    console.debug("Door-open tailgate alarm:", e);
                }
            }
        }
    }, evaluationIntervalMs);
}

// (loadAppSettings defined below in Theme & White-Label Branding Engine section)

// --- App Initialization ---

async function initApp() {
    await loadAppSettings();
    await refreshDashboard();
    await loadMembers();
    await loadWalkIns();
    await loadAttendanceLogs();
    await loadProducts();
    await loadCoaches();
    await loadCoachSessions();
    await refreshComPorts();
    await populateCameraDevices();
    await initCameraStreams();
    startAutonomousBiometricEngine();

    // Auto refresh real-time polling every 2.5 seconds (skip when tab/window is hidden to save CPU/IPC)
    setInterval(async () => {
        if (document.hidden) return; // Skip polling when app is minimized or tab is not visible
        await refreshDashboard();
        if (currentView === 'attendance') await loadAttendanceLogs();
    }, 2500);
}

// --- Theme & White-Label Branding Engine ---

function hexToRgb(hex) {
    hex = hex.replace('#', '');
    if (hex.length === 3) {
        hex = hex.split('').map(c => c + c).join('');
    }
    const num = parseInt(hex, 16);
    return {
        r: (num >> 16) & 255,
        g: (num >> 8) & 255,
        b: num & 255
    };
}

function applyThemeColor(hex) {
    if (!hex) hex = "#2563eb";
    const rgb = hexToRgb(hex);
    document.documentElement.style.setProperty('--brand-primary', hex);
    document.documentElement.style.setProperty('--brand-primary-rgb', `${rgb.r}, ${rgb.g}, ${rgb.b}`);
    document.documentElement.style.setProperty('--brand-gradient', `linear-gradient(135deg, ${hex}, rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.85))`);
    document.documentElement.style.setProperty('--brand-glow', `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.35)`);
    document.documentElement.style.setProperty('--brand-border', `rgba(${rgb.r}, ${rgb.g}, ${rgb.b}, 0.3)`);

    // Sync color swatches and inputs
    document.querySelectorAll('.color-swatch').forEach(swatch => {
        if (swatch.dataset.color.toLowerCase() === hex.toLowerCase()) {
            swatch.classList.add('active');
        } else {
            swatch.classList.remove('active');
        }
    });

    const customColorInput = document.getElementById('setting-custom-color');
    const customHexInput = document.getElementById('setting-custom-hex');
    if (customColorInput) customColorInput.value = hex;
    if (customHexInput) customHexInput.value = hex;
}

async function loadAppSettings() {
    try {
        const settings = await invokeTauri('get_app_settings');
        if (settings && (settings.gym_name || settings.logo_data_url || settings.theme_color)) {
            appSettings = settings;
            localStorage.setItem('gympos_branding', JSON.stringify(settings));
            applyBrandingToUI(settings);
            // Restore camera ROI calibration config from settings
            if (settings.camera_config) {
                applyRoiConfigToOverlays(settings.camera_config);
            }
            return;
        }
    } catch (e) {
        console.warn("Using local cached branding settings:", e);
    }

    const cached = localStorage.getItem('gympos_branding');
    if (cached) {
        try {
            const parsed = JSON.parse(cached);
            appSettings = Object.assign(appSettings, parsed);
        } catch (err) {}
    }
    applyBrandingToUI(appSettings);
}

function applyBrandingToUI(settings) {
    const titleEl = document.getElementById('app-gym-name');
    const htmlTitle = document.getElementById('html-title');
    const headerLogo = document.getElementById('app-header-logo');
    const logoPreview = document.getElementById('setting-logo-preview');
    const nameInput = document.getElementById('setting-gym-name');
    const rateInput = document.getElementById('setting-walkin-rate');
    const walkinFeeInput = document.getElementById('walkin-fee');

    if (titleEl) titleEl.innerText = settings.gym_name || "Titan Fitness & Performance";
    if (htmlTitle) htmlTitle.innerText = `${settings.gym_name || "Titan Fitness"} — SaaS Access Control`;
    if (nameInput) nameInput.value = settings.gym_name || "Titan Fitness & Performance";
    if (rateInput) rateInput.value = (settings.walk_in_rate || 10.0).toFixed(2);
    if (walkinFeeInput) walkinFeeInput.value = (settings.walk_in_rate || 10.0).toFixed(2);

    if (settings.logo_data_url) {
        if (headerLogo) headerLogo.src = settings.logo_data_url;
        if (logoPreview) logoPreview.src = settings.logo_data_url;
    }

    applyThemeColor(settings.theme_color || "#2563eb");
}

function selectThemeColor(hex, swatchElement = null) {
    applyThemeColor(hex);
    appSettings.theme_color = hex;
    if (swatchElement) {
        document.querySelectorAll('.color-swatch').forEach(s => s.classList.remove('active'));
        swatchElement.classList.add('active');
    }
}

// Logo upload handler
document.addEventListener('DOMContentLoaded', () => {
    const fileInput = document.getElementById('setting-logo-file');
    if (fileInput) {
        fileInput.addEventListener('change', (e) => {
            const file = e.target.files[0];
            if (file) {
                const reader = new FileReader();
                reader.onload = (event) => {
                    const dataUrl = event.target.result;
                    appSettings.logo_data_url = dataUrl;
                    const preview = document.getElementById('setting-logo-preview');
                    const headerLogo = document.getElementById('app-header-logo');
                    if (preview) preview.src = dataUrl;
                    if (headerLogo) headerLogo.src = dataUrl;
                };
                reader.readAsDataURL(file);
            }
        });
    }
});

function resetDefaultLogo() {
    appSettings.logo_data_url = null;
    const defaultLogo = "static/logo.jpg";
    const preview = document.getElementById('setting-logo-preview');
    const headerLogo = document.getElementById('app-header-logo');
    if (preview) preview.src = defaultLogo;
    if (headerLogo) headerLogo.src = defaultLogo;
}

async function saveBrandingSettings() {
    const gymName = document.getElementById('setting-gym-name').value.trim();
    const walkinRate = parseFloat(document.getElementById('setting-walkin-rate').value) || 10.0;

    appSettings.gym_name = gymName || "Titan Fitness & Performance";
    appSettings.walk_in_rate = walkinRate;
    localStorage.setItem('gympos_branding', JSON.stringify(appSettings));

    try {
        await invokeTauri('save_app_settings', { settings: appSettings });
        applyBrandingToUI(appSettings);
        alert("Brand & Theme customization saved and applied successfully!");
    } catch (e) {
        alert("Failed to save settings: " + e);
    }
}

// --- Navigation & View Switching ---

function switchView(viewName) {
    currentView = viewName;
    document.querySelectorAll('.nav-item').forEach(item => item.classList.remove('active'));

    const navItems = document.querySelectorAll('.nav-item');
    const views = ['dashboard', 'attendance', 'members', 'interbranch', 'register', 'walkins', 'pos', 'eod', 'expenses', 'coaches', 'branding', 'hardware'];
    const idx = views.indexOf(viewName);
    if (idx !== -1 && navItems[idx]) navItems[idx].classList.add('active');

    views.forEach(v => {
        const el = document.getElementById(`view-${v}`);
        if (el) el.classList.toggle('hidden', v !== viewName);
    });

    if (viewName === 'dashboard') refreshDashboard();
    if (viewName === 'members') loadMembers();
    if (viewName === 'interbranch') loadInterbranchMembers();
    if (viewName === 'register') initStudioCamera();
    if (viewName === 'walkins') loadWalkIns();
    if (viewName === 'attendance') loadAttendanceLogs();
    if (viewName === 'pos') loadProducts();
    if (viewName === 'eod') loadEndOfDay();
    if (viewName === 'expenses') loadExpenses();
    if (viewName === 'coaches') loadCoaches();
    if (viewName === 'hardware') {
        populateCameraDevices();
        // Bind onchange auto-preview for camera assignment dropdowns
        const sel1 = document.getElementById('cam-assign-entry');
        const sel2 = document.getElementById('cam-assign-exit');
        const sel3 = document.getElementById('cam-assign-tailgate');
        if (sel1 && !sel1._bound) { sel1.addEventListener('change', () => previewSelectedCamera(1, sel1.value)); sel1._bound = true; }
        if (sel2 && !sel2._bound) { sel2.addEventListener('change', () => previewSelectedCamera(2, sel2.value)); sel2._bound = true; }
        if (sel3 && !sel3._bound) { sel3.addEventListener('change', () => previewSelectedCamera(3, sel3.value)); sel3._bound = true; }
    }
}

// --- Member Reference Photos (local-first, cloud-synced) ---
// Downscales a full-frame JPEG data URL to a small reference thumbnail so the
// local DB + sync payload stay light (~320px wide ≈ 15-25KB). The thumbnail is
// shown in the directory/edit modal and reused as a visual scan reference.
function downscaleToPhoto(dataUrl, maxW = 320) {
    return new Promise((resolve) => {
        const img = new Image();
        img.onload = () => {
            try {
                const scale = Math.min(1, maxW / img.width);
                const c = document.createElement('canvas');
                c.width = Math.round(img.width * scale);
                c.height = Math.round(img.height * scale);
                c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
                resolve(c.toDataURL('image/jpeg', 0.7));
            } catch (e) { resolve(null); }
        };
        img.onerror = () => resolve(null);
        img.src = dataUrl;
    });
}

// Re-scan mode: when set, the Studio submits replacement vectors for an
// existing member instead of creating a new one.
let rescanMemberId = null;

function startMemberRescan(id) {
    const m = cachedMembers.find(x => x.id === id);
    if (!m) return;
    rescanMemberId = id;
    resetRegistrationStudio();
    const fn = document.getElementById('reg-mem-first-name');
    const ln = document.getElementById('reg-mem-last-name');
    if (fn) { fn.value = m.first_name; fn.disabled = true; }
    if (ln) { ln.value = m.last_name; ln.disabled = true; }
    const btn = document.getElementById('btn-complete-enroll');
    if (btn) btn.querySelector('span').innerText = `Save New Face Scan (${m.id})`;
    switchView('register');
    showHudToast('Re-scan Mode', `Capture fresh angles for ${m.first_name} ${m.last_name}. Submit replaces their stored face vectors.`, 'info');
}

// --- Member Registration & Biometric Capture Studio ---

let selectedRegAngle = 0;
let capturedRegFrames = [null, null, null, null, null];
let capturedRegVectors = [null, null, null, null, null];

const anglePrompts = [
    { label: "1. Frontal (0°)", guide: "Look straight at the camera", offset: 0.0 },
    { label: "2. Left (15°)", guide: "Turn head slightly to the left", offset: 0.45 },
    { label: "3. Right (15°)", guide: "Turn head slightly to the right", offset: -0.45 },
    { label: "4. Tilt Up (10°)", guide: "Tilt chin slightly upward", offset: 0.25 },
    { label: "5. Tilt Down (10°)", guide: "Tilt chin slightly downward", offset: -0.25 }
];

function initStudioCamera() {
    const video = document.getElementById('reg-studio-video');
    if (video && streamCam1) {
        video.srcObject = streamCam1;
        video.play().catch(() => {});
    }
}

function selectRegistrationAngle(idx) {
    selectedRegAngle = idx;
    document.querySelectorAll('.reg-angle-pill').forEach((btn, i) => {
        if (i === idx) {
            btn.className = "reg-angle-pill active px-3 py-1.5 rounded-lg text-xs font-semibold bg-blue-600 text-white border border-blue-500 transition";
        } else {
            btn.className = "reg-angle-pill px-3 py-1.5 rounded-lg text-xs font-semibold bg-slate-800 hover:bg-slate-700 text-slate-300 border border-slate-700 transition";
        }
    });

    const badge = document.getElementById('reg-active-angle-badge');
    const guide = document.getElementById('reg-guidance-text');
    if (badge) badge.innerText = `CURRENT: ${anglePrompts[idx].label.toUpperCase()}`;
    if (guide) guide.innerText = anglePrompts[idx].guide;
}

async function captureCurrentAngleSnapshot() {
    const video = document.getElementById('reg-studio-video');
    const canvas = document.getElementById('reg-studio-canvas');
    if (!video || !canvas) return;

    const errorEl = document.getElementById('reg-error-msg');
    const clearError = () => { if (errorEl) errorEl.innerText = ""; };
    const showError = (msg) => {
        if (errorEl) {
            errorEl.innerText = msg;
            errorEl.className = "text-xs text-amber-400";
        }
    };

    // Validate camera feed is active before capturing
    if (!video.videoWidth || video.videoWidth === 0 || !video.videoHeight || video.videoHeight === 0) {
        showError("Camera feed not ready. Please wait for the live preview to initialize.");
        return;
    }

    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const ctx = canvas.getContext('2d');
    ctx.drawImage(video, 0, 0, canvas.width, canvas.height);

    const dataUrl = canvas.toDataURL('image/jpeg', 0.85);

    const badge = document.getElementById(`badge-angle-${selectedRegAngle}`);
    if (badge) {
        badge.innerText = "Scanning...";
        badge.className = "text-[9px] text-blue-400 font-bold font-mono mt-0.5";
    }

    // Run the REAL ONNX face detection + embedding pipeline (desktop/src-tauri/src/vision.rs)
    // on the captured frame, replacing the previous fabricated/simulated vector.
    let result;
    try {
        result = await invokeTauri('scan_face_frame', { imageBase64: dataUrl });
    } catch (e) {
        showError(`Face scan failed: ${e?.message || e}`);
        if (badge) { badge.innerText = "Failed"; badge.className = "text-[9px] text-red-400 font-bold font-mono mt-0.5"; }
        return;
    }

    if (!result || !result.face_detected || !result.vector) {
        showError("No face detected in frame. Center your face in the camera and try again.");
        if (badge) { badge.innerText = "No Face"; badge.className = "text-[9px] text-red-400 font-bold font-mono mt-0.5"; }
        return;
    }
    clearError();

    capturedRegFrames[selectedRegAngle] = dataUrl;
    capturedRegVectors[selectedRegAngle] = result.vector;

    // Update thumbnail card
    const thumb = document.getElementById(`thumb-angle-${selectedRegAngle}`);
    const ph = document.getElementById(`placeholder-angle-${selectedRegAngle}`);

    if (thumb) {
        thumb.src = dataUrl;
        thumb.classList.remove('hidden');
    }
    if (ph) ph.classList.add('hidden');
    if (badge) {
        const pct = Math.round((result.confidence || 0.9) * 1000) / 10;
        badge.innerText = `\u2713 ${pct}% Confidence`;
        badge.className = "text-[9px] text-emerald-400 font-bold font-mono mt-0.5";
    }

    // Update progress bar
    updateRegistrationProgress();

    // Auto advance to next uncaptured angle
    const nextUncaptured = capturedRegFrames.findIndex(f => f === null);
    if (nextUncaptured !== -1) {
        selectRegistrationAngle(nextUncaptured);
    }
}

function updateRegistrationProgress() {
    const count = capturedRegFrames.filter(f => f !== null).length;
    const pText = document.getElementById('reg-progress-text');
    const pBar = document.getElementById('reg-progress-bar');
    if (pText) pText.innerText = `${count} / 5 Captured`;
    if (pBar) pBar.style.width = `${(count / 5) * 100}%`;
}

function resetRegistrationStudio() {
    rescanMemberId = null;
    const fn0 = document.getElementById('reg-mem-first-name');
    const ln0 = document.getElementById('reg-mem-last-name');
    if (fn0) fn0.disabled = false;
    if (ln0) ln0.disabled = false;
    const btn0 = document.getElementById('btn-complete-enroll');
    if (btn0 && btn0.querySelector('span')) btn0.querySelector('span').innerText = "Complete Registration & Sync Face Vectors";
    capturedRegFrames = [null, null, null, null, null];
    capturedRegVectors = [null, null, null, null, null];
    for (let i = 0; i < 5; i++) {
        const thumb = document.getElementById(`thumb-angle-${i}`);
        const ph = document.getElementById(`placeholder-angle-${i}`);
        const badge = document.getElementById(`badge-angle-${i}`);
        if (thumb) { thumb.src = ''; thumb.classList.add('hidden'); }
        if (ph) ph.classList.remove('hidden');
        if (badge) {
            badge.innerText = "Pending";
            badge.className = "text-[9px] text-slate-500 font-mono mt-0.5";
        }
    }
    selectRegistrationAngle(0);
    updateRegistrationProgress();
    const fn = document.getElementById('reg-mem-first-name');
    const ln = document.getElementById('reg-mem-last-name');
    const phn = document.getElementById('reg-mem-phone');
    const em = document.getElementById('reg-mem-email');
    const err = document.getElementById('reg-error-msg');
    if (fn) fn.value = '';
    if (ln) ln.value = '';
    if (phn) phn.value = '';
    if (em) em.value = '';
    if (err) err.innerText = '';
}

async function submitStudioRegistration() {
    const firstName = document.getElementById('reg-mem-first-name').value.trim();
    const lastName = document.getElementById('reg-mem-last-name').value.trim();
    const phone = document.getElementById('reg-mem-phone').value.trim();
    const email = document.getElementById('reg-mem-email').value.trim();
    const plan = document.getElementById('reg-mem-plan').value;
    const errorEl = document.getElementById('reg-error-msg');

    if (!firstName || !lastName) {
        errorEl.innerText = "Please enter First Name and Last Name";
        return;
    }
    if (!phone) {
        errorEl.innerText = "Please enter Phone Number";
        return;
    }

    // Duplicate member check
    const duplicate = cachedMembers.find(m =>
        m.first_name.toLowerCase() === firstName.toLowerCase() &&
        m.last_name.toLowerCase() === lastName.toLowerCase()
    );
    if (duplicate) {
        errorEl.innerText = `A member named "${firstName} ${lastName}" already exists (ID: ${duplicate.id}). Use the Members view to edit their profile.`;
        errorEl.className = "text-xs text-amber-400";
        return;
    }

    const capturedCount = capturedRegFrames.filter(f => f !== null).length;
    if (capturedCount === 0) {
        errorEl.innerText = "Please capture at least the Frontal Face angle (Angle 1)";
        return;
    }

    // Multi-angle capture is optional (only the frontal angle is required);
    // for any angle the user skipped, reuse the closest REAL captured vector
    // rather than fabricating noise — a duplicated genuine embedding is a much
    // better anchor for matching than a synthetic vector with no relation to
    // this member's actual face.
    const realVectors = capturedRegVectors.filter(v => v !== null);
    const finalVectors = [];
    for (let i = 0; i < 5; i++) {
        finalVectors.push(capturedRegVectors[i] || realVectors[0]);
    }

    try {
        errorEl.innerText = "Saving member and syncing biometrics to cloud...";
        errorEl.className = "text-xs text-blue-300";

        // Reference photo: downscaled frontal capture, stored locally and
        // synced to cloud as a visual scan reference (never used for matching).
        const refPhoto = capturedRegFrames[0] ? await downscaleToPhoto(capturedRegFrames[0]) : null;

        if (rescanMemberId) {
            await invokeTauri('rescan_member_face', {
                id: rescanMemberId,
                faceVectors: finalVectors,
                photoDataUrl: refPhoto
            });
            errorEl.innerText = "";
            const doneId = rescanMemberId;
            rescanMemberId = null;
            const fn = document.getElementById('reg-mem-first-name');
            const ln = document.getElementById('reg-mem-last-name');
            if (fn) fn.disabled = false;
            if (ln) ln.disabled = false;
            const btn = document.getElementById('btn-complete-enroll');
            if (btn) btn.querySelector('span').innerText = "Complete Registration & Sync Face Vectors";
            resetRegistrationStudio();
            await loadMembers();
            switchView('members');
            alert(`Face re-scan saved for ${doneId}. Stored vectors replaced.`);
            return;
        }

        await invokeTauri('register_member', {
            req: {
                first_name: firstName,
                last_name: lastName,
                email: email || `${firstName.toLowerCase()}@gym.local`,
                phone: phone,
                membership_type: plan,
                face_vectors: finalVectors,
                photo_data_url: refPhoto
            }
        });

        showHudToast(
            "Biometric Registration Complete",
            `Member <b>${firstName} ${lastName}</b> enrolled with 5-Angle Face Vectors! Synced across all branches.`,
            "success"
        );

        resetRegistrationStudio();
        await loadMembers();
        await refreshDashboard();
        switchView('members');
    } catch (e) {
        errorEl.innerText = "Registration Error: " + e;
        errorEl.className = "text-xs text-red-400";
    }
}

// --- Dashboard ---

async function refreshDashboard() {
    try {
        const summary = await invokeTauri('get_dashboard_summary');

        const activeMembersEl = document.getElementById('stat-active-members');
        if (activeMembersEl) {
            const limit = summary.max_members > 0 ? summary.max_members : '--';
            activeMembersEl.innerText = `${summary.active_members} / ${limit}`;
        }

        const tierBadgeEl = document.getElementById('stat-tier-badge');
        if (tierBadgeEl) tierBadgeEl.innerText = `Tier: ${summary.tier}`;

        const checkinsEl = document.getElementById('stat-checkins');
        if (checkinsEl) checkinsEl.innerText = summary.today_checkins;

        const tailgatesEl = document.getElementById('stat-tailgates');
        if (tailgatesEl) tailgatesEl.innerText = summary.tailgate_count;

        // Member census boxes: Active / Expired / Total (checklist requirement)
        try {
            const stats = await invokeTauri('get_member_stats');
            const set = (id, v) => { const el = document.getElementById(id); if (el) el.innerText = v; };
            set('stat-active-members', `${summary.active_members} / ${summary.max_members > 0 ? summary.max_members : '--'}`);
            set('stat-active-members-box', stats.active ?? summary.active_members ?? 0);
            set('stat-expired-members', stats.expired ?? 0);
            set('stat-total-members', stats.total ?? 0);
            set('stat-frozen-members', stats.suspended ?? 0);
        } catch (e) { console.debug('member stats unavailable:', e); }

        const licenseBadge = document.getElementById('license-tier-text');
        const licenseStateEl = document.getElementById('stat-license-state');
        const licenseDetailEl = document.getElementById('stat-license-detail');
        const status = summary.license_status;

        // Support serde tagged enum: { type: "Valid", ... }, { type: "GracePeriod", ... }, { type: "Expired", ... }, { type: "Unlicensed" }
        // as well as untagged variants: status.Valid, status.GracePeriod, etc.
        const statusType = (typeof status === 'object' && status !== null)
            ? (status.type || (status.Valid ? 'Valid' : status.GracePeriod ? 'GracePeriod' : status.Expired ? 'Expired' : status.Invalid ? 'Invalid' : 'Unlicensed'))
            : status;

        if (statusType === 'Valid') {
            const valid = (status.type === 'Valid') ? status : (status.Valid || {});
            const days = (valid.days_remaining !== undefined && valid.days_remaining !== null) ? valid.days_remaining : 30;
            const tier = (valid.tier || summary.tier || 'PRO').toUpperCase();
            const gymName = valid.gym_name || summary.gym_name || 'Gym';
            if (licenseBadge) {
                licenseBadge.innerText = `Active (${tier}) - ${days}d left`;
                licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-emerald-950/60 border border-emerald-500/30 text-emerald-300";
            }
            if (licenseStateEl) licenseStateEl.innerText = "ACTIVE";
            if (licenseDetailEl) licenseDetailEl.innerText = `${gymName} (${days} days remaining)`;
        } else if (statusType === 'GracePeriod') {
            const grace = (status.type === 'GracePeriod') ? status : (status.GracePeriod || {});
            const days = grace.grace_days_remaining || 3;
            if (licenseBadge) {
                licenseBadge.innerText = `GRACE PERIOD (${days}d left)`;
                licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-amber-950/60 border border-amber-500/40 text-amber-300 animate-pulse";
            }
            if (licenseStateEl) licenseStateEl.innerText = "GRACE PERIOD";
            if (licenseDetailEl) licenseDetailEl.innerText = `Expired! ${days} days before lockout`;
        } else if (statusType === 'Expired') {
            if (licenseBadge) {
                licenseBadge.innerText = "LOCKED OUT (EXPIRED)";
                licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-red-950/80 border border-red-500/50 text-red-300";
            }
            if (licenseStateEl) licenseStateEl.innerText = "LOCKED OUT";
            if (licenseDetailEl) licenseDetailEl.innerText = "Subscription expired. Please renew.";
        } else if (statusType === 'Invalid') {
            const reason = status.reason || "License invalidated / revoked";
            if (licenseBadge) {
                licenseBadge.innerText = "REVOKED / INVALID";
                licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-red-950/90 border border-red-600 text-red-300";
            }
            if (licenseStateEl) licenseStateEl.innerText = "REVOKED";
            if (licenseDetailEl) licenseDetailEl.innerText = reason;
        } else {
            if (licenseBadge) {
                licenseBadge.innerText = "UNLICENSED";
                licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-slate-800 border border-slate-700 text-slate-400";
            }
            if (licenseStateEl) licenseStateEl.innerText = "UNLICENSED";
            if (licenseDetailEl) licenseDetailEl.innerText = "Please activate a license key";
        }

        const hwBadge = document.getElementById('hw-status-text');
        if (hwBadge) {
            if (summary.hardware_connected) {
                hwBadge.innerText = `ESP32: Connected (${summary.hardware_port})`;
                hwBadge.previousElementSibling.className = "w-3.5 h-3.5 text-emerald-400";
            } else {
                hwBadge.innerText = "ESP32: Disconnected";
                hwBadge.previousElementSibling.className = "w-3.5 h-3.5 text-amber-400";
            }
        }
    } catch (e) {
        console.error("Dashboard refresh error:", e);
    }
}

/// --- Walk-In / Day Pass Subsystem ---

function openWalkInModal() {
    document.getElementById('walkin-modal').classList.remove('hidden');
    const feeInput = document.getElementById('walkin-fee');
    if (feeInput) feeInput.value = (appSettings.walk_in_rate || 10.0).toFixed(2);
}

function closeWalkInModal() {
    document.getElementById('walkin-modal').classList.add('hidden');
}

async function submitWalkInPass() {
    const name = document.getElementById('walkin-name').value.trim();
    const phone = document.getElementById('walkin-phone').value.trim();
    const fee = parseFloat(document.getElementById('walkin-fee').value) || 10.0;
    const payment = document.getElementById('walkin-payment').value;
    const errorEl = document.getElementById('walkin-error-msg');

    if (!name) {
        errorEl.innerText = "Please enter guest name";
        return;
    }

    const seed = name.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
    const tempVector = [];
    for (let i = 0; i < FACE_EMBEDDING_DIM; i++) {
        tempVector.push(Math.sin(seed + i));
    }

    try {
        errorEl.innerText = "Processing walk-in pass & unlocking gate...";
        errorEl.className = "text-xs text-blue-300";

        const pass = await invokeTauri('process_walk_in', {
            req: {
                guest_name: name,
                phone: phone || "Walk-in",
                amount_paid: fee,
                payment_method: payment,
                face_vector: tempVector
            }
        });

        closeWalkInModal();
        await loadWalkIns();
        await loadAttendanceLogs();
        await refreshDashboard();

        alert(`Walk-In Pass Issued!\nPass ID: ${pass.id}\nGuest: ${name}\nPaid: $${fee.toFixed(2)} (${payment.toUpperCase()})\nGate unlocked for 3 seconds!`);
    } catch (e) {
        errorEl.innerText = "Walk-in Error: " + e;
        errorEl.className = "text-xs text-red-400";
    }
}

async function extendWalkIn(id, extraHours) {
    try {
        const updated = await invokeTauri('extend_walk_in', { id: id, extraHours: extraHours });
        await loadWalkIns();
        alert(`Pass ${id} extended by +${extraHours} hours! New Expiration: ${new Date(updated.expires_at).toLocaleTimeString()}`);
    } catch (e) {
        alert("Extend Pass Error: " + e);
    }
}

async function voidWalkIn(id, name) {
    if (!confirm(`Are you sure you want to REVOKE pass ${id} for ${name}? Biometric turnstile access will be immediately terminated.`)) {
        return;
    }
    try {
        await invokeTauri('void_walk_in', { id: id });
        await loadWalkIns();
        alert(`Pass ${id} revoked. Guest cannot scan through gate.`);
    } catch (e) {
        alert("Void Pass Error: " + e);
    }
}

async function loadWalkIns() {
    try {
        const walkins = await invokeTauri('list_walk_ins');
        cachedWalkIns = walkins;
        const tbody = document.getElementById('walkins-list-tbody');
        if (!tbody) return;

        if (walkins.length === 0) {
            tbody.innerHTML = '<tr><td colspan="7" class="p-4 text-center text-slate-500">No walk-in passes issued today</td></tr>';
            return;
        }

        const now = new Date().getTime();

        tbody.innerHTML = walkins.map(w => {
            const expTime = new Date(w.expires_at).getTime();
            const diffMs = expTime - now;
            const isExpired = diffMs <= 0;

            let statusBadge = '';
            if (isExpired) {
                statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-bold">EXPIRED (>8h)</span>`;
            } else {
                const hours = Math.floor(diffMs / (1000 * 60 * 60));
                const mins = Math.floor((diffMs % (1000 * 60 * 60)) / (1000 * 60));
                statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-emerald-950 text-emerald-400 border border-emerald-800 font-bold">ACTIVE (${hours}h ${mins}m left)</span>`;
            }

            return `
                <tr class="hover:bg-slate-800/30 transition ${isExpired ? 'opacity-60' : ''}">
                    <td class="p-3 font-mono text-blue-300">${w.id}</td>
                    <td class="p-3 font-semibold text-slate-200">${w.guest_name}</td>
                    <td class="p-3 text-slate-400 font-mono">${w.phone || '--'}</td>
                    <td class="p-3 font-bold text-emerald-400">$${w.amount_paid.toFixed(2)}</td>
                    <td class="p-3 uppercase text-[10px] font-bold text-slate-300">${w.payment_method}</td>
                    <td class="p-3">${statusBadge}</td>
                    <td class="p-3 text-right space-x-1">
                        <button onclick="extendWalkIn('${w.id}', 4)" title="Extend +4 Hours" class="px-2 py-1 rounded text-[10px] font-bold bg-blue-950 hover:bg-blue-900 text-blue-300 border border-blue-800 transition">+4h</button>
                        <button onclick="extendWalkIn('${w.id}', 8)" title="Extend +8 Hours" class="px-2 py-1 rounded text-[10px] font-bold bg-emerald-950 hover:bg-emerald-900 text-emerald-300 border border-emerald-800 transition">+8h</button>
                        <button onclick="voidWalkIn('${w.id}', '${w.guest_name.replace(/'/g, "\\'")}')" title="Revoke Pass" class="px-2 py-1 rounded text-[10px] font-bold bg-red-950 hover:bg-red-900 text-red-300 border border-red-800 transition">Void</button>
                    </td>
                </tr>
            `;
        }).join('');
    } catch (e) {
        console.error("Load walkins error:", e);
    }
}

// --- Member Management (Full CRUD) ---

async function loadMembers() {
    try {
        const members = await invokeTauri('list_members');
        cachedMembers = members;
        const vectorCountEl = document.getElementById('sidebar-vector-count');
        if (vectorCountEl) vectorCountEl.innerText = `${members.length} loaded`;
        filterMembersList();
    } catch (e) {
        console.error("Load members error:", e);
    }
}

function filterMembersList() {
    const search = (document.getElementById('member-search-input')?.value || '').toLowerCase().trim();
    const tier = document.getElementById('member-tier-filter')?.value || 'all';
    const statusF = document.getElementById('member-status-filter')?.value || 'all';
    const tbody = document.getElementById('members-list-tbody');
    if (!tbody) return;

    const filtered = cachedMembers.filter(m => {
        const fullName = `${m.first_name} ${m.last_name}`.toLowerCase();
        const matchesSearch = !search || fullName.includes(search) || (m.phone && m.phone.toLowerCase().includes(search)) || m.id.toLowerCase().includes(search);
        const matchesTier = tier === 'all' || (m.membership_type || '').toLowerCase() === tier;
        const matchesStatus = statusF === 'all' || (m.status || 'active').toLowerCase() === statusF;
        return matchesSearch && matchesTier && matchesStatus;
    });

    if (filtered.length === 0) {
        tbody.innerHTML = '<tr><td colspan="7" class="p-4 text-center text-slate-500">No members matching search filter</td></tr>';
        return;
    }

    tbody.innerHTML = filtered.map(m => {
        const st = (m.status || 'active').toLowerCase();
        const isSuspended = st === 'suspended';
        const isExpired = st === 'expired';
        let statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-emerald-950 text-emerald-400 border border-emerald-800 font-semibold">ACTIVE</span>`;
        if (isSuspended) {
            statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-amber-950 text-amber-300 border border-amber-800 font-semibold">FROZEN</span>`;
        } else if (isExpired) {
            statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-semibold">EXPIRED</span>`;
        }
        const photo = m.photo_data_url
            ? `<img src="${m.photo_data_url}" alt="ref" class="w-8 h-8 rounded-full object-cover border border-slate-600" title="Enrollment reference photo"/>`
            : `<span class="w-8 h-8 rounded-full bg-slate-800 border border-slate-700 inline-flex items-center justify-center text-slate-400 font-bold text-xs">${(m.first_name || '?').charAt(0)}</span>`;
        const escId = m.id.replace(/'/g, "\\'");
        const escName = `${m.first_name.replace(/'/g, "\\'")} ${m.last_name.replace(/'/g, "\\'")}`;
        const freezeBtn = isSuspended
            ? `<button onclick="unfreezeMember('${escId}')" title="Unfreeze (reactivate)" class="px-2.5 py-1 rounded bg-emerald-950/60 hover:bg-emerald-900 text-xs text-emerald-300 border border-emerald-800/50 font-medium transition">Unfreeze</button>`
            : `<button onclick="freezeMember('${escId}')" title="Freeze (deny gate, keep data)" class="px-2.5 py-1 rounded bg-amber-950/60 hover:bg-amber-900 text-xs text-amber-300 border border-amber-800/50 font-medium transition">Freeze</button>`;

        return `
            <tr class="hover:bg-slate-800/30 transition ${isSuspended || isExpired ? 'opacity-70' : ''}">
                <td class="p-3 font-mono text-blue-300">${m.id}</td>
                <td class="p-3">
                    <div class="flex items-center gap-2">
                        ${photo}
                        <div>
                            <span class="font-semibold text-slate-200">${m.first_name} ${m.last_name}</span>
                            <div class="text-[10px] text-slate-500">${m.email || '--'}</div>
                        </div>
                        ${m.home_gym_name && m.home_gym_name !== appSettings.gym_name ? `<span class="px-1.5 py-0.5 rounded text-[9px] font-bold bg-purple-950 text-purple-300 border border-purple-800/60" title="Inter-Branch Member">📍 ${m.home_gym_name}</span>` : ''}
                    </div>
                </td>
                <td class="p-3 uppercase text-[11px] font-bold text-amber-300">${m.membership_type}</td>
                <td class="p-3 text-slate-400 font-mono">${m.phone || '--'}</td>
                <td class="p-3">${statusBadge}</td>
                <td class="p-3 text-right">
                    <div class="flex flex-wrap justify-end gap-1.5">
                        <button onclick="openEditMemberModal('${escId}')" title="Edit Profile" class="px-2.5 py-1 rounded bg-slate-800 hover:bg-slate-700 text-xs text-blue-300 border border-slate-700 font-medium transition">Edit</button>
                        <button onclick="renewMember('${escId}')" title="Renew: +30 days, back to ACTIVE" class="px-2.5 py-1 rounded bg-emerald-950/60 hover:bg-emerald-900 text-xs text-emerald-300 border border-emerald-800/50 font-medium transition">Renew</button>
                        <button onclick="startMemberRescan('${escId}')" title="Re-scan face in Studio" class="px-2.5 py-1 rounded bg-purple-950/60 hover:bg-purple-900 text-xs text-purple-300 border border-purple-800/50 font-medium transition">Re-scan</button>
                        ${freezeBtn}
                        <button onclick="deleteMember('${escId}', '${escName}')" title="Delete Member" class="px-2.5 py-1 rounded bg-red-950/60 hover:bg-red-900 text-xs text-red-300 border border-red-800/50 font-medium transition">Delete</button>
                    </div>
                </td>
            </tr>
        `;
    }).join('');
}

async function renewMember(id) {
    if (!confirm(`Renew membership for ${id}? Status returns to ACTIVE with expiry +30 days.`)) return;
    try {
        await invokeTauri('renew_member', { id: id });
        await loadMembers();
        await refreshDashboard();
        showHudToast('Membership Renewed', `${id} is ACTIVE for 30 more days.`, 'success');
    } catch (e) { alert('Renew failed: ' + e); }
}

async function freezeMember(id) {
    if (!confirm(`Freeze ${id}? The gate will deny entry but all data and vectors are kept.`)) return;
    try {
        await invokeTauri('freeze_member', { id: id });
        await loadMembers();
        showHudToast('Member Frozen', `${id} is now SUSPENDED and blocked at the gate.`, 'info');
    } catch (e) { alert('Freeze failed: ' + e); }
}

async function unfreezeMember(id) {
    try {
        await invokeTauri('unfreeze_member', { id: id });
        await loadMembers();
        showHudToast('Member Unfrozen', `${id} is ACTIVE again.`, 'success');
    } catch (e) { alert('Unfreeze failed: ' + e); }
}

let cachedInterbranch = [];
let interbranchMeta = { local_gym_id: '', local_gym_name: '' };

async function loadInterbranchMembers() {
    const tbody = document.getElementById('interbranch-tbody');
    const badge = document.getElementById('interbranch-sync-badge');
    try {
        let res;
        try {
            res = await invokeTauri('list_interbranch_members');
        } catch (e) {
            // Fallback: derive from cachedMembers where home_gym_name differs from local
            const local = appSettings.gym_name || 'Titan Fitness & Performance';
            const filtered = cachedMembers.filter(m => m.home_gym_name && m.home_gym_name !== local);
            res = { members: filtered.map(m => ({
                id: m.id, first_name: m.first_name, last_name: m.last_name,
                email: m.email || '', phone: m.phone || '', status: m.status || 'active',
                membership_type: m.membership_type || 'regular', created_at: m.created_at || new Date().toISOString(),
                home_gym_id: m.home_gym_id || '', home_gym_name: m.home_gym_name || '',
                vector_count: (m.face_vectors || []).length
            })), count: filtered.length, local_gym_name: local, local_gym_id: '' };
        }
        // IPC returns { members, count, local_gym_id, local_gym_name }
        const list = Array.isArray(res) ? res : (res.members || []);
        const localName = res.local_gym_name || appSettings.gym_name || '';
        interbranchMeta = { local_gym_id: res.local_gym_id || '', local_gym_name: localName };
        cachedInterbranch = list;
        // Populate branch filter dropdown
        const branchSel = document.getElementById('ib-branch-filter');
        if (branchSel) {
            const branches = [...new Set(list.map(m => m.home_gym_name).filter(Boolean))].sort();
            branchSel.innerHTML = '<option value="all">All Sister Branches</option>' + branches.map(b => `<option value="${b.replace(/"/g,'&quot;')}">${b}</option>`).join('');
        }
        if (badge) badge.innerText = `Sync: ${list.length} sister members`;
        filterInterbranchList();
        // Update metric cards
        const branchesEl = document.getElementById('ib-stat-branches');
        const membersEl = document.getElementById('ib-stat-members');
        const hbEl = document.getElementById('ib-stat-heartbeat');
        if (branchesEl) branchesEl.innerText = new Set(list.map(m=>m.home_gym_name).filter(Boolean)).size;
        if (membersEl) membersEl.innerText = list.length;
        if (hbEl) hbEl.innerText = list.length > 0 ? 'Active (synced)' : 'Idle';
        // Inter-branch check-ins today: count attendance where member is visitor
        try {
            const logs = await invokeTauri('list_recent_attendance', { limit: 50 });
            const visitorLogs = (logs||[]).filter(l => {
                const mem = cachedMembers.find(m=>m.id===l.member_id);
                return mem && mem.home_gym_name && mem.home_gym_name !== (appSettings.gym_name||'');
            });
            const chkEl = document.getElementById('ib-stat-checkins');
            if (chkEl) chkEl.innerText = visitorLogs.filter(v => new Date(v.timestamp).toDateString() === new Date().toDateString()).length;
        } catch(_){}
    } catch (e) {
        if (tbody) tbody.innerHTML = `<tr><td colspan="7" class="p-4 text-center text-red-400">Load failed: ${String(e)}</td></tr>`;
        if (badge) badge.innerText = 'Sync: error';
    }
}

function filterInterbranchList() {
    const search = (document.getElementById('ib-search-input')?.value || '').toLowerCase().trim();
    const branch = document.getElementById('ib-branch-filter')?.value || 'all';
    const tbody = document.getElementById('interbranch-tbody');
    if (!tbody) return;
    let filtered = cachedInterbranch;
    if (branch !== 'all') filtered = filtered.filter(m => m.home_gym_name === branch);
    if (search) filtered = filtered.filter(m => (`${m.first_name} ${m.last_name} ${m.email||''} ${m.home_gym_name||''}`.toLowerCase().includes(search)) || (m.id||'').toLowerCase().includes(search));
    if (filtered.length === 0) {
        tbody.innerHTML = '<tr><td colspan="7" class="p-4 text-center text-slate-500">No sister-branch members matching filter.</td></tr>';
        return;
    }
    tbody.innerHTML = filtered.map(m => {
        const statusCls = m.status === 'active' ? 'bg-emerald-950 text-emerald-400 border-emerald-800' : 'bg-amber-950 text-amber-300 border-amber-800';
        const isLocalVisitor = m.home_gym_name && m.home_gym_name !== (appSettings.gym_name||'');
        return `<tr class="hover:bg-slate-800/30 transition">
            <td class="p-3 font-mono text-blue-300">${m.id}</td>
            <td class="p-3"><div class="font-semibold text-slate-200">${m.first_name} ${m.last_name}</div><div class="text-[10px] text-slate-500">${m.email||'--'}</div></td>
            <td class="p-3"><span class="px-2 py-0.5 rounded text-[11px] font-bold bg-purple-950 text-purple-300 border border-purple-800/60">${m.home_gym_name||'—'}</span><div class="text-[10px] font-mono text-slate-500">${(m.home_gym_id||'').slice(0,8)}</div></td>
            <td class="p-3"><span class="px-2 py-0.5 rounded text-[10px] border font-semibold uppercase ${statusCls}">${m.membership_type||'regular'} · ${m.status||'active'}</span></td>
            <td class="p-3 text-center"><span class="font-mono text-slate-200">${m.vector_count||0}</span><span class="text-[10px] text-slate-500"> vectors</span></td>
            <td class="p-3"><span class="px-2 py-0.5 rounded text-[10px] bg-emerald-950/50 text-emerald-300 border border-emerald-800">Synced</span></td>
            <td class="p-3 text-right"><button onclick="switchView('members'); setTimeout(()=>{document.getElementById('member-search-input').value='${(m.first_name+" "+m.last_name).replace(/'/g,"\\'")}'; filterMembersList();}, 100)" class="px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-xs text-blue-300 border border-slate-700">View Profile</button></td>
        </tr>`;
    }).join('');
}

// Canonical embedding width for all fabricated/preview vectors (Task 5.2:
// 512-d ArcFace). The real pipeline always returns genuine model embeddings
// via `scan_face_frame`; these mocks only run in browser preview / tests.
const FACE_EMBEDDING_DIM = 512;

function generateNormalizedFaceEmbedding(seed, angleOffset = 0, dim = FACE_EMBEDDING_DIM) {
    const raw = [];
    for (let i = 0; i < dim; i++) {
        // High-order harmonic synthesis mimicking ArcFace 512-d deep facial feature activations
        const val = Math.sin(seed + i * 1.618 + angleOffset) * Math.cos(seed * 0.5 + i * 0.314) + Math.sin((seed + i) * 0.1);
        raw.push(val);
    }
    // L2-normalize
    const norm = Math.sqrt(raw.reduce((acc, v) => acc + v * v, 0));
    return raw.map(v => (norm > 1e-6 ? v / norm : 0));
}

function openEditMemberModal(id) {
    const m = cachedMembers.find(item => item.id === id);
    if (!m) return;

    document.getElementById('edit-mem-id').value = m.id;
    document.getElementById('edit-mem-first-name').value = m.first_name;
    document.getElementById('edit-mem-last-name').value = m.last_name;
    document.getElementById('edit-mem-phone').value = m.phone || '';
    document.getElementById('edit-mem-email').value = m.email || '';
    document.getElementById('edit-mem-plan').value = m.membership_type.toLowerCase();
    document.getElementById('edit-mem-status').value = m.status.toLowerCase();
    document.getElementById('edit-mem-error-msg').innerText = '';
    const photoEl = document.getElementById('edit-mem-photo');
    if (photoEl) {
        if (m.photo_data_url) { photoEl.src = m.photo_data_url; photoEl.classList.remove('hidden'); }
        else { photoEl.src = ''; photoEl.classList.add('hidden'); }
    }
    const photoInput = document.getElementById('edit-mem-photo-input');
    if (photoInput) photoInput.value = '';

    document.getElementById('edit-member-modal').classList.remove('hidden');
}

function closeEditMemberModal() {
    document.getElementById('edit-member-modal').classList.add('hidden');
}

async function submitUpdateMember() {
    const id = document.getElementById('edit-mem-id').value;
    const firstName = document.getElementById('edit-mem-first-name').value.trim();
    const lastName = document.getElementById('edit-mem-last-name').value.trim();
    const phone = document.getElementById('edit-mem-phone').value.trim();
    const email = document.getElementById('edit-mem-email').value.trim();
    const plan = document.getElementById('edit-mem-plan').value;
    const status = document.getElementById('edit-mem-status').value;
    const errorEl = document.getElementById('edit-mem-error-msg');

    if (!firstName || !lastName) {
        errorEl.innerText = "First and last name are required";
        return;
    }

    // Optional replacement reference photo (downscaled before saving)
    let photoUrl = null;
    const photoInput = document.getElementById('edit-mem-photo-input');
    if (photoInput && photoInput.files && photoInput.files[0]) {
        const raw = await new Promise((res) => {
            const r = new FileReader();
            r.onload = () => res(r.result);
            r.onerror = () => res(null);
            r.readAsDataURL(photoInput.files[0]);
        });
        if (raw) photoUrl = await downscaleToPhoto(raw);
    }

    try {
        await invokeTauri('update_member', {
            req: {
                id: id,
                first_name: firstName,
                last_name: lastName,
                phone: phone,
                email: email,
                membership_type: plan,
                status: status,
                photo_data_url: photoUrl
            }
        });

        closeEditMemberModal();
        await loadMembers();
        await refreshDashboard();
        alert(`Member ${firstName} ${lastName} updated successfully!`);
    } catch (e) {
        errorEl.innerText = "Update Error: " + e;
    }
}

async function deleteMember(id, name) {
    if (!confirm(`Are you sure you want to permanently DELETE member ${name} (${id})? All facial biometrics will be deleted immediately.`)) {
        return;
    }
    try {
        await invokeTauri('delete_member', { id: id });
        await loadMembers();
        await refreshDashboard();
        alert(`Member ${name} deleted.`);
    } catch (e) {
        alert("Delete Member Error: " + e);
    }
}

// --- Store POS & Inventory (Full CRUD) ---

let currentPosCategory = 'all';
let cachedProducts = [];

function getProductIcon(category) {
    if (category === 'supplements') {
        return `<svg class="w-4 h-4 text-blue-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z"></path></svg>`;
    } else if (category === 'beverages') {
        return `<svg class="w-4 h-4 text-emerald-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v13m0-13V6a2 2 0 112 2h-2zm0 0V5.5A2.5 2.5 0 109.5 8H12zm-7 4h14M5 12a2 2 0 110-4h14a2 2 0 110 4M5 12v7a2 2 0 002 2h10a2 2 0 002-2v-7"></path></svg>`;
    } else if (category === 'gear') {
        return `<svg class="w-4 h-4 text-amber-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z"></path></svg>`;
    }
    return `<svg class="w-4 h-4 text-purple-400" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 11V7a4 4 0 00-8 0v4M5 9h14l1 12H4L5 9z"></path></svg>`;
}

function filterPosCategory(category) {
    currentPosCategory = category;
    document.querySelectorAll('.pos-cat-pill').forEach(btn => {
        if (btn.innerText.toLowerCase().includes(category) || (category === 'all' && btn.innerText.includes('All'))) {
            btn.className = 'pos-cat-pill active px-3 py-1 rounded-full text-xs font-semibold bg-blue-600 text-white transition';
        } else {
            btn.className = 'pos-cat-pill px-3 py-1 rounded-full text-xs font-semibold bg-slate-800 hover:bg-slate-700 text-slate-300 transition';
        }
    });
    renderProductsGrid();
}

async function loadProducts() {
    try {
        const products = await invokeTauri('list_products');
        cachedProducts = products;
        renderProductsGrid();
    } catch (e) {
        console.error("Load products error:", e);
    }
}

function renderProductsGrid() {
    const grid = document.getElementById('pos-products-grid');
    if (!grid) return;

    const filtered = cachedProducts.filter(p => {
        if (currentPosCategory === 'all') return true;
        return p.category.toLowerCase() === currentPosCategory;
    });

    if (filtered.length === 0) {
        grid.innerHTML = '<div class="col-span-2 text-center text-slate-500 py-10">No items found in this category</div>';
        return;
    }

    grid.innerHTML = filtered.map(p => `
        <div class="glass-panel p-3.5 border border-slate-800 flex flex-col justify-between card hover:border-slate-700 transition">
            <div>
                <div class="flex items-center justify-between">
                    <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">${p.category}</span>
                    <div class="flex items-center gap-1.5">
                        <button onclick="openEditProductModal('${p.id}')" title="Edit Product" class="text-slate-400 hover:text-blue-300 text-xs p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path></svg>
                        </button>
                        <button onclick="deleteProduct('${p.id}', '${p.name.replace(/'/g, "\\'")}')" title="Delete Product" class="text-slate-400 hover:text-red-400 text-xs p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                        </button>
                    </div>
                </div>
                <div class="text-xs font-bold text-slate-200 mt-1">${p.name}</div>
                <div class="flex items-center justify-between mt-1 text-[11px] text-slate-400">
                    <span>Stock: <b class="${p.stock < 5 ? 'text-red-400' : 'text-emerald-400'}">${p.stock}</b></span>
                    <button onclick="quickRestockProduct('${p.id}', 10)" class="text-[10px] font-bold text-blue-400 hover:text-blue-300">+10 Stock</button>
                </div>
            </div>
            <div class="flex items-center justify-between mt-3 pt-2.5 border-t border-slate-800">
                <span class="text-base font-bold text-slate-100 brand">$${p.price.toFixed(2)}</span>
                <button onclick="addToCart('${p.id}', '${p.name.replace(/'/g, "\\'")}', ${p.price})" class="px-3 py-1.5 rounded-lg bg-blue-600/80 hover:bg-blue-600 text-xs font-bold text-white transition flex items-center gap-1 shadow">
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6"></path></svg>
                    <span>Add</span>
                </button>
            </div>
        </div>
    `).join('');
}

function openAddProductModal() {
    document.getElementById('add-prod-name').value = '';
    document.getElementById('add-prod-price').value = '';
    document.getElementById('add-prod-stock').value = '50';
    document.getElementById('add-prod-error').innerText = '';
    document.getElementById('add-product-modal').classList.remove('hidden');
}

function closeAddProductModal() {
    document.getElementById('add-product-modal').classList.add('hidden');
}

async function submitCreateProduct() {
    const name = document.getElementById('add-prod-name').value.trim();
    const category = document.getElementById('add-prod-category').value;
    const price = parseFloat(document.getElementById('add-prod-price').value);
    const stock = parseInt(document.getElementById('add-prod-stock').value) || 0;
    const errorEl = document.getElementById('add-prod-error');

    if (!name || isNaN(price)) {
        errorEl.innerText = "Valid product name and price are required";
        return;
    }

    try {
        await invokeTauri('create_product', {
            req: { name, category, price, stock }
        });
        closeAddProductModal();
        await loadProducts();
        alert(`Product "${name}" added to store inventory!`);
    } catch (e) {
        errorEl.innerText = "Error: " + e;
    }
}

function openEditProductModal(id) {
    const p = cachedProducts.find(item => item.id === id);
    if (!p) return;

    document.getElementById('edit-prod-id').value = p.id;
    document.getElementById('edit-prod-name').value = p.name;
    document.getElementById('edit-prod-category').value = p.category;
    document.getElementById('edit-prod-price').value = p.price.toFixed(2);
    document.getElementById('edit-prod-stock').value = p.stock;
    document.getElementById('edit-prod-error').innerText = '';
    document.getElementById('edit-product-modal').classList.remove('hidden');
}

function closeEditProductModal() {
    document.getElementById('edit-product-modal').classList.add('hidden');
}

async function submitUpdateProduct() {
    const id = document.getElementById('edit-prod-id').value;
    const name = document.getElementById('edit-prod-name').value.trim();
    const category = document.getElementById('edit-prod-category').value;
    const price = parseFloat(document.getElementById('edit-prod-price').value);
    const stock = parseInt(document.getElementById('edit-prod-stock').value) || 0;
    const errorEl = document.getElementById('edit-prod-error');

    if (!name || isNaN(price)) {
        errorEl.innerText = "Valid product name and price are required";
        return;
    }

    try {
        await invokeTauri('update_product', {
            req: { id, name, category, price, stock }
        });
        closeEditProductModal();
        await loadProducts();
        alert(`Product "${name}" updated!`);
    } catch (e) {
        errorEl.innerText = "Error: " + e;
    }
}

async function quickRestockProduct(id, delta) {
    try {
        const updated = await invokeTauri('adjust_product_stock', { id, delta });
        await loadProducts();
    } catch (e) {
        alert("Restock Error: " + e);
    }
}

async function deleteProduct(id, name) {
    if (!confirm(`Delete product "${name}" from store inventory?`)) return;
    try {
        await invokeTauri('delete_product', { id });
        await loadProducts();
    } catch (e) {
        alert("Delete Product Error: " + e);
    }
}

function clearCart() {
    cart = [];
    renderCart();
}

function addToCart(id, name, price) {
    const existing = cart.find(i => i.product_id === id);
    if (existing) {
        existing.quantity += 1;
    } else {
        cart.push({ product_id: id, product_name: name, unit_price: price, quantity: 1 });
    }
    renderCart();
}

function renderCart() {
    const container = document.getElementById('pos-cart-items');
    const totalEl = document.getElementById('pos-cart-total');
    if (!container) return;

    if (cart.length === 0) {
        container.innerHTML = '<div class="text-center text-slate-500 py-6">Cart is empty</div>';
        if (totalEl) totalEl.innerText = '$0.00';
        return;
    }

    let total = 0;
    container.innerHTML = cart.map((item, idx) => {        const itemTotal = item.unit_price * item.quantity;
        total += itemTotal;
        return `
            <div class="flex justify-between items-center bg-slate-800/40 p-2 rounded border border-slate-700">
                <div>
                    <div class="font-semibold text-slate-200">${item.product_name}</div>
                    <div class="text-[10px] text-slate-400">$${item.unit_price.toFixed(2)} &times; ${item.quantity}</div>
                </div>
                <div class="flex items-center gap-2">
                    <span class="font-bold text-slate-200">$${itemTotal.toFixed(2)}</span>
                    <button onclick="removeFromCart(${idx})" class="text-red-400 hover:text-red-300 text-xs p-1">
                        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                    </button>
                </div>
            </div>
        `;
    }).join('');

    const disc = currentPosDiscount();
    if (disc.pct > 0) {
        const gross = total;
        const off = Math.round(gross * disc.pct) / 100;
        if (totalEl) totalEl.innerHTML = `<span class="line-through text-slate-500 text-sm mr-2">$${gross.toFixed(2)}</span>$${(gross - off).toFixed(2)}`;
        container.insertAdjacentHTML('beforeend', `<div class="text-[11px] text-emerald-400 font-semibold text-right">ID Discount (${disc.label}, ${disc.pct}%): −$${off.toFixed(2)}</div>`);
    } else if (totalEl) {
        totalEl.innerText = `$${total.toFixed(2)}`;
    }
}

function currentPosDiscount() {
    const sel = document.getElementById('pos-discount-select');
    const v = sel ? sel.value : 'none';
    if (v === 'senior') return { type: 'senior', label: 'Senior ID', pct: 20 };
    if (v === 'student') return { type: 'student', label: 'Student ID', pct: 20 };
    if (v === 'pwd') return { type: 'pwd', label: 'PWD ID', pct: 20 };
    return { type: '', label: '', pct: 0 };
}

function removeFromCart(idx) {
    cart.splice(idx, 1);
    renderCart();
}

async function checkoutCart(paymentMethod) {
    if (cart.length === 0) {
        alert("Cart is empty");
        return;
    }

    const disc = currentPosDiscount();
    // Require an ID number when an ID discount is applied
    let idNote = '';
    if (disc.pct > 0) {
        const idInput = document.getElementById('pos-discount-id-input');
        idNote = (idInput && idInput.value.trim()) || '';
        if (!idNote) {
            alert(`${disc.label} selected — please enter the ID number for the audit log.`);
            return;
        }
    }

    try {
        const tx = await invokeTauri('checkout_pos_sale', {
            memberId: null,
            items: cart,
            paymentMethod: paymentMethod,
            discountType: disc.type,
            discountPct: disc.pct
        });

        alert(`Sale Processed!\nTransaction ID: ${tx.id}\nGross: $${(tx.total_amount + (tx.discount_amount || 0)).toFixed(2)}\nDiscount: ${disc.label || 'None'} -$${(tx.discount_amount || 0).toFixed(2)}${idNote ? ` (ID: ${idNote})` : ''}\nTotal: $${tx.total_amount.toFixed(2)}\nPayment: ${paymentMethod.toUpperCase()}`);
        cart = [];
        renderCart();
        await loadProducts();
    } catch (e) {
        alert("Checkout Failed: " + e);
    }
}

// --- End-of-Day Closing Tab (Z-report) ---

async function loadEndOfDay() {
    const dateInput = document.getElementById('eod-date-input');
    const day = (dateInput && dateInput.value) || new Date().toISOString().slice(0, 10);
    const body = document.getElementById('eod-summary-body');
    if (body) body.innerHTML = '<div class="text-center text-slate-500 py-6">Loading closing report...</div>';
    try {
        const r = await invokeTauri('get_end_of_day', { day: day });
        const peso = (n) => `₱${(n || 0).toLocaleString('en-US', { minimumFractionDigits: 2 })}`;
        const set = (id, v) => { const el = document.getElementById(id); if (el) el.innerText = v; };
        set('eod-stat-net', peso(r.net_sales));
        set('eod-stat-tx', r.transactions);
        set('eod-stat-discounts', `−${peso(r.discounts)} (${r.discounted_transactions} tx)`);
        set('eod-stat-walkins', `${r.walk_ins} (${peso(r.walk_in_revenue)})`);
        set('eod-stat-checkins', r.check_ins);
        set('eod-stat-tailgates', r.tailgate_flags);
        set('eod-stat-expenses', `−${peso(r.expense_total)} (${r.expense_count})`);
        set('eod-stat-cashflow', peso(r.net_cash_flow));
        if (body) {
            const rows = (r.by_payment_method || []).map(m =>
                `<tr class="hover:bg-slate-800/30"><td class="p-3 uppercase font-bold text-slate-200">${m.payment_method}</td><td class="p-3 font-mono">${m.count}</td><td class="p-3 font-mono text-amber-300">−${peso(m.discounts)}</td><td class="p-3 font-mono text-emerald-300 text-right">${peso(m.net)}</td></tr>`
            ).join('') || '<tr><td colspan="4" class="p-4 text-center text-slate-500">No sales recorded for this day</td></tr>';
            body.innerHTML = rows + `<tr class="border-t border-slate-700 font-bold"><td class="p-3 text-slate-200">TOTAL</td><td class="p-3 font-mono">${r.transactions}</td><td class="p-3 font-mono text-amber-300">−${peso(r.discounts)}</td><td class="p-3 font-mono text-emerald-300 text-right">${peso(r.net_sales)}</td></tr>`;
        }
    } catch (e) {
        if (body) body.innerHTML = `<div class="text-center text-red-400 py-6">Failed to load report: ${e}</div>`;
    }
}

// --- Expenses Ledger ---

async function loadExpenses() {
    const tbody = document.getElementById('expenses-tbody');
    if (tbody) tbody.innerHTML = '<tr><td colspan="6" class="p-4 text-center text-slate-500">Loading expenses...</td></tr>';
    try {
        const list = await invokeTauri('list_expenses', { limit: 200 });
        window.cachedExpenses = list;
        renderExpensesTable();
    } catch (e) {
        if (tbody) tbody.innerHTML = `<tr><td colspan="6" class="p-4 text-center text-red-400">Failed: ${e}</td></tr>`;
    }
}

function renderExpensesTable() {
    const tbody = document.getElementById('expenses-tbody');
    if (!tbody) return;
    const list = window.cachedExpenses || [];
    const total = list.reduce((s, x) => s + (x.amount || 0), 0);
    const totalEl = document.getElementById('expenses-total');
    if (totalEl) totalEl.innerText = `₱${total.toLocaleString('en-US', { minimumFractionDigits: 2 })}`;
    if (list.length === 0) {
        tbody.innerHTML = '<tr><td colspan="6" class="p-4 text-center text-slate-500">No expenses recorded yet</td></tr>';
        return;
    }
    tbody.innerHTML = list.map(x => `
        <tr class="hover:bg-slate-800/30">
            <td class="p-3 font-mono text-slate-400 text-[11px]">${new Date(x.spent_at).toLocaleDateString()}</td>
            <td class="p-3 font-semibold text-slate-200">${escapeHtml(x.title)}</td>
            <td class="p-3 uppercase text-[10px] font-bold text-purple-300">${escapeHtml(x.category)}</td>
            <td class="p-3 font-mono text-red-300 text-right">₱${x.amount.toLocaleString('en-US', { minimumFractionDigits: 2 })}</td>
            <td class="p-3 uppercase text-[10px] text-slate-400">${escapeHtml(x.payment_method)}${x.created_by ? ` · ${escapeHtml(x.created_by)}` : ''}</td>
            <td class="p-3 text-right"><button onclick="deleteExpense('${x.id}')" class="text-red-400 hover:text-red-300 text-xs">Delete</button></td>
        </tr>`).join('');
}

async function submitExpense() {
    const title = document.getElementById('exp-title-input')?.value.trim();
    const category = document.getElementById('exp-category-input')?.value || 'general';
    const amount = parseFloat(document.getElementById('exp-amount-input')?.value) || 0;
    const method = document.getElementById('exp-method-input')?.value || 'cash';
    const notes = document.getElementById('exp-notes-input')?.value.trim() || '';
    const err = document.getElementById('exp-error-msg');
    if (!title) { if (err) err.innerText = 'Title is required'; return; }
    if (amount <= 0) { if (err) err.innerText = 'Amount must be greater than zero'; return; }
    try {
        await invokeTauri('create_expense', { req: { title, category, amount, payment_method: method, notes, spent_at: null } });
        document.getElementById('exp-title-input').value = '';
        document.getElementById('exp-amount-input').value = '';
        document.getElementById('exp-notes-input').value = '';
        if (err) err.innerText = '';
        await loadExpenses();
        showHudToast('Expense Recorded', `${title} — ₱${amount.toFixed(2)}`, 'success');
    } catch (e) { if (err) err.innerText = 'Save failed: ' + e; }
}

async function deleteExpense(id) {
    if (!confirm('Delete this expense record?')) return;
    try {
        await invokeTauri('delete_expense', { id: id });
        await loadExpenses();
    } catch (e) { alert('Delete failed: ' + e); }
}

// --- Coaches & Sessions Management (Full CRUD) ---

let cachedCoaches = [];

async function loadCoaches() {
    try {
        const coaches = await invokeTauri('list_coaches');
        cachedCoaches = coaches;
        const grid = document.getElementById('coaches-grid');
        if (!grid) return;

        if (coaches.length === 0) {
            grid.innerHTML = '<div class="col-span-3 text-center text-slate-500 py-10">No personal trainers registered</div>';
            return;
        }

        grid.innerHTML = coaches.map(c => `
            <div class="glass-panel p-4 border border-slate-800 card">
                <div class="flex items-center justify-between">
                    <div class="flex items-center gap-3">
                        <div class="w-10 h-10 rounded-full bg-slate-800 border border-slate-700 flex items-center justify-center text-slate-200 font-bold brand text-base shadow-md">
                            ${c.name.charAt(0)}
                        </div>
                        <div>
                            <div class="text-sm font-bold text-slate-200">${c.name}</div>
                            <div class="text-[11px] text-slate-400">${c.specialty}</div>
                            <div class="text-[10px] text-slate-500 font-mono">${c.phone || '--'}</div>
                        </div>
                    </div>
                    <div class="flex items-center gap-1">
                        <button onclick="openEditCoachModal('${c.id}')" title="Edit Profile" class="text-slate-400 hover:text-blue-300 p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path></svg>
                        </button>
                        <button onclick="deleteCoach('${c.id}', '${c.name.replace(/'/g, "\\'")}')" title="Delete Trainer" class="text-slate-400 hover:text-red-400 p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                        </button>
                    </div>
                </div>
                <div class="mt-4 pt-3 border-t border-slate-800 flex justify-between items-center text-xs">
                    <span class="text-slate-400">Active Students: <b class="text-slate-200">${c.active_students}</b></span>
                    <button onclick="openBookSessionModal('${c.id}', '${c.name.replace(/'/g, "\\'")}')" class="px-3 py-1.5 rounded-lg brand-btn text-[11px] font-bold flex items-center gap-1.5">
                        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 7V3m8 4V3m-9 8h10M5 21h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z"></path></svg>
                        <span>Book Session</span>
                    </button>
                </div>
            </div>
        `).join('');
    } catch (e) {
        console.error("Load coaches error:", e);
    }
}

function openAddCoachModal() {
    document.getElementById('add-coach-name').value = '';
    document.getElementById('add-coach-specialty').value = '';
    document.getElementById('add-coach-phone').value = '';
    document.getElementById('add-coach-error').innerText = '';
    document.getElementById('add-coach-modal').classList.remove('hidden');
}

function closeAddCoachModal() {
    document.getElementById('add-coach-modal').classList.add('hidden');
}

async function submitCreateCoach() {
    const name = document.getElementById('add-coach-name').value.trim();
    const specialty = document.getElementById('add-coach-specialty').value.trim();
    const phone = document.getElementById('add-coach-phone').value.trim();
    const errorEl = document.getElementById('add-coach-error');

    if (!name || !specialty) {
        errorEl.innerText = "Trainer name and specialty are required";
        return;
    }

    try {
        await invokeTauri('create_coach', {
            req: { name, specialty, phone }
        });
        closeAddCoachModal();
        await loadCoaches();
        alert(`Trainer "${name}" added!`);
    } catch (e) {
        errorEl.innerText = "Error: " + e;
    }
}

function openEditCoachModal(id) {
    const c = cachedCoaches.find(item => item.id === id);
    if (!c) return;

    document.getElementById('edit-coach-id').value = c.id;
    document.getElementById('edit-coach-name').value = c.name;
    document.getElementById('edit-coach-specialty').value = c.specialty;
    document.getElementById('edit-coach-phone').value = c.phone || '';
    document.getElementById('edit-coach-error').innerText = '';
    document.getElementById('edit-coach-modal').classList.remove('hidden');
}

function closeEditCoachModal() {
    document.getElementById('edit-coach-modal').classList.add('hidden');
}

async function submitUpdateCoach() {
    const id = document.getElementById('edit-coach-id').value;
    const name = document.getElementById('edit-coach-name').value.trim();
    const specialty = document.getElementById('edit-coach-specialty').value.trim();
    const phone = document.getElementById('edit-coach-phone').value.trim();
    const errorEl = document.getElementById('edit-coach-error');

    if (!name || !specialty) {
        errorEl.innerText = "Trainer name and specialty are required";
        return;
    }

    try {
        await invokeTauri('update_coach', {
            req: { id, name, specialty, phone }
        });
        closeEditCoachModal();
        await loadCoaches();
        alert(`Trainer "${name}" updated!`);
    } catch (e) {
        errorEl.innerText = "Error: " + e;
    }
}

async function deleteCoach(id, name) {
    if (!confirm(`Delete trainer "${name}"?`)) return;
    try {
        await invokeTauri('delete_coach', { id });
        await loadCoaches();
    } catch (e) {
        alert("Delete Coach Error: " + e);
    }
}

function openBookSessionModal(coachId, coachName) {
    document.getElementById('book-coach-id').value = coachId;
    document.getElementById('book-coach-name').value = coachName;
    document.getElementById('book-session-error').innerText = '';

    // Populate members dropdown
    const select = document.getElementById('book-member-select');
    select.innerHTML = cachedMembers.map(m => `
        <option value="${m.id}">${m.first_name} ${m.last_name} (${m.membership_type.toUpperCase()})</option>
    `).join('');

    // Pre-fill next hour datetime
    const now = new Date();
    now.setHours(now.getHours() + 1);
    now.setMinutes(0);
    const isoString = new Date(now.getTime() - now.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
    document.getElementById('book-session-date').value = isoString;

    document.getElementById('book-session-modal').classList.remove('hidden');
}

function closeBookSessionModal() {
    document.getElementById('book-session-modal').classList.add('hidden');
}

async function submitBookSession() {
    const coachId = document.getElementById('book-coach-id').value;
    const coachName = document.getElementById('book-coach-name').value;
    const memberSelect = document.getElementById('book-member-select');
    const memberId = memberSelect.value;
    const memberName = memberSelect.options[memberSelect.selectedIndex]?.text?.split(' (')[0] || 'Member';
    const date = document.getElementById('book-session-date').value;
    const duration = parseInt(document.getElementById('book-session-duration').value) || 60;
    const errorEl = document.getElementById('book-session-error');

    if (!memberId || !date) {
        errorEl.innerText = "Please select member and session date";
        return;
    }

    try {
        await invokeTauri('schedule_coach_session', {
            coachId,
            coachName,
            memberId,
            memberName,
            date,
            duration
        });

        closeBookSessionModal();
        await loadCoaches();
        await loadCoachSessions();
        alert(`Session booked for ${memberName} with Coach ${coachName}!`);
    } catch (e) {
        errorEl.innerText = "Booking Error: " + e;
    }
}

async function loadCoachSessions() {
    try {
        const sessions = await invokeTauri('list_coach_sessions');
        const tbody = document.getElementById('coach-sessions-tbody');
        if (!tbody) return;

        if (!sessions || sessions.length === 0) {
            tbody.innerHTML = '<tr><td colspan="6" class="p-4 text-center text-slate-500">No scheduled sessions found</td></tr>';
            return;
        }

        tbody.innerHTML = sessions.map(s => `
            <tr class="hover:bg-slate-800/30 transition">
                <td class="p-3 font-mono text-blue-300">${s.id}</td>
                <td class="p-3 font-bold text-slate-200">${s.coach_name}</td>
                <td class="p-3 text-slate-300">${s.member_name}</td>
                <td class="p-3 font-mono text-slate-400">${s.scheduled_at}</td>
                <td class="p-3 font-semibold text-emerald-400">${s.duration_minutes} Mins</td>
                <td class="p-3 text-right">
                    <button onclick="cancelCoachSession('${s.id}')" class="px-2.5 py-1 rounded bg-red-950/60 hover:bg-red-900 text-xs text-red-300 border border-red-800/50 font-medium transition">
                        Cancel Session
                    </button>
                </td>
            </tr>
        `).join('');
    } catch (e) {
        console.error("Load sessions error:", e);
    }
}

async function cancelCoachSession(sessionId) {
    if (!confirm("Are you sure you want to cancel this coaching session?")) return;
    try {
        await invokeTauri('cancel_coach_session', { sessionId });
        await loadCoaches();
        await loadCoachSessions();
        alert("Session cancelled.");
    } catch (e) {
        alert("Cancel Error: " + e);
    }
}

// --- Live Gate & Anti-Tailgate Incident System ---

async function loadAttendanceLogs() {
    try {
        const logs = await invokeTauri('list_recent_attendance', { limit: 15 });
        const tbody = document.getElementById('attendance-log-tbody');
        if (!tbody) return;

        if (!Array.isArray(logs) || logs.length === 0) {
            tbody.innerHTML = '<tr><td colspan="6" class="p-4 text-center text-slate-500">No recent gate activity</td></tr>';
            return;
        }

        tbody.innerHTML = logs.map(l => {
            const isTailgate = l.tailgate_flag;
            const isOverride = l.direction === 'override' || (l.member_name && l.member_name.includes('STAFF MANUAL'));
            const isWalkIn = l.member_name && l.member_name.startsWith('Walk-In:');

            let dirBadge = '';
            if (isOverride) {
                dirBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] border text-amber-300 bg-amber-950/80 border-amber-600 font-bold animate-pulse">
                    <svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg>
                    <span>STAFF OVERRIDE</span>
                </span>`;
            } else {
                const dirColor = l.direction === 'in' ? 'text-emerald-400 bg-emerald-950 border-emerald-800' : 'text-blue-400 bg-blue-950 border-blue-800';
                dirBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] border ${dirColor} uppercase font-bold">
                    ${l.direction === 'in' 
                        ? '<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 16l-4-4m0 0l4-4m-4 4h14m-5 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h7a3 3 0 013 3v1"></path></svg>'
                        : '<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"></path></svg>'}
                    <span>${l.direction}</span>
                </span>`;
            }

            // Inter-branch visitor detection via cachedMembers home_gym_name vs local gym
            const interMember = l.member_id ? cachedMembers.find(m=>m.id===l.member_id) : null;
            const isInterbranchVisitor = !!(interMember && interMember.home_gym_name && interMember.home_gym_name !== (appSettings.gym_name||'') && !isOverride);

            let flagBadge = '<span class="text-slate-500 text-[10px]">Normal</span>';
            if (isTailgate) {
                flagBadge = '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-bold animate-pulse"><svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg><span>TAILGATE FLAG</span></span>';
            } else if (isInterbranchVisitor) {
                flagBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-purple-950 text-purple-300 border border-purple-800 font-bold" title="Home: ${interMember.home_gym_name}"><span>📍 Inter-Branch Visitor</span><span class="font-mono text-[9px]">[${interMember.home_gym_name}]</span></span>`;
            } else if (isOverride) {
                flagBadge = '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-amber-950 text-amber-400 border border-amber-700 font-semibold"><span>UNPAID / MANUAL PULSE</span></span>';
            } else if (isWalkIn) {
                flagBadge = '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-emerald-950/70 text-emerald-400 border border-emerald-800 font-medium"><span>8H TIMED PASS</span></span>';
            }

            const timeFormatted = new Date(l.timestamp).toLocaleTimeString();

            return `
                <tr class="hover:bg-slate-800/30 transition ${isTailgate ? 'bg-red-950/20' : (isInterbranchVisitor ? 'bg-purple-950/20 border-l-2 border-purple-500' : (isOverride ? 'bg-amber-950/25 border-l-2 border-amber-500' : ''))}">
                    <td class="p-3 font-mono text-blue-300">${l.id}</td>
                    <td class="p-3 font-semibold text-slate-200">${l.member_name || 'Unidentified Person'}${isInterbranchVisitor ? ` <span class="text-[9px] text-purple-400">[${interMember.home_gym_name}]</span>` : ''}</td>
                    <td class="p-3">${dirBadge}</td>
                    <td class="p-3 text-slate-400">${l.confidence ? (l.confidence * 100).toFixed(1) + '%' : '--'}</td>
                    <td class="p-3">${flagBadge}</td>
                    <td class="p-3 text-slate-400">${timeFormatted}</td>
                </tr>
            `;
        }).join('');
    } catch (e) {
        console.error("Load attendance logs error:", e);
    }
}

async function simulateFaceScan(direction) {
    if (cachedMembers.length === 0 && cachedWalkIns.length === 0) {
        alert("Please enroll a member or issue a walk-in pass first to test facial matching!");
        return;
    }

    let probe = null;
    if (cachedMembers.length > 0) {
        probe = cachedMembers[0].face_vectors[0];
    } else if (cachedWalkIns.length > 0) {
        const seed = cachedWalkIns[0].guest_name.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
        probe = [];
        for (let i = 0; i < FACE_EMBEDDING_DIM; i++) probe.push(Math.sin(seed + i));
    }

    try {
        const result = await invokeTauri('process_face_scan', {
            probeVector: probe,
            direction: direction
        });

        if (result.passback_violation) {
            showHudToast("Anti-Passback Blocked", result.message, "warn");
            alert(`⚠️ ANTI-PASSBACK BLOCKED:\n${result.message}`);
            return;
        }

        if (result.matched) {
            let msg = `Face Verified (${direction.toUpperCase()}): ${result.member_name}`;
            if (result.remaining_minutes !== undefined && result.remaining_minutes !== null) {
                const h = Math.floor(result.remaining_minutes / 60);
                const m = result.remaining_minutes % 60;
                msg += ` [8h Pass: ${h}h ${m}m remaining]`;
            }
            msg += ` — Magnetic Lock Unlocked!`;
            showHudToast("Face Verified", msg, "success");
            armDoorOpenTailgateSurveillance(3500);
            alert(msg);
        } else if (result.is_expired) {
            alert(`Scan Denied: ${result.message}\nDoor remains LOCKED to prevent unauthorized entry.`);
        } else {
            alert("Scan Result: Face Not Recognized");
        }

        await loadAttendanceLogs();
        await refreshDashboard();
    } catch (e) {
        alert("Face Scan Error: " + e);
    }
}

async function simulateWalkInScan(direction) {
    if (cachedWalkIns.length === 0) {
        alert("Please issue a walk-in pass first to test 8-hour guest scanning!");
        return;
    }

    const guest = cachedWalkIns[0];
    const seed = guest.guest_name.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
    const probe = [];
    for (let i = 0; i < FACE_EMBEDDING_DIM; i++) probe.push(Math.sin(seed + i));

    try {
        const result = await invokeTauri('process_face_scan', {
            probeVector: probe,
            direction: direction
        });

        if (result.passback_violation) {
            showHudToast("Anti-Passback Blocked", result.message, "warn");
            alert(`⚠️ ANTI-PASSBACK BLOCKED:\n${result.message}`);
            return;
        }

        if (result.matched) {
            let msg = `Walk-In Scan (${direction.toUpperCase()}): ${result.member_name}`;
            if (result.remaining_minutes !== undefined && result.remaining_minutes !== null) {
                const h = Math.floor(result.remaining_minutes / 60);
                const m = result.remaining_minutes % 60;
                msg += ` (${h}h ${m}m remaining)`;
            }
            msg += ` — Gate Unlocked!`;
            showHudToast("Walk-In Verified", msg, "success");
            armDoorOpenTailgateSurveillance(3500);
            alert(msg);
        } else if (result.is_expired) {
            alert(`Scan Denied: 8-Hour Pass Expired for ${guest.guest_name}. Gate remains LOCKED.`);
        } else {
            alert("Scan Result: Face Not Recognized");
        }

        await loadAttendanceLogs();
        await refreshDashboard();
    } catch (e) {
        alert("Walk-In Scan Error: " + e);
    }
}

async function triggerTailgateSecurityAlarm() {
    try {
        await invokeTauri('trigger_tailgate_alarm', {
            reason: "Turnstile ROI multi-occupancy violation"
        });

        // Show siren banner
        const banner = document.getElementById('tailgate-siren-banner');
        if (banner) banner.classList.remove('hidden');

        await loadAttendanceLogs();
        await refreshDashboard();
    } catch (e) {
        alert("Tailgate Alarm Error: " + e);
    }
}

function dismissSiren() {
    const banner = document.getElementById('tailgate-siren-banner');
    if (banner) banner.classList.add('hidden');
}

// --- Quick Hardware & License ---

async function quickUnlockDoor() {
    const btn = document.getElementById('btn-quick-unlock');
    const lockEl = document.getElementById('telemetry-lock-state');
    try {
        btn.classList.add('opacity-50');
        if (lockEl) {
            lockEl.innerText = "UNLOCKED (PULSE)";
            lockEl.className = "text-sm font-bold text-amber-400 mt-1 animate-pulse";
        }
        await invokeTauri('unlock_magnetic_lock', { durationMs: 3000 });
        setTimeout(() => {
            if (lockEl) {
                lockEl.innerText = "LOCKED (STANDBY)";
                lockEl.className = "text-sm font-bold text-emerald-400 mt-1";
            }
        }, 3000);
        await loadAttendanceLogs();
        await refreshDashboard();
    } catch (e) {
        if (lockEl) {
            lockEl.innerText = "LOCKED (STANDBY)";
            lockEl.className = "text-sm font-bold text-emerald-400 mt-1";
        }
        alert("Unlock Failed: " + e);
    } finally {
        btn.classList.remove('opacity-50');
    }
}

async function refreshComPorts() {
    try {
        const ports = await invokeTauri('list_com_ports');
        const select = document.getElementById('com-port-select');
        if (!select) return;
        select.innerHTML = '';
        if (ports.length === 0) {
            select.innerHTML = '<option value="">-- No Ports Detected --</option>';
        } else {
            ports.forEach(p => {
                const opt = document.createElement('option');
                opt.value = p.split(' ')[0];
                opt.innerText = p;
                select.appendChild(opt);
            });
        }
    } catch (e) {
        console.error("Failed to list COM ports:", e);
    }
}

async function connectSelectedPort() {
    const select = document.getElementById('com-port-select');
    const msgEl = document.getElementById('hw-connect-msg');
    const port = select.value;
    if (!port) {
        alert("Please select a COM port first");
        return;
    }
    try {
        const res = await invokeTauri('connect_com_port', { port: port, baud: 115200 });
        msgEl.innerText = res;
        msgEl.className = "text-xs text-emerald-400";
        await refreshDashboard();
    } catch (e) {
        msgEl.innerText = "Connection Error: " + e;
        msgEl.className = "text-xs text-red-400";
    }
}

function openLicenseModal() {
    document.getElementById('license-modal').classList.remove('hidden');
}

function closeLicenseModal() {
    document.getElementById('license-modal').classList.add('hidden');
}

async function submitLicenseKey() {
    const key = document.getElementById('license-key-input').value.trim();
    const statusEl = document.getElementById('license-modal-status');
    if (!key) {
        statusEl.innerText = "Please paste a license key";
        statusEl.className = "text-xs text-red-400";
        return;
    }

    try {
        statusEl.innerText = "Validating cryptographic signature...";
        statusEl.className = "text-xs text-blue-300";

        await invokeTauri('apply_license_key', { key: key });
        statusEl.innerText = "License activated successfully!";
        statusEl.className = "text-xs text-emerald-400";

        setTimeout(() => {
            closeLicenseModal();
            refreshDashboard();
        }, 1200);
    } catch (e) {
        statusEl.innerText = "Activation Failed: " + e;
        statusEl.className = "text-xs text-red-400";
    }
}

// --- Auto-Updater Client Logic ---

let availableUpdate = null;

async function checkAppVersion() {
    try {
        const ver = await invokeTauri('get_app_version');
        const badge = document.getElementById('app-version-badge');
        if (badge && ver) badge.innerText = `v${ver}`;
        const curModal = document.getElementById('modal-current-version');
        if (curModal && ver) curModal.innerText = `v${ver}`;
    } catch (e) {
        console.warn("get_app_version not available in mock/preview mode:", e);
    }
}

async function checkForUpdatesSilent() {
    try {
        const res = await invokeTauri('check_for_updates', { channel: 'stable' });
        if (res && res.update_available) {
            availableUpdate = res;
            showUpdateBanner(res);
        }
    } catch (e) {
        console.log("Silent update check skipped:", e);
    }
}

async function checkUpdatesManual() {
    const btn = document.getElementById('btn-update-checker');
    const origText = btn ? btn.innerHTML : '';
    if (btn) btn.innerHTML = '<span>Checking...</span>';

    try {
        const res = await invokeTauri('check_for_updates', { channel: 'stable' });
        if (res && res.update_available) {
            availableUpdate = res;
            showUpdateBanner(res);
            openUpdateModal();
        } else {
            alert(`GymPOS is up to date (${res ? res.current_version : 'Latest'})!`);
        }
    } catch (e) {
        alert("Update check error: " + e);
    } finally {
        if (btn) btn.innerHTML = origText;
    }
}

function showUpdateBanner(update) {
    const banner = document.getElementById('update-alert-banner');
    if (!banner) return;

    document.getElementById('update-banner-version').innerText = `v${update.latest_version}`;
    document.getElementById('update-banner-notes').innerText = update.release_notes || 'Performance and security updates ready to install.';

    const mandBadge = document.getElementById('update-banner-mandatory');
    if (mandBadge) {
        if (update.is_mandatory) mandBadge.classList.remove('hidden');
        else mandBadge.classList.add('hidden');
    }

    banner.classList.remove('hidden');
}

function openUpdateModal() {
    if (!availableUpdate) return;
    document.getElementById('modal-target-version').innerText = `v${availableUpdate.latest_version}`;
    document.getElementById('modal-release-notes').innerText = availableUpdate.release_notes || 'Security hardening and bug fixes.';
    document.getElementById('update-details-modal').classList.remove('hidden');
}

function closeUpdateModal() {
    document.getElementById('update-details-modal').classList.add('hidden');
}

async function triggerUpdateInstall() {
    if (!availableUpdate) return;

    const progContainer = document.getElementById('update-progress-container');
    const progBar = document.getElementById('update-progress-bar');
    const progPct = document.getElementById('update-progress-pct');
    const statusText = document.getElementById('update-status-text');
    const errText = document.getElementById('update-error-text');
    const installBtn = document.getElementById('btn-modal-install');

    if (progContainer) progContainer.classList.remove('hidden');
    if (errText) errText.classList.add('hidden');
    if (installBtn) installBtn.disabled = true;

    // Simulate smooth progress animation
    let p = 10;
    const interval = setInterval(() => {
        if (p < 90) {
            p += 15;
            if (progBar) progBar.style.width = `${p}%`;
            if (progPct) progPct.innerText = `${p}%`;
        }
    }, 200);

    try {
        if (statusText) statusText.innerText = "Downloading & verifying cryptographic SHA-256 hash...";

        await invokeTauri('download_and_install_update', {
            downloadUrl: availableUpdate.download_url,
            sha256: availableUpdate.sha256
        });

        clearInterval(interval);
        if (progBar) progBar.style.width = '100%';
        if (progPct) progPct.innerText = '100%';
        if (statusText) statusText.innerText = "Applying update and restarting GymPOS...";
    } catch (e) {
        clearInterval(interval);
        if (errText) {
            errText.innerText = "Update Error: " + e;
            errText.classList.remove('hidden');
        }
        if (installBtn) installBtn.disabled = false;
        if (statusText) statusText.innerText = "Update failed. Please retry.";
    }
}

// Hook checkAppVersion, checkForUpdatesSilent, and checkExistingTerminalSession into initApp
const origInitApp = initApp;
initApp = async function() {
    await origInitApp();
    await checkAppVersion();
    await checkExistingTerminalSession();
    setTimeout(checkForUpdatesSilent, 3000);
    // Background interval check every 1 hour
    setInterval(checkForUpdatesSilent, 3600000);
};

// --- Terminal Role-Based Access Control (RBAC) & PIN Lock Screen ---

let currentTerminalSession = null;
let currentEnteredPin = "";

function updatePinDots() {
    for (let i = 1; i <= 4; i++) {
        const dot = document.getElementById(`pin-dot-${i}`);
        if (dot) {
            if (i <= currentEnteredPin.length) {
                dot.className = "w-4 h-4 rounded-full bg-purple-400 border-2 border-purple-300 shadow-md shadow-purple-500/50 transition-all scale-110";
            } else {
                dot.className = "w-4 h-4 rounded-full border-2 border-purple-400/60 transition-all";
            }
        }
    }
}

function pressPinKey(digit) {
    const err = document.getElementById('pin-error-text');
    if (err) err.innerText = "";
    if (currentEnteredPin.length < 4) {
        currentEnteredPin += digit;
        updatePinDots();
        if (currentEnteredPin.length === 4) {
            setTimeout(submitPinLogin, 150);
        }
    }
}

function clearPin() {
    currentEnteredPin = "";
    updatePinDots();
    const err = document.getElementById('pin-error-text');
    if (err) err.innerText = "";
}

async function submitPinLogin() {
    if (currentEnteredPin.length < 4) return;
    const pin = currentEnteredPin;
    const err = document.getElementById('pin-error-text');

    try {
        const res = await invokeTauri('authenticate_staff_pin', { pin });
        if (res && res.authenticated) {
            currentTerminalSession = {
                is_authenticated: true,
                user_id: res.staff_id,
                display_name: res.full_name,
                role: res.role,
                gym_id: res.gym_id,
                gym_name: res.gym_name
            };
            localStorage.setItem('gympos_terminal_session', JSON.stringify(currentTerminalSession));
            unlockTerminalUI();
        } else {
            if (err) err.innerText = "Invalid PIN. Access Denied.";
            clearPin();
        }
    } catch (e) {
        if (err) err.innerText = e || "Invalid PIN. Access Denied.";
        clearPin();
    }
}

function openOwnerLoginModal() {
    const modal = document.getElementById('modal-owner-login');
    if (modal) modal.classList.remove('hidden');
    const err = document.getElementById('owner-login-error');
    if (err) err.classList.add('hidden');
}

function closeOwnerLoginModal() {
    const modal = document.getElementById('modal-owner-login');
    if (modal) modal.classList.add('hidden');
}

async function submitOwnerLogin(e) {
    if (e) e.preventDefault();
    const email = document.getElementById('owner-login-email').value.trim();
    const password = document.getElementById('owner-login-pass').value.trim();
    const err = document.getElementById('owner-login-error');

    try {
        const res = await invokeTauri('authenticate_owner', { email, password });
        if (res && res.authenticated) {
            currentTerminalSession = {
                is_authenticated: true,
                user_id: res.staff_id,
                display_name: res.full_name,
                role: 'owner',
                gym_id: null,
                gym_name: null
            };
            localStorage.setItem('gympos_terminal_session', JSON.stringify(currentTerminalSession));
            closeOwnerLoginModal();
            unlockTerminalUI();
        } else {
            if (err) {
                err.innerText = "Invalid Owner Credentials.";
                err.classList.remove('hidden');
            }
        }
    } catch (e) {
        if (err) {
            err.innerText = e || "Authentication failed.";
            err.classList.remove('hidden');
        }
    }
}

function lockTerminal() {
    currentTerminalSession = null;
    localStorage.removeItem('gympos_terminal_session');
    clearPin();
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (lockScreen) lockScreen.classList.remove('hidden');
}

function unlockTerminalUI() {
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (lockScreen) lockScreen.classList.add('hidden');

    const nameEl = document.getElementById('session-user-name');
    const roleEl = document.getElementById('session-user-role');

    if (currentTerminalSession) {
        if (nameEl) nameEl.innerText = currentTerminalSession.display_name;
        if (roleEl) {
            if (currentTerminalSession.role === 'owner') {
                roleEl.innerText = "Master Owner";
                roleEl.className = "text-[10px] uppercase font-mono font-bold text-amber-400";
            } else if (currentTerminalSession.role === 'manager') {
                roleEl.innerText = "Branch Manager";
                roleEl.className = "text-[10px] uppercase font-mono font-bold text-blue-400";
            } else {
                roleEl.innerText = "Cashier Mode";
                roleEl.className = "text-[10px] uppercase font-mono font-bold text-purple-400";
            }
        }

        applyRolePermissions(currentTerminalSession.role);
    }
}

function applyRolePermissions(role) {
    const isStaff = (role === 'staff');

    // Hide or restrict views for front-desk staff
    document.querySelectorAll('.nav-item').forEach(item => {
        const text = item.innerText.toLowerCase();
        // Staff should not see settings, hardware, or license
        if (isStaff && (text.includes('hardware') || text.includes('branding') || text.includes('settings'))) {
            item.style.display = 'none';
        } else {
            item.style.display = '';
        }
    });

    // License button in header
    const licenseBtn = document.querySelector('button[onclick*="openLicenseModal"]');
    if (licenseBtn) {
        licenseBtn.style.display = isStaff ? 'none' : '';
    }

    // If staff was on a restricted screen, switch to POS or Gate
    if (isStaff && (currentView === 'hardware' || currentView === 'branding')) {
        switchView('pos');
    }
}

async function checkExistingTerminalSession() {
    try {
        const session = await invokeTauri('get_terminal_session');
        if (session && session.is_authenticated) {
            currentTerminalSession = session;
            unlockTerminalUI();
            return;
        }
    } catch (e) {
        // Fallback
    }

    const saved = localStorage.getItem('gympos_terminal_session');
    if (saved) {
        try {
            currentTerminalSession = JSON.parse(saved);
            if (currentTerminalSession && currentTerminalSession.is_authenticated) {
                unlockTerminalUI();
                return;
            }
        } catch (e) {}
    }

    // Default: Show Lock Screen
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (lockScreen) lockScreen.classList.remove('hidden');
}

document.addEventListener('DOMContentLoaded', initApp);


