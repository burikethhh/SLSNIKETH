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
        } else if (command === 'list_tailgate_incidents') {
            const all = window.cachedAttendanceLogs || [];
            const incidents = all.filter(l => l.tailgate_flag);
            return { incidents, unacked: incidents.length };
        } else if (command === 'resolve_tailgate_incident') {
            return { resolved: true };
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
            return { status: "ALARM_TRIGGERED", reason: "Turnstile ROI multi-occupancy violation", siren_suppressed: false, policy_enabled: true };
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
        } else if (command === 'poll_hardware_buttons') {
            // Preview: press Ctrl+Shift+1 for ENTRY, Ctrl+Shift+2 for EXIT to inject a fake EVT
            return [];
        } else if (command === 'get_license_key_diagnostics') {
            return { cloud_url: 'preview', embedded_fingerprint: 'preview', cloud_fingerprint: 'preview', match: true };
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

// --- Multi-Camera Stream Controller & Occupancy Prober ---
let streamCam1 = null;
let streamCam2 = null;
let streamCam3 = null;
let streamCam1DeviceId = null;
let streamCam2DeviceId = null;
let streamCam3DeviceId = null;
let probedDevicesCache = [];

/**
 * Safely requests a video stream for a given deviceId.
 * Uses bandwidth-safe 640x480 constraints so that 3 simultaneous USB webcams
 * do not saturate the Windows USB 2.0/3.0 root hub isochronous transfer bandwidth.
 */
async function getStreamForDevice(deviceId) {
    if (!navigator.mediaDevices || typeof navigator.mediaDevices.getUserMedia !== 'function') {
        return {
            stream: null,
            error: { isOccupied: false, name: 'Unsupported', message: 'getUserMedia is not supported on this platform' }
        };
    }

    // Windows UVC bandwidth optimization: 640x480 uncompressed YUY2 is ~147Mbps.
    // Three 640x480 cameras can stream simultaneously on a single USB root hub
    // without triggering NotReadableError (USB isochronous bandwidth starvation).
    const attempts = [
        deviceId
            ? { deviceId: { exact: deviceId }, width: { ideal: 640, max: 1280 }, height: { ideal: 480, max: 720 }, frameRate: { ideal: 30, max: 30 } }
            : { width: { ideal: 640, max: 1280 }, height: { ideal: 480, max: 720 }, frameRate: { ideal: 30, max: 30 }, facingMode: "user" },
        // Fallback for bandwidth-constrained hubs (360p / 24fps)
        deviceId
            ? { deviceId: { exact: deviceId }, width: { ideal: 640 }, height: { ideal: 360 }, frameRate: { ideal: 24 } }
            : { width: { ideal: 640 }, height: { ideal: 360 }, frameRate: { ideal: 24 } },
        // Minimum viable fallback
        deviceId
            ? { deviceId: { exact: deviceId } }
            : { video: true }
    ];

    let lastErr = null;
    for (const constraints of attempts) {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({ video: constraints, audio: false });
            return { stream, error: null };
        } catch (err) {
            lastErr = err;
            console.warn("getStreamForDevice constraint attempt failed:", constraints, err);
            if (err.name === 'OverconstrainedError') continue;
            if ((err.name === 'NotReadableError' || err.name === 'TrackStartError') && constraints === attempts[0]) {
                await new Promise(r => setTimeout(r, 120));
                continue;
            }
            break;
        }
    }

    const isOccupied = lastErr && (
        lastErr.name === 'NotReadableError'
        || lastErr.name === 'TrackStartError'
        || lastErr.name === 'AbortError'
        || (lastErr.message && /in use|busy|occupied|could not start|concurrent|exclusive/i.test(lastErr.message))
    );

    // Saved-device gone (unplugged / USB renumber / different PC): every
    // exact-deviceId attempt fails with NotFound/OverconstrainedError and the
    // slot would stay dead forever behind a "Camera access error" toast.
    // Fall back to ANY available camera so the terminal keeps scanning, and
    // flag `recovered` so the caller tells the operator to re-assign.
    const deviceMissing = !!deviceId && lastErr && (
        lastErr.name === 'NotFoundError'
        || lastErr.name === 'OverconstrainedError'
        || (lastErr.message && /not found|could not find|device.*(gone|removed|unplug)|overconstrain/i.test(lastErr.message))
    );
    if (deviceMissing) {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({
                video: { width: { ideal: 640, max: 1280 }, height: { ideal: 480, max: 720 } },
                audio: false,
            });
            const track = stream.getVideoTracks()[0];
            const settings = (track && track.getSettings && track.getSettings()) || {};
            return {
                stream,
                error: null,
                recovered: true,
                recoveredLabel: (track && track.label) || 'default camera',
                recoveredDeviceId: settings.deviceId || '',
            };
        } catch (e2) {
            console.warn("Device-less fallback also failed:", e2);
        }
    }

    const userMsg = isOccupied
        ? "Camera is currently locked by another application or USB bus bandwidth is exceeded. Please close conflicting video apps (Zoom/OBS/Teams)."
        : `Camera access error (${lastErr?.name || 'Error'}): ${lastErr?.message || lastErr}`;

    return {
        stream: null,
        error: { isOccupied, name: lastErr?.name || 'Error', message: userMsg, raw: lastErr }
    };
}

function stopStream(stream) {
    if (stream && typeof stream.getTracks === 'function') {
        stream.getTracks().forEach(t => {
            try { t.stop(); } catch (e) {}
        });
    }
}

/**
 * Universal Camera Viewport Synchronizer
 * Binds active video streams and hides standby overlays across Dashboard, Kiosk, and Hardware previews.
 */
function syncAllCameraViewports() {
    const bindViewport = (videoEl, standbyEl, stream) => {
        if (!videoEl) return;
        if (stream && stream.active) {
            if (videoEl.srcObject !== stream) {
                videoEl.srcObject = stream;
                if (videoEl.dataset) delete videoEl.dataset.framesSeen;
            }
            videoEl.play().catch(e => console.debug("Autoplay handled:", e));
            // Standby hides ONLY on proven frames. A MediaStream reports
            // active=true while delivering zero frames (starved USB, ended
            // track, un-fan-out-able mirror) — hiding standby on active alone
            // produced the reported black-void dashboard. The watchdog below
            // re-shows standby for stalls after first frames.
            if (videoEl.videoWidth > 0) {
                if (videoEl.dataset) videoEl.dataset.framesSeen = '1';
                if (standbyEl) standbyEl.classList.add('hidden');
            } else if (!(videoEl.dataset && videoEl.dataset.framesSeen)) {
                if (standbyEl) standbyEl.classList.remove('hidden');
            }
        } else {
            videoEl.srcObject = null;
            if (standbyEl) standbyEl.classList.remove('hidden');
        }
    };

    // Camera 1 (Entry Face Terminal)
    bindViewport(document.getElementById('worker-cam1-entry'), null, streamCam1);
    bindViewport(document.getElementById('dash-cam1-entry'), document.getElementById('dash-cam1-standby'), streamCam1);
    bindViewport(document.getElementById('kiosk-cam1-entry'), document.getElementById('kiosk-cam1-standby'), streamCam1);
    bindViewport(document.getElementById('test-preview-cam1'), null, streamCam1);
    bindViewport(document.getElementById('reg-studio-video'), null, streamCam1);

    // Camera 2 (Exit Face Terminal)
    bindViewport(document.getElementById('worker-cam2-exit'), null, streamCam2);
    bindViewport(document.getElementById('dash-cam2-exit'), document.getElementById('dash-cam2-standby'), streamCam2);
    bindViewport(document.getElementById('kiosk-cam2-exit'), document.getElementById('kiosk-cam2-standby'), streamCam2);
    bindViewport(document.getElementById('test-preview-cam2'), null, streamCam2);

    // Camera 3 (Anti-Tailgate Overhead Radar)
    bindViewport(document.getElementById('worker-cam3-tailgate'), null, streamCam3);
    bindViewport(document.getElementById('dash-cam3-tailgate'), document.getElementById('dash-cam3-standby'), streamCam3);
    bindViewport(document.getElementById('kiosk-cam3-tailgate'), document.getElementById('kiosk-cam3-standby'), streamCam3);
    bindViewport(document.getElementById('test-preview-cam3'), null, streamCam3);
    bindViewport(document.getElementById('roi-preview-video'), null, streamCam3);
}

/**
 * Camera signal watchdog: a bound-but-frameless slot shows its standby panel
 * plus a NO SIGNAL pill instead of a black box. Runs every 2.5s; started once
 * from initCameraStreams. Badges are created by JS so no markup changes were
 * needed across the 6 dashboard/kiosk slots.
 */
function setNoSignalBadge(videoId, show) {
    const videoEl = document.getElementById(videoId);
    if (!videoEl || !videoEl.parentElement) return;
    let badge = videoEl.parentElement.querySelector('[data-nosignal-badge]');
    if (!badge) {
        badge = document.createElement('div');
        badge.setAttribute('data-nosignal-badge', '1');
        badge.className = 'absolute top-1 right-1 px-2 py-0.5 rounded text-[9px] font-bold bg-red-950/90 text-red-300 border border-red-700 font-mono pointer-events-none';
        badge.innerText = 'NO SIGNAL';
        videoEl.parentElement.appendChild(badge);
    }
    badge.classList.toggle('hidden', !show);
}

function watchCameraSignals() {
    const slots = [
        ['dash-cam1-entry', 'dash-cam1-standby'],
        ['dash-cam2-exit', 'dash-cam2-standby'],
        ['dash-cam3-tailgate', 'dash-cam3-standby'],
        ['kiosk-cam1-entry', 'kiosk-cam1-standby'],
        ['kiosk-cam2-exit', 'kiosk-cam2-standby'],
        ['kiosk-cam3-tailgate', 'kiosk-cam3-standby'],
    ];
    for (const [vid, sid] of slots) {
        const v = document.getElementById(vid);
        if (!v) continue;
        const s = document.getElementById(sid);
        const hasStream = !!(v.srcObject && v.srcObject.active);
        const hasFrames = hasStream && v.videoWidth > 0 && v.readyState >= 2;
        if (hasFrames) {
            if (v.dataset) v.dataset.framesSeen = '1';
            if (s) s.classList.add('hidden');
            setNoSignalBadge(vid, false);
        } else {
            if (s) s.classList.remove('hidden');
            // Badge only when a stream is attached but barren (as opposed to
            // simply unassigned, where the standby panel alone suffices).
            setNoSignalBadge(vid, hasStream);
        }
    }
}

/**
 * Updates status alert banners and occupied overlay on Camera card 1, 2, or 3.
 */
function setCameraSlotFeedback(camNumber, state, detail = '') {
    const msgEl = document.getElementById(`cam-status-msg-${camNumber}`);
    const overlayEl = document.getElementById(`cam-occupied-overlay-${camNumber}`);
    const badgeEl = document.getElementById(`cam-badge-${camNumber}`);

    if (state === 'bandwidth' || state === 'occupied') {
        if (msgEl) {
            msgEl.className = "text-[11px] p-2.5 rounded-lg leading-snug bg-amber-950/80 border border-amber-600 text-amber-200 block";
            msgEl.innerHTML = `<strong>⚠️ USB Hub Bandwidth Limit / Conflict:</strong> ${detail || 'Windows cannot stream 3 webcams through one shared USB hub. Plug 1 camera directly into another PC USB port (or select the laptop webcam).'}`;
        }
        if (overlayEl) overlayEl.classList.remove('hidden');
        if (badgeEl) {
            badgeEl.innerText = "USB BUS LIMIT";
            badgeEl.className = "text-[9px] font-mono text-amber-400 bg-amber-950/80 px-1.5 py-0.5 rounded border border-amber-700 font-bold";
        }
    } else if (state === 'error') {
        if (msgEl) {
            msgEl.className = "text-[11px] p-2.5 rounded-lg leading-snug bg-red-950/80 border border-red-700 text-red-300 block";
            msgEl.innerHTML = `<strong>🔴 Hardware Error:</strong> ${detail || 'Unable to access video stream.'}`;
        }
        if (overlayEl) overlayEl.classList.add('hidden');
        if (badgeEl) {
            badgeEl.innerText = "ERROR";
            badgeEl.className = "text-[9px] font-mono text-red-400 bg-red-950/80 px-1.5 py-0.5 rounded border border-red-700";
        }
    } else if (state === 'shared') {
        if (msgEl) {
            msgEl.className = "text-[11px] p-2 rounded-lg leading-snug bg-blue-950/50 border border-blue-800 text-blue-300 block";
            msgEl.innerHTML = `<strong>ℹ️ Notice:</strong> ${detail || 'Webcam mirrored with Camera 1 (Shared test mode).'}`;
        }
        if (overlayEl) overlayEl.classList.add('hidden');
        if (badgeEl) {
            badgeEl.innerText = "SHARED (30 FPS)";
            badgeEl.className = "text-[9px] font-mono text-blue-400 bg-blue-950/80 px-1.5 py-0.5 rounded border border-blue-800";
        }
    } else if (state === 'active') {
        if (msgEl) {
            msgEl.className = "text-[11px] p-2 rounded-lg leading-snug bg-emerald-950/40 border border-emerald-800/60 text-emerald-300 block";
            msgEl.innerHTML = `<strong>✓ Ready:</strong> Video stream active and running at 30 FPS.`;
        }
        if (overlayEl) overlayEl.classList.add('hidden');
        if (badgeEl) {
            badgeEl.innerText = "30 FPS";
            badgeEl.className = "text-[9px] font-mono text-emerald-400 bg-emerald-950/80 px-1.5 py-0.5 rounded border border-emerald-800";
        }
    } else {
        if (msgEl) msgEl.classList.add('hidden');
        if (overlayEl) overlayEl.classList.add('hidden');
    }
}

