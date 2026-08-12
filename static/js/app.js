/* Solo Leveling Gym — Shared JS */

// RFID Keyboard Listener (USB HID readers type characters like a keyboard)
(function() {
    let rfidBuffer = '';
    let rfidTimer = null;
    let lastScanTime = 0;

    document.addEventListener('keydown', function(e) {
        // Skip if user is typing in an input
        const tag = document.activeElement.tagName;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;

        if (e.key === 'Enter' && rfidBuffer.length >= 4) {
            // Rate-limit: ignore repeated scans within 2 seconds
            const now = Date.now();
            if (now - lastScanTime < 2000) { rfidBuffer = ''; return; }
            lastScanTime = now;
            handleRfidScan(rfidBuffer.toUpperCase());
            rfidBuffer = '';
            return;
        }

        if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) {
            rfidBuffer += e.key;
            clearTimeout(rfidTimer);
            rfidTimer = setTimeout(function() { rfidBuffer = ''; }, 200);
        }
    });

    function handleRfidScan(uid) {
        console.log('[RFID] Scanned:', uid);

        // Check page-level suppress flag (set by individual pages that handle
        // RFID themselves and don't want the global door-trigger to fire)
        if (window._rfidSuppressDoor === true) {
            // Try to fill any RFID input field on the page
            var input = document.getElementById('walkinRfid')
                     || document.querySelector('input[name="rfid_uid"]')
                     || document.querySelector('input.rfid-capture');
            if (input) {
                input.value = uid;
                input.style.transition = 'border-color .3s';
                input.style.borderColor = '#22c55e';
                setTimeout(function() { input.style.borderColor = ''; }, 1500);
            }
            return;
        }

        // URL-based check as secondary defence
        var path = window.location.pathname;
        if (path === '/sales/walkin' || path === '/sales/walkin/') {
            var wi = document.getElementById('walkinRfid');
            if (wi) {
                wi.value = uid;
                wi.style.transition = 'border-color .3s';
                wi.style.borderColor = '#22c55e';
                setTimeout(function() { wi.style.borderColor = ''; }, 1500);
            }
            return;
        }

        // Post to RFID scan API (all other pages — triggers door)
        const fd = new FormData();
        fd.append('uid', uid);
        fetch('/api/rfid-scan', { method: 'POST', body: fd })
            .then(r => r.json())
            .then(data => {
                console.log('[RFID] Response:', data);
                if (typeof refreshFeed === 'function') refreshFeed();
                if (typeof showToast === 'function') {
                    if (data.status === 'ok') {
                        showToast(data.message || 'Access granted', 'unlock');
                    } else if (data.status === 'denied' || data.status === 'expired') {
                        showToast(data.message || 'Access denied', 'deny');
                    } else if (data.status === 'already_in') {
                        showToast(data.message || 'Already checked in', 'warn');
                    } else if (data.status === 'unknown') {
                        showToast(data.message || 'Unknown badge', 'warn');
                    }
                }
            })
            .catch(err => console.error('[RFID] Error:', err));
    }
})();
