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
    walk_in_rate: 10.0
};

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

    // Auto refresh every 8 seconds
    setInterval(async () => {
        if (currentView === 'dashboard') await refreshDashboard();
        if (currentView === 'attendance') await loadAttendanceLogs();
    }, 8000);
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
        if (settings) {
            appSettings = settings;
            applyBrandingToUI(settings);
        }
    } catch (e) {
        console.warn("Using default branding settings:", e);
        applyBrandingToUI(appSettings);
    }
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
    const views = ['dashboard', 'attendance', 'members', 'walkins', 'pos', 'coaches', 'branding', 'hardware'];
    const idx = views.indexOf(viewName);
    if (idx !== -1 && navItems[idx]) navItems[idx].classList.add('active');

    views.forEach(v => {
        const el = document.getElementById(`view-${v}`);
        if (el) el.classList.toggle('hidden', v !== viewName);
    });

    if (viewName === 'dashboard') refreshDashboard();
    if (viewName === 'members') loadMembers();
    if (viewName === 'walkins') loadWalkIns();
    if (viewName === 'attendance') loadAttendanceLogs();
    if (viewName === 'pos') loadProducts();
    if (viewName === 'coaches') loadCoaches();
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

        const licenseBadge = document.getElementById('license-tier-text');
        const licenseStateEl = document.getElementById('stat-license-state');
        const licenseDetailEl = document.getElementById('stat-license-detail');
        const status = summary.license_status;

        if (typeof status === 'object' && status.Valid) {
            const valid = status.Valid;
            licenseBadge.innerText = `Active (${valid.tier.toUpperCase()}) - ${valid.days_remaining}d left`;
            licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-emerald-950/60 border border-emerald-500/30 text-emerald-300";
            if (licenseStateEl) licenseStateEl.innerText = "ACTIVE";
            if (licenseDetailEl) licenseDetailEl.innerText = `${valid.gym_name} (${valid.days_remaining} days remaining)`;
        } else if (typeof status === 'object' && status.GracePeriod) {
            const grace = status.GracePeriod;
            licenseBadge.innerText = `GRACE PERIOD (${grace.grace_days_remaining}d left)`;
            licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-amber-950/60 border border-amber-500/40 text-amber-300 animate-pulse";
            if (licenseStateEl) licenseStateEl.innerText = "GRACE PERIOD";
            if (licenseDetailEl) licenseDetailEl.innerText = `Expired! ${grace.grace_days_remaining} days before lockout`;
        } else if (typeof status === 'object' && status.Expired) {
            licenseBadge.innerText = "LOCKED OUT (EXPIRED)";
            licenseBadge.parentElement.className = "flex items-center gap-2 px-3 py-1 rounded-full text-xs font-semibold bg-red-950/80 border border-red-500/50 text-red-300";
            if (licenseStateEl) licenseStateEl.innerText = "LOCKED OUT";
            if (licenseDetailEl) licenseDetailEl.innerText = "Subscription expired. Please renew.";
        } else {
            licenseBadge.innerText = "UNLICENSED";
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
    for (let i = 0; i < 128; i++) {
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
    const tbody = document.getElementById('members-list-tbody');
    if (!tbody) return;

    const filtered = cachedMembers.filter(m => {
        const fullName = `${m.first_name} ${m.last_name}`.toLowerCase();
        const matchesSearch = !search || fullName.includes(search) || (m.phone && m.phone.toLowerCase().includes(search)) || m.id.toLowerCase().includes(search);
        const matchesTier = tier === 'all' || m.membership_type.toLowerCase() === tier;
        return matchesSearch && matchesTier;
    });

    if (filtered.length === 0) {
        tbody.innerHTML = '<tr><td colspan="6" class="p-4 text-center text-slate-500">No members matching search filter</td></tr>';
        return;
    }

    tbody.innerHTML = filtered.map(m => {
        const isSuspended = m.status === 'suspended';
        const isExpired = m.status === 'expired';
        let statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-emerald-950 text-emerald-400 border border-emerald-800 font-semibold">ACTIVE</span>`;
        if (isSuspended) {
            statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-amber-950 text-amber-300 border border-amber-800 font-semibold">SUSPENDED</span>`;
        } else if (isExpired) {
            statusBadge = `<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-semibold">EXPIRED</span>`;
        }

        return `
            <tr class="hover:bg-slate-800/30 transition ${isSuspended || isExpired ? 'opacity-70' : ''}">
                <td class="p-3 font-mono text-blue-300">${m.id}</td>
                <td class="p-3">
                    <div class="font-semibold text-slate-200">${m.first_name} ${m.last_name}</div>
                    <div class="text-[10px] text-slate-500">${m.email || '--'}</div>
                </td>
                <td class="p-3 uppercase text-[11px] font-bold text-amber-300">${m.membership_type}</td>
                <td class="p-3 text-slate-400 font-mono">${m.phone || '--'}</td>
                <td class="p-3">${statusBadge}</td>
                <td class="p-3 text-right space-x-1.5">
                    <button onclick="openEditMemberModal('${m.id}')" title="Edit Profile" class="px-2.5 py-1 rounded bg-slate-800 hover:bg-slate-700 text-xs text-blue-300 border border-slate-700 font-medium transition">
                        Edit
                    </button>
                    <button onclick="deleteMember('${m.id}', '${m.first_name.replace(/'/g, "\\'")} ${m.last_name.replace(/'/g, "\\'")}')" title="Delete Member" class="px-2.5 py-1 rounded bg-red-950/60 hover:bg-red-900 text-xs text-red-300 border border-red-800/50 font-medium transition">
                        Delete
                    </button>
                </td>
            </tr>
        `;
    }).join('');
}

function openEnrollModal() {
    document.getElementById('enroll-modal').classList.remove('hidden');
}

function closeEnrollModal() {
    document.getElementById('enroll-modal').classList.add('hidden');
}

async function submitEnrollMember() {
    const firstName = document.getElementById('mem-first-name').value.trim();
    const lastName = document.getElementById('mem-last-name').value.trim();
    const phone = document.getElementById('mem-phone').value.trim();
    const plan = document.getElementById('mem-plan').value;
    const errorEl = document.getElementById('enroll-error-msg');

    if (!firstName || !lastName) {
        errorEl.innerText = "First and last name are required";
        return;
    }

    const baseSeed = (firstName + lastName).split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
    const vectors = [0, 1, 2].map(angle => {
        const vec = [];
        for (let i = 0; i < 128; i++) {
            vec.push(Math.sin(baseSeed + i + angle * 10));
        }
        return vec;
    });

    try {
        errorEl.innerText = "Registering member and storing facial vectors...";
        errorEl.className = "text-xs text-blue-300";

        await invokeTauri('register_member', {
            req: {
                first_name: firstName,
                last_name: lastName,
                email: `${firstName.toLowerCase()}@gym.local`,
                phone: phone,
                membership_type: plan,
                face_vectors: vectors
            }
        });

        closeEnrollModal();
        await loadMembers();
        await refreshDashboard();
        alert(`Member ${firstName} ${lastName} enrolled with 3-angle biometrics!`);
    } catch (e) {
        errorEl.innerText = "Enrollment Error: " + e;
        errorEl.className = "text-xs text-red-400";
    }
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

    try {
        await invokeTauri('update_member', {
            req: {
                id: id,
                first_name: firstName,
                last_name: lastName,
                phone: phone,
                email: email,
                membership_type: plan,
                status: status
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
    container.innerHTML = cart.map((item, idx) => {
        const itemTotal = item.unit_price * item.quantity;
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

    if (totalEl) totalEl.innerText = `$${total.toFixed(2)}`;
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

    try {
        const tx = await invokeTauri('checkout_pos_sale', {
            memberId: null,
            items: cart,
            paymentMethod: paymentMethod
        });

        alert(`Sale Processed!\nTransaction ID: ${tx.id}\nTotal: $${tx.total_amount.toFixed(2)}\nPayment: ${paymentMethod.toUpperCase()}`);
        cart = [];
        renderCart();
        await loadProducts();
    } catch (e) {
        alert("Checkout Failed: " + e);
    }
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
            const dirColor = l.direction === 'in' ? 'text-emerald-400 bg-emerald-950 border-emerald-800' : 'text-blue-400 bg-blue-950 border-blue-800';
            const timeFormatted = new Date(l.timestamp).toLocaleTimeString();

            return `
                <tr class="hover:bg-slate-800/30 transition ${isTailgate ? 'bg-red-950/20' : ''}">
                    <td class="p-3 font-mono text-blue-300">${l.id}</td>
                    <td class="p-3 font-semibold text-slate-200">${l.member_name || 'Unidentified Person'}</td>
                    <td class="p-3">
                        <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] border ${dirColor} uppercase font-bold">
                            ${l.direction === 'in' 
                                ? '<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 16l-4-4m0 0l4-4m-4 4h14m-5 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h7a3 3 0 013 3v1"></path></svg>'
                                : '<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17 16l4-4m0 0l-4-4m4 4H7m6 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h4a3 3 0 013 3v1"></path></svg>'}
                            <span>${l.direction}</span>
                        </span>
                    </td>
                    <td class="p-3 text-slate-400">${l.confidence ? (l.confidence * 100).toFixed(1) + '%' : '--'}</td>
                    <td class="p-3">
                        ${isTailgate 
                            ? '<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] bg-red-950 text-red-400 border border-red-800 font-bold animate-pulse"><svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"></path></svg><span>TAILGATE FLAG</span></span>'
                            : '<span class="text-slate-500 text-[10px]">Normal</span>'}
                    </td>
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
        for (let i = 0; i < 128; i++) probe.push(Math.sin(seed + i));
    }

    try {
        const result = await invokeTauri('process_face_scan', {
            probeVector: probe,
            direction: direction
        });

        if (result.matched) {
            let msg = `Face Verified (${direction.toUpperCase()}): ${result.member_name}`;
            if (result.remaining_minutes !== undefined && result.remaining_minutes !== null) {
                const h = Math.floor(result.remaining_minutes / 60);
                const m = result.remaining_minutes % 60;
                msg += ` [8h Pass: ${h}h ${m}m remaining]`;
            }
            msg += ` — Magnetic Lock Unlocked!`;
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
    for (let i = 0; i < 128; i++) probe.push(Math.sin(seed + i));

    try {
        const result = await invokeTauri('process_face_scan', {
            probeVector: probe,
            direction: direction
        });

        if (result.matched) {
            let msg = `Walk-In Scan (${direction.toUpperCase()}): ${result.member_name}`;
            if (result.remaining_minutes !== undefined && result.remaining_minutes !== null) {
                const h = Math.floor(result.remaining_minutes / 60);
                const m = result.remaining_minutes % 60;
                msg += ` (${h}h ${m}m remaining)`;
            }
            msg += ` — Gate Unlocked!`;
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
    try {
        btn.classList.add('opacity-50');
        await invokeTauri('unlock_magnetic_lock', { durationMs: 3000 });
        alert("Magnetic Lock Triggered: Door Unlocked for 3 seconds");
    } catch (e) {
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

document.addEventListener('DOMContentLoaded', initApp);