/**
 * Scans all video inputs on the system using passive enumeration (non-destructive).
 * Disambiguates identical webcam labels (e.g. 3 x Web Camera 4a54:5232) with port indices.
 */
async function scanActiveCameras(notify = false) {
    const summaryEl = document.getElementById('cam-health-summary');
    const detailEl = document.getElementById('cam-health-detail');
    const dotEl = document.getElementById('cam-health-dot');

    if (summaryEl) summaryEl.innerText = "Scanning connected video devices...";
    if (dotEl) dotEl.className = "w-2 h-2 rounded-full bg-amber-400 animate-pulse";

    if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) {
        if (summaryEl) summaryEl.innerText = "MediaDevices API unavailable";
        if (dotEl) dotEl.className = "w-2 h-2 rounded-full bg-red-500";
        return;
    }

    try {
        const devices = await navigator.mediaDevices.enumerateDevices();
        const videoDevices = devices.filter(d => d.kind === 'videoinput');

        let readyCount = 0;
        let activeCount = 0;

        probedDevicesCache = [];

        // Active device IDs currently streaming in GymPOS
        const activeGymPosDeviceIds = [streamCam1DeviceId, streamCam2DeviceId, streamCam3DeviceId].filter(Boolean);

        // Count occurrences of labels to disambiguate identical camera models
        const labelCounts = {};
        videoDevices.forEach(d => {
            const base = d.label || 'Web Camera';
            labelCounts[base] = (labelCounts[base] || 0) + 1;
        });
        const labelOccurrences = {};

        for (let i = 0; i < videoDevices.length; i++) {
            const dev = videoDevices[i];
            const baseLabel = dev.label || `Web Camera (${dev.deviceId.slice(0, 8)}...)`;
            let displayLabel = baseLabel;
            if (labelCounts[baseLabel] > 1) {
                labelOccurrences[baseLabel] = (labelOccurrences[baseLabel] || 0) + 1;
                displayLabel = `${baseLabel} [Port #${labelOccurrences[baseLabel]}]`;
            }

            let status = 'ready';
            if (activeGymPosDeviceIds.includes(dev.deviceId)) {
                status = 'active_by_us';
                activeCount++;
                readyCount++;
            } else {
                readyCount++;
            }

            probedDevicesCache.push({
                deviceId: dev.deviceId,
                label: displayLabel,
                health: status,
                healthObj: { status, label: displayLabel }
            });
        }

        // Update Health Status Bar
        if (dotEl) {
            dotEl.className = readyCount > 0
                ? "w-2 h-2 rounded-full bg-emerald-400"
                : "w-2 h-2 rounded-full bg-slate-500";
        }
        if (summaryEl) {
            summaryEl.innerText = `Detected ${videoDevices.length} camera(s): ${readyCount} Available${activeCount > 0 ? ` (${activeCount} active in GymPOS)` : ''}`;
        }
        if (detailEl) {
            detailEl.innerText = readyCount >= 3 ? "All webcams detected and ready for routing" : "Cameras detected and ready for routing";
        }

        // Populate dropdowns with descriptive status indicators
        const sel1 = document.getElementById('cam-assign-entry');
        const sel2 = document.getElementById('cam-assign-exit');
        const sel3 = document.getElementById('cam-assign-tailgate');

        const cfg = appSettings.camera_config || {};

        const buildOptions = (selectedId, fallbackIndex) => {
            let html = '<option value="">-- Select Camera Device --</option>';
            probedDevicesCache.forEach((item, idx) => {
                const isSel = (item.deviceId && item.deviceId === selectedId)
                    || (!selectedId && idx === fallbackIndex);
                let prefix = "🟢";
                let tag = "[Ready]";
                if (item.health === 'active_by_us') {
                    prefix = "🟢";
                    tag = "[Streaming - GymPOS]";
                } else if (item.health === 'occupied') {
                    prefix = "⚠️";
                    tag = "[USB LIMIT]";
                }
                html += `<option value="${item.deviceId}" ${isSel ? 'selected' : ''}>${prefix} ${tag} ${item.label}</option>`;
            });
            return html;
        };

        if (sel1) sel1.innerHTML = buildOptions(cfg.camera1_entry_device_id || "", 0);
        if (sel2) sel2.innerHTML = buildOptions(cfg.camera2_exit_device_id || "", 1);
        if (sel3) sel3.innerHTML = buildOptions(cfg.camera3_tailgate_device_id || "", 2);

        if (notify) {
            showHudToast("Cameras Scanned", `Found ${videoDevices.length} camera(s). All devices ready for assignment.`, "success");
        }

    } catch (e) {
        console.error("Error scanning video devices:", e);
        if (summaryEl) summaryEl.innerText = "Error scanning camera hardware: " + e;
        if (dotEl) dotEl.className = "w-2 h-2 rounded-full bg-red-500";
    }
}

async function populateCameraDevices() {
    await scanActiveCameras(false);
}

/**
 * Initializes streams on boot/settings load.
 * Auto-discovers distinct physical webcams for Camera 1, 2, and 3.
 * Includes graceful mirroring fallback if 3 webcams exceed single USB hub bandwidth.
 */
async function initCameraStreams() {
    if (!navigator.mediaDevices || typeof navigator.mediaDevices.getUserMedia !== 'function') return;

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
    const cfg = appSettings.camera_config;

    try {
        // If device IDs are not explicitly configured, discover available webcams
        // and assign distinct physical webcams to slots 1, 2, and 3 if available.
        if (!cfg.camera1_entry_device_id && !cfg.camera2_exit_device_id && !cfg.camera3_tailgate_device_id) {
            try {
                const devs = await navigator.mediaDevices.enumerateDevices();
                const vdevs = devs.filter(d => d.kind === 'videoinput');
                if (vdevs.length >= 1) cfg.camera1_entry_device_id = vdevs[0].deviceId;
                if (vdevs.length >= 2) cfg.camera2_exit_device_id = vdevs[1].deviceId;
                if (vdevs.length >= 3) cfg.camera3_tailgate_device_id = vdevs[2].deviceId;
            } catch (e) {
                console.debug("Auto-assigning default devices failed:", e);
            }
        }

        // 1. Camera 1: Face Scan Entry
        if (!streamCam1 || !streamCam1.active) {
            const res1 = await getStreamForDevice(cfg.camera1_entry_device_id);
            if (res1.stream) {
                streamCam1 = res1.stream;
                streamCam1DeviceId = (res1.recovered && res1.recoveredDeviceId) ? res1.recoveredDeviceId : cfg.camera1_entry_device_id;
                setCameraSlotFeedback(1, 'active');
                if (res1.recovered) {
                    // Slot is alive on a substitute camera — say so loudly so
                    // the saved (now missing) device gets re-assigned instead
                    // of silently scanning the wrong lens.
                    setCameraSlotFeedback(1, 'shared', `Saved Camera 1 not found — using ${res1.recoveredLabel}. Re-assign in Hardware Settings.`);
                    showHudToast("Camera 1 Reassigned", `Saved device missing. Streaming ${res1.recoveredLabel} until you re-assign Camera 1.`, "warn");
                }
            } else if (res1.error) {
                setCameraSlotFeedback(1, 'bandwidth', res1.error.message);
                showHudToast("Camera 1 Error", res1.error.message, "danger");
            }
        }

        // Brief delay between opening multiple USB webcams to allow bus initialization
        await new Promise(r => setTimeout(r, 100));

        // 2. Camera 2: Face Scan Exit
        if (!streamCam2 || !streamCam2.active) {
            if (cfg.camera2_exit_device_id && cfg.camera2_exit_device_id !== cfg.camera1_entry_device_id) {
                const res2 = await getStreamForDevice(cfg.camera2_exit_device_id);
                if (res2.stream) {
                    streamCam2 = res2.stream;
                    streamCam2DeviceId = cfg.camera2_exit_device_id;
                    setCameraSlotFeedback(2, 'active');
                } else if (res2.error) {
                    setCameraSlotFeedback(2, 'bandwidth', "USB bus limit reached on shared hub. Mirroring Camera 1 until one camera is moved to a separate PC USB port.");
                    // Fallback to streamCam1 so dashboard is never black
                    if (streamCam1) {
                        streamCam2 = streamCam1;
                        streamCam2DeviceId = streamCam1DeviceId;
                    }
                }
            } else if (streamCam1) {
                streamCam2 = streamCam1;
                streamCam2DeviceId = streamCam1DeviceId;
                setCameraSlotFeedback(2, 'shared', 'Shared with Camera 1 (1-camera test mode)');
            }
        }

        await new Promise(r => setTimeout(r, 100));

        // 3. Camera 3: Anti-Tailgate Overhead
        if (!streamCam3 || !streamCam3.active) {
            let gotDedicatedCam3 = false;
            if (cfg.camera3_tailgate_device_id && cfg.camera3_tailgate_device_id !== cfg.camera1_entry_device_id && cfg.camera3_tailgate_device_id !== cfg.camera2_exit_device_id) {
                const res3 = await getStreamForDevice(cfg.camera3_tailgate_device_id);
                if (res3.stream) {
                    streamCam3 = res3.stream;
                    streamCam3DeviceId = cfg.camera3_tailgate_device_id;
                    setCameraSlotFeedback(3, 'active');
                    gotDedicatedCam3 = true;
                }
            }

            // If 3rd camera failed due to USB hub bandwidth limit:
            if (!gotDedicatedCam3) {
                // Check if an alternate available camera can be used (e.g. laptop camera)
                let altFound = false;
                try {
                    const devs = await navigator.mediaDevices.enumerateDevices();
                    const vdevs = devs.filter(d => d.kind === 'videoinput');
                    const usedIds = [streamCam1DeviceId, streamCam2DeviceId].filter(Boolean);
                    const freeDev = vdevs.find(d => !usedIds.includes(d.deviceId));
                    if (freeDev) {
                        const altRes = await getStreamForDevice(freeDev.deviceId);
                        if (altRes.stream) {
                            streamCam3 = altRes.stream;
                            streamCam3DeviceId = freeDev.deviceId;
                            setCameraSlotFeedback(3, 'active', `Streaming on ${freeDev.label || 'alternate webcam'}`);
                            altFound = true;
                        }
                    }
                } catch (e) {}

                // Fallback to sharing Camera 1 so the dashboard is NEVER black
                if (!altFound && streamCam1) {
                    streamCam3 = streamCam1;
                    streamCam3DeviceId = streamCam1DeviceId;
                    setCameraSlotFeedback(3, 'bandwidth', "USB Hub Bandwidth Exceeded. Windows cannot stream 3 webcams through 1 hub. Move 1 camera to another PC USB port, or select the laptop camera.");
                    showHudToast("USB Hub Limit Exceeded", "3 webcams cannot share 1 USB hub. Mirroring Camera 1 until one camera is moved to a separate PC USB port.", "warning");
                }
            }
        }

        // Synchronize all video viewports across the entire application immediately
        syncAllCameraViewports();

        // Signal watchdog (once): turns barren-but-bound slots into standby +
        // NO SIGNAL instead of black boxes.
        if (!window.__camSignalWatchdog) {
            window.__camSignalWatchdog = true;
            setInterval(watchCameraSignals, 2500);
        }

        // Apply ROI Calibrated Zone styling across overlays
        applyRoiConfigToOverlays(cfg);
    } catch (err) {
        console.warn("Camera streams initialization error:", err);
    }
}

/**
 * Handles administrator camera dropdown selection change.
 * Immediately updates live previews AND synchronizes to Dashboard and Kiosks.
 */
async function previewSelectedCamera(camNumber, deviceId) {
    const feedbackPrefix = `Camera ${camNumber}`;
    try {
        if (!appSettings.camera_config) {
            appSettings.camera_config = {
                camera1_entry_device_id: "",
                camera2_exit_device_id: "",
                camera3_tailgate_device_id: "",
                roi_x: 20.0, roi_y: 20.0, roi_width: 60.0, roi_height: 60.0, roi_sensitivity: 85.0
            };
        }

        // Release old stream for this camera slot if dedicated
        if (camNumber === 1) {
            if (streamCam1 && streamCam1 !== streamCam2 && streamCam1 !== streamCam3) stopStream(streamCam1);
            streamCam1 = null;
            streamCam1DeviceId = null;
            appSettings.camera_config.camera1_entry_device_id = deviceId;
        } else if (camNumber === 2) {
            if (streamCam2 && streamCam2 !== streamCam1 && streamCam2 !== streamCam3) stopStream(streamCam2);
            streamCam2 = null;
            streamCam2DeviceId = null;
            appSettings.camera_config.camera2_exit_device_id = deviceId;
        } else if (camNumber === 3) {
            if (streamCam3 && streamCam3 !== streamCam1 && streamCam3 !== streamCam2) stopStream(streamCam3);
            streamCam3 = null;
            streamCam3DeviceId = null;
            appSettings.camera_config.camera3_tailgate_device_id = deviceId;
        }
        // Preview selection edits in-memory routing: flag unsaved until
        // Save & Bind Routing persists it.
        markRoutingDirty();

        // Brief delay to allow Windows UVC driver to free USB endpoint lock
        await new Promise(r => setTimeout(r, 100));

        let streamToUse = null;

        // Check if deviceId is already opened by another slot in GymPOS
        if (camNumber !== 1 && deviceId && deviceId === streamCam1DeviceId && streamCam1) {
            streamToUse = streamCam1;
        } else if (camNumber !== 2 && deviceId && deviceId === streamCam2DeviceId && streamCam2) {
            streamToUse = streamCam2;
        } else if (camNumber !== 3 && deviceId && deviceId === streamCam3DeviceId && streamCam3) {
            streamToUse = streamCam3;
        }

        if (!streamToUse) {
            const res = await getStreamForDevice(deviceId);
            if (res.error) {
                setCameraSlotFeedback(camNumber, 'bandwidth', "USB Hub Bandwidth Exceeded. Windows cannot run 3 webcams on the same USB hub. Move this camera directly to a different PC USB port (not the hub), or choose the laptop webcam.");
                showHudToast("USB Hub Limit Exceeded", "Windows cannot run 3 webcams on the same USB hub. Move one camera to another PC USB port, or choose the laptop webcam.", "danger");
                // If it fails, fallback to streamCam1 so dashboard and preview don't go black
                if (streamCam1) {
                    streamToUse = streamCam1;
                    deviceId = streamCam1DeviceId;
                } else {
                    syncAllCameraViewports();
                    return;
                }
            } else {
                streamToUse = res.stream;
            }
        }

        if (camNumber === 1) {
            streamCam1 = streamToUse;
            streamCam1DeviceId = deviceId;
            setCameraSlotFeedback(1, 'active');
        } else if (camNumber === 2) {
            streamCam2 = streamToUse;
            streamCam2DeviceId = deviceId;
            const isShared = (streamToUse === streamCam1);
            setCameraSlotFeedback(2, isShared ? 'shared' : 'active', isShared ? 'Shared with Camera 1' : '');
        } else if (camNumber === 3) {
            streamCam3 = streamToUse;
            streamCam3DeviceId = deviceId;
            const isShared = (streamToUse === streamCam1);
            setCameraSlotFeedback(3, isShared ? 'shared' : 'active', isShared ? 'Shared with Camera 1' : '');
        }

        // Synchronize immediately to Dashboard, Kiosk, and Hardware previews!
        syncAllCameraViewports();
        showHudToast("Camera Assigned", `${feedbackPrefix} bound successfully and streaming.`, "success");

    } catch (e) {
        console.warn(`Failed to preview camera ${camNumber}:`, e);
        setCameraSlotFeedback(camNumber, 'error', String(e));
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
            roi_sensitivity: 85.0,
            match_threshold: 0.62,
            adapt_threshold: 0.80,
            liveness_min_px: 0.5,
            mog_sensitivity: 0.5
        };
    }

    const newId1 = sel1 ? sel1.value : appSettings.camera_config.camera1_entry_device_id;
    const newId2 = sel2 ? sel2.value : appSettings.camera_config.camera2_exit_device_id;
    const newId3 = sel3 ? sel3.value : appSettings.camera_config.camera3_tailgate_device_id;

    appSettings.camera_config.camera1_entry_device_id = newId1;
    appSettings.camera_config.camera2_exit_device_id = newId2;
    appSettings.camera_config.camera3_tailgate_device_id = newId3;

    try {
        await invokeTauri('save_app_settings', { settings: appSettings });
        localStorage.setItem('gympos_branding', JSON.stringify(appSettings));

        // Re-initialize any streams cleanly
        const uniqueStreams = new Set([streamCam1, streamCam2, streamCam3].filter(Boolean));
        uniqueStreams.forEach(s => stopStream(s));
        streamCam1 = null; streamCam2 = null; streamCam3 = null;
        streamCam1DeviceId = null; streamCam2DeviceId = null; streamCam3DeviceId = null;

        await new Promise(r => setTimeout(r, 120));
        await initCameraStreams();

        // Ensure all views are updated
        syncAllCameraViewports();
        clearRoutingDirty();

        showHudToast("Camera Routing Saved", "All 3 camera assignments saved to database and live streams routed to Dashboard & Kiosks.", "success");
    } catch (e) {
        alert("Failed to save camera routing: " + e);
    }
}

// --- Turnstile ROI Zone Calibration ---

let suppressRoutingDirty = false;

function updateRoiPreview() {
    if (!suppressRoutingDirty) markRoutingDirty();
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

    // Also reflect on the test-preview box + dashboard and kiosk overlays
    const testBox = document.getElementById('test-roi-box');
    if (testBox) {
        testBox.style.left = `${x}%`;
        testBox.style.top = `${y}%`;
        testBox.style.width = `${w}%`;
        testBox.style.height = `${h}%`;
    }
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

function tuningCfg() {
    const cfg = (appSettings.camera_config) || {};
    return {
        match_threshold: cfg.match_threshold ?? 0.62,
        adapt_threshold: cfg.adapt_threshold ?? 0.80,
        liveness_min_px: cfg.liveness_min_px ?? 0.5,
        mog_sensitivity: cfg.mog_sensitivity ?? 0.5,
    };
}

function updateTuningTexts() {
    const g = (id) => document.getElementById(id);
    if (g('slider-match-thr') && g('val-match-thr')) g('val-match-thr').innerText = parseFloat(g('slider-match-thr').value).toFixed(2);
    if (g('slider-adapt-thr') && g('val-adapt-thr')) g('val-adapt-thr').innerText = parseFloat(g('slider-adapt-thr').value).toFixed(2);
    if (g('slider-live-px') && g('val-live-px')) g('val-live-px').innerText = parseFloat(g('slider-live-px').value).toFixed(1);
    if (g('slider-mog-sens') && g('val-mog-sens')) g('val-mog-sens').innerText = parseFloat(g('slider-mog-sens').value).toFixed(2);
    markRoutingDirty();
}

function applyTuningToSliders(cfg) {
    const t = {
        match_threshold: cfg.match_threshold ?? 0.62,
        adapt_threshold: cfg.adapt_threshold ?? 0.80,
        liveness_min_px: cfg.liveness_min_px ?? 0.5,
        mog_sensitivity: cfg.mog_sensitivity ?? 0.5,
    };
    const set = (id, v) => { const el = document.getElementById(id); if (el) el.value = v; };
    set('slider-match-thr', t.match_threshold);
    set('slider-adapt-thr', t.adapt_threshold);
    set('slider-live-px', t.liveness_min_px);
    set('slider-mog-sens', t.mog_sensitivity);
    updateTuningTextsSilent();
    return t;
}

function updateTuningTextsSilent() {
    const g = (id) => document.getElementById(id);
    // Same labels as updateTuningTexts but without tripping the dirty flag
    // during initial apply.
    if (g('slider-match-thr') && g('val-match-thr')) g('val-match-thr').innerText = parseFloat(g('slider-match-thr').value).toFixed(2);
    if (g('slider-adapt-thr') && g('val-adapt-thr')) g('val-adapt-thr').innerText = parseFloat(g('slider-adapt-thr').value).toFixed(2);
    if (g('slider-live-px') && g('val-live-px')) g('val-live-px').innerText = parseFloat(g('slider-live-px').value).toFixed(1);
    if (g('slider-mog-sens') && g('val-mog-sens')) g('val-mog-sens').innerText = parseFloat(g('slider-mog-sens').value).toFixed(2);
}

// Dirty-dot: any routing/calibration/tuning edit marks the Hardware view
// unsaved until Save & Bind / Save & Apply is pressed.
function markRoutingDirty() {
    const dot = document.getElementById('routing-dirty-dot');
    if (dot) dot.classList.remove('hidden');
    const btn = document.getElementById('btn-save-routing');
    if (btn) btn.classList.add('ring-2', 'ring-amber-400');
}

function clearRoutingDirty() {
    const dot = document.getElementById('routing-dirty-dot');
    if (dot) dot.classList.add('hidden');
    const btn = document.getElementById('btn-save-routing');
    if (btn) btn.classList.remove('ring-2', 'ring-amber-400');
}

function applyRoiConfigToOverlays(cfg) {
    suppressRoutingDirty = true;
    try {
        applyTuningToSliders(cfg);
    } finally {
        suppressRoutingDirty = false;
    }
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
    const g = (id, fb) => {
        const el = document.getElementById(id);
        const v = el ? parseFloat(el.value) : NaN;
        return Number.isFinite(v) ? v : fb;
    };
    appSettings.camera_config.match_threshold = g('slider-match-thr', 0.62);
    appSettings.camera_config.adapt_threshold = g('slider-adapt-thr', 0.80);
    appSettings.camera_config.liveness_min_px = g('slider-live-px', 0.5);
    appSettings.camera_config.mog_sensitivity = g('slider-mog-sens', 0.5);

    try {
        await invokeTauri('save_app_settings', { settings: appSettings });
        applyRoiConfigToOverlays(appSettings.camera_config);
        clearRoutingDirty();
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
let autoScanBusy = false; // reentrancy guard — prevents overlapping scan ticks
let autoScanCamToggle = false; // false = entry cam (in), true = exit cam (out)

// --- Two-frame confirmation + liveness-lite (Phase A) ---
// The ONNX pipeline is deterministic: a static photo yields bit-identical
// embeddings frame after frame, while a live face always differs slightly
// (sensor noise, micro-motion). So: first match only arms a pending state;
// a second consecutive match for the SAME member confirms liveness when
// either the embedding differs (cosine < 0.999) or the eyes moved (>= 0.5px).
// Two identical frames in a row = static image: require a 3rd frame, deny
// with "liveness failed" if still identical. State is per scan lane
// ('cam1', 'cam2', 'btn-in', 'btn-out') so cameras never interfere.
const liveConfirmState = {}; // key -> {member_id, vector, landmarks, strikes, ts}
function livenessMinPx() {
    const v = parseFloat(tuningCfg().liveness_min_px);
    return Number.isFinite(v) ? v : 0.5;
}

function cosineOf(a, b) {
    if (!a || !b || a.length !== b.length || a.length === 0) return 0;
    let dot = 0, na = 0, nb = 0;
    for (let i = 0; i < a.length; i++) {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if (na <= 1e-12 || nb <= 1e-12) return 0;
    return dot / (Math.sqrt(na) * Math.sqrt(nb));
}

function eyeDisplacement(lm1, lm2) {
    // YuNet landmark order: [right eye, left eye, nose, right mouth, left mouth]
    if (!lm1 || !lm2 || lm1.length < 2 || lm2.length < 2) return Infinity;
    let sum = 0;
    for (let i = 0; i < 2; i++) {
        const dx = (lm1[i]?.x ?? 0) - (lm2[i]?.x ?? 0);
        const dy = (lm1[i]?.y ?? 0) - (lm2[i]?.y ?? 0);
        sum += Math.sqrt(dx * dx + dy * dy);
    }
    return sum / 2;
}

function confirmLiveMatch(key, memberId, vector, landmarks) {
    // A live face detected with YuNet landmarks and matched by ArcFace/SFace confirms immediately.
    // The frontend 12-second memberCooldownMap and backend 3-second atomic cooldown
    // guarantee single-pulse turnstile unlocking and prevent duplicate spam.
    delete liveConfirmState[key];
    return 'confirmed';
}

function clearLiveConfirm(key) {
    delete liveConfirmState[key];
}

// Pre-match liveness (deadlock-free): each lane holds ONE pending detection.
// process_face_scan is called only AFTER two consecutive live frames, so the
// backend 3s atomic cooldown never suppresses the confirmation frame (that
// was the old deadlock: frame 1 armed 'wait', frame 2 arrived at ~650ms and
// Rust returned matched:false). Static photos yield near-identical embeddings
// frame after frame; live faces always differ slightly (sensor noise,
// micro-motion), so identical consecutive frames = spoof.
const livePending = { cam1: null, cam2: null };
const liveSpoofToastAt = { cam1: 0, cam2: 0 };
function checkLivePending(lane, vector, landmarks) {
    const prev = livePending[lane];
    if (!prev) {
        livePending[lane] = { vector, landmarks, ts: Date.now(), strikes: 0 };
        return 'wait';
    }
    if (Date.now() - prev.ts > 2000) { // stale pending — re-arm
        livePending[lane] = { vector, landmarks, ts: Date.now(), strikes: 0 };
        return 'wait';
    }
    const cos = cosineOf(prev.vector, vector);
    const disp = eyeDisplacement(prev.landmarks, landmarks);
    if (cos >= 0.999 && disp < livenessMinPx()) {
        const strikes = (prev.strikes || 0) + 1;
        if (strikes >= 2) {
            livePending[lane] = null;
            return 'spoof';
        }
        livePending[lane] = { vector, landmarks, ts: Date.now(), strikes };
        return 'wait';
    }
    livePending[lane] = null;
    return 'confirmed';
}

// Per-lane consecutive inference-failure counters — surfaces WHICH camera is
// struggling (entry/exit/tailgate) instead of failing silently every 650ms.
const camErr = { cam1: 0, cam2: 0, cam3: 0 };
function noteCamErr(lane, label) {
    camErr[lane] = (camErr[lane] || 0) + 1;
    if (camErr[lane] === 15) {
        showHudToast(`${label} struggling`, "15 consecutive scan failures — check Hardware Settings camera assignment.", "warn");
    }
}
function noteCamOk(lane) { camErr[lane] = 0; }

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

/**
 * Captures a JPEG frame from a live <video> element, downscaled to <=640px
 * wide for ONNX model input. Returns base64 data-URL or null if video not ready.
 */
function captureVideoFrame(videoEl) {
    if (!videoEl || !videoEl.videoWidth || videoEl.videoWidth === 0) return null;
    const scale = Math.min(1, 640 / videoEl.videoWidth);
    const w = Math.round(videoEl.videoWidth * scale);
    const h = Math.round(videoEl.videoHeight * scale);
    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    ctx.drawImage(videoEl, 0, 0, w, h);
    try { return canvas.toDataURL('image/jpeg', 0.85); } catch (e) { return null; }
}

/**
 * Finds the first active (playing, has dimensions) video element from a
 * list of candidate element IDs.
 */
function findActiveVideoElement(candidateIds) {
    for (const id of candidateIds) {
        const el = document.getElementById(id);
        if (el && el.videoWidth > 0 && el.videoHeight > 0) return el;
    }
    return null;
}

/**
 * Resolves the active capture source for a camera slot.
 * Prioritizes the dedicated continuous worker video viewports (which never sleep or freeze),
 * falling back to dashboard/kiosk video elements.
 */
function getCaptureElement(camNumber) {
    const map = {
        1: ['worker-cam1-entry', 'dash-cam1-entry', 'kiosk-cam1-entry', 'test-preview-cam1'],
        2: ['worker-cam2-exit', 'dash-cam2-exit', 'kiosk-cam2-exit', 'test-preview-cam2'],
        3: ['worker-cam3-tailgate', 'dash-cam3-tailgate', 'kiosk-cam3-tailgate', 'test-preview-cam3']
    };
    return findActiveVideoElement(map[camNumber] || []);
}

let autoScanCam1Busy = false;
let autoScanCam2Busy = false;
let autoScanCam3Busy = false;

let activeDoorPassageWindow = false;
let doorOpenFrameCount = 0;
let suspiciousFrames = 0;
let maxTailgateFrames = 21; // 7.5s at 350ms interval
const TAILGATE_WINDOW_MS = 7500;
const TAILGATE_TICK_MS = 350;
const TAILGATE_SUSPICIOUS_NEEDED = 2; // consecutive multi-person ticks to alarm

// --- Tailgate person tracker (B3): stable per-person IDs across ticks ---
// Nearest-center matching on ROI box centers; tracks enter/exit + distinct
// IDs seen inside the ROI during the window. Drawn on the cam3 overlay.
const tailgateTracks = new Map(); // id -> {cx, cy, lastSeen, inRoi, everInRoi}
let tailgateNextId = 1;
const TRACK_MAX_DIST_PX = 90;
const TRACK_STALE_MS = 1200;

function updateTailgateTracks(boxes, frameW, frameH, roi) {
    const now = Date.now();
    const used = new Set();
    let distinctInRoi = 0;
    for (const b of boxes || []) {
        const cx = b.cx ?? (b.x + b.w / 2);
        const cy = b.cy ?? (b.y + b.h / 2);
        let bestId = null, bestDist = TRACK_MAX_DIST_PX;
        for (const [id, t] of tailgateTracks) {
            if (used.has(id)) continue;
            const d = Math.hypot(t.cx - cx, t.cy - cy);
            if (d < bestDist) { bestDist = d; bestId = id; }
        }
        const inRoi = boxInRoi(b, cx, cy, roi, frameW, frameH);
        if (bestId === null) {
            bestId = 'P' + (tailgateNextId++);
            tailgateTracks.set(bestId, { cx, cy, lastSeen: now, inRoi, everInRoi: inRoi });
        } else {
            const t = tailgateTracks.get(bestId);
            t.cx = cx; t.cy = cy; t.lastSeen = now; t.inRoi = inRoi;
            if (inRoi) t.everInRoi = true;
        }
        used.add(bestId);
        if (inRoi) distinctInRoi++;
    }
    // Expire stale tracks (person left the scene).
    for (const [id, t] of tailgateTracks) {
        if (now - t.lastSeen > TRACK_STALE_MS) tailgateTracks.delete(id);
    }
    let everCount = 0;
    for (const t of tailgateTracks.values()) {
        if (t.everInRoi && now - t.lastSeen <= TRACK_STALE_MS) everCount++;
    }
    return { distinctInRoi, everCount, tracks: [...tailgateTracks.entries()].map(([id, t]) => ({ id, ...t })) };
}

function boxInRoi(b, cx, cy, roi, frameW, frameH) {
    // roi in percent (0-100), box coords in original pixels.
    const rx = (roi.x / 100) * frameW, ry = (roi.y / 100) * frameH;
    const rw = (roi.w / 100) * frameW, rh = (roi.h / 100) * frameH;
    const centerIn = cx >= rx && cx <= rx + rw && cy >= ry && cy <= ry + rh;
    const intersects = b.x < rx + rw && b.x + b.w > rx && b.y < ry + rh && b.y + b.h > ry;
    return centerIn || intersects; // locked rule: matches Rust count_and_locate_in_roi
}

function drawTailgateOverlay(tracks, roi, videoEl) {
    // Overlay canvas sits atop the cam3 card (created on demand).
    let canvas = document.getElementById('tailgate-track-overlay');
    const host = videoEl ? videoEl.closest('.glass-panel') || videoEl.parentElement : null;
    if (!canvas && host) {
        canvas = document.createElement('canvas');
        canvas.id = 'tailgate-track-overlay';
        canvas.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;pointer-events:none;z-index:5;';
        const pos = window.getComputedStyle(host).position;
        if (pos === 'static') host.style.position = 'relative';
        host.appendChild(canvas);
    }
    if (!canvas || !videoEl || !videoEl.videoWidth) return;
    const vw = videoEl.videoWidth, vh = videoEl.videoHeight;
    canvas.width = vw; canvas.height = vh;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, vw, vh);
    // ROI box (cyan).
    ctx.strokeStyle = 'rgba(34,211,238,0.9)';
    ctx.lineWidth = Math.max(2, vw / 320);
    ctx.strokeRect((roi.x / 100) * vw, (roi.y / 100) * vh, (roi.w / 100) * vw, (roi.h / 100) * vh);
    // Tracked persons: green = moving, amber = static, red = in ROI.
    for (const t of tracks) {
        const color = t.inRoi ? 'rgba(248,113,113,0.95)' : 'rgba(52,211,153,0.9)';
        ctx.strokeStyle = color;
        ctx.lineWidth = Math.max(2, vw / 320);
        const bx = t.cx - 30, by = t.cy - 40;
        ctx.strokeRect(bx, by, 60, 80);
        ctx.fillStyle = color;
        ctx.font = `bold ${Math.max(12, vw / 53)}px sans-serif`;
        ctx.fillText(t.id + (t.inRoi ? ' ROI' : ''), bx, by - 4);
    }
}

async function testTransitPassage() {
    // Real single-frame count on the overhead feed (replaces the old blind
    // 2s arm): reports persons + motion so routing can be verified live.
    const out = document.getElementById('transit-test-result');
    const say = (t) => { if (out) out.innerText = t; };
    say('Capturing overhead frame…');
    try {
        const video = getCaptureElement(3);
        if (!video) { say('No overhead video feed — check routing.'); return; }
        const frame = captureVideoFrame(video);
        if (!frame) { say('Frame capture failed.'); return; }
        const cfg = appSettings.camera_config || {};
        const res = await invokeTauri('count_persons_in_frame', {
            imageBase64: frame,
            roiX: cfg.roi_x ?? 20, roiY: cfg.roi_y ?? 20,
            roiWidth: cfg.roi_width ?? 60, roiHeight: cfg.roi_height ?? 60,
        });
        const motion = res && res.motion_in_roi !== undefined
            ? `, motion ${((res.motion_in_roi || 0) * 100).toFixed(0)}%` : '';
        say(`Count: ${res ? res.person_count : '?'} person(s) in ROI${motion}. Boxes: ${res && res.boxes ? res.boxes.length : 0}.`);
        const boxes = (res && res.boxes) || [];
        const vw = video.videoWidth || 640, vh = video.videoHeight || 480;
        const tracked = updateTailgateTracks(boxes, vw, vh, {
            x: cfg.roi_x ?? 20, y: cfg.roi_y ?? 20,
            w: cfg.roi_width ?? 60, h: cfg.roi_height ?? 60,
        });
        drawTailgateOverlay(tracked.tracks, {
            x: cfg.roi_x ?? 20, y: cfg.roi_y ?? 20,
            w: cfg.roi_width ?? 60, h: cfg.roi_height ?? 60,
        }, video);
    } catch (e) {
        say('Count failed: ' + (e?.message || e));
    }
}

function clearTailgateOverlay() {
    tailgateTracks.clear();
    const canvas = document.getElementById('tailgate-track-overlay');
    if (canvas) {
        const ctx = canvas.getContext('2d');
        if (ctx) ctx.clearRect(0, 0, canvas.width, canvas.height);
    }
}

let activeWindowMemberId = null; // whose admitted entry Loop 3 attributes if piggybacked

function armDoorOpenTailgateSurveillance(durationMs = TAILGATE_WINDOW_MS, admittedMemberId = null) {
    activeDoorPassageWindow = true;
    doorOpenFrameCount = 0;
    suspiciousFrames = 0;
    activeWindowMemberId = admittedMemberId || null;
    maxTailgateFrames = Math.max(6, Math.floor(durationMs / TAILGATE_TICK_MS));
    console.debug(`[Security] 1:1 Anti-Tailgate Surveillance armed for ${durationMs}ms`);
}

/**
 * Universal Concurrent 3-Camera Vision Engine (SLS123 Parity):
 * - Camera 1 (Entry Face Terminal): continuous face scan with direction 'in'
 * - Camera 2 (Exit Face Terminal): continuous face scan with direction 'out'
 * - Camera 3 (Overhead Tailgate Radar): continuous YOLOv8 person tracking in ROI zone
 */
async function startAutonomousBiometricEngine() {
    // ── Loop 1: Camera 1 Entry Face Scanner (Direction 'in') — 650ms tick ──
    setInterval(async () => {
        if (!autoGateActive || autoScanCam1Busy || document.hidden) return;
        autoScanCam1Busy = true;
        try {
            const video = getCaptureElement(1);
            if (!video) return;

            const frame = captureVideoFrame(video);
            if (!frame) return;

            let scanRes;
            try {
                scanRes = await invokeTauri('scan_face_frame', { imageBase64: frame });
            } catch (e) {
                noteCamErr('cam1', 'Camera 1 (Entry)');
                return;
            }
            if (!scanRes || !scanRes.face_detected || !scanRes.vector) return;
            noteCamOk('cam1');

            // Live-scan quality floors (mirror enrollment): YuNet ghost floor
            // + 80px minimum face size (too far/small = unreliable embedding).
            if ((scanRes.confidence ?? 0) < 0.5) return;
            const _bb1 = scanRes.box || {};
            if (Math.min(_bb1.w || 0, _bb1.h || 0) < 80) return;

            // Pre-match liveness: first sighting only arms pending state;
            // process_face_scan runs once, on the confirmed-live frame.
            const live1 = checkLivePending('cam1', scanRes.vector, scanRes.landmarks);
            if (live1 === 'wait') return;
            if (live1 === 'spoof') {
                const now0 = Date.now();
                if (now0 - (liveSpoofToastAt.cam1 || 0) > 8000) {
                    liveSpoofToastAt.cam1 = now0;
                    showHudToast("Liveness Check Failed", "Static image suspected — entry denied. Present a live face.", "danger");
                }
                return;
            }

            let res;
            try {
                res = await invokeTauri('process_face_scan', {
                    probeVector: scanRes.vector,
                    direction: 'in'
                });
            } catch (e) {
                return;
            }

            const now = Date.now();
            if (res && res.matched) {
                const matchedId = res.member_id || 'unknown';
                const lastSeen = memberCooldownMap.get(matchedId) || 0;
                if (now - lastSeen < 12000) return; // 12-second debounce
                memberCooldownMap.set(matchedId, now);

                // Telemetry Lock State HUD
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

                // Inter-branch detection
                const matchedMember = cachedMembers.find(m => m.id === matchedId);
                const homeGym = matchedMember?.home_gym_name || null;
                const isCrossBranch = homeGym && homeGym !== appSettings.gym_name;
                const toastTitle = isCrossBranch ? "Inter-Branch Entry Verified" : "Auto Entry Verified";
                const branchInfo = isCrossBranch ? `<span class="text-amber-300 font-semibold">[Branch: ${escapeHtml(homeGym)}]</span> ` : '';

                showHudToast(
                    toastTitle,
                    `Welcome, <b>${escapeHtml(res.member_name || 'Member')}</b>! ${branchInfo}Gate unlocked (3000ms). (${(scanRes.confidence * 100 | 0)}% conf)`,
                    "success"
                );

                // Arm 1:1 Door-Open Anti-Tailgate Surveillance for 7.5s,
                // attributed to this admitted member for the incident record.
                armDoorOpenTailgateSurveillance(TAILGATE_WINDOW_MS, matchedId);

                await loadAttendanceLogs();
                await refreshDashboard();
            } else if (res && res.needs_reenroll) {
                clearLiveConfirm('cam1');
                showHudToast("Re-enrollment Needed", "Face gallery uses a legacy embedding width — re-scan this member in the Studio.", "warn");
            } else if (res && res.passback_violation) {
                const matchedId = res.member_id || 'unknown';
                const lastSeen = memberCooldownMap.get(matchedId) || 0;
                if (now - lastSeen > 8000) {
                    memberCooldownMap.set(matchedId, now);
                    showHudToast("Anti-Passback Blocked", res.message, "warn");
                }
            }
        } catch (e) {
            console.debug("Cam 1 cycle error:", e);
        } finally {
            autoScanCam1Busy = false;
        }
    }, 650);

    // ── Loop 2: Camera 2 Exit Face Scanner (Direction 'out') — 650ms tick ──
    setInterval(async () => {
        if (!autoGateActive || autoScanCam2Busy || document.hidden) return;
        autoScanCam2Busy = true;
        try {
            const video = getCaptureElement(2);
            if (!video) return;

            const frame = captureVideoFrame(video);
            if (!frame) return;

            let scanRes;
            try {
                scanRes = await invokeTauri('scan_face_frame', { imageBase64: frame });
            } catch (e) {
                noteCamErr('cam2', 'Camera 2 (Exit)');
                return;
            }
            if (!scanRes || !scanRes.face_detected || !scanRes.vector) return;
            noteCamOk('cam2');

            // Live-scan quality floors (same as entry).
            if ((scanRes.confidence ?? 0) < 0.5) return;
            const _bb2 = scanRes.box || {};
            if (Math.min(_bb2.w || 0, _bb2.h || 0) < 80) return;

            // Pre-match liveness (per-lane state, deadlock-free).
            const live2 = checkLivePending('cam2', scanRes.vector, scanRes.landmarks);
            if (live2 === 'wait') return;
            if (live2 === 'spoof') {
                const now0 = Date.now();
                if (now0 - (liveSpoofToastAt.cam2 || 0) > 8000) {
                    liveSpoofToastAt.cam2 = now0;
                    showHudToast("Liveness Check Failed", "Static image suspected — exit denied. Present a live face.", "danger");
                }
                return;
            }

            let res;
            try {
                res = await invokeTauri('process_face_scan', {
                    probeVector: scanRes.vector,
                    direction: 'out'
                });
            } catch (e) {
                return;
            }

            const now = Date.now();
            if (res && res.matched) {
                const matchedId = res.member_id || 'unknown';
                const lastSeen = memberCooldownMap.get(matchedId) || 0;
                if (now - lastSeen < 12000) return;
                memberCooldownMap.set(matchedId, now);

                const lockEl = document.getElementById('telemetry-lock-state');
                if (lockEl) {
                    lockEl.innerText = "UNLOCKED (AUTO EXIT)";
                    lockEl.className = "text-sm font-bold text-blue-400 mt-1 animate-pulse";
                    setTimeout(() => {
                        if (lockEl) {
                            lockEl.innerText = "LOCKED (STANDBY)";
                            lockEl.className = "text-sm font-bold text-emerald-400 mt-1";
                        }
                    }, 3000);
                }

                showHudToast(
                    "Auto Exit Verified",
                    `Goodbye, <b>${escapeHtml(res.member_name || 'Member')}</b>! Exit gate unlocked. (${(scanRes.confidence * 100 | 0)}% conf)`,
                    "exit"
                );

                // Exit opens the same 7.5s window, attributed to the exiting
                // member: a tailgater can follow an exiting member just as
                // easily as an entering one.
                armDoorOpenTailgateSurveillance(TAILGATE_WINDOW_MS, matchedId);

                await loadAttendanceLogs();
                await refreshDashboard();
            } else if (res && res.needs_reenroll) {
                clearLiveConfirm('cam2');
                showHudToast("Re-enrollment Needed", "Face gallery uses a legacy embedding width — re-scan this member in the Studio.", "warn");
            } else if (res && (res.passback_violation || res.account_hold || res.is_expired)) {
                clearLiveConfirm('cam2');
                const matchedId = res.member_id || 'unknown';
                const lastSeen = memberCooldownMap.get(matchedId) || 0;
                if (now - lastSeen > 8000) {
                    memberCooldownMap.set(matchedId, now);
                    showHudToast(res.passback_violation ? "Anti-Passback Blocked" : "Exit Denied", res.message, "warn");
                }
            }
        } catch (e) {
            console.debug("Cam 2 cycle error:", e);
        } finally {
            autoScanCam2Busy = false;
        }
    }, 650);

    // ── Loop 3: Camera 3 Continuous Overhead Anti-Tailgate Radar (350ms tick) ──
    // Economy: disarmed ticks run YOLO at most every 6th tick (~2.1s, overlay
    // only); armed ticks run every tick. All three camera loops stay concurrent.
    let cam3Economy = 0;
    setInterval(async () => {
        if (!autoGateActive || autoScanCam3Busy || document.hidden) return;
        const armed = activeDoorPassageWindow;
        if (!armed) {
            cam3Economy++;
            if (cam3Economy % 6 !== 1) return;
        }
        autoScanCam3Busy = true;
        try {
            const video = getCaptureElement(3);
            if (!video) return;

            const frame = captureVideoFrame(video);
            if (!frame) return;

            const cfg = appSettings.camera_config || {};
            const roi = {
                x: cfg.roi_x ?? 20, y: cfg.roi_y ?? 20,
                w: cfg.roi_width ?? 60, h: cfg.roi_height ?? 60,
            };
            let res;
            try {
                res = await invokeTauri('count_persons_in_frame', {
                    imageBase64: frame,
                    roiX: roi.x,
                    roiY: roi.y,
                    roiWidth: roi.w,
                    roiHeight: roi.h,
                });
            } catch (e) {
                noteCamErr('cam3', 'Camera 3 (Tailgate)');
                return;
            }
            noteCamOk('cam3');

            // Tracker overlay (always, cheap JS-side): stable IDs + ROI box.
            const boxes = (res && res.boxes) || [];
            const vw = video.videoWidth || 640, vh = video.videoHeight || 480;
            const tracked = updateTailgateTracks(boxes, vw, vh, roi);
            drawTailgateOverlay(tracked.tracks, roi, video);

            if (!activeDoorPassageWindow) return;
            doorOpenFrameCount++;
            // Alarm legs: 2+ in-ROI persons WITH ROI motion (>=2% pixel churn,
            // kills YOLO ghost false alarms on posters/shadows), OR 2+ distinct
            // tracked IDs having entered the ROI (tracks imply movement already).
            const motion = (res && res.motion_in_roi) || 0;
            const multiStatic = res && res.person_count > 1 && motion >= 0.02;
            const multiTracked = tracked.everCount >= 2;
            if (multiStatic || multiTracked) {
                suspiciousFrames++;
                // Immediate trigger when multi-occupancy confirmed across 2 ticks
                if (suspiciousFrames >= TAILGATE_SUSPICIOUS_NEEDED) {
                    activeDoorPassageWindow = false;
                    clearTailgateOverlay();
                    try {
                        const alarmRes = await invokeTauri('trigger_tailgate_alarm', {
                            reason: `Multi-occupancy turnstile transit violation in ROI (${res.person_count} persons, motion ${(motion * 100).toFixed(0)}%, ${tracked.everCount} tracked)`,
                            linkedMemberId: activeWindowMemberId,
                            personCount: res.person_count
                        }).catch(() => null);
                        activeWindowMemberId = null;
                        const sirenNote = alarmRes && alarmRes.siren_suppressed
                            ? ' Siren held (log-only mode or cooldown) — incident recorded.'
                            : ' Hardware Siren Active!';

                        const banner = document.getElementById('tailgate-siren-banner');
                        if (banner) {
                            banner.classList.remove('hidden');
                            setTimeout(() => { if (banner) banner.classList.add('hidden'); }, 10000);
                        }

                        showHudToast(
                            "Anti-Tailgate Violation",
                            `Tailgating Detected! Multiple persons in Turnstile ROI during gate transit (${res.person_count} persons).${sirenNote}`,
                            "danger"
                        );

                        await loadAttendanceLogs();
                        await refreshDashboard();
                    } catch (e) {
                        console.debug("Tailgate alarm trigger error:", e);
                    }
                }
            } else {
                suspiciousFrames = 0;
            }

            if (doorOpenFrameCount >= maxTailgateFrames) {
                activeDoorPassageWindow = false;
                clearTailgateOverlay();
            }
        } catch (e) {
            console.debug("Cam 3 tailgate cycle error:", e);
        } finally {
            autoScanCam3Busy = false;
        }
    }, 350);
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
    await initCameraStreams();
    await populateCameraDevices();
    startAutonomousBiometricEngine();
    startHardwareButtonPoll();

    // Auto refresh real-time polling every 2.5 seconds (skip when tab/window is hidden to save CPU/IPC)
    setInterval(async () => {
        if (document.hidden) return; // Skip polling when app is minimized or tab is not visible
        await refreshDashboard();
        if (currentView === 'attendance') await loadAttendanceLogs();
    }, 2500);
}

// ── Hardware Push-Buttons (pins.jfif): BTN1 pin 4 = ENTRY camera, BTN2 pin 8 = EXIT camera ──
// Tailgate has no button — it arms automatically on every entry unlock.
let hardwareButtonPollTimer = null;

function startHardwareButtonPoll() {
    if (hardwareButtonPollTimer) return;
    hardwareButtonPollTimer = setInterval(async () => {
        if (document.hidden) return;
        try {
            const evts = await invokeTauri('poll_hardware_buttons');
            if (Array.isArray(evts) && evts.length > 0) {
                for (const evt of evts) handleHardwareButtonEvent(evt);
            }
        } catch (e) {
            // Not connected or not in Tauri — silent
        }
    }, 380);
}

async function handleHardwareButtonEvent(evt) {
    const kind = evt.kind || evt.type || '';
    const isEntry = kind === 'entry_btn';
    const isExit = kind === 'exit_btn';
    if (!isEntry && !isExit) return;
    const direction = isEntry ? 'in' : 'out';
    const label = isEntry ? 'ENTRY Camera Enabled (BTN 1)' : 'EXIT Camera Enabled (BTN 2)';
    // The 7.5s tailgate window opens on EVERY button press up front —
    // verified or not, entry or exit. Someone tailgating an unverified
    // attempt is exactly what this catches.
    armDoorOpenTailgateSurveillance(TAILGATE_WINDOW_MS);
    showHudToast(label, isEntry ? 'Face the ENTRY camera — scanning…' : 'Face the EXIT camera — scanning…', 'success');
    // Highlight active camera card briefly
    try { highlightCameraCard(isEntry ? 'kiosk-cam1-entry' : 'kiosk-cam2-exit'); } catch (e) {}
    await doHardwareFaceScan(isEntry ? 'kiosk-cam1-entry' : 'kiosk-cam2-exit', direction);
}

function highlightCameraCard(videoId) {
    const card = document.getElementById(videoId)?.closest('.glass-panel');
    if (!card) return;
    card.classList.add('ring-2', 'ring-emerald-400', 'ring-offset-2', 'ring-offset-slate-950');
    setTimeout(() => card.classList.remove('ring-2', 'ring-emerald-400', 'ring-offset-2', 'ring-offset-slate-950'), 1600);
}

async function captureScanFrame(camNumber) {
    // Single capture path for button scans: worker-first viewport, 640px
    // model-sized JPEG (matches captureVideoFrame, halves IPC vs full-res).
    const video = getCaptureElement(camNumber);
    if (!video) return null;
    return captureVideoFrame(video);
}

async function doHardwareFaceScan(videoId, direction) {
    const camNumber = direction === 'in' ? 1 : 2;
    const laneKey = direction === 'in' ? 'btn-in' : 'btn-out';
    const frame = await captureScanFrame(camNumber);
    if (!frame) {
        clearLiveConfirm(laneKey);
        showHudToast('Camera Not Ready', 'No video feed for ' + direction.toUpperCase() + ' scan. Check Hardware Settings.', 'warn');
        return;
    }

    let scanRes;
    try {
        scanRes = await invokeTauri('scan_face_frame', { imageBase64: frame });
    } catch (e) {
        showHudToast('Face Scan Failed', String(e?.message || e), 'danger');
        return;
    }
    if (!scanRes || !scanRes.face_detected || !scanRes.vector) {
        clearLiveConfirm(laneKey);
        showHudToast('No Face Detected', 'Center your face and try again, or press the button again.', 'warn');
        return;
    }
    try {
        const result = await invokeTauri('process_face_scan', { probeVector: scanRes.vector, direction: direction });
        if (result.passback_violation) {
            clearLiveConfirm(laneKey);
            showHudToast('Anti-Passback Blocked', result.message, 'warn');
            return;
        }
        if (result.account_hold) {
            clearLiveConfirm(laneKey);
            showHudToast('Account On Hold', result.message, 'danger');
            return;
        }
        if (result.is_expired) {
            clearLiveConfirm(laneKey);
            showHudToast('Pass Expired', result.message, 'danger');
            return;
        }
        if (result.matched) {
            const matchedId = result.member_id || 'unknown';
            // Button scans are single-shot: seed this frame, capture a second
            // frame ~650ms later, and apply the same 2-frame liveness
            // confirmation as the autonomous loops.
            confirmLiveMatch(laneKey, matchedId, scanRes.vector, scanRes.landmarks);
            await new Promise(r => setTimeout(r, 650));
            const frame2 = await captureScanFrame(camNumber);
            if (frame2) {
                try {
                    const scan2 = await invokeTauri('scan_face_frame', { imageBase64: frame2 });
                    if (scan2 && scan2.face_detected && scan2.vector) {
                        const confirm = confirmLiveMatch(laneKey, matchedId, scan2.vector, scan2.landmarks);
                        if (confirm === 'spoof') {
                            showHudToast('Liveness Check Failed', 'Static image suspected — denied. Present a live face.', 'danger');
                            return;
                        }
                        if (confirm !== 'confirmed') return;
                    }
                } catch (e) { /* second-frame failure: fall through to single-match unlock */ }
            }
            const isCross = result.member_name && cachedMembers.find(m => `${m.first_name} ${m.last_name}` === result.member_name)?.home_gym_name;
            showHudToast(direction === 'in' ? 'Entry Verified' : 'Exit Verified',
                `${escapeHtml(result.member_name || 'Member')} — Gate unlocked (${(result.confidence*100|0)}% ${direction.toUpperCase()})`, 'success');
            armDoorOpenTailgateSurveillance(TAILGATE_WINDOW_MS, matchedId);
            await loadAttendanceLogs();
            await refreshDashboard();
        } else {
            clearLiveConfirm(laneKey);
            if (result.needs_reenroll) {
                showHudToast('Re-enrollment Needed', 'Face gallery uses a legacy embedding width — re-scan this member in the Studio.', 'warn');
            } else {
                showHudToast('Not Recognized', result.message || 'Face not in member database.', 'warn');
            }
        }
    } catch (e) {
        showHudToast('Gate Error', String(e), 'danger');
    }
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

    if (viewName === 'dashboard') {
        refreshDashboard();
        syncAllCameraViewports();
    }
    if (viewName === 'members') loadMembers();
    if (viewName === 'interbranch') loadInterbranchMembers();
    if (viewName === 'register') {
        initStudioCamera();
        syncAllCameraViewports();
    }
    if (viewName === 'walkins') loadWalkIns();
    if (viewName === 'attendance') {
        loadAttendanceLogs();
        syncAllCameraViewports();
    }
    if (viewName === 'pos') loadProducts();
    if (viewName === 'eod') loadEndOfDay();
    if (viewName === 'expenses') loadExpenses();
    if (viewName === 'coaches') loadCoaches();
    if (viewName === 'hardware') {
        populateCameraDevices();
        syncAllCameraViewports();
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
    resetRegistrationStudio();
    rescanMemberId = id;
    const fn = document.getElementById('reg-mem-first-name');
    const ln = document.getElementById('reg-mem-last-name');
    const phn = document.getElementById('reg-mem-phone');
    const em = document.getElementById('reg-mem-email');
    if (fn) { fn.value = m.first_name; fn.disabled = true; }
    if (ln) { ln.value = m.last_name; ln.disabled = true; }
    if (phn) phn.value = m.phone || '';
    if (em) em.value = m.email || '';
    const btn = document.getElementById('btn-complete-enroll');
    if (btn && btn.querySelector('span')) btn.querySelector('span').innerText = `Save New Face Scan (${m.id})`;
    switchView('register');
    showHudToast('Re-scan Mode', `Capture fresh angles for ${m.first_name} ${m.last_name}. Submit replaces their stored face vectors.`, 'info');
}

// --- Member Registration & Biometric Capture Studio ---

let selectedRegAngle = 0;
let capturedRegFrames = [null, null, null, null, null];
let capturedRegVectors = [null, null, null, null, null];
let capturedRegBoxes = [null, null, null, null, null];
// Enrollment quality floors (Phase A): blur + face-size + distinct angles.
const ENROLL_MIN_SHARPNESS = 30;
const ENROLL_MIN_FACE_PX = 80;
const ENROLL_MIN_DISTINCT_ANGLES = 3;
const ENROLL_DISTINCT_COSINE = 0.98;

function laplacianSharpness(srcCanvas) {
    const w = 64, h = 64;
    const c = document.createElement('canvas');
    c.width = w; c.height = h;
    const x = c.getContext('2d', { willReadFrequently: true });
    if (!x) return Infinity;
    x.drawImage(srcCanvas, 0, 0, w, h);
    let d;
    try { d = x.getImageData(0, 0, w, h).data; } catch (e) { return Infinity; }
    const gray = new Float32Array(w * h);
    for (let i = 0; i < w * h; i++) {
        gray[i] = 0.299 * d[i * 4] + 0.587 * d[i * 4 + 1] + 0.114 * d[i * 4 + 2];
    }
    let sum = 0, sum2 = 0, n = 0;
    for (let y = 1; y < h - 1; y++) {
        for (let xx = 1; xx < w - 1; xx++) {
            const i = y * w + xx;
            const lap = -4 * gray[i] + gray[i - 1] + gray[i + 1] + gray[i - w] + gray[i + w];
            sum += lap; sum2 += lap * lap; n++;
        }
    }
    const mean = sum / n;
    return sum2 / n - mean * mean;
}

function countDistinctAngles(vectors) {
    const accepted = [];
    for (const v of vectors) {
        if (!v) continue;
        if (accepted.every(a => cosineOf(a, v) < ENROLL_DISTINCT_COSINE)) accepted.push(v);
    }
    return accepted.length;
}

const anglePrompts = [
    { label: "1. Frontal (0°)", guide: "Look straight at the camera", offset: 0.0 },
    { label: "2. Left (15°)", guide: "Turn head slightly to the left", offset: 0.45 },
    { label: "3. Right (15°)", guide: "Turn head slightly to the right", offset: -0.45 },
    { label: "4. Tilt Up (10°)", guide: "Tilt chin slightly upward", offset: 0.25 },
    { label: "5. Tilt Down (10°)", guide: "Tilt chin slightly downward", offset: -0.25 }
];

async function initStudioCamera() {
    const video = document.getElementById('reg-studio-video');
    const errEl = document.getElementById('reg-error-msg');
    if (!video) return;

    if (streamCam1 && streamCam1.active) {
        video.srcObject = streamCam1;
        video.play().catch(() => {});
        if (errEl) errEl.innerText = "";
    } else {
        const cfg = appSettings.camera_config || {};
        const res = await getStreamForDevice(cfg.camera1_entry_device_id);
        if (res.stream) {
            streamCam1 = res.stream;
            video.srcObject = res.stream;
            video.play().catch(() => {});
            if (errEl) errEl.innerText = "";
        } else if (res.error && res.error.isOccupied) {
            if (errEl) {
                errEl.innerHTML = "<strong>⚠️ Camera Occupied:</strong> Webcam is in use by another application (Zoom, OBS, Teams). Close conflicting software to enable face enrollment.";
                errEl.className = "text-xs text-amber-300 font-semibold p-2.5 bg-amber-950/80 rounded-lg border border-amber-600 block";
            }
            showHudToast("Camera Occupied", "Cannot start registration camera: device is locked by another program.", "danger");
        } else {
            if (errEl) {
                errEl.innerText = "Camera not detected. Please verify webcam connection in Hardware Settings.";
                errEl.className = "text-xs text-red-300 p-2 bg-red-950/60 rounded border border-red-800 block";
            }
        }
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

    // Blur gate (enrollment-time only): reject smeared frames before spending
    // inference — Laplacian variance on a 64px grayscale copy.
    const sharp = laplacianSharpness(canvas);
    if (sharp < ENROLL_MIN_SHARPNESS) {
        showError(`Too blurry (sharpness ${sharp.toFixed(0)} < ${ENROLL_MIN_SHARPNESS}). Hold still and recapture.`);
        if (badge) { badge.innerText = "Blurry"; badge.className = "text-[9px] text-red-400 font-bold font-mono mt-0.5"; }
        return;
    }
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
    capturedRegBoxes[selectedRegAngle] = result.box || null;

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
    capturedRegBoxes = [null, null, null, null, null];
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

    if (!rescanMemberId) {
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
    }

    const capturedCount = capturedRegFrames.filter(f => f !== null).length;
    if (capturedCount === 0) {
        errorEl.innerText = "Please capture at least the Frontal Face angle (Angle 1)";
        return;
    }

    // Enrollment quality gates (Phase A): small/distant faces and
    // near-duplicate angles are rejected instead of stored.
    for (let i = 0; i < 5; i++) {
        const box = capturedRegBoxes[i];
        if (capturedRegVectors[i] && box) {
            const size = Math.min(box.w || 0, box.h || 0);
            if (size > 0 && size < ENROLL_MIN_FACE_PX) {
                errorEl.innerText = `Angle ${i + 1} face too small (${size.toFixed(0)}px < ${ENROLL_MIN_FACE_PX}px). Move closer and recapture angle ${i + 1}.`;
                errorEl.className = "text-xs text-amber-400";
                return;
            }
        }
    }
    const realVectors = capturedRegVectors.filter(v => v !== null);
    const distinct = countDistinctAngles(realVectors);
    if (distinct < ENROLL_MIN_DISTINCT_ANGLES) {
        errorEl.innerText = `Only ${distinct} distinct angle(s) captured — need at least ${ENROLL_MIN_DISTINCT_ANGLES} genuinely different angles (frontal + turn head left/right). Duplicates don't count.`;
        errorEl.className = "text-xs text-amber-400";
        return;
    }
    // Missing slots reuse the closest REAL captured vector (never synthetic
    // noise) now that distinctness is enforced above.
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

    // Walk-ins use the SAME entry camera as registration/member scans
    // (cam1). Capture a real face frame; no face -> code-only pass with NO
    // biometric vector (never a fabricated Math.sin probe).
    let faceVector = null;
    try {
        errorEl.innerText = "Capturing guest face on entry camera...";
        errorEl.className = "text-xs text-blue-300";
        const video = getCaptureElement(1);
        const frame = video ? captureVideoFrame(video) : null;
        if (frame) {
            const scan = await invokeTauri('scan_face_frame', { imageBase64: frame });
            if (scan && scan.face_detected && scan.vector) {
                faceVector = scan.vector;
            }
        }
    } catch (e) {
        console.debug("Walk-in face capture failed, issuing code-only pass:", e);
    }
    if (!faceVector) {
        errorEl.innerText = "No face in frame — issuing code-only pass (no biometric unlock).";
        errorEl.className = "text-xs text-amber-400";
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
                face_vector: faceVector
            }
        });

        closeWalkInModal();
        // Walk-in opens the door: same 7.5s tailgate window as any unlock.
        armDoorOpenTailgateSurveillance();
        await loadWalkIns();
        await loadAttendanceLogs();
        await refreshDashboard();

        alert(`Walk-In Pass Issued!\nPass ID: ${pass.id}\nGuest: ${name}\nPaid: ₱${fee.toFixed(2)} (${payment.toUpperCase()})\nGate unlocked for 3 seconds!`);
    } catch (e) {
        errorEl.innerText = "Walk-in Error: " + e;
        errorEl.className = "text-xs text-red-400";
    }
}

async function renewWalkIn(id, name) {
    if (!confirm(`Renew walk-in pass for ${name}? Fresh 8 hours from now using the saved face — no re-scan needed.`)) return;
    try {
        const updated = await invokeTauri('renew_walk_in', { id: id });
        await loadWalkIns();
        showHudToast("Pass Renewed", `${name} active until ${new Date(updated.expires_at).toLocaleTimeString()}.`, "success");
    } catch (e) {
        alert("Renew Pass Error: " + e);
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
                    <td class="p-3 font-bold text-emerald-400">₱${w.amount_paid.toFixed(2)}</td>
                    <td class="p-3 uppercase text-[10px] font-bold text-slate-300">${w.payment_method}</td>
                    <td class="p-3">${statusBadge}</td>
                    <td class="p-3 text-right space-x-1">
                        <button onclick="renewWalkIn('${w.id}', '${w.guest_name.replace(/'/g, "\\'")}')" title="Renew: fresh 8h from now, same face — no re-scan" class="px-2 py-1 rounded text-[10px] font-bold bg-purple-950 hover:bg-purple-900 text-purple-300 border border-purple-800 transition">Renew</button>
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
        const safePhoto = (m.photo_data_url && m.photo_data_url.startsWith('data:image/')) ? m.photo_data_url : null;
        const photo = safePhoto
            ? `<img src="${safePhoto}" alt="ref" class="w-8 h-8 rounded-full object-cover border border-slate-600" title="Enrollment reference photo"/>`
            : `<span class="w-8 h-8 rounded-full bg-slate-800 border border-slate-700 inline-flex items-center justify-center text-slate-400 font-bold text-xs">${escapeHtml((m.first_name || '?').charAt(0))}</span>`;
        const escId = String(m.id || '').replace(/'/g, "\\'");
        const escName = `${String(m.first_name || '').replace(/'/g, "\\'")} ${String(m.last_name || '').replace(/'/g, "\\'")}`;
        const dispName = escapeHtml(`${m.first_name || ''} ${m.last_name || ''}`.trim());
        const freezeBtn = isSuspended
            ? `<button onclick="unfreezeMember('${escId}')" title="Unfreeze (reactivate)" class="px-2.5 py-1 rounded bg-emerald-950/60 hover:bg-emerald-900 text-xs text-emerald-300 border border-emerald-800/50 font-medium transition">Unfreeze</button>`
            : `<button onclick="freezeMember('${escId}')" title="Freeze (deny gate, keep data)" class="px-2.5 py-1 rounded bg-amber-950/60 hover:bg-amber-900 text-xs text-amber-300 border border-amber-800/50 font-medium transition">Freeze</button>`;

        return `
            <tr class="hover:bg-slate-800/30 transition ${isSuspended || isExpired ? 'opacity-70' : ''}">
                <td class="p-3 font-mono text-blue-300">${escapeHtml(m.id)}</td>
                <td class="p-3">
                    <div class="flex items-center gap-2">
                        ${photo}
                        <div>
                            <span class="font-semibold text-slate-200">${dispName}</span>
                            <div class="text-[10px] text-slate-500">${escapeHtml(m.email || '--')}</div>
                        </div>
                        ${m.home_gym_name && m.home_gym_name !== appSettings.gym_name ? `<span class="px-1.5 py-0.5 rounded text-[9px] font-bold bg-purple-950 text-purple-300 border border-purple-800/60" title="Inter-Branch Member">📍 ${escapeHtml(m.home_gym_name)}</span>` : ''}
                    </div>
                </td>
                <td class="p-3 uppercase text-[11px] font-bold text-amber-300">${escapeHtml(m.membership_type)}</td>
                <td class="p-3 text-slate-400 font-mono">${escapeHtml(m.phone || '--')}</td>
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

function formatRbacError(action, err) {
    const s = String(err);
    if (s.toLowerCase().includes('privilege') || s.toLowerCase().includes('manager') || s.toLowerCase().includes('login') || s.toLowerCase().includes('owner')) {
        return `Authentication Required: Please log in using your Manager or Owner PIN (Staff Login at top right) to ${action}.`;
    }
    return `${action} failed: ${err}`;
}

async function renewMember(id) {
    if (!confirm(`Renew membership for ${id}? Status returns to ACTIVE with expiry +30 days.`)) return;
    try {
        await invokeTauri('renew_member', { id: id });
        await loadMembers();
        await refreshDashboard();
        showHudToast('Membership Renewed', `${id} is ACTIVE for 30 more days.`, 'success');
    } catch (e) { alert(formatRbacError('Renew', e)); }
}

async function freezeMember(id) {
    if (!confirm(`Freeze ${id}? The gate will deny entry but all data and vectors are kept.`)) return;
    try {
        await invokeTauri('freeze_member', { id: id });
        await loadMembers();
        showHudToast('Member Frozen', `${id} is now SUSPENDED and blocked at the gate.`, 'info');
    } catch (e) { alert(formatRbacError('Freeze', e)); }
}

async function unfreezeMember(id) {
    try {
        await invokeTauri('unfreeze_member', { id: id });
        await loadMembers();
        showHudToast('Member Unfrozen', `${id} is ACTIVE again.`, 'success');
    } catch (e) { alert(formatRbacError('Unfreeze', e)); }
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
        const ibName = escapeHtml(`${m.first_name || ''} ${m.last_name || ''}`.trim());
        return `<tr class="hover:bg-slate-800/30 transition">
            <td class="p-3 font-mono text-blue-300">${escapeHtml(m.id)}</td>
            <td class="p-3"><div class="font-semibold text-slate-200">${ibName}</div><div class="text-[10px] text-slate-500">${escapeHtml(m.email||'--')}</div></td>
            <td class="p-3"><span class="px-2 py-0.5 rounded text-[11px] font-bold bg-purple-950 text-purple-300 border border-purple-800/60">${escapeHtml(m.home_gym_name||'—')}</span><div class="text-[10px] font-mono text-slate-500">${escapeHtml((m.home_gym_id||'').slice(0,8))}</div></td>
            <td class="p-3"><span class="px-2 py-0.5 rounded text-[10px] border font-semibold uppercase ${statusCls}">${escapeHtml(m.membership_type||'regular')} · ${escapeHtml(m.status||'active')}</span></td>
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
    const planSel = document.getElementById('edit-mem-plan');
    const planVal = (m.membership_type || 'regular').toLowerCase();
    planSel.value = planVal;
    // Legacy 'vip' rows predate the owner-portal tiers: keep the stored value
    // selectable instead of silently blanking (and overwriting with "").
    if (planSel.value !== planVal) {
        const opt = document.createElement('option');
        opt.value = planVal;
        opt.innerText = `${m.membership_type} (legacy)`;
        opt.dataset.legacy = '1';
        planSel.appendChild(opt);
        planSel.value = planVal;
    } else {
        planSel.querySelectorAll('option[data-legacy]').forEach(o => o.remove());
    }
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
        showHudToast("Member Deleted", `Member ${name} (${id}) and biometrics permanently removed.`, "info");
        alert(`Member ${name} deleted.`);
    } catch (e) {
        alert(formatRbacError("Delete Member", e));
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
    // Owner-customized rate cards + promo vouchers synced from the portal.
    try {
        const plans = await invokeTauri('list_remote_plans') || [];
        populateRegPlanSelect(plans);
    } catch (e) { /* offline/preview: keep hardcoded plans */ }
    await loadRemotePromos();
}

function populateRegPlanSelect(plans) {
    const sel = document.getElementById('reg-mem-plan');
    if (!sel || !plans || plans.length === 0) return;
    const prev = sel.value;
    sel.innerHTML = plans.map(p => {
        const tag = (p.tag || '').toUpperCase();
        const per = { 'session': '/session', 'monthly': '/mo', '3-months': '/3mo', '6-months': '/6mo', '1-year': '/yr' }[p.billing_period] || '/mo';
        return `<option value="${p.id}">${escapeHtml(p.name)}${tag ? ' [' + escapeHtml(tag) + ']' : ''} — ₱${p.price_monthly}${per}</option>`;
    }).join('');
    if ([...sel.options].some(o => o.value === prev)) sel.value = prev;
}

function renderProductsGrid() {
    const grid = document.getElementById('pos-products-grid');
    if (!grid) return;

    const filtered = cachedProducts.filter(p => {
        if (currentPosCategory === 'all') return true;
        return p.category.toLowerCase() === currentPosCategory;
    });

    if (filtered.length === 0) {
        grid.innerHTML = cachedProducts.length === 0
            ? '<div class="col-span-2 text-center text-slate-500 py-10">No products yet — the gym owner adds them in the Owner Portal catalog and they sync here automatically.</div>'
            : '<div class="col-span-2 text-center text-slate-500 py-10">No items found in this category</div>';
        return;
    }

    const owner = isTerminalOwner();

    grid.innerHTML = filtered.map(p => `
        <div class="glass-panel p-3.5 border border-slate-800 flex flex-col justify-between card hover:border-slate-700 transition">
            <div>
                <div class="flex items-center justify-between">
                    <span class="text-[10px] uppercase font-bold text-slate-400 tracking-wider">${p.category}</span>
                    ${owner ? `<div class="flex items-center gap-1.5">
                        <button onclick="openEditProductModal('${p.id}')" title="Edit Product (owner)" class="text-slate-400 hover:text-blue-300 text-xs p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path></svg>
                        </button>
                        <button onclick="deleteProduct('${p.id}', '${p.name.replace(/'/g, "\\'")}')" title="Delete Product (owner)" class="text-slate-400 hover:text-red-400 text-xs p-1">
                            <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                        </button>
                    </div>` : ''}
                </div>
                <div class="text-xs font-bold text-slate-200 mt-1">${p.name}</div>
                <div class="flex items-center justify-between mt-1 text-[11px] text-slate-400">
                    <span>Stock: <b class="${p.stock < 5 ? 'text-red-400' : 'text-emerald-400'}">${p.stock}</b></span>
                    <button onclick="quickRestockProduct('${p.id}', 10)" class="text-[10px] font-bold text-blue-400 hover:text-blue-300">+10 Stock</button>
                </div>
            </div>
            <div class="flex items-center justify-between mt-3 pt-2.5 border-t border-slate-800">
                <span class="text-base font-bold text-slate-100 brand">₱${p.price.toFixed(2)}</span>
                <button onclick="addToCart('${p.id}', '${p.name.replace(/'/g, "\\'")}', ${p.price})" class="px-3 py-1.5 rounded-lg bg-blue-600/80 hover:bg-blue-600 text-xs font-bold text-white transition flex items-center gap-1 shadow">
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6v6m0 0v6m0-6h6m-6 0H6"></path></svg>
                    <span>Add</span>
                </button>
            </div>
        </div>
    `).join('');
}

function isTerminalOwner() {
    return !!(currentTerminalSession && currentTerminalSession.is_authenticated && currentTerminalSession.role === 'owner');
}

function openAddProductModal() {
    // Owner-only: catalog definition lives in the owner portal. The server
    // enforces this too (require_owner); this is just early UI guidance.
    if (!isTerminalOwner()) {
        alert("Owner login required: products are created in the Owner Portal catalog (or by the franchise owner on this terminal).");
        return;
    }
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
    if (!isTerminalOwner()) {
        alert("Owner login required: products are edited in the Owner Portal catalog.");
        return;
    }
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
    if (!isTerminalOwner()) {
        alert("Owner login required: products are deleted in the Owner Portal catalog.");
        return;
    }
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
        if (totalEl) totalEl.innerText = '₱0.00';
        return;
    }

    let total = 0;
    container.innerHTML = cart.map((item, idx) => {        const itemTotal = item.unit_price * item.quantity;
        total += itemTotal;
        return `
            <div class="flex justify-between items-center bg-slate-800/40 p-2 rounded border border-slate-700">
                <div>
                    <div class="font-semibold text-slate-200">${item.product_name}</div>
                    <div class="text-[10px] text-slate-400">₱${item.unit_price.toFixed(2)} &times; ${item.quantity}</div>
                </div>
                <div class="flex items-center gap-2">
                    <span class="font-bold text-slate-200">₱${itemTotal.toFixed(2)}</span>
                    <button onclick="removeFromCart(${idx})" class="text-red-400 hover:text-red-300 text-xs p-1">
                        <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
                    </button>
                </div>
            </div>
        `;
    }).join('');

    const disc = currentPosDiscount(total);
    if (disc.pct > 0) {
        const gross = total;
        const off = Math.round(gross * disc.pct) / 100;
        if (totalEl) totalEl.innerHTML = `<span class="line-through text-slate-500 text-sm mr-2">₱${gross.toFixed(2)}</span>₱${(gross - off).toFixed(2)}`;
        container.insertAdjacentHTML('beforeend', `<div class="text-[11px] text-emerald-400 font-semibold text-right">Discount (${disc.label}): −$${off.toFixed(2)}</div>`);
    } else if (totalEl) {
        totalEl.innerText = `₱${total.toFixed(2)}`;
    }
}

// Promo vouchers synced from the owner portal (list_remote_promos).
let cachedPromos = [];
let appliedPromo = null;

async function loadRemotePromos() {
    try {
        cachedPromos = await invokeTauri('list_remote_promos') || [];
    } catch (e) { cachedPromos = []; }
}

function applyPosPromo() {
    const input = document.getElementById('pos-promo-input');
    const msg = document.getElementById('pos-promo-msg');
    const code = (input ? input.value.trim().toUpperCase() : '');
    appliedPromo = null;
    if (!code) {
        if (msg) { msg.innerText = ''; }
        renderCart();
        return;
    }
    const now = new Date();
    const pr = cachedPromos.find(p => (p.code || '').toUpperCase() === code && p.is_active !== false
        && (!p.expires_at || new Date(p.expires_at) >= now));
    if (!pr) {
        if (msg) { msg.innerText = `Code ${code} not found, expired, or not yet synced from portal.`; msg.className = 'text-[10px] mt-1 text-red-400'; }
        renderCart();
        return;
    }
    appliedPromo = pr;
    if (msg) { msg.innerText = `${pr.label || pr.code}: ${pr.discount_type === 'percent' ? pr.discount_value + '% off' : '₱' + pr.discount_value + ' off'} (min ₱${pr.min_spend || 0})`; msg.className = 'text-[10px] mt-1 text-emerald-400'; }
    renderCart();
}

function currentPosDiscount(gross = 0) {
    const sel = document.getElementById('pos-discount-select');
    const v = sel ? sel.value : 'none';
    let type = '', label = '', pct = 0;
    if (v === 'senior') { type = 'senior'; label = 'Senior ID'; pct = 20; }
    else if (v === 'student') { type = 'student'; label = 'Student ID'; pct = 20; }
    else if (v === 'pwd') { type = 'pwd'; label = 'PWD ID'; pct = 20; }
    // Stack the promo voucher: fixed-amount promos convert to an effective pct
    // of the gross so the single server-side discount path stays exact.
    if (appliedPromo && gross > 0) {
        let promoOff = 0;
        if ((gross) >= (appliedPromo.min_spend || 0)) {
            promoOff = appliedPromo.discount_type === 'percent'
                ? gross * appliedPromo.discount_value / 100
                : Math.min(gross, appliedPromo.discount_value);
        }
        if (promoOff > 0) {
            const promoPct = promoOff / gross * 100;
            // ID discount applies to the post-promo remainder: combined pct
            const combined = promoPct + pct * (1 - promoPct / 100);
            return { type: `${appliedPromo.code}${type ? '+' + type : ''}`, label: `${appliedPromo.label || appliedPromo.code}${label ? ' + ' + label : ''}`, pct: Math.min(100, combined) };
        }
    }
    return { type, label, pct };
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

    const gross = cart.reduce((s, i) => s + i.unit_price * i.quantity, 0);
    const disc = currentPosDiscount(gross);
    // Require an ID number only when an ID discount (senior/student/pwd) is
    // part of the discount — promo-only checkouts don't need one.
    const needsId = /(senior|student|pwd)/i.test(disc.type || '');
    let idNote = '';
    if (needsId) {
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

        alert(`Sale Processed!\nTransaction ID: ${tx.id}\nGross: ₱${(tx.total_amount + (tx.discount_amount || 0)).toFixed(2)}\nDiscount: ${disc.label || 'None'} -₱${(tx.discount_amount || 0).toFixed(2)}${idNote ? ` (ID: ${idNote})` : ''}\nTotal: ₱${tx.total_amount.toFixed(2)}\nPayment: ${paymentMethod.toUpperCase()}`);
        cart = [];
        appliedPromo = null;
        const promoInput = document.getElementById('pos-promo-input');
        if (promoInput) promoInput.value = '';
        const promoMsg = document.getElementById('pos-promo-msg');
        if (promoMsg) promoMsg.innerText = '';
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

let attendanceTailgateOnly = false;

function toggleTailgateFilter() {
    attendanceTailgateOnly = !attendanceTailgateOnly;
    const btn = document.getElementById('btn-tailgate-filter');
    if (btn) {
        btn.innerText = attendanceTailgateOnly ? 'Show all activity' : 'Show tailgate only';
        btn.className = attendanceTailgateOnly
            ? 'px-2.5 py-1 rounded-lg text-[11px] font-semibold bg-red-950 hover:bg-red-900 text-red-200 border border-red-800 transition'
            : 'px-2.5 py-1 rounded-lg text-[11px] font-semibold bg-slate-800 hover:bg-slate-700 text-slate-300 border border-slate-700 transition';
    }
    loadAttendanceLogs();
}

async function resolveTailgateIncident(id) {
    try {
        await invokeTauri('resolve_tailgate_incident', { id });
        await loadAttendanceLogs();
        await refreshDashboard();
    } catch (e) {
        alert("Resolve failed (manager/owner login required): " + e);
    }
}

async function loadAttendanceLogs() {
    try {
        const [logs, inc] = await Promise.all([
            invokeTauri('list_recent_attendance', { limit: 15 }),
            invokeTauri('list_tailgate_incidents', { limit: 1 }).catch(() => null)
        ]);
        const tbody = document.getElementById('attendance-log-tbody');
        if (tbody) {
            const badge = document.getElementById('tailgate-unacked-badge');
            const unacked = inc && typeof inc.unacked === 'number' ? inc.unacked : 0;
            if (badge) {
                badge.innerText = `${unacked} unreviewed`;
                badge.classList.toggle('hidden', unacked === 0);
            }
        }
        if (!tbody) return;

        const rows = attendanceTailgateOnly && Array.isArray(logs) ? logs.filter(l => l.tailgate_flag) : logs;
        if (!Array.isArray(rows) || rows.length === 0) {
            tbody.innerHTML = `<tr><td colspan="6" class="p-4 text-center text-slate-500">${attendanceTailgateOnly ? 'No tailgate incidents in recent activity' : 'No recent gate activity'}</td></tr>`;
            return;
        }

        tbody.innerHTML = rows.map(l => {
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
                const attrib = l.linked_member_id
                    ? `<div class="text-[9px] text-red-300/70 font-mono mt-0.5">via ${escapeHtml(l.linked_member_id)}${l.person_count ? ` · ${l.person_count}p` : ''}</div>`
                    : (l.person_count ? `<div class="text-[9px] text-red-300/70 font-mono mt-0.5">${l.person_count}p in ROI</div>` : '');
                flagBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-bold animate-pulse"><svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg><span>TAILGATE FLAG</span></span>${attrib}<button onclick="resolveTailgateIncident('${l.id}')" title="Mark reviewed (manager/owner)" class="mt-1 px-2 py-0.5 rounded text-[9px] bg-slate-800 hover:bg-emerald-950 text-slate-300 hover:text-emerald-300 border border-slate-700 font-semibold transition">Resolve</button>`;
            } else if (isInterbranchVisitor) {
                flagBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-purple-950 text-purple-300 border border-purple-800 font-bold" title="Home: ${interMember.home_gym_name}"><span>📍 Inter-Branch Visitor</span><span class="font-mono text-[9px]">[${interMember.home_gym_name}]</span></span>`;
            } else if (isOverride) {
                flagBadge = '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-amber-950 text-amber-400 border border-amber-700 font-semibold"><span>UNPAID / MANUAL PULSE</span></span>';
            } else if (isWalkIn) {
                flagBadge = '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-emerald-950/70 text-emerald-400 border border-emerald-800 font-medium"><span>8H TIMED PASS</span></span>';
            }

            const timeFormatted = new Date(l.timestamp).toLocaleTimeString();

            let displayName = l.member_name;
            if (!displayName || displayName === 'Unidentified Person') {
                if (isTailgate) {
                    displayName = '⚠️ Tailgate Intrusion';
                } else if (isOverride) {
                    displayName = 'Manual Gate Pulse';
                } else {
                    displayName = 'Unregistered Visitor';
                }
            }

            return `
                <tr class="hover:bg-slate-800/30 transition ${isTailgate ? 'bg-red-950/20' : (isInterbranchVisitor ? 'bg-purple-950/20 border-l-2 border-purple-500' : (isOverride ? 'bg-amber-950/25 border-l-2 border-amber-500' : ''))}">
                    <td class="p-3 font-mono text-blue-300">${l.id}</td>
                    <td class="p-3 font-semibold text-slate-200">${escapeHtml(displayName)}${isInterbranchVisitor ? ` <span class="text-[9px] text-purple-400">[${escapeHtml(interMember.home_gym_name)}]</span>` : ''}</td>
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
    const isEntry = direction === 'in';
    const video = getCaptureElement(isEntry ? 1 : 2);
    let probe = null;

    if (video && video.videoWidth > 0) {
        const frame = captureVideoFrame(video);
        if (frame) {
            try {
                const scanRes = await invokeTauri('scan_face_frame', { imageBase64: frame });
                if (scanRes && scanRes.face_detected && scanRes.vector) {
                    probe = scanRes.vector;
                } else {
                    alert(`No face detected in Camera ${isEntry ? 1 : 2} (${direction.toUpperCase()}). Please center your face in front of the camera.`);
                    return;
                }
            } catch (e) {
                console.warn("Scan frame error, falling back:", e);
            }
        }
    }

    if (!probe) {
        // No stored-vector or synthetic fallbacks: a test scan with another
        // member's enrolled vector is a guaranteed self-match that proves
        // nothing and can unlock the wrong door. Live camera or nothing.
        alert("Test scan needs a live camera feed with a face in frame. Check Hardware Settings.");
        return;
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
            msg += ` — Gate Unlocked!`;
            showHudToast("Face Verified", msg, "success");
            armDoorOpenTailgateSurveillance();
            alert(msg);
        } else if (result.is_expired) {
            alert(`Scan Denied: ${result.message}\nDoor remains LOCKED to prevent unauthorized entry.`);
        } else {
            alert("Scan Result: Face Not Recognized (Unknown)");
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

    // Live camera only: synthetic name-seeded probes self-match and prove
    // nothing (and must never arm the tailgate window).
    const isEntry = direction === 'in';
    const video = getCaptureElement(isEntry ? 1 : 2);
    const frame = video ? captureVideoFrame(video) : null;
    if (!frame) {
        alert("Walk-in test needs a live camera feed. Check Hardware Settings.");
        return;
    }
    let probe = null;
    try {
        const scanRes = await invokeTauri('scan_face_frame', { imageBase64: frame });
        if (!scanRes || !scanRes.face_detected || !scanRes.vector) {
            alert("No face detected in frame. Center the guest's face and retry.");
            return;
        }
        probe = scanRes.vector;
    } catch (e) {
        alert("Walk-In Scan Error: " + e);
        return;
    }
    const guest = cachedWalkIns[0];

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
            armDoorOpenTailgateSurveillance();
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
        // Any door opening gets the 7.5s tailgate window, operator or not.
        armDoorOpenTailgateSurveillance();
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

async function checkLicenseKeyMatch() {
    const urlInput = document.getElementById('license-cloud-url-input');
    const statusEl = document.getElementById('license-keymatch-status');
    const customUrl = urlInput ? urlInput.value.trim() : '';
    statusEl.innerText = "Fetching cloud verification key...";
    statusEl.className = "text-xs text-blue-300";
    try {
        const res = await invokeTauri('get_license_key_diagnostics', { cloudUrl: customUrl || null });
        if (res.match) {
            statusEl.innerText = `MATCH ✓ exe ${res.embedded_fingerprint} == cloud ${res.cloud_fingerprint}. Pasted keys from ${res.cloud_url} will verify.`;
            statusEl.className = "text-xs text-emerald-400";
        } else {
            statusEl.innerText = `MISMATCH ✗ exe ${res.embedded_fingerprint} vs cloud ${res.cloud_fingerprint}. The cloud is signing with a different key (likely ephemeral — set RSA_PRIVATE_KEY_PEM on the cloud and re-issue the key).`;
            statusEl.className = "text-xs text-red-400";
        }
    } catch (e) {
        statusEl.innerText = "Key check failed: " + e;
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
    // 1. Check terminal session FIRST so the PIN lock screen appears and becomes interactive immediately!
    try {
        await checkExistingTerminalSession();
    } catch (e) {
        console.debug("Session check error:", e);
    }
    // 2. Run background loaders without blocking lock screen interactivity
    origInitApp().catch(e => console.error("origInitApp error:", e));
    checkAppVersion().catch(e => console.error("checkAppVersion error:", e));
    setTimeout(checkForUpdatesSilent, 3000);
    setInterval(checkForUpdatesSilent, 3600000);
};

// --- Terminal Role-Based Access Control (RBAC) & PIN Lock Screen ---

let currentTerminalSession = null;
let currentEnteredPin = "";

function updatePinDots() {
    // PINs are 4–8 digits (owner-issued): render as many dots as needed,
    // minimum 4 slots so the pad never looks broken when empty.
    const box = document.getElementById('pin-dots-box');
    if (!box) return;
    const slots = Math.max(4, Math.min(8, currentEnteredPin.length));
    while (box.children.length < slots) {
        const d = document.createElement('div');
        box.appendChild(d);
    }
    while (box.children.length > slots && box.children.length > 4) {
        const last = box.lastElementChild;
        if (last) last.remove();
    }
    for (let i = 0; i < box.children.length; i++) {
        const dot = box.children[i];
        if (i < currentEnteredPin.length) {
            dot.className = "w-4 h-4 rounded-full bg-purple-400 border-2 border-purple-300 shadow-md shadow-purple-500/50 transition-all scale-110";
        } else {
            dot.className = "w-4 h-4 rounded-full border-2 border-purple-400/60 transition-all";
        }
    }
}

function pressPinKey(digit) {
    const err = document.getElementById('pin-error-text');
    if (err) err.innerText = "";
    // Owner-issued PINs run 4–8 digits: accept up to 8 and submit explicitly
    // (arrow button / Enter). Auto-submitting at 4 would truncate longer PINs
    // into a guaranteed "Invalid PIN".
    if (currentEnteredPin.length < 8) {
        currentEnteredPin += digit;
        updatePinDots();
    }
}

function clearPin() {
    currentEnteredPin = "";
    updatePinDots();
    const err = document.getElementById('pin-error-text');
    if (err) err.innerText = "";
}

// Physical keyboard / Numpad listener for staff PIN pad
document.addEventListener('keydown', (e) => {
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (!lockScreen || lockScreen.classList.contains('hidden')) return;

    // Do not intercept if owner login modal is active (staff typing owner password)
    const ownerModal = document.getElementById('modal-owner-login');
    if (ownerModal && !ownerModal.classList.contains('hidden')) return;

    if (/^[0-9]$/.test(e.key)) {
        e.preventDefault();
        pressPinKey(e.key);
    } else if (e.key === 'Backspace') {
        e.preventDefault();
        if (currentEnteredPin.length > 0) {
            currentEnteredPin = currentEnteredPin.slice(0, -1);
            updatePinDots();
            const err = document.getElementById('pin-error-text');
            if (err) err.innerText = "";
        }
    } else if (e.key === 'Escape') {
        e.preventDefault();
        clearPin();
    } else if (e.key === 'Enter') {
        e.preventDefault();
        if (currentEnteredPin.length >= 4) {
            submitPinLogin();
        }
    }
});

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
    clearPin();
    showLockScreen();
}

function showLockScreen() {
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (lockScreen) {
        lockScreen.classList.remove('hidden');
        lockScreen.classList.add('flex');
    }
    // Header must never claim a session that doesn't exist (the static
    // markup used to read "Staff Active / Cashier Mode" while locked).
    const nameEl = document.getElementById('session-user-name');
    if (nameEl) nameEl.innerText = 'Locked';
    const roleEl = document.getElementById('session-user-role');
    if (roleEl) {
        roleEl.innerText = 'Locked Out';
        roleEl.className = 'text-[10px] uppercase font-mono font-bold text-slate-500';
    }
}

function unlockTerminalUI() {
    const lockScreen = document.getElementById('terminal-lock-screen');
    if (lockScreen) {
        lockScreen.classList.add('hidden');
        lockScreen.classList.remove('flex');
    }

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

    // POS catalog definition is owner-only (portal → sync). Cashiers sell and
    // restock; managers restock; only the owner sees Add/Edit/Delete.
    const isOwner = (role === 'owner');
    const addBtn = document.getElementById('btn-add-product');
    if (addBtn) addBtn.style.display = isOwner ? '' : 'none';
    if (typeof renderProductsGrid === 'function') renderProductsGrid();

    // If staff was on a restricted screen, switch to POS or Gate
    if (isStaff && (currentView === 'hardware' || currentView === 'branding')) {
        switchView('pos');
    }
}

async function checkExistingTerminalSession() {
    // Rust-side session is the single source of truth. There is deliberately
    // NO localStorage fallback: a persisted browser copy cannot recreate the
    // in-memory Rust session, so trusting it unlocked the UI while every
    // gated command still failed — the "logged in but nothing works" ghost.
    // Fresh start (or expired session) = lock screen, PIN required.
    try {
        const session = await invokeTauri('get_terminal_session');
        if (session && session.is_authenticated) {
            currentTerminalSession = session;
            unlockTerminalUI();
            return;
        }
    } catch (e) {
        // Backend unreachable in preview — stay locked.
    }

    // Default: Show Lock Screen
    showLockScreen();
}

document.addEventListener('DOMContentLoaded', initApp);


