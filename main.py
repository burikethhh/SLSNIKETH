"""
GymPOS - Source-mode entry point
Solo Leveling Gym - Standalone Edition

Bootstraps the compiled FastAPI application (main.pyc) then registers
all extension routes on top of it.

Default credentials are generated randomly on first DB seed — check logs.
Change immediately via Admin → Maintenance.
"""
import importlib.util, sys, os, logging, secrets
from datetime import datetime, timezone
_os = os   # alias used inside closures throughout this module

logger = logging.getLogger("gympos.extensions")

# -- Secret key guard (C2) — refuse weak default --
_WEAK_SECRETS = {"change-me-to-a-random-secret-key-in-production", "change-me-generate-with-python-c-secrets-token-urlsafe-32", ""}
_env_secret = os.environ.get("SECRET_KEY", "")
if _env_secret in _WEAK_SECRETS:
    logger.warning("SECRET_KEY not set or weak — generating ephemeral key (set SECRET_KEY in .env for persistence)")
    _env_secret = secrets.token_urlsafe(32)
    os.environ["SECRET_KEY"] = _env_secret
    # also try to inject into compiled config module if already loaded
    try:
        import config as _cfg
        if getattr(_cfg, "settings", None) and getattr(_cfg.settings, "secret_key", "") in _WEAK_SECRETS:
            _cfg.settings.secret_key = _env_secret
    except Exception:
        pass

# -- Philippine Standard Time helpers (UTC+8) --
try:
    from tzlocal import get_localzone as _get_localzone
    _LOCAL_TZ = _get_localzone()
except Exception:
    import zoneinfo
    _LOCAL_TZ = zoneinfo.ZoneInfo("Asia/Manila")

def _now_utc() -> datetime:
    """Return current Philippine Standard Time (UTC+8) as a naive datetime.
    Named _now_utc for backward compatibility — all timestamps are stored
    in PHT so that SQLite date() comparisons and date.today() agree.
    Previously stored UTC; DB was migrated +8h on 2026-05-12.
    """
    return datetime.now(_LOCAL_TZ).replace(tzinfo=None)

def _log_activity(db, staff_id: int, action: str, target_type: str,
                   target_id: int, details: str) -> None:
    """Log activity with PHT timestamp (overrides compiled version)."""
    try:
        db.execute(text(
            "INSERT INTO staff_activities "
            "(staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,:act,:tt,:tid,:det,:ts)"
        ), {"sid": staff_id, "act": action, "tt": target_type,
            "tid": target_id, "det": details, "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

def _to_local(dt) -> str:
    """Format a PHT-stored datetime for display (no conversion needed)."""
    if dt is None: return ""
    try:
        if isinstance(dt, str): dt = datetime.fromisoformat(dt)
        # Timestamps are stored as PHT — display as-is, no tz conversion
        if dt.tzinfo is not None:
            dt = dt.replace(tzinfo=None)
        return dt.strftime("%Y-%m-%d %H:%M:%S")
    except Exception: return str(dt)

def _to_local_time(dt) -> str:
    """Format a PHT-stored time for display."""
    if dt is None: return ""
    try:
        if isinstance(dt, str): dt = datetime.fromisoformat(dt)
        if dt.tzinfo is not None:
            dt = dt.replace(tzinfo=None)
        return dt.strftime("%H:%M")
    except Exception: return str(dt)

def _to_local_date(dt) -> str:
    """Format a PHT-stored date for display."""
    if dt is None: return ""
    try:
        if isinstance(dt, str): dt = datetime.fromisoformat(dt)
        if dt.tzinfo is not None:
            dt = dt.replace(tzinfo=None)
        return dt.strftime("%Y-%m-%d")
    except Exception: return str(dt)

# -- Inject filters into EVERY Jinja2 Environment created hereafter --
import jinja2.defaults as _jd

# photo_url: handles both relative and absolute Windows paths from compiled routes
def _photo_url_early(path):
    if not path: return ""
    if path.startswith("/") or path.startswith("http"): return path
    p = path.replace("\\", "/")
    # Strip common absolute prefixes — project_root not yet defined here,
    # so use a portable check for drive-letter paths like C:/...
    if len(p) > 2 and p[1] == ':':
        # Find 'static/' in the absolute path and return from there
        idx = p.find('/static/')
        if idx >= 0:
            return p[idx:]   # → /static/photos/...
        return "/" + p.lstrip("/")
    return "/" + p.lstrip("/")

def _get_student_id(member_id) -> str:
    """Extract discount ID number or voucher code for the profile modal."""
    if not member_id: return ""
    try:
        from database.connection import SessionLocal as _SL
        from sqlalchemy import text as _tx
        _db = _SL()
        import re
        row = _db.execute(
            _tx("SELECT discount_type, notes, voucher_code, discount_id_number FROM members WHERE id=:id"),
            {"id": member_id}
        ).fetchone()
        _db.close()
        if not row:
            return ""
        dt, notes, vc, din = row[0] or "", row[1] or "", row[2] or "", row[3] or ""
        if dt == "voucher" and vc:
            return vc
        if din:
            return din
        if notes:
            m = re.search(r"(Student|Senior|PWD)ID:(\S+)", str(notes))
            if m:
                return m.group(2)
    except Exception:
        pass
    return ""

_jd.DEFAULT_FILTERS["to_local"]      = _to_local
_jd.DEFAULT_FILTERS["to_local_time"] = _to_local_time
_jd.DEFAULT_FILTERS["to_local_date"] = _to_local_date
_jd.DEFAULT_FILTERS["photo_url"]     = _photo_url_early
_jd.DEFAULT_FILTERS["student_id"]    = _get_student_id
logger.info("Injected to_local + photo_url filters into jinja2.defaults.DEFAULT_FILTERS")

# -- Bootstrap --
project_root = os.path.dirname(os.path.abspath(__file__))
if project_root not in sys.path:
    sys.path.insert(0, project_root)

# Load main.pyc as "gympos_main" (avoids __main__ guard)
spec = importlib.util.spec_from_file_location(
    "gympos_main", os.path.join(project_root, "main.pyc"),
    submodule_search_locations=[],
)
mod = importlib.util.module_from_spec(spec)
mod.__name__ = "gympos_main"
sys.modules["gympos_main"] = mod
spec.loader.exec_module(mod)

app = getattr(mod, "app", None)
if app is None:
    print("ERROR: Could not find 'app' in main.pyc", file=sys.stderr)
    sys.exit(1)

# -- Patch compiled SessionMiddleware secret if still weak --
try:
    import config as _cfg2
    _sk = getattr(getattr(_cfg2, "settings", None), "secret_key", "")
    if _sk in _WEAK_SECRETS and _env_secret not in _WEAK_SECRETS:
        _cfg2.settings.secret_key = _env_secret
        logger.info("Patched config.settings.secret_key from weak default")
except Exception:
    pass

# -- Monkey-patch compiled log_activity to use PHT instead of UTC --
try:
    import routers.admin as _ra
    import routers.members as _rm
    import routers.attendance as _rat
    import routers.coaches as _rc
    import routers.expenses as _rex
    import routers.auth as _rauth
    from database.models import StaffActivity as _StaffActivity
    # Replace the compiled log_activity reference in each router module
    # so future activity entries use _now_utc() (PHT) instead of datetime.utcnow
    for _mod in (_ra, _rm, _rat, _rc, _rex, _rauth):
        _mod.log_activity = _log_activity
    # Also override the model's ColumnDefault so direct StaffActivity() calls use PHT
    from sqlalchemy import ColumnDefault as _ColumnDefault
    _StaffActivity.__table__.columns['timestamp'].default = _ColumnDefault(_now_utc)
    logger.info("Patched StaffActivity timestamp default → PHT")
except Exception as _e:
    logger.warning("Could not patch log_activity: %s", _e)

# -- CSRF lightweight guard (C3) — Origin check for POST --
try:
    from starlette.responses import JSONResponse as _JResp
    @app.middleware("http")
    async def _csrf_guard(request, call_next):
        if request.method == "POST":
            path = request.url.path
            if path not in ("/login", "/admin/login"):
                origin = request.headers.get("origin") or request.headers.get("referer") or ""
                try:
                    from urllib.parse import urlparse
                    host = urlparse(origin).hostname or ""
                except Exception:
                    host = ""
                if host not in ("127.0.0.1", "localhost"):
                    return _JResp({"detail": "CSRF origin mismatch"}, status_code=403)
        return await call_next(request)
    logger.info("CSRF guard middleware added")
except Exception as _e:
    logger.warning("CSRF guard failed: %s", _e)

# -- License guard (multi-branch + 7d heartbeat + tamper) --
try:
    from license.validator import validate_license as _validate_lic, heartbeat_ok as _heartbeat_ok
    from starlette.responses import HTMLResponse as _HResp
    @app.middleware("http")
    async def _license_guard(request, call_next):
        # allow auth + license activation + static + health without license
        p = request.url.path
        if p.startswith("/static") or p in ("/login","/admin/login","/license/activate","/health","/docs","/openapi.json"):
            return await call_next(request)
        if p.startswith("/api/license"):
            return await call_next(request)
        try:
            res = _validate_lic(project_root, os.environ.get("LICENSE_PUBKEY",""))
            if res["status"] == "LOCKED":
                reason = res.get("reason","expired")
                html = f"<html><body style='background:#080b12;color:#c4cde0;font-family:Inter;padding:40px;text-align:center'><h1 style='color:#f87171'>GymPOS Locked</h1><p>License {reason} — contact CEO to renew. Grace expired.</p><p><a href='/license/activate' style='color:#7c3aed'>Activate License</a></p></body></html>"
                return _HResp(html, status_code=403)
            # 7-day heartbeat: refresh last_verify on every successful guard pass (cloud-online period)
            # Mirrors Tauri sync.rs heartbeat_ok() after sync_push 200 — keeps kiosk unlocked while online
            try:
                if res.get("claims") and os.environ.get("LICENSE_PUBKEY"):
                    _heartbeat_ok(project_root, res["claims"].get("gym_id",""))
            except Exception:
                pass
        except Exception as _le:
            logger.warning("license guard error %s — fail-closed", _le)
            return _HResp("<html><body style='background:#080b12;color:#f87171;padding:40px;text-align:center'><h1>License check failed</h1><p>Contact support.</p></body></html>", status_code=503)
        return await call_next(request)
    logger.info("License guard middleware added")
except Exception as _e:
    logger.warning("License guard failed: %s", _e)

# -- AuthZ guard (AAA) — IDOR + unauth APIs + per-gym —
try:
    @app.middleware("http")
    async def _authz_guard(request, call_next):
        p = request.url.path
        # unauth APIs -> require login
        if p.startswith("/api/esp32") or p in ("/api/rfid-scan","/api/live-feed","/api/hardware-status","/api/rfid-latest"):
            if not request.session.get("user_id"):
                from starlette.responses import JSONResponse as _JR
                return _JR({"detail":"auth required"}, status_code=401)
        # destructive DORs -> require admin
        if p.startswith("/members/") and p.endswith("/delete"):
            if request.session.get("role") != "admin":
                from starlette.responses import JSONResponse as _JR2
                return _JR2({"detail":"admin required"}, status_code=403)
        if p.startswith("/admin/staff/") and "/delete" in p:
            if request.session.get("role") != "admin":
                from starlette.responses import JSONResponse as _JR3
                return _JR3({"detail":"admin required"}, status_code=403)
        if p.startswith("/store/sales/") and p.endswith("/delete"):
            if request.session.get("role") != "admin":
                from starlette.responses import JSONResponse as _JR4
                return _JR4({"detail":"admin required"}, status_code=403)
        # GET /admin/* already requires admin via routers, but enforce for direct pyc bypass
        if p.startswith("/admin/") and p not in ("/admin/login",):
            if not request.session.get("user_id"):
                from starlette.responses import RedirectResponse as _RR
                return _RR("/admin/login", status_code=303)
        return await call_next(request)
    logger.info("AuthZ guard added")
except Exception as _e:
    logger.warning("AuthZ guard failed: %s", _e)

# -- Tier cap guard (Basic 200/Pro 500/Ultra 1000) — global dedup=1 --
try:
    from license.gates import can_register as _can_reg
    @app.middleware("http")
    async def _tier_guard(request, call_next):
        pth = request.url.path
        if request.method == "POST" and any(s in pth for s in ("/members/register", "/register/step", "/walkins", "/sales/walkin")):
            try:
                from license.validator import _db_path as _tdb
                import sqlite3 as _sl2
                _dbp = _tdb(project_root)
                _c = _sl2.connect(_dbp, timeout=5)
                _r = _c.execute("SELECT owner_email, max_members FROM cloud_licenses LIMIT 1").fetchone()
                _c.close()
                if _r and _r[1]:
                    owner, maxm = _r[0] or "", int(_r[1])
                    res = _can_reg(project_root, maxm, owner)
                    if not res.get("allowed"):
                        from starlette.responses import JSONResponse as _JR
                        # allow renewals (path contains renew) but block new
                        if "renew" not in pth:
                            return _JR({"detail": f"Tier cap {res.get('count')}/{maxm} — upgrade required"}, status_code=403)
            except Exception as _te:
                pass
        return await call_next(request)
    logger.info("Tier guard added")
except Exception as _e2:
    logger.warning("Tier guard failed: %s", _e2)

# -- Monkey-patch ManualOverride timestamp to use PHT instead of UTC --
try:
    from database.models import ManualOverride as _ManualOverride
    from sqlalchemy import ColumnDefault as _ColumnDefault
    _ManualOverride.__table__.columns['timestamp'].default = _ColumnDefault(_now_utc)
    logger.info("Patched ManualOverride timestamp default \u2192 PHT")
except Exception as _e:
    logger.warning("Could not patch ManualOverride timestamp: %s", _e)

# -- Add discount_type/voucher_code as non-mapped class attributes on Member so compiled routes don't AttributeError --
from database.models import Member as _Member
_Member.discount_type = None
_Member.voucher_code = None
logger.info("Added discount_type/voucher_code defaults to Member model")

# -- Patch discount_type/voucher_code on every Member ORM load --
from sqlalchemy import event as _member_event, text as _member_text

@_member_event.listens_for(_Member, 'load')
def _patch_member_extra_fields(target, context):
    """After ORM load, set discount_type/voucher_code/discount_id_number from DB (not in compiled model)."""
    try:
        sess = getattr(context, 'session', None)
        if sess is None:
            return
        row = sess.execute(
            _member_text("SELECT discount_type, voucher_code, discount_id_number FROM members WHERE id = :id"),
            {"id": target.id}
        ).fetchone()
        if row:
            target.discount_type = row[0]
            target.voucher_code = row[1]
            target.discount_id_number = row[2]
    except Exception:
        pass

# ── Add discount_id_number column + migrate from notes ──
try:
    import sqlite3 as _sq_mig
    _db_path_mig = os.path.join(project_root, "gym.db")
    _conn_mig = _sq_mig.connect(_db_path_mig)
    _conn_mig.execute("PRAGMA journal_mode=WAL")
    cols_mig = [r[1] for r in _conn_mig.execute("PRAGMA table_info(members)").fetchall()]
    if "discount_id_number" not in cols_mig:
        _conn_mig.execute("ALTER TABLE members ADD COLUMN discount_id_number VARCHAR(128)")
        import re as _re_mig
        rows_mig = _conn_mig.execute(
            "SELECT id, notes FROM members WHERE notes LIKE '%ID:%'"
        ).fetchall()
        migrated_count = 0
        for mid_mig, notes_mig in rows_mig:
            m_mig = _re_mig.search(r"(Student|Senior|PWD)ID:(\S+)", notes_mig or "")
            if m_mig:
                _conn_mig.execute(
                    "UPDATE members SET discount_id_number=? WHERE id=?",
                    (m_mig.group(2), mid_mig)
                )
                migrated_count += 1
        _conn_mig.commit()
        logger.info("Added discount_id_number column, migrated %d records from notes", migrated_count)
    _conn_mig.close()
except Exception as e:
    logger.warning("discount_id_number migration: %s", e)

# -- Monkey-patch Sale.created_at to use PHT instead of UTC --
try:
    from database.models import Sale as _Sale
    from sqlalchemy import ColumnDefault as _ColumnDefault
    _Sale.__table__.columns['created_at'].default = _ColumnDefault(_now_utc)
    logger.info("Patched Sale.created_at default \u2192 PHT")
except Exception as _e:
    logger.warning("Could not patch Sale.created_at default: %s", _e)

# -- Monkey-patch Member, Attendance, Expense, SecurityIncident defaults to PHT --
for _patch_def in [
    ("Member", "created_at"),
    ("Attendance", "timestamp"),
    ("Expense", "created_at"),
    ("SecurityIncident", "timestamp"),
]:
    try:
        _mod = getattr(__import__("database.models", fromlist=[_patch_def[0]]), _patch_def[0])
        from sqlalchemy import ColumnDefault as _CD
        _mod.__table__.columns[_patch_def[1]].default = _CD(_now_utc)
        logger.info("Patched %s.%s default \u2192 PHT", _patch_def[0], _patch_def[1])
    except Exception as _e:
        logger.warning("Could not patch %s.%s: %s", _patch_def[0], _patch_def[1], _e)


# -- Monkey-patch _utc_iso in access_control to return PHT ISO --
try:
    import services.access_control as _sac
    def _pht_iso():
        """Return current PHT as ISO string (replaces _utc_iso)."""
        return _now_utc().isoformat()[:19]
    _sac._utc_iso = _pht_iso
    logger.info("Patched access_control._utc_iso -> PHT (was UTC)")
except Exception as _e:
    logger.warning("Could not patch _utc_iso: %s", _e)

# -- Startup camera-assignment fix --
import threading as _threading

def _force_camera_indices():
    """
    Runs 8 s after startup. Corrects cam1/cam2 indices when the compiled
    _auto_detect_cameras assigns both to the same device (index 0).

    Critical ordering:
      1. Stop both streams first — each thread cleans up _ACTIVE_INDICES
         for whatever index it currently holds.
      2. THEN change camera_index — so the cleanup key matches the index
         that was actually registered.
      3. Explicitly clear any stale _ACTIVE_INDICES entries.
      4. Restart cam1 first, wait, then cam2.
    """
    import time as _time
    _time.sleep(8)
    try:
        cam1_idx = int(os.environ.get("CAM1_INDEX", "1"))
        cam2_idx = int(os.environ.get("CAM2_INDEX", "0"))
        cam3_idx = int(os.environ.get("CAM3_INDEX", "2"))
        from services.access_control import access_control as _ac
        if not (hasattr(_ac, "cam1") and hasattr(_ac, "cam2")):
            return
        has_cam3 = hasattr(_ac, "cam3")

        old1 = _ac.cam1.camera_index
        old2 = _ac.cam2.camera_index
        old3 = _ac.cam3.camera_index if has_cam3 else None

        if old1 == cam1_idx and old2 == cam2_idx and (not has_cam3 or old3 == cam3_idx):
            logger.info("Camera indices already correct: cam1=%d cam2=%d%s", cam1_idx, cam2_idx, f" cam3={cam3_idx}" if has_cam3 else "")
            return

        logger.info("Correcting camera indices: cam1 %d→%d  cam2 %d→%d%s",
                    old1, cam1_idx, old2, cam2_idx, f" cam3 {old3}→{cam3_idx}" if has_cam3 else "")

        # Step 1: stop both streams (threads clean up _ACTIVE_INDICES
        # using the current camera_index value — do NOT change index yet)
        _ac.cam1.stop()
        _ac.cam2.stop()
        if has_cam3:
            _ac.cam3.stop()
        _time.sleep(3)   # let thread teardowns complete

        # Step 2: clear any stale _ACTIVE_INDICES entries
        try:
            from services.camera_manager import _ACTIVE_INDICES, _ACTIVE_LOCK
            with _ACTIVE_LOCK:
                _ACTIVE_INDICES.clear()
            logger.info("_ACTIVE_INDICES cleared")
        except Exception as e:
            logger.debug("Could not clear _ACTIVE_INDICES: %s", e)

        # Step 3: reset failure counters so cameras start with clean backoff state,
        # then set the new indices
        _ac.cam1._consecutive_failures = 0
        _ac.cam2._consecutive_failures = 0
        _ac.cam1.camera_index = cam1_idx
        _ac.cam2.camera_index = cam2_idx
        if has_cam3:
            _ac.cam3._consecutive_failures = 0
            _ac.cam3.camera_index = cam3_idx

        # Step 4: restart sequentially so DShow COM is not hit simultaneously
        _ac.cam1.start()
        _time.sleep(4)   # give cam1 time to claim its index
        _ac.cam2.start()
        if has_cam3:
            _time.sleep(4)
            _ac.cam3.start()

        logger.info("Camera restart complete: cam1=%d cam2=%d%s", cam1_idx, cam2_idx, f" cam3={cam3_idx}" if has_cam3 else "")

    except Exception as e:
        logger.warning("startup camera fix failed: %s", e)

_threading.Thread(target=_force_camera_indices, daemon=True,
                  name="camera-index-fix").start()


# ══════════════════════════════════════════════════════════════════
# ACCESS CONTROL STATE MACHINE PATCH
#
# Enforced flow:
#   Face scan  → ENTRY only
#   RFID scan  → EXIT only
#   Must face-scan IN before RFID exit is allowed
#   Must RFID OUT before next face-scan IN is allowed
#   5-second pause after RFID OUT before face scanner re-arms
# ══════════════════════════════════════════════════════════════════

import time as _time_module
import types as _types

# Shared re-arm delay tracker — keyed by person key
# When RFID OUT is processed, we set _face_rearm_until[key] = now + 5
# The face loops and _AttendanceGateDict check this before allowing entry.
# Defined at module level so all closures and route handlers can access it.
_face_rearm_until: dict = {}


def _robust_attendance_insert(db_path: str, member_id: int = None,
                               staff_id: int = None, familiar_id: int = None,
                               direction: str = "IN", method: str = "FACE") -> bool:
    """
    Insert an attendance record. Always inserts — caller handles dedup via cooldown.
    """
    import sqlite3 as _sq4
    import time as _t4
    ts = _now_utc().isoformat()
    max_retries = 5

    if member_id is not None:
        fk_col, fk_val = "member_id", member_id
    elif staff_id is not None:
        fk_col, fk_val = "staff_id", staff_id
    elif familiar_id is not None:
        fk_col, fk_val = "familiar_id", familiar_id
    else:
        logger.warning("Attendance insert: no person ID provided")
        return False

    # hardened: include gym_id + visitor cols + retry
    try:
        current_gym = "default"
        owner = ""
        try:
            import sqlite3 as _sq_tmp
            _c = _sq_tmp.connect(db_path, timeout=5)
            _r = _c.execute("SELECT gym_id, owner_email FROM cloud_licenses LIMIT 1").fetchone()
            if _r:
                current_gym, owner = _r[0] or "default", _r[1] or ""
            _c.close()
        except Exception:
            pass
        # visitor check
        is_vis = 0
        home_gym = ""
        home_owner = ""
        if member_id is not None:
            try:
                import sqlite3 as _sq2
                _c2 = _sq2.connect(db_path, timeout=5)
                _mr = _c2.execute("SELECT gym_id, owner_email FROM members WHERE id=?", (member_id,)).fetchone()
                if _mr:
                    home_gym, home_owner = _mr[0] or "", _mr[1] or ""
                    if home_gym and home_gym != current_gym:
                        is_vis = 1
                _c2.close()
            except Exception:
                pass
    except Exception:
        current_gym, is_vis, home_gym, home_owner, owner = "default", 0, "", "", ""

    for attempt in range(max_retries):
        conn = None
        try:
            conn = _sq4.connect(db_path, timeout=10.0)
            conn.execute("PRAGMA journal_mode=WAL")
            conn.execute(
                f"INSERT INTO attendance ({fk_col}, direction, method, timestamp, gym_id, is_interbranch, visitor_home_gym_id, visitor_home_owner) VALUES (?,?,?,?,?,?,?,?)",
                (fk_val, direction, method, ts, current_gym, is_vis, home_gym, home_owner)
            )
            conn.commit()
            conn.close()
            return True
        except _sq4.IntegrityError as e:
            logger.debug("Attendance insert blocked: %s", e)
            try:
                if conn: conn.close()
            except Exception:
                pass
            return True
        except _sq4.OperationalError as e:
            err_msg = str(e).lower()
            if ("locked" in err_msg or "busy" in err_msg) and attempt < max_retries - 1:
                _t4.sleep(0.2 * (attempt + 1))
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                continue
            logger.warning("Attendance insert DB error (attempt %d): %s", attempt + 1, e)
            try:
                if conn: conn.close()
            except Exception:
                pass
        except Exception as e:
            logger.warning("Attendance insert error (attempt %d): %s", attempt + 1, e)
            try:
                if conn: conn.close()
            except Exception:
                pass
            break
    return False


def _ensure_attendance_logged(member_id: int, db_path: str) -> bool:
    """
    Log an attendance IN record for this member.
    Always inserts — the gate's cooldown mechanism prevents rapid-fire duplicates.
    """
    import sqlite3 as _sq
    import time as _t
    ts = _now_utc().isoformat()
    max_retries = 5
    # inter-branch gym
    try:
        import sqlite3 as _sq_tmp3
        _c3 = _sq_tmp3.connect(db_path, timeout=5)
        _r3 = _c3.execute("SELECT gym_id FROM cloud_licenses LIMIT 1").fetchone()
        _cg3 = _r3[0] if _r3 else "default"
        _mr3 = _c3.execute("SELECT gym_id, owner_email FROM members WHERE id=?", (member_id,)).fetchone()
        _hg3, _ho3 = (_mr3[0] or "", _mr3[1] or "") if _mr3 else ("", "")
        _is_vis3 = 1 if _hg3 and _hg3 != _cg3 else 0
        _c3.close()
    except Exception:
        _cg3, _hg3, _ho3, _is_vis3 = "default", "", "", 0
    for attempt in range(max_retries):
        conn = None
        try:
            conn = _sq.connect(db_path, timeout=10.0)
            conn.execute("PRAGMA journal_mode=WAL")
            conn.execute(
                "INSERT INTO attendance (member_id, direction, method, timestamp, gym_id, is_interbranch, visitor_home_gym_id, visitor_home_owner) VALUES (?, 'IN', 'FACE', ?, ?, ?, ?, ?)",
                (member_id, ts, _cg3, _is_vis3, _hg3, _ho3)
            )
            conn.commit()
            conn.close()
            logger.info("Attendance IN logged for member %d (ts=%s)", member_id, ts)
            return True
        except _sq.IntegrityError as e:
            logger.info("Attendance insert blocked for member %d: %s", member_id, e)
            try:
                if conn: conn.close()
            except Exception:
                pass
            return True
        except _sq.OperationalError as e:
            err_msg = str(e).lower()
            if ("locked" in err_msg or "busy" in err_msg) and attempt < max_retries - 1:
                _t.sleep(0.2 * (attempt + 1))
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                continue
            logger.warning("Attendance log DB error (attempt %d): %s", attempt + 1, e)
            try:
                if conn: conn.close()
            except Exception:
                pass
        except Exception as e:
            logger.warning("Attendance log error (attempt %d): %s", attempt + 1, e)
            try:
                if conn: conn.close()
            except Exception:
                pass
            break
    return False


def _patch_access_control():
    try:
        from services.access_control import access_control as _ac

        # Set tailgate monitoring window to 7 s.
        _ac.TAILGATE_MONITOR_SECONDS = 7.0
        _ac.__class__.TAILGATE_MONITOR_SECONDS = 7.0
        logger.info("TAILGATE_MONITOR_SECONDS = 7.0s")

        # ── Attendance-driven _face_cooldown gate ────────────────────
        # The compiled _entry_loop writes _face_cooldown[member_id] = now
        # after every face match (on every camera frame ≈ 10x/sec).
        # This _AttendanceGateDict intercepts those writes and decides:
        #   • Last attendance today = FACE-IN  → block (now+86400)
        #     person is inside, compiled loop must not re-fire
        #   • Last attendance today = RFID-OUT → re-arm (now+5)
        #     person exited, face scan re-arms after 5s grace period
        #   • No attendance today              → re-arm (0.0)
        #     clean slate, allow first entry
        # This makes the compiled loop's cooldown 100% attendance-driven,
        # exactly matching the RFID handler's own cycle logic.
        if hasattr(_ac, '_face_cooldown'):
            _db_path = os.path.join(project_root, "gym.db")

            class _FaceCooldownGate(dict):
                """
                Logs IN on every face match. Uses compiled cooldown to
                prevent rapid-fire duplicates (5s window).
                """
                def __getitem__(self, key):
                    return super().__getitem__(key)

                def get(self, key, default=0.0):
                    return super().get(key, default)

                def __contains__(self, key):
                    return super().__contains__(key)

                def __setitem__(self, member_id, expiry):
                    now = _time_module.time()
                    logger.info("=== GATE SETITEM: member_id=%s, expiry=%.1f, now=%.1f ===", member_id, expiry, now)

                    # 5s re-arm grace period after RFID OUT
                    rearm_deadline = _face_rearm_until.get(member_id, 0.0)
                    if now < rearm_deadline:
                        logger.info("Gate: member %d in REARM GRACE (%.1fs remaining)", member_id, rearm_deadline - now)
                        super().__setitem__(member_id, rearm_deadline)
                        return

                    # Log attendance IN — direct sqlite3, bypasses SQLAlchemy
                    logger.info("Gate: member %d logging attendance IN", member_id)
                    try:
                        result = _ensure_attendance_logged(member_id, _db_path)
                        logger.info("Gate: member %d attendance log result: %s", member_id, "OK" if result else "FAILED")
                    except Exception as e:
                        logger.warning("Gate attendance log failed for member %d: %s", member_id, e)

                    # Store compiled cooldown to prevent rapid-fire matches
                    super().__setitem__(member_id, expiry)
                    logger.info("Gate: member %d cooldown set to %.1f", member_id, expiry)

                def pop(self, key, *default):
                    return super().pop(key, *default)

            ag = _FaceCooldownGate()
            _ac._face_cooldown = ag
            logger.info("Face cooldown gate installed (logs IN on every face match)")

        # UNLOCK rate-limiter: 1 UNLOCK per 2.5s (suppress liveness burst)
        try:
            from services import serial_bridge as _sb
            _orig_send = _sb.serial_bridge.send_command
            _ul_lock = _threading.Lock()
            _last_unlock = [0.0]

            def _guarded_send(cmd: str, **kw):
                if isinstance(cmd, str) and cmd.upper().startswith("UNLOCK"):
                    with _ul_lock:
                        now_t = _time_module.time()
                        if now_t - _last_unlock[0] < 2.5:
                            return "RATE_LIMITED"
                        _last_unlock[0] = now_t
                return _orig_send(cmd, **kw)

            _sb.serial_bridge.send_command = _guarded_send
            logger.info("UNLOCK rate-limiter applied (1 per 2.5s)")
        except Exception as e:
            logger.warning("UNLOCK rate-limiter failed: %s", e)

    except Exception as e:
        logger.warning("access_control patch failed: %s", e)

_patch_access_control()


def _patch_face_service_logging():
    """
    Patch face_service.match_all_faces to log attendance IN directly
    when a face match is detected. This bypasses the compiled unlock logic
    and ensures every face match logs an attendance record.
    """
    try:
        _db_path = os.path.join(project_root, "gym.db")

        # Get face_service from the _entry_loop's globals (where it's actually used)
        from services.access_control import access_control as _ac
        entry_loop = getattr(_ac, '_entry_loop', None)
        face_service = None

        if entry_loop and hasattr(entry_loop, '__globals__'):
            face_service = entry_loop.__globals__.get('face_service')
            logger.info("Face service from _entry_loop globals: %s", face_service)

        if face_service is None:
            from services import access_control as _ac_module
            face_service = getattr(_ac_module, 'face_service', None)
            logger.info("Face service from access_control module: %s", face_service)

        if face_service and hasattr(face_service, 'match_all_faces'):
            _orig_match = face_service.match_all_faces
            import functools

            @functools.wraps(_orig_match)
            def _patched_match(*args, **kwargs):
                results = _orig_match(*args, **kwargs)
                # Log attendance for each matched member
                if results:
                    for match in results:
                        member_id = getattr(match, 'id', None) or getattr(match, 'member_id', None)
                        if member_id:
                            try:
                                _ensure_attendance_logged(member_id, _db_path)
                                logger.info("Face match attendance: member %d IN logged", member_id)
                            except Exception as e:
                                logger.warning("Face match attendance log failed for member %d: %s", member_id, e)
                return results

            face_service.match_all_faces = _patched_match
            logger.info("Face match attendance logging patch installed")
            return True
        else:
            logger.warning("Could not find face_service.match_all_faces to patch")
            return False
    except Exception as e:
        logger.warning("Face service logging patch failed: %s", e)
        return False


# Try immediate patch, and also schedule for startup in case face_service isn't ready yet
if not _patch_face_service_logging():
    @app.on_event("startup")
    async def _retry_face_service_patch():
        import asyncio
        await asyncio.sleep(2)
        _patch_face_service_logging()


def _patch_attendance_logging():
    """
    Patch the compiled _entry_loop to use robust attendance logging.
    The compiled code uses SQLAlchemy ORM which can silently fail on DB locks.
    This patch intercepts the db.commit() call and ensures the attendance
    record is written using direct sqlite3 with retry logic.
    """
    try:
        from services.access_control import access_control as _ac
        _db_path = os.path.join(project_root, "gym.db")

        # Store original _push_event for reference
        _orig_push = getattr(_ac, '_push_event', None)

        def _robust_push_event(event: dict):
            """Intercept unlock events and ensure attendance is logged."""
            logger.debug("_push_event intercepted: type=%s direction=%s member_id=%s",
                         event.get("type"), event.get("direction"), event.get("member_id"))
            if event.get("type") == "unlock" and event.get("direction") == "IN":
                member_id = event.get("member_id")
                if member_id:
                    try:
                        result = _ensure_attendance_logged(member_id, _db_path)
                        logger.info("Attendance log for member %d: %s", member_id, "OK" if result else "FAILED")
                    except Exception as e:
                        logger.warning("Robust attendance log failed: %s", e)
            if _orig_push:
                return _orig_push(event)

        _ac._push_event = _robust_push_event
        logger.info("Robust attendance logging patch installed on _push_event")
    except Exception as e:
        logger.warning("Attendance logging patch failed: %s", e)


_patch_attendance_logging()


def _arm_tailgate(window_s: float = 7.0) -> None:
    """
    Extend the tailgate monitoring window to `window_s` seconds from now.
    Called on every door-open event (face scan IN and RFID exit) so the
    overhead camera watches for tailgaters during the door-open period.
    7 seconds matches the door unlock duration — enough to catch anyone
    sneaking in behind the authorised person.
    """
    try:
        import time as _tm
        from services.access_control import access_control as _ac
        new_deadline = _tm.monotonic() + window_s
        if new_deadline > _ac._tailgate_armed_until:
            _ac._tailgate_armed_until = new_deadline
    except Exception:
        pass


# ── Module-level face-vector helpers ─────────────────────────────
# Defined here (not inside a closure) so they can be shared between
# _start_staff_familiar_face_loop AND the /api/face-detect override.

def _cosine_sim(a, b) -> float:
    """Cosine similarity between two numpy float32 vectors."""
    import numpy as _np_cs
    norm = float(_np_cs.linalg.norm(a)) * float(_np_cs.linalg.norm(b))
    if norm < 1e-9:
        return 0.0
    return float(_np_cs.dot(a, b) / norm)


def _unpack_face_vector(blob: bytes):
    """
    Unpack a pickled face_vector blob from the DB.
    encode_multi_image_files stores pickle.dumps({'vector': ndarray, ...}).
    Falls back to a raw pickled ndarray for legacy blobs.
    Returns a float32 ndarray or None.
    """
    if not blob:
        return None
    try:
        import pickle as _pk
        import numpy as _np_uv
        data = _pk.loads(blob)
        if isinstance(data, dict):
            vec = data.get("vector")
            if vec is None:
                return None
            return _np_uv.array(vec, dtype=_np_uv.float32).flatten()
        return _np_uv.array(data, dtype=_np_uv.float32).flatten()
    except Exception:
        return None


def _face_detect_build_sf_roster(db_path: str) -> dict:
    """
    Build a staff+familiar face roster on-demand for the /api/face-detect
    override.  Returns {person_key: (name, vec, type_str, db_id, photo_path)}.
    Same key-space as _start_staff_familiar_face_loop:
      staff   →  10000 + staff_id
      familiar → -familiar_id
    """
    import sqlite3 as _sq3fd
    roster: dict = {}
    try:
        conn = _sq3fd.connect(db_path)
        for sid, uname, dname, fv, sphoto in conn.execute(
            "SELECT id, username, display_name, face_vector, photo_path FROM staff "
            "WHERE face_vector IS NOT NULL AND is_active=1 AND role != 'admin'"
        ).fetchall():
            vec = _unpack_face_vector(fv)
            if vec is not None and len(vec) == 128:
                roster[10000 + sid] = (dname or uname, vec, "staff", sid, sphoto or "")

        for fid, fname, fv, fphoto in conn.execute(
            "SELECT id, name, face_vector, photo_path FROM familiars "
            "WHERE face_vector IS NOT NULL AND is_active=1"
        ).fetchall():
            vec = _unpack_face_vector(fv)
            if vec is not None and len(vec) == 128:
                roster[-fid] = (fname, vec, "familiar", fid, fphoto or "")
        conn.close()
    except Exception as _e:
        logger.warning("_face_detect_build_sf_roster error: %s", _e)
    return roster


# ══════════════════════════════════════════════════════════════════
# PARALLEL FACE RECOGNITION LOOP — Staff & Familiars
#
# The compiled _entry_loop handles MEMBERS only (queries members table).
# This thread handles STAFF (non-admin) and FAMILIARS using the same
# cam1 feed, same _face_cooldown state machine, and same UNLOCK rate
# limiter as members.
#
# Key space in _face_cooldown:
#   members   →  member_id          (1 … N)
#   familiars →  -fam_id            (-1 … -N)
#   staff     →  10000 + staff_id   (10001 … 10N)
#
# Face matching:
#   1. Get latest cam1 frame
#   2. Save to temp .jpg
#   3. face_recognition_ml.extract_face_vector(path) → 128-d SFace vector
#   4. Cosine similarity vs staff+familiar roster (threshold = 0.55)
#   5. Liveness gate: 2 consecutive matched frames with gap < 1.0s
#   6. If match & cooldown expired → UNLOCK + attendance IN + 86400s lock
#      If match & cooldown active  → LCD "Wait Ns"
# ══════════════════════════════════════════════════════════════════

# ══════════════════════════════════════════════════════════════════
# ENHANCED ANTI-TAILGATE — YOLO-based parallel monitor
#
# The compiled tailgate loop uses MOG2 background subtraction which
# has three critical weaknesses:
#  1. history=300 frames → slow-moving people get LEARNED as background
#     and become invisible to blob detection
#  2. MIN_BLOB_AREA=3000 px² at 320×180 → two people close together
#     merge into one large blob, counting as 1 instead of 2
#  3. varThreshold=40 → too insensitive to catch subtle/slow entries
#
# This thread runs YOLO person detection (person_counter ONNX) which:
#  • Detects ACTUAL PEOPLE regardless of movement speed
#  • Honours the detection zone polygon set in the admin dashboard
#  • Can distinguish 2 people even when adjacent
#  • Not fooled by lighting changes, shadows, or reflections
#
# Parameters tuned for overhead camera (heads + shoulders only):
#  PERSON_THRESHOLD  = 2       # ≥2 heads/shoulders in zone simultaneously = tailgate
#  CONFIRM_FRAMES    = 3       # must see ≥2 people in 3 consecutive frames
#                              # (3 frames @ 8FPS = ~0.36s — robust for overhead)
#  MONITOR_WINDOW_S  = 7.0     # watch for 7 s after door opens (door unlock = 5 s)
#  ALERT_COOLDOWN_S  = 5.0     # minimum gap between back-to-back alarms
#  POLL_INTERVAL_S   = 0.12    # ~8 FPS — heads move fast overhead, need coverage
#  SMOOTHING         = enabled # person_counter uses a 5-frame median window
# ══════════════════════════════════════════════════════════════════

def _start_enhanced_tailgate():
    """Enhanced YOLO anti-tailgate daemon thread — overhead camera edition.

    Camera orientation: cam3 is mounted overhead, looking straight down at the
    entrance. It sees the top of people's heads and their shoulders — NOT full
    bodies. All tuning below accounts for this:

      - POLL_INTERVAL_S = 0.12  (~8 FPS) — heads move quickly overhead; more
        frames per second means we catch the brief window when both people are
        simultaneously in the zone.
      - CONFIRM_FRAMES  = 3     — require 3 consecutive frames with ≥2 heads
        seen, to avoid false positives from one noisy overhead frame.
      - MONITOR_WINDOW_S = 7.0  — door unlock is 5 s; 7 s gives 2 s of buffer
        after the door closes to catch anyone slipping through late.
      - ALERT_COOLDOWN_S = 5.0  — 5 s minimum between back-to-back alarms.

    Reinforced ROI enforcement:
      Before passing each frame to YOLO, every pixel OUTSIDE the configured
      cam2 detection-zone polygon is blacked out using cv2.fillPoly + bitwise_and.
      This is a hard boundary — YOLO literally cannot see anything outside the
      zone, so exterior detections (people through glass, in hallways, etc.) are
      physically impossible to count.

      The polygon (cam2_roi.json, percentage coordinates) is reloaded from disk
      every ROI_RELOAD_S seconds so zone changes in the admin dashboard take
      effect within 30 s without restarting the app.

      The compiled person_counter.pyc's own point-in-polygon check still runs as
      a secondary redundant filter on the already-masked frame.
    """
    _PERSON_THRESHOLD = 2
    _CONFIRM_FRAMES   = 3      # 3 consecutive frames — robust for overhead/partial view
    _MONITOR_WINDOW_S = 7.0    # 7 s watch window after door opens
    _ALERT_COOLDOWN_S = 5.0    # min gap between back-to-back alarms
    _POLL_INTERVAL_S  = 0.12   # ~8 FPS — fast enough to catch heads passing through
    _ROI_RELOAD_S     = 30.0   # re-read cam3_roi.json every 30 s

    # ── ROI pixel-mask helpers ────────────────────────────────────
    def _load_roi_points(roi_path):
        """Read cam3_roi.json → list of {x,y} % dicts, or None on failure."""
        import json as _json
        try:
            with open(roi_path, "r") as _f:
                pts = _json.load(_f)
            if isinstance(pts, list) and len(pts) >= 3:
                return pts
        except Exception:
            pass
        return None

    def _build_pixel_mask(frame_h, frame_w, roi_pts):
        """
        Convert percentage roi_pts to pixel coordinates, then paint a white
        filled polygon on a black canvas.  Returns a 3-channel uint8 mask
        the same size as the frame — white inside the zone, black outside.
        Using a 3-channel mask lets cv2.bitwise_and work directly on BGR frames
        without needing to convert or repeat the mask across channels.
        """
        import cv2 as _cv2m
        import numpy as _npm
        pixel_pts = _npm.array(
            [[int(p["x"] / 100.0 * frame_w),
              int(p["y"] / 100.0 * frame_h)]
             for p in roi_pts],
            dtype=_npm.int32,
        )
        mask = _npm.zeros((frame_h, frame_w), dtype=_npm.uint8)
        _cv2m.fillPoly(mask, [pixel_pts], 255)
        return mask

    def _loop():
        import time as _tm
        import os as _osroi

        from services.person_counter    import person_counter as _pc
        from services.access_control   import access_control as _ac
        from services.serial_bridge    import serial_bridge as _sb
        from services.camera_context   import CameraContext
        from services.camera_context   import camera_context as _ctx

        # Ensure the ONNX model is loaded — PersonCounter.__init__ does NOT
        # auto-load; load() must be called explicitly before available=True.
        if not _pc.available:
            try:
                _pc.load()
                logger.info("PersonCounter loaded: backend=%s", _pc._backend)
            except Exception as _le:
                logger.warning("PersonCounter load() failed: %s", _le)

        last_alert_at  = 0.0
        frames_above   = 0     # consecutive frames with ≥ threshold people

        # ROI pixel-mask state
        roi_path        = _osroi.path.join(
                              _osroi.path.dirname(_osroi.path.abspath(__file__)),
                              "cam3_roi.json")
        roi_pts         = None   # list of {x,y} % dicts
        roi_mask        = None   # numpy uint8 pixel mask (h,w,3)
        roi_mask_shape  = None   # (h, w) the mask was built for
        roi_last_reload = 0.0    # monotonic timestamp of last disk read

        logger.info(
            "Enhanced YOLO tailgate started "
            "(threshold=%d persons, window=%.1fs, cooldown=%.1fs, ONNX=%s)",
            _PERSON_THRESHOLD, _MONITOR_WINDOW_S,
            _ALERT_COOLDOWN_S, _pc._backend or "not loaded"
        )

        while True:
            _time_module.sleep(_POLL_INTERVAL_S)

            # Only run during the monitoring window
            if _tm.monotonic() >= _ac._tailgate_armed_until:
                frames_above = 0
                continue

            # Skip while camera is busy (registration etc.)
            if _ctx.current != CameraContext.IDLE:
                frames_above = 0
                continue

            # Need cam3 running (tailgate overhead)
            tail_cam = getattr(_ac, "cam3", getattr(_ac, "cam2", None))
            if tail_cam is None or tail_cam.status != "running":
                continue

            frame = tail_cam.get_latest_frame()
            if frame is None:
                continue

            # ── Reload ROI polygon from disk every 30 s ──────────
            now_mono = _tm.monotonic()
            if now_mono - roi_last_reload > _ROI_RELOAD_S:
                new_pts = _load_roi_points(roi_path)
                if new_pts != roi_pts:
                    roi_pts  = new_pts
                    roi_mask = None   # force rebuild on next frame
                    logger.info(
                        "Tailgate ROI reloaded: %s",
                        ("%d-point polygon" % len(roi_pts)) if roi_pts else "none (full frame)",
                    )
                roi_last_reload = now_mono

            # ── Apply hard pixel mask (black out outside zone) ────
            # This is the primary enforcement layer: YOLO receives a frame
            # where every pixel outside the polygon is exactly 0 (black).
            # No detection of persons outside the zone is physically possible.
            if roi_pts:
                try:
                    import cv2 as _cv2roi
                    fh, fw = frame.shape[:2]
                    if roi_mask is None or roi_mask_shape != (fh, fw):
                        roi_mask       = _build_pixel_mask(fh, fw, roi_pts)
                        roi_mask_shape = (fh, fw)
                        logger.debug(
                            "Tailgate pixel mask built: %dx%d, %d vertices",
                            fw, fh, len(roi_pts)
                        )
                    frame = _cv2roi.bitwise_and(frame, frame, mask=roi_mask)
                except Exception as _roi_err:
                    logger.debug("ROI mask error (skipping): %s", _roi_err)
            else:
                logger.debug("Tailgate ROI not set — processing full frame")

            # ── YOLO person count ─────────────────────────────────
            # person_counter's own point-in-polygon check also runs here
            # as a secondary filter on the already-masked frame.
            try:
                if not _pc.available:
                    continue
                count = _pc.count_persons_smoothed(frame)
            except Exception as e:
                logger.debug("YOLO count error: %s", e)
                continue

            logger.debug("Tailgate YOLO: %d person(s) in zone (armed=%.1fs left)",
                         count,
                         max(0.0, _ac._tailgate_armed_until - _tm.monotonic()))

            if count >= _PERSON_THRESHOLD:
                frames_above += 1
            else:
                frames_above = 0
                continue

            # Wait for confirmation across consecutive frames
            if frames_above < _CONFIRM_FRAMES:
                continue

            # Cooldown gate
            now = _time_module.time()
            if now - last_alert_at < _ALERT_COOLDOWN_S:
                continue

            # ── FIRE TAILGATE ALERT ─────────────────────────────────
            last_alert_at = now
            frames_above  = 0
            _pc.reset_window()   # flush smoothing so next window is fresh

            logger.warning(
                "TAILGATE ALERT (YOLO): %d persons detected in entrance zone", count
            )

            # ESP32: 10-second rapid buzzer alarm + LCD alert icon
            _sb.send_command("ALERT_TAILGATE")

            # Capture cam2 snapshot for the incident report
            snap_path = None
            try:
                import cv2 as _cv2_snap
                ts_snap = _now_utc().strftime('%Y%m%d_%H%M%S')
                clips_dir = _osroi.path.join(
                    _osroi.path.dirname(_osroi.path.abspath(__file__)),
                    "static", "clips")
                _osroi.makedirs(clips_dir, exist_ok=True)
                snap_name = f"tailgate_{ts_snap}_{count}p.jpg"
                snap_full = _osroi.path.join(clips_dir, snap_name)
                _cv2_snap.imwrite(snap_full, frame)
                snap_path = f"static/clips/{snap_name}"
                logger.info("Tailgate snapshot saved: %s", snap_path)
            except Exception as _snap_err:
                logger.debug("Tailgate snapshot error: %s", _snap_err)

            # Push to live feed / dashboard
            try:
                _ac._push_event({
                    "type":    "incident",
                    "message": f"Tailgate alert — {count} persons detected at entrance",
                    "alert":   "red",
                    "time":    _to_local(_now_utc()),
                })
            except Exception:
                pass

            # Log security incident to DB with snapshot
            try:
                from database.connection import SessionLocal as _SL
                from database.models import SecurityIncident as _SI
                _db = _SL()
                try:
                    _db.add(_SI(
                        incident_type="tailgate_attempt",
                        description=f"YOLO detected {count} persons simultaneously in entrance zone",
                        person_count=count,
                        face_count=0,
                        photo_path=snap_path,
                    ))
                    _db.commit()
                finally:
                    _db.close()
            except Exception as e:
                logger.debug("Security incident log error: %s", e)

    t = _threading.Thread(target=_loop, daemon=True, name="enhanced-tailgate")
    t.start()
    logger.info("Enhanced YOLO tailgate thread started")

_start_enhanced_tailgate()

# ── 30-day clip prune ────────────────────────────────────────────────
def _start_clip_prune():
    def _loop():
        import time as _t
        while True:
            _t.sleep(3600)  # check hourly
            try:
                clips_dir = os.path.join(project_root, "static", "clips")
                if not os.path.isdir(clips_dir):
                    continue
                cutoff = _t.time() - 30 * 24 * 3600
                for fn in os.listdir(clips_dir):
                    fp = os.path.join(clips_dir, fn)
                    try:
                        if os.path.isfile(fp) and os.path.getmtime(fp) < cutoff:
                            os.remove(fp)
                            logger.info("Pruned old clip %s", fn)
                    except Exception:
                        pass
            except Exception as e:
                logger.debug("clip prune error %s", e)
    _threading.Thread(target=_loop, daemon=True, name="clip-prune").start()

_start_clip_prune()


def _start_staff_familiar_face_loop():
    """Spawn the parallel face-recognition daemon for staff, familiars AND members.
    All person types use the attendance-table cycle: FACE-IN → RFID-OUT → FACE-IN.
    The compiled _entry_loop is blocked for all members via _face_cooldown=MAX
    so this loop is the sole handler for member face recognition.
    """
    import pickle, tempfile
    import numpy as _np

    _STAFF_KEY_OFFSET  = 10000     # staff_id 1 → key 10001
    _THRESHOLD         = 0.65      # SFace cosine similarity minimum
    _FPS               = 8         # 8 frames/sec (was 3, increased for responsiveness)
    _LIVENESS_FRAMES   = 2         # consecutive frames needed before entry
    _LIVENESS_GAP_S    = 1.5       # max gap (s) between liveness frames
    _ROSTER_TTL_S      = 30.0      # rebuild roster every 30 seconds (signature-based, only when DB changes)

    _cosine_sim_fn    = _cosine_sim
    _unpack_vector_fn = _unpack_face_vector

    # Persistent temp file for face extraction (avoids create/delete overhead every frame)
    _persistent_tmp_fd, _persistent_tmp_path = tempfile.mkstemp(suffix=".jpg", prefix="sf_face_persistent_", dir=tempfile.gettempdir())
    os.close(_persistent_tmp_fd)

    # Roster signature cache — only rebuild when DB actually changes
    _roster_sig = None

    def _get_roster_signature(db_path: str) -> str:
        """Compute a lightweight signature to detect DB changes."""
        import sqlite3 as _sq_sig
        try:
            conn = _sq_sig.connect(db_path)
            try:
                r1 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(updated_at),'') FROM members WHERE face_vector IS NOT NULL").fetchone()
            except Exception:
                r1 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0) FROM members WHERE face_vector IS NOT NULL").fetchone()
            r2 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(created_at),'') FROM staff WHERE face_vector IS NOT NULL AND is_active=1 AND role != 'admin'").fetchone()
            r3 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(created_at),'') FROM familiars WHERE face_vector IS NOT NULL AND is_active=1").fetchone()
            conn.close()
            return f"{r1}{r2}{r3}"
        except Exception:
            return None

    def _build_roster(db_path: str) -> dict:
        """
        Query staff, familiars AND regular members with face_vector IS NOT NULL.
        Key space:
          members   → member_id           (1..N, positive)
          staff     → 10000 + staff_id
          familiars → -familiar_id
        """
        import sqlite3 as _sq3
        roster: dict = {}
        try:
            conn = _sq3.connect(db_path)

            # Regular & student & senior members: active, has face_vector, not expired
            for row in conn.execute(
                "SELECT id, name, face_vector, photo_path FROM members "
                "WHERE face_vector IS NOT NULL AND status='active' "
                "AND member_type IN ('regular', 'student', 'senior') "
                "AND (expiry_date IS NULL OR expiry_date >= date('now'))"
            ).fetchall():
                mid, mname, fv, mphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    roster[mid] = (mname or "Member", vec, "member", mid, mphoto or "")

            # Staff: role != 'admin', is_active=1, has face_vector
            for row in conn.execute(
                "SELECT id, username, display_name, face_vector, photo_path FROM staff "
                "WHERE face_vector IS NOT NULL AND is_active=1 AND role != 'admin'"
            ).fetchall():
                sid, uname, dname, fv, sphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    key = _STAFF_KEY_OFFSET + sid
                    roster[key] = (dname or uname, vec, "staff", sid, sphoto or "")

            # Familiars: is_active=1, has face_vector
            for row in conn.execute(
                "SELECT id, name, face_vector, photo_path FROM familiars "
                "WHERE face_vector IS NOT NULL AND is_active=1"
            ).fetchall():
                fid, fname, fv, fphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    key = -fid
                    roster[key] = (fname, vec, "familiar", fid, fphoto or "")

            conn.close()
        except Exception as e:
            logger.warning("Roster build error: %s", e)
        if roster:
            logger.info("Face roster: %d entries (%s)",
                        len(roster),
                        ", ".join(f"{v[0]}({v[2]})" for v in roster.values()))
        return roster

    def _log_attendance_in(person_key: int, person_type: str, db_id: int,
                           db_path: str):
        """Insert attendance IN record (member, staff, or familiar) with retry."""
        import sqlite3 as _sq3
        import time as _t3
        ts = _now_utc().isoformat()
        today = _now_utc().strftime("%Y-%m-%d")
        max_retries = 5
        for attempt in range(max_retries):
            conn = None
            try:
                conn = _sq3.connect(db_path, timeout=10.0)
                conn.execute("PRAGMA journal_mode=WAL")
                # Check for existing IN today
                if person_type == "member":
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE member_id=? AND direction='IN' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    # gym_id for branch isolation
                    try:
                        _cg = conn.execute("SELECT gym_id FROM cloud_licenses LIMIT 1").fetchone()
                        _cgv2 = _cg[0] if _cg else "default"
                    except Exception:
                        _cgv2 = "default"
                    conn.execute(
                        "INSERT INTO attendance "
                        "(member_id, direction, method, timestamp, gym_id) VALUES (?,?,?,?,?)",
                        (db_id, "IN", "FACE", ts, _cgv2)
                    )
                elif person_type == "staff":
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE staff_id=? AND direction='IN' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    conn.execute(
                        "INSERT INTO attendance "
                        "(staff_id, direction, method, timestamp) VALUES (?,?,?,?)",
                        (db_id, "IN", "FACE", ts)
                    )
                else:  # familiar
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE familiar_id=? AND direction='IN' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    conn.execute(
                        "INSERT INTO attendance "
                        "(familiar_id, direction, method, timestamp) VALUES (?,?,?,?)",
                        (db_id, "IN", "FACE", ts)
                    )
                conn.commit()
                conn.close()
                return
            except _sq3.OperationalError as e:
                err_msg = str(e).lower()
                if ("locked" in err_msg or "busy" in err_msg) and attempt < max_retries - 1:
                    _t3.sleep(0.2 * (attempt + 1))
                    try:
                        if conn: conn.close()
                    except Exception:
                        pass
                    continue
                logger.warning("Attendance IN DB error (attempt %d): %s", attempt + 1, e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break
            except _sq3.IntegrityError as e:
                if "duplicate" in str(e).lower():
                    logger.debug("Attendance IN duplicate for %s id=%d", person_type, db_id)
                    try:
                        if conn: conn.close()
                    except Exception:
                        pass
                    return
                logger.warning("Attendance IN integrity error: %s", e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break
            except Exception as e:
                logger.warning("Attendance IN error (attempt %d): %s", attempt + 1, e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break


    def _loop():
        """The actual daemon loop."""
        import cv2 as _cv2
        from services.access_control import access_control as _ac
        from services.camera_context import CameraContext
        from services.camera_context import camera_context as _ctx
        from services.face_recognition_ml import face_recognition_ml as _frml
        from services.serial_bridge import serial_bridge as _sb

        # Resolve the database path once
        solo = _os.environ.get("SOLO_DATA_DIR", "").rstrip("/\\")
        db_path = (_os.path.join(solo, "gym.db") if solo
                   else _os.path.join(
                       _os.path.dirname(_os.path.abspath(__file__)), "gym.db"))

        roster: dict = {}
        roster_built_at: float = 0.0
        presence: dict = {}          # person_key → (count, last_seen_mono)
        last_seq: int = -1
        frame_interval = 1.0 / _FPS
        nonlocal _roster_sig

        logger.info("Staff+familiar face loop started (threshold=%.2f, liveness=%d)",
                    _THRESHOLD, _LIVENESS_FRAMES)

        while True:
            _time_module.sleep(frame_interval)

            # Skip when camera is borrowed by a registration page
            if _ctx.current != CameraContext.IDLE:
                continue

            # Rebuild roster periodically — signature-based, skip if DB unchanged
            now = _time_module.time()
            if now - roster_built_at > _ROSTER_TTL_S:
                new_sig = _get_roster_signature(db_path)
                if new_sig != _roster_sig:
                    roster = _build_roster(db_path)
                    roster_built_at = now
                    _roster_sig = new_sig
                    # Invalidate face recognition roster cache so new
                    # staff/familiar vectors are picked up by all matchers
                    try:
                        from services.face_recognition import face_service
                        face_service.invalidate_roster_cache()
                    except Exception:
                        pass

            if not roster:
                continue

            # Get latest cam1 frame — non-destructive read
            seq, _ = _ac.cam1.get_latest_jpeg_seq()
            if seq == last_seq:
                continue
            last_seq = seq
            frame = _ac.cam1.get_latest_frame()
            if frame is None:
                continue

            # Extract live face vector via persistent temp file (avoids create/delete I/O) — thread-safe
            live_vec = None
            try:
                if not _cv2.imwrite(_persistent_tmp_path, frame):
                    continue
                with _FACE_ML_LOCK:
                    result = _frml.extract_face_vector(_persistent_tmp_path)
                if result is None:
                    continue
                # extract_face_vector returns EITHER:
                #   tuple: (ndarray, meta_dict)  — old/current API
                #   dict:  {"vector": ndarray, ...} — alternate format
                if isinstance(result, (tuple, list)):
                    live_vec = _np.array(result[0], dtype=_np.float32).flatten()
                elif isinstance(result, dict):
                    live_vec = _np.array(result["vector"], dtype=_np.float32).flatten()
                else:
                    live_vec = _np.array(result, dtype=_np.float32).flatten()
                if len(live_vec) != 128:
                    continue
            except Exception:
                continue

            # Vectorized cosine similarity against all roster entries at once
            now_mono = _time_module.monotonic()
            best_key, best_score = None, 0.0
            try:
                # Stack all stored vectors into a single matrix for batch comparison
                keys = list(roster.keys())
                stored_matrix = _np.array([roster[k][1] for k in keys], dtype=_np.float32)
                # Normalize live vector
                live_norm = _np.linalg.norm(live_vec)
                if live_norm > 0:
                    live_norm_vec = live_vec / live_norm
                else:
                    continue
                # Normalize all stored vectors
                stored_norms = _np.linalg.norm(stored_matrix, axis=1, keepdims=True)
                stored_norms[stored_norms == 0] = 1.0  # avoid division by zero
                stored_norm_matrix = stored_matrix / stored_norms
                # Batch cosine similarity: dot product of normalized vectors
                scores = stored_norm_matrix @ live_norm_vec
                # Find best match above threshold
                above_thresh = scores > _THRESHOLD
                if _np.any(above_thresh):
                    best_idx = _np.argmax(scores)
                    best_score = float(scores[best_idx])
                    best_key = keys[best_idx]
            except Exception:
                # Fallback to sequential comparison if vectorized fails
                for key, (name, stored_vec, ptype, db_id, _ph) in roster.items():
                    try:
                        score = _cosine_sim_fn(live_vec, stored_vec)
                        if score > _THRESHOLD and score > best_score:
                            best_score, best_key = score, key
                    except Exception:
                        continue

            # Prune stale presence entries for anyone not seen this frame
            for k in list(presence.keys()):
                cnt, last_mono = presence[k]
                if now_mono - last_mono > _LIVENESS_GAP_S:
                    presence.pop(k, None)

            if best_key is None:
                continue

            name, stored_vec, ptype, db_id, photo_path = roster[best_key]

            # Update liveness counter
            cnt, last_mono = presence.get(best_key, (0, now_mono))
            if now_mono - last_mono > _LIVENESS_GAP_S:
                cnt = 0   # gap too long — reset
            presence[best_key] = (cnt + 1, now_mono)

            # Liveness gate
            if presence[best_key][0] < _LIVENESS_FRAMES:
                logger.debug("Liveness: %s %d/%d frames",
                             name, presence[best_key][0], _LIVENESS_FRAMES)
                continue

            # ── Re-arm delay gate: block face scan for 5s after RFID OUT ──
            now_mono_check = _time_module.monotonic()
            rearm_deadline = _face_rearm_until.get(best_key, 0.0)
            if rearm_deadline > 0 and now_mono_check < rearm_deadline:
                remaining = rearm_deadline - now_mono_check
                logger.debug("Re-arm gate — %s blocked for %.1fs more (RFID exit grace)",
                             name, remaining)
                presence.pop(best_key, None)
                continue

            # ── Batch DB checks: cycle gate + expiry (single connection) ──
            _gate_skip = False
            try:
                import sqlite3 as _sq_batch
                _today = _now_utc().strftime("%Y-%m-%d")
                _conn = _sq_batch.connect(db_path)

                # Cycle gate: if last record today = IN, must RFID-OUT first
                if ptype == "member":
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE member_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()
                elif ptype == "staff":
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE staff_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()
                else:  # familiar
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE familiar_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()

                if _last:
                    _eff = _last[0].upper()
                    logger.debug("Cycle gate — %s (key=%s) last=%s/%s effective=%s",
                                 name, best_key, _last[0], _last[1], _eff)
                    if _eff == "IN":
                        first_fg = name.split()[0] if name else ptype.title()
                        try:
                            _sb.send_command(f"LCD:Face to exit|{first_fg}")
                        except Exception:
                            pass
                        logger.info("Face BLOCKED — %s already inside (last=%s/%s), need FACE-OUT",
                                    name, _last[0], _last[1])
                        presence.pop(best_key, None)
                        _gate_skip = True
                else:
                    logger.debug("Cycle gate — %s (key=%s) no attendance today, allowing entry",
                                 name, best_key)

                # Real-time expiry check (members only) — only if cycle gate passed
                if not _gate_skip and ptype == "member":
                    _ok_ex = _conn.execute(
                        "SELECT 1 FROM members "
                        "WHERE id=? AND status='active' "
                        "AND (expiry_date IS NULL OR expiry_date >= ?)",
                        (db_id, _today)
                    ).fetchone()
                    if not _ok_ex:
                        # Auto-update status in DB so UI shows correct status
                        _conn.execute(
                            "UPDATE members SET status='expired' WHERE id=? "
                            "AND status IN ('active', 'frozen')",
                            (db_id,)
                        )
                        _conn.commit()
                        logger.info("Face DENIED — member id=%s auto-marked expired", db_id)
                        first_ex = name.split()[0] if name else "Member"
                        try:
                            _sb.send_command("DENY:Membership Expired")
                            _sb.send_command("BEEP")
                            _sb.send_command(f"LCD:Expired|{first_ex}")
                        except Exception:
                            pass
                        logger.info("Face DENIED — %s membership expired", name)
                        presence.pop(best_key, None)
                        roster.pop(best_key, None)
                        _gate_skip = True

                _conn.close()
            except Exception as _batch_e:
                logger.warning("Batch DB check error for %s: %s", name, _batch_e)

            if _gate_skip:
                continue

            # ── GRANT ENTRY ─────────────────────────────────────────
            logger.info("Face GRANT — %s (%s) key=%s score=%.3f method=FACE",
                        name, ptype, best_key, best_score)
            ok = _sb.send_command("UNLOCK")
            first = name.split()[0] if name else ptype.title()
            _sb.send_command(f"LCD:Welcome!|{first}")

            # Log attendance
            _log_attendance_in(best_key, ptype, db_id, db_path)

            # Arm tailgate monitor via shared helper (7s window = door open duration)
            _arm_tailgate(7.0)

            # Push to live feed — use ptype ("staff" / "familiar") as event type
            # and include the real photo_path so the dashboard shows the person's photo.
            try:
                _ac._push_event({
                    "type":        ptype,       # "staff" or "familiar"
                    "message":     f"Welcome, {name}",
                    "member_name": name,
                    "photo":       photo_path,  # real photo from roster
                    "alert":       "green",
                    "direction":   "IN",
                    "method":      "FACE",
                    "time":        _to_local(_now_utc()),
                })
            except Exception:
                pass

            logger.info("Face entry: %s (%s) key=%s score=%.3f unlock=%s",
                        name, ptype, best_key, best_score, ok)

            # Reset presence so the same person doesn't immediately
            # trigger a second entry if they linger in the camera frame
            presence.pop(best_key, None)

    def _loop_with_cleanup():
        """Wrapper to ensure persistent temp file cleanup on exit."""
        try:
            _loop()
        finally:
            try:
                os.unlink(_persistent_tmp_path)
            except Exception:
                pass

    t = _threading.Thread(target=_loop_with_cleanup, daemon=True, name="sf-face-loop")
    t.start()
    logger.info("Staff+familiar parallel face loop thread started")

_start_staff_familiar_face_loop()

def _start_face_out_loop():
    """Spawn the parallel face-recognition daemon for staff, familiars AND members.
    All person types use the attendance-table cycle: FACE-IN → FACE-OUT → FACE-IN.
    The compiled _entry_loop is blocked for all members via _face_cooldown=MAX
    so this loop is the sole handler for member face recognition.
    """
    import pickle, tempfile
    import numpy as _np

    _STAFF_KEY_OFFSET  = 10000     # staff_id 1 → key 10001
    _THRESHOLD         = 0.65      # SFace cosine similarity minimum
    _FPS               = 8         # 8 frames/sec (was 3, increased for responsiveness)
    _LIVENESS_FRAMES   = 2         # consecutive frames needed before entry
    _LIVENESS_GAP_S    = 1.5       # max gap (s) between liveness frames
    _ROSTER_TTL_S      = 30.0      # rebuild roster every 30 seconds (signature-based, only when DB changes)

    _cosine_sim_fn    = _cosine_sim
    _unpack_vector_fn = _unpack_face_vector

    # Persistent temp file for face extraction (avoids create/delete overhead every frame)
    _persistent_tmp_fd, _persistent_tmp_path = tempfile.mkstemp(suffix=".jpg", prefix="sf_face_out_", dir=tempfile.gettempdir())
    os.close(_persistent_tmp_fd)

    # Roster signature cache — only rebuild when DB actually changes
    _roster_sig = None

    def _get_roster_signature(db_path: str) -> str:
        """Compute a lightweight signature to detect DB changes."""
        import sqlite3 as _sq_sig
        try:
            conn = _sq_sig.connect(db_path)
            try:
                r1 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(updated_at),'') FROM members WHERE face_vector IS NOT NULL").fetchone()
            except Exception:
                r1 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0) FROM members WHERE face_vector IS NOT NULL").fetchone()
            r2 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(created_at),'') FROM staff WHERE face_vector IS NOT NULL AND is_active=1 AND role != 'admin'").fetchone()
            r3 = conn.execute("SELECT COUNT(*), COALESCE(MAX(id),0), COALESCE(MAX(created_at),'') FROM familiars WHERE face_vector IS NOT NULL AND is_active=1").fetchone()
            conn.close()
            return f"{r1}{r2}{r3}"
        except Exception:
            return None

    def _build_roster(db_path: str) -> dict:
        """
        Query staff, familiars AND regular members with face_vector IS NOT NULL.
        Key space:
          members   → member_id           (1..N, positive)
          staff     → 10000 + staff_id
          familiars → -familiar_id
        """
        import sqlite3 as _sq3
        roster: dict = {}
        try:
            conn = _sq3.connect(db_path)

            # Regular & student & senior members: active, has face_vector, not expired
            for row in conn.execute(
                "SELECT id, name, face_vector, photo_path FROM members "
                "WHERE face_vector IS NOT NULL AND status='active' "
                "AND member_type IN ('regular', 'student', 'senior') "
                "AND (expiry_date IS NULL OR expiry_date >= date('now'))"
            ).fetchall():
                mid, mname, fv, mphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    roster[mid] = (mname or "Member", vec, "member", mid, mphoto or "")

            # Staff: role != 'admin', is_active=1, has face_vector
            for row in conn.execute(
                "SELECT id, username, display_name, face_vector, photo_path FROM staff "
                "WHERE face_vector IS NOT NULL AND is_active=1 AND role != 'admin'"
            ).fetchall():
                sid, uname, dname, fv, sphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    key = _STAFF_KEY_OFFSET + sid
                    roster[key] = (dname or uname, vec, "staff", sid, sphoto or "")

            # Familiars: is_active=1, has face_vector
            for row in conn.execute(
                "SELECT id, name, face_vector, photo_path FROM familiars "
                "WHERE face_vector IS NOT NULL AND is_active=1"
            ).fetchall():
                fid, fname, fv, fphoto = row
                vec = _unpack_vector_fn(fv)
                if vec is not None and len(vec) == 128:
                    key = -fid
                    roster[key] = (fname, vec, "familiar", fid, fphoto or "")

            conn.close()
        except Exception as e:
            logger.warning("Roster build error: %s", e)
        if roster:
            logger.info("Face roster: %d entries (%s)",
                        len(roster),
                        ", ".join(f"{v[0]}({v[2]})" for v in roster.values()))
        return roster

    def _log_attendance_in(person_key: int, person_type: str, db_id: int,
                           db_path: str):
        """Insert attendance IN record (member, staff, or familiar) with retry."""
        import sqlite3 as _sq3
        import time as _t3
        ts = _now_utc().isoformat()
        today = _now_utc().strftime("%Y-%m-%d")
        max_retries = 5
        for attempt in range(max_retries):
            conn = None
            try:
                conn = _sq3.connect(db_path, timeout=10.0)
                conn.execute("PRAGMA journal_mode=WAL")
                # Check for existing IN today
                if person_type == "member":
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE member_id=? AND direction='OUT' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    try:
                        _cg = conn.execute("SELECT gym_id FROM cloud_licenses LIMIT 1").fetchone()
                        _cgv2 = _cg[0] if _cg else "default"
                    except Exception:
                        _cgv2 = "default"
                    conn.execute(
                        "INSERT INTO attendance "
                        "(member_id, direction, method, timestamp, gym_id) VALUES (?,?,?,?,?)",
                        (db_id, "OUT", "FACE", ts, _cgv2)
                    )
                elif person_type == "staff":
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE staff_id=? AND direction='OUT' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    conn.execute(
                        "INSERT INTO attendance "
                        "(staff_id, direction, method, timestamp) VALUES (?,?,?,?)",
                        (db_id, "OUT", "FACE", ts)
                    )
                else:  # familiar
                    row = conn.execute(
                        "SELECT id FROM attendance WHERE familiar_id=? AND direction='OUT' AND date(timestamp)=? LIMIT 1",
                        (db_id, today)
                    ).fetchone()
                    if row:
                        conn.close()
                        return
                    conn.execute(
                        "INSERT INTO attendance "
                        "(familiar_id, direction, method, timestamp) VALUES (?,?,?,?)",
                        (db_id, "OUT", "FACE", ts)
                    )
                conn.commit()
                conn.close()
                return
            except _sq3.OperationalError as e:
                err_msg = str(e).lower()
                if ("locked" in err_msg or "busy" in err_msg) and attempt < max_retries - 1:
                    _t3.sleep(0.2 * (attempt + 1))
                    try:
                        if conn: conn.close()
                    except Exception:
                        pass
                    continue
                logger.warning("Attendance OUT DB error (attempt %d): %s", attempt + 1, e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break
            except _sq3.IntegrityError as e:
                if "duplicate" in str(e).lower():
                    logger.debug("Attendance OUT duplicate for %s id=%d", person_type, db_id)
                    try:
                        if conn: conn.close()
                    except Exception:
                        pass
                    return
                logger.warning("Attendance OUT integrity error: %s", e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break
            except Exception as e:
                logger.warning("Attendance OUT error (attempt %d): %s", attempt + 1, e)
                try:
                    if conn: conn.close()
                except Exception:
                    pass
                break


    def _loop():
        """The actual daemon loop."""
        import cv2 as _cv2
        from services.access_control import access_control as _ac
        from services.camera_context import CameraContext
        from services.camera_context import camera_context as _ctx
        from services.face_recognition_ml import face_recognition_ml as _frml
        from services.serial_bridge import serial_bridge as _sb

        # Resolve the database path once
        solo = _os.environ.get("SOLO_DATA_DIR", "").rstrip("/\\")
        db_path = (_os.path.join(solo, "gym.db") if solo
                   else _os.path.join(
                       _os.path.dirname(_os.path.abspath(__file__)), "gym.db"))

        roster: dict = {}
        roster_built_at: float = 0.0
        presence: dict = {}          # person_key → (count, last_seen_mono)
        last_seq: int = -1
        frame_interval = 1.0 / _FPS
        nonlocal _roster_sig

        logger.info("Staff+familiar face loop started (threshold=%.2f, liveness=%d)",
                    _THRESHOLD, _LIVENESS_FRAMES)

        while True:
            _time_module.sleep(frame_interval)

            # Skip when camera is borrowed by a registration page
            if _ctx.current != CameraContext.IDLE:
                continue

            # Rebuild roster periodically — signature-based, skip if DB unchanged
            now = _time_module.time()
            if now - roster_built_at > _ROSTER_TTL_S:
                new_sig = _get_roster_signature(db_path)
                if new_sig != _roster_sig:
                    roster = _build_roster(db_path)
                    roster_built_at = now
                    _roster_sig = new_sig
                    # Invalidate face recognition roster cache so new
                    # staff/familiar vectors are picked up by all matchers
                    try:
                        from services.face_recognition import face_service
                        face_service.invalidate_roster_cache()
                    except Exception:
                        pass

            if not roster:
                continue

            # Get latest cam1 frame — non-destructive read
            seq, _ = _ac.cam2.get_latest_jpeg_seq()
            if seq == last_seq:
                continue
            last_seq = seq
            frame = _ac.cam2.get_latest_frame()
            if frame is None:
                continue

            # Extract live face vector via persistent temp file (avoids create/delete I/O) — thread-safe
            live_vec = None
            try:
                if not _cv2.imwrite(_persistent_tmp_path, frame):
                    continue
                with _FACE_ML_LOCK:
                    result = _frml.extract_face_vector(_persistent_tmp_path)
                if result is None:
                    continue
                # extract_face_vector returns EITHER:
                #   tuple: (ndarray, meta_dict)  — old/current API
                #   dict:  {"vector": ndarray, ...} — alternate format
                if isinstance(result, (tuple, list)):
                    live_vec = _np.array(result[0], dtype=_np.float32).flatten()
                elif isinstance(result, dict):
                    live_vec = _np.array(result["vector"], dtype=_np.float32).flatten()
                else:
                    live_vec = _np.array(result, dtype=_np.float32).flatten()
                if len(live_vec) != 128:
                    continue
            except Exception:
                continue

            # Vectorized cosine similarity against all roster entries at once
            now_mono = _time_module.monotonic()
            best_key, best_score = None, 0.0
            try:
                # Stack all stored vectors into a single matrix for batch comparison
                keys = list(roster.keys())
                stored_matrix = _np.array([roster[k][1] for k in keys], dtype=_np.float32)
                # Normalize live vector
                live_norm = _np.linalg.norm(live_vec)
                if live_norm > 0:
                    live_norm_vec = live_vec / live_norm
                else:
                    continue
                # Normalize all stored vectors
                stored_norms = _np.linalg.norm(stored_matrix, axis=1, keepdims=True)
                stored_norms[stored_norms == 0] = 1.0  # avoid division by zero
                stored_norm_matrix = stored_matrix / stored_norms
                # Batch cosine similarity: dot product of normalized vectors
                scores = stored_norm_matrix @ live_norm_vec
                # Find best match above threshold
                above_thresh = scores > _THRESHOLD
                if _np.any(above_thresh):
                    best_idx = _np.argmax(scores)
                    best_score = float(scores[best_idx])
                    best_key = keys[best_idx]
            except Exception:
                # Fallback to sequential comparison if vectorized fails
                for key, (name, stored_vec, ptype, db_id, _ph) in roster.items():
                    try:
                        score = _cosine_sim_fn(live_vec, stored_vec)
                        if score > _THRESHOLD and score > best_score:
                            best_score, best_key = score, key
                    except Exception:
                        continue

            # Prune stale presence entries for anyone not seen this frame
            for k in list(presence.keys()):
                cnt, last_mono = presence[k]
                if now_mono - last_mono > _LIVENESS_GAP_S:
                    presence.pop(k, None)

            if best_key is None:
                continue

            name, stored_vec, ptype, db_id, photo_path = roster[best_key]

            # Update liveness counter
            cnt, last_mono = presence.get(best_key, (0, now_mono))
            if now_mono - last_mono > _LIVENESS_GAP_S:
                cnt = 0   # gap too long — reset
            presence[best_key] = (cnt + 1, now_mono)

            # Liveness gate
            if presence[best_key][0] < _LIVENESS_FRAMES:
                logger.debug("Liveness: %s %d/%d frames",
                             name, presence[best_key][0], _LIVENESS_FRAMES)
                continue

            # ── Re-arm delay gate: block face scan for 5s after RFID OUT ──
            now_mono_check = _time_module.monotonic()
            rearm_deadline = _face_rearm_until.get(best_key, 0.0)
            if rearm_deadline > 0 and now_mono_check < rearm_deadline:
                remaining = rearm_deadline - now_mono_check
                logger.debug("Re-arm gate — %s blocked for %.1fs more (RFID exit grace)",
                             name, remaining)
                presence.pop(best_key, None)
                continue

            # ── Batch DB checks: cycle gate + expiry (single connection) ──
            _gate_skip = False
            try:
                import sqlite3 as _sq_batch
                _today = _now_utc().strftime("%Y-%m-%d")
                _conn = _sq_batch.connect(db_path)

                # Cycle gate: if last record today = IN, must RFID-OUT first
                if ptype == "member":
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE member_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()
                elif ptype == "staff":
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE staff_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()
                else:  # familiar
                    _last = _conn.execute(
                        "SELECT direction, method FROM attendance "
                        "WHERE familiar_id=? AND date(timestamp)=? ORDER BY id DESC LIMIT 1",
                        (db_id, _today)
                    ).fetchone()

                if _last:
                    _eff = _last[0].upper()
                    logger.debug("Cycle gate — %s (key=%s) last=%s/%s effective=%s",
                                 name, best_key, _last[0], _last[1], _eff)
                    if _eff == "OUT":
                        first_fg = name.split()[0] if name else ptype.title()
                        try:
                            _sb.send_command(f"LCD:Already out|{first_fg}")
                        except Exception:
                            pass
                        logger.info("Face OUT BLOCKED — %s already outside (last=%s/%s)",
                                    name, _last[0], _last[1])
                        presence.pop(best_key, None)
                        _gate_skip = True
                else:
                    logger.info("Face OUT BLOCKED — %s no IN today, need FACE-IN", name)
                    presence.pop(best_key, None)
                    _gate_skip = True

                # Real-time expiry check (members only) — only if cycle gate passed
                if not _gate_skip and ptype == "member":
                    _ok_ex = _conn.execute(
                        "SELECT 1 FROM members "
                        "WHERE id=? AND status='active' "
                        "AND (expiry_date IS NULL OR expiry_date >= ?)",
                        (db_id, _today)
                    ).fetchone()
                    if not _ok_ex:
                        # Auto-update status in DB so UI shows correct status
                        _conn.execute(
                            "UPDATE members SET status='expired' WHERE id=? "
                            "AND status IN ('active', 'frozen')",
                            (db_id,)
                        )
                        _conn.commit()
                        logger.info("Face DENIED — member id=%s auto-marked expired", db_id)
                        first_ex = name.split()[0] if name else "Member"
                        try:
                            _sb.send_command("DENY:Membership Expired")
                            _sb.send_command("BEEP")
                            _sb.send_command(f"LCD:Expired|{first_ex}")
                        except Exception:
                            pass
                        logger.info("Face DENIED — %s membership expired", name)
                        presence.pop(best_key, None)
                        roster.pop(best_key, None)
                        _gate_skip = True

                _conn.close()
            except Exception as _batch_e:
                logger.warning("Batch DB check error for %s: %s", name, _batch_e)

            if _gate_skip:
                continue

            # ── GRANT ENTRY ─────────────────────────────────────────
            logger.info("Face GRANT — %s (%s) key=%s score=%.3f method=FACE",
                        name, ptype, best_key, best_score)
            ok = _sb.send_command("UNLOCK")
            first = name.split()[0] if name else ptype.title()
            _sb.send_command(f"LCD:Welcome!|{first}")

            # Log attendance
            _log_attendance_in(best_key, ptype, db_id, db_path)

            # Arm tailgate monitor via shared helper (7s window = door open duration)
            _arm_tailgate(7.0)

            # Push to live feed — use ptype ("staff" / "familiar") as event type
            # and include the real photo_path so the dashboard shows the person's photo.
            try:
                _ac._push_event({
                    "type":        ptype,       # "staff" or "familiar"
                    "message":     f"Welcome, {name}",
                    "member_name": name,
                    "photo":       photo_path,  # real photo from roster
                    "alert":       "green",
                    "direction":   "IN",
                    "method":      "FACE",
                    "time":        _to_local(_now_utc()),
                })
            except Exception:
                pass

            logger.info("Face entry: %s (%s) key=%s score=%.3f unlock=%s",
                        name, ptype, best_key, best_score, ok)

            # Reset presence so the same person doesn't immediately
            # trigger a second entry if they linger in the camera frame
            presence.pop(best_key, None)

    def _loop_with_cleanup():
        """Wrapper to ensure persistent temp file cleanup on exit."""
        try:
            _loop()
        finally:
            try:
                os.unlink(_persistent_tmp_path)
            except Exception:
                pass

    t = _threading.Thread(target=_loop_with_cleanup, daemon=True, name="sf-face-loop")
    t.start()
    logger.info("Staff+familiar parallel face loop thread started")

_start_face_out_loop()



def _start_expiry_check_loop():
    """Background thread: marks expired members every 60 seconds.
    Uses our own Python query — does NOT rely on the compiled maintenance_service
    which only runs at :05 of each hour and may miss updates."""
    def _loop():
        import time as _t
        import sqlite3 as _sq
        while True:
            _t.sleep(60)
            try:
                db_path = os.path.join(project_root, "gym.db")
                conn = _sq.connect(db_path)
                today = _now_utc().strftime("%Y-%m-%d")
                cur = conn.execute(
                    "UPDATE members SET status='expired' "
                    "WHERE expiry_date IS NOT NULL "
                    "AND expiry_date < ? "
                    "AND status IN ('active', 'frozen')",
                    (today,)
                )
                count = cur.rowcount
                conn.commit()
                conn.close()
                if count:
                    logger.info("Expiry check: %d member(s) marked expired", count)
            except Exception as _e:
                logger.debug("Expiry check loop error: %s", _e)
    t = _threading.Thread(target=_loop, daemon=True, name="expiry-check")
    t.start()
    logger.info("Member expiry check loop started (every 60s)")

def _start_walkin_auto_logout_loop():
    """Background thread: auto-logs out all walk-in clients at midnight.
    Walk-in member records and names are kept (not deleted).
    """
    import time as _t
    import sqlite3 as _sq
    last_logout_date = None
    
    def _loop():
        nonlocal last_logout_date
        while True:
            _t.sleep(60)
            try:
                today = _now_utc().strftime("%Y-%m-%d")
                if last_logout_date and last_logout_date == today:
                    continue
                # Check if it's past midnight (after 00:00)
                hour = _now_utc().hour
                if hour == 0 and last_logout_date != today:
                    db_path = os.path.join(project_root, "gym.db")
                    conn = _sq.connect(db_path)
                    # Log OUT for all walk-ins that are currently IN
                    conn.execute("""
                        INSERT INTO attendance (member_id, direction, method, timestamp)
                        SELECT id, 'OUT', 'AUTO', datetime('now', '+8 hours')
                        FROM members
                        WHERE member_type='walkin' AND uid IS NOT NULL
                        AND id NOT IN (
                            SELECT member_id FROM attendance
                            WHERE direction='OUT' AND date(timestamp)=?
                        )
                    """, (today,))
                    # Clear RFID UIDs so cards can be reused the next day
                    conn.execute(
                        "UPDATE members SET uid=NULL WHERE member_type='walkin' AND uid IS NOT NULL"
                    )
                    conn.commit()
                    conn.close()
                    logger.info("Walk-in auto-logout: logged out and cleared UIDs for card reuse")
                    last_logout_date = today
            except Exception as _e:
                logger.debug("Walk-in auto-logout loop error: %s", _e)
    t = _threading.Thread(target=_loop, daemon=True, name="walkin-auto-logout")
    t.start()
    logger.info("Walk-in auto-logout loop started (midnight daily)")

def _start_attendance_cleanup_loop():
    """Background thread: archives attendance to daily summaries, then cleans up raw records.
    
    Strategy:
      - Raw attendance records are kept for 7 days (configurable via RETENTION_DAYS)
      - At midnight, each member's daily stats are summarized into attendance_daily table
      - Raw records older than RETENTION_DAYS are deleted to prevent database bloat
      - The state machine (FACE-IN → RFID-OUT cycle) already resets daily via date() filter
      - Historical analytics use the summary table instead of scanning millions of raw rows
    """
    _RETENTION_DAYS = 7  # keep raw records for 7 days before archiving
    
    def _ensure_summary_table(db_path):
        """Create attendance_daily table if it doesn't exist."""
        import sqlite3 as _sq
        conn = _sq.connect(db_path)
        conn.execute("""
            CREATE TABLE IF NOT EXISTS attendance_daily (
                id INTEGER NOT NULL PRIMARY KEY,
                member_id INTEGER,
                staff_id INTEGER,
                familiar_id INTEGER,
                date TEXT NOT NULL,
                total_ins INTEGER DEFAULT 0,
                total_outs INTEGER DEFAULT 0,
                first_in TEXT,
                last_out TEXT,
                created_at TEXT
            )
        """)
        # Add unique constraint to prevent duplicate summaries
        conn.execute("""
            CREATE UNIQUE INDEX IF NOT EXISTS idx_attendance_daily_unique 
            ON attendance_daily(COALESCE(member_id,0), COALESCE(staff_id,0), COALESCE(familiar_id,0), date)
        """)
        conn.commit()
        conn.close()
    
    def _archive_day(db_path, target_date):
        """Summarize a single day's attendance into attendance_daily, then delete raw records."""
        import sqlite3 as _sq
        conn = _sq.connect(db_path)
        now = _now_utc().isoformat()
        
        # Archive members
        conn.execute("""
            INSERT OR REPLACE INTO attendance_daily 
                (member_id, staff_id, familiar_id, date, total_ins, total_outs, first_in, last_out, created_at)
            SELECT 
                member_id, NULL, NULL, date(timestamp),
                SUM(CASE WHEN direction='IN' THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction='OUT' THEN 1 ELSE 0 END),
                MIN(CASE WHEN direction='IN' THEN timestamp END),
                MAX(CASE WHEN direction='OUT' THEN timestamp END),
                ?
            FROM attendance 
            WHERE member_id IS NOT NULL AND date(timestamp)=?
            GROUP BY member_id, date(timestamp)
        """, (now, target_date))
        
        # Archive staff
        conn.execute("""
            INSERT OR REPLACE INTO attendance_daily 
                (member_id, staff_id, familiar_id, date, total_ins, total_outs, first_in, last_out, created_at)
            SELECT 
                NULL, staff_id, NULL, date(timestamp),
                SUM(CASE WHEN direction='IN' THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction='OUT' THEN 1 ELSE 0 END),
                MIN(CASE WHEN direction='IN' THEN timestamp END),
                MAX(CASE WHEN direction='OUT' THEN timestamp END),
                ?
            FROM attendance 
            WHERE staff_id IS NOT NULL AND date(timestamp)=?
            GROUP BY staff_id, date(timestamp)
        """, (now, target_date))
        
        # Archive familiars
        conn.execute("""
            INSERT OR REPLACE INTO attendance_daily 
                (member_id, staff_id, familiar_id, date, total_ins, total_outs, first_in, last_out, created_at)
            SELECT 
                NULL, NULL, familiar_id, date(timestamp),
                SUM(CASE WHEN direction='IN' THEN 1 ELSE 0 END),
                SUM(CASE WHEN direction='OUT' THEN 1 ELSE 0 END),
                MIN(CASE WHEN direction='IN' THEN timestamp END),
                MAX(CASE WHEN direction='OUT' THEN timestamp END),
                ?
            FROM attendance 
            WHERE familiar_id IS NOT NULL AND date(timestamp)=?
            GROUP BY familiar_id, date(timestamp)
        """, (now, target_date))
        
        # Delete raw records older than retention period
        from datetime import date, timedelta
        cutoff = (date.fromisoformat(target_date) - timedelta(days=_RETENTION_DAYS)).isoformat()
        
        deleted = conn.execute("DELETE FROM attendance WHERE date(timestamp) < ?", (cutoff,)).rowcount
        conn.commit()
        conn.close()
        return deleted
    
    def _loop():
        import time as _t
        from datetime import date
        
        solo = _os.environ.get("SOLO_DATA_DIR", "").rstrip("/\\")
        db_path = (_os.path.join(solo, "gym.db") if solo
                   else _os.path.join(
                       _os.path.dirname(_os.path.abspath(__file__)), "gym.db"))
        
        _ensure_summary_table(db_path)
        
        last_archive_date = None
        logger.info("Attendance cleanup loop started (retention=%d days)", _RETENTION_DAYS)
        
        while True:
            _t.sleep(300)  # check every 5 minutes
            
            today = date.today().isoformat()
            yesterday = (date.today() - __import__('datetime').timedelta(days=1)).isoformat()
            
            # Run archive at midnight (when today changes)
            if last_archive_date != today:
                logger.info("Running attendance archive for %s", yesterday)
                try:
                    deleted = _archive_day(db_path, yesterday)
                    logger.info("Archived %s: %d raw records cleaned up", yesterday, deleted)
                except Exception as _e:
                    logger.warning("Attendance archive error for %s: %s", yesterday, _e)
                last_archive_date = today
    
    t = _threading.Thread(target=_loop, daemon=True, name="attendance-cleanup")
    t.start()
    logger.info("Attendance cleanup thread started")


_start_expiry_check_loop()
_start_walkin_auto_logout_loop()

# Run once immediately at startup so expired members are blocked right away
try:
    from services.maintenance import maintenance_service as _ms_startup
    _startup_expired = _ms_startup.run_expiry_check()
    if _startup_expired:
        logger.info("Startup expiry check: %d member(s) marked expired", _startup_expired)
except Exception as _se:
    logger.debug("Startup expiry check error: %s", _se)

# ── Startup DB migration — create extension tables / columns ─────
try:
    import sqlite3 as _sq3_s
    _db_path_s = os.path.join(project_root, "gym.db")
    _conn_s = _sq3_s.connect(_db_path_s, timeout=10)
    _conn_s.execute("PRAGMA foreign_keys = OFF")

    _conn_s.execute("""CREATE TABLE IF NOT EXISTS coaching_sessions (
        id INTEGER NOT NULL PRIMARY KEY,
        coach_id INTEGER NOT NULL REFERENCES staff(id),
        member_name VARCHAR(128) NOT NULL,
        price FLOAT NOT NULL DEFAULT 0,
        gym_commission_pct FLOAT DEFAULT 0,
        gym_share_type TEXT DEFAULT 'pct',
        session_date TEXT NOT NULL,
        notes TEXT, created_at DATETIME,
        created_by INTEGER REFERENCES staff(id)
    )""")

    _conn_s.execute("""CREATE TABLE IF NOT EXISTS admin_settings (
        key TEXT NOT NULL PRIMARY KEY,
        value TEXT NOT NULL
    )""")
    # Seed defaults — only inserted if key does not exist
    _conn_s.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_session_price', '500')")
    _conn_s.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_gym_ratio', '60')")
    _conn_s.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_gym_share_type', 'pct')")
    _conn_s.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_gym_share_peso', '0')")

    _conn_s.execute("""CREATE TABLE IF NOT EXISTS vouchers (
        id INTEGER NOT NULL PRIMARY KEY,
        title VARCHAR(128) NOT NULL,
        code VARCHAR(64) NOT NULL UNIQUE,
        quantity INTEGER NOT NULL DEFAULT 0,
        used_count INTEGER NOT NULL DEFAULT 0,
        is_active BOOLEAN DEFAULT 1,
        created_at DATETIME
    )""")
    _conn_s.execute("""CREATE TABLE IF NOT EXISTS voucher_usage (
        id INTEGER NOT NULL PRIMARY KEY,
        voucher_id INTEGER NOT NULL REFERENCES vouchers(id),
        member_id INTEGER NOT NULL REFERENCES members(id),
        voucher_title VARCHAR(128) NOT NULL DEFAULT '',
        used_at DATETIME
    )""")
    # Migration: add voucher_title column if missing (existing installs)
    _vu_cols = [r[1] for r in _conn_s.execute("PRAGMA table_info(voucher_usage)").fetchall()]
    if 'voucher_title' not in _vu_cols:
        _conn_s.execute("ALTER TABLE voucher_usage ADD COLUMN voucher_title VARCHAR(128) DEFAULT ''")
    # Backfill any rows missing voucher_title (new column or edge cases)
    _conn_s.execute("UPDATE voucher_usage SET voucher_title = COALESCE((SELECT title FROM vouchers WHERE id=voucher_usage.voucher_id), '') WHERE voucher_title IS NULL OR voucher_title = ''")
    # Drop old per-voucher_id index
    _conn_s.execute("DROP INDEX IF EXISTS idx_voucher_usage_unique")
    # Deduplicate before creating title-based unique index (old schema allowed same title per member)
    _conn_s.execute("""DELETE FROM voucher_usage WHERE id NOT IN (
        SELECT MIN(id) FROM voucher_usage GROUP BY voucher_title, member_id
    )""")
    # Create new title-based unique index
    try:
        _conn_s.execute("CREATE UNIQUE INDEX IF NOT EXISTS idx_vu_title_member ON voucher_usage(voucher_title, member_id)")
    except Exception:
        pass

    _plan_cols_s = [r[1] for r in _conn_s.execute("PRAGMA table_info(plans)").fetchall()]
    if 'commission_pct' not in _plan_cols_s:
        _conn_s.execute("ALTER TABLE plans ADD COLUMN commission_pct FLOAT DEFAULT 0")

    # Add discount_type and voucher_code columns to members (if not already there)
    _member_cols_s = [r[1] for r in _conn_s.execute("PRAGMA table_info(members)").fetchall()]
    if 'discount_type' not in _member_cols_s:
        _conn_s.execute("ALTER TABLE members ADD COLUMN discount_type VARCHAR(16)")
    if 'voucher_code' not in _member_cols_s:
        _conn_s.execute("ALTER TABLE members ADD COLUMN voucher_code VARCHAR(64)")
    # Backfill existing is_student=1 members
    _conn_s.execute("UPDATE members SET is_student=1, member_type='student', discount_type='student' WHERE is_student=1")

    _conn_s.execute("PRAGMA foreign_keys = ON")
    _conn_s.commit()
    _conn_s.close()
    logger.info("Startup DB migration complete")
except Exception as _mse:
    logger.warning("Startup DB migration error: %s", _mse)

# The dedup trigger is no longer needed — the _AttendanceGateDict in
# _patch_access_control() prevents the compiled entry_loop from re-firing.
# Drop it on startup so it doesn't block 2nd-cycle FACE-IN inserts
# (SQLAlchemy session isolation caused trigger to see stale state).
def _apply_dedup_trigger():
    """Drop the old dedup trigger — replaced by _AttendanceGateDict."""
    import sqlite3 as _sq, os as _os
    dbs = [
        _os.path.join(project_root, "gym.db"),
        _os.path.join(project_root, "electron", "node_modules", "electron",
                      "dist", "GymPOS_Data", "gym.db"),
    ]
    solo_data = _os.environ.get("SOLO_DATA_DIR", "")
    if solo_data:
        dbs.insert(0, _os.path.join(solo_data, "gym.db"))
    seen = set()
    for db_path in dbs:
        if not _os.path.exists(db_path) or db_path in seen:
            continue
        seen.add(db_path)
        try:
            conn = _sq.connect(db_path)
            conn.execute("DROP TRIGGER IF EXISTS trg_attendance_dedup")
            conn.commit()
            conn.close()
            logger.info("Dropped dedup trigger from %s", db_path)
        except Exception as e:
            logger.warning("Drop trigger error on %s: %s", db_path, e)

_apply_dedup_trigger()


def _install_attendance_insert_trigger():
    """
    Install a SQLite trigger that prevents duplicate IN records for the same
    member on the same day. This catches cases where the compiled ORM code's
    dedup check fails due to session isolation or DB lock contention.

    The trigger RAISEs an error on duplicate inserts, which the robust
    _ensure_attendance_logged() function handles gracefully.
    """
    import sqlite3 as _sq, os as _os
    dbs = [
        _os.path.join(project_root, "gym.db"),
        _os.path.join(project_root, "electron", "node_modules", "electron",
                      "dist", "GymPOS_Data", "gym.db"),
    ]
    solo_data = _os.environ.get("SOLO_DATA_DIR", "")
    if solo_data:
        dbs.insert(0, _os.path.join(solo_data, "gym.db"))
    seen = set()
    for db_path in dbs:
        if not _os.path.exists(db_path) or db_path in seen:
            continue
        seen.add(db_path)
        try:
            conn = _sq.connect(db_path)
            # Drop old trigger if exists
            conn.execute("DROP TRIGGER IF EXISTS trg_attendance_no_dup_in")
            # Create new trigger: prevent duplicate IN on same day
            conn.execute("""
                CREATE TRIGGER trg_attendance_no_dup_in
                BEFORE INSERT ON attendance
                WHEN NEW.direction = 'IN'
                BEGIN
                    SELECT RAISE(ABORT, 'duplicate_in')
                    WHERE EXISTS (
                        SELECT 1 FROM attendance
                        WHERE member_id = NEW.member_id
                          AND direction = 'IN'
                          AND date(timestamp) = date(NEW.timestamp)
                    );
                END;
            """)
            conn.commit()
            conn.close()
            logger.info("Installed no-dup IN trigger on %s", db_path)
        except Exception as e:
            logger.warning("Install trigger error on %s: %s", db_path, e)


_install_attendance_insert_trigger()


# â”€â”€ Imports for extensions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
from datetime import datetime
from pathlib import Path

from fastapi import Request, Form, Depends, UploadFile, File
from fastapi.responses import RedirectResponse, JSONResponse, HTMLResponse
from sqlalchemy.orm import Session
from sqlalchemy import text

from database.connection import get_db, SessionLocal


def _get_templates():
    """Get the Jinja2 templates engine."""
    from starlette.templating import Jinja2Templates
    # In source mode, templates are in the project root
    tpl_dir = os.path.join(project_root, "templates")
    if os.path.isdir(tpl_dir):
        return Jinja2Templates(directory=tpl_dir)
    # Fallback: use paths module
    from paths import templates_dir
    td = templates_dir() if callable(templates_dir) else templates_dir
    return Jinja2Templates(directory=str(td))

templates = _get_templates()

# Register template globals / filters on MY templates engine
_PROJECT_ROOT_FWD = project_root.replace("\\", "/").rstrip("/")

def _photo_url(path):
    """Convert a photo_path to a URL-safe string.
    Handles:
      - None / empty          → ""
      - Already a URL         → unchanged
      - Relative path         → "/" + path
      - Absolute Windows path → strip project root → "/" + relative
    """
    if not path:
        return ""
    if path.startswith("/") or path.startswith("http"):
        return path
    # Normalize backslashes
    p = path.replace("\\", "/")
    # Strip absolute project root prefix (compiled routes store absolute paths)
    if p.startswith(_PROJECT_ROOT_FWD + "/"):
        p = p[len(_PROJECT_ROOT_FWD) + 1:]
    elif p.startswith(_PROJECT_ROOT_FWD):
        p = p[len(_PROJECT_ROOT_FWD):]
    return "/" + p.lstrip("/")

# Inject photo_url into ALL Jinja2 environments (including compiled routes)
# Update to the full version now that project_root is known
import jinja2.defaults as _jd2
_jd2.DEFAULT_FILTERS["photo_url"] = _photo_url
logger.info("photo_url filter updated with project_root: %s", _PROJECT_ROOT_FWD)

for _fn, _name in [(_photo_url, "photo_url"), (_to_local, "to_local"),
                   (_to_local_time, "to_local_time"), (_to_local_date, "to_local_date")]:
    templates.env.globals[_name]  = _fn
    templates.env.filters[_name]  = _fn


# ── Remove compiled /api/live-feed route (we override it below) ──
def _remove_route(path, method="GET"):
    from starlette.routing import Route
    for i, r in enumerate(app.routes):
        if isinstance(r, Route) and r.path == path and method.upper() in (r.methods or set()):
            app.routes.pop(i)
            return r.endpoint
    return None

_remove_route("/api/live-feed",    "GET")
_remove_route("/api/face-detect",  "GET")   # replaced below with staff+familiar support

# ═══════════════════════════════════════════════════════════════════
# SHARED FACE ENCODING HELPERS
# ═══════════════════════════════════════════════════════════════════

import base64 as _b64, tempfile as _tmp

# Global lock protecting the face recognition ML backend.
# MediaPipe + SFace are NOT thread-safe — simultaneous calls from the
# access-control face-detection loop and a registration form POST
# cause a C++ segfault that kills the entire Python process.
# Every call to encode_multi_image_files / encode_image_file must
# hold this lock.
_FACE_ML_LOCK = _threading.Lock()

def _save_b64_photo(b64: str, prefix: str) -> "str | None":
    """Decode a data-URI base64 image and save it to the photos directory.
    Returns a relative path like 'static/photos/<filename>.jpg' or None."""
    if not b64 or not b64.startswith("data:"):
        return None
    try:
        _, b64data = b64.split(",", 1)
        img_bytes = _b64.b64decode(b64data)
        from paths import data_root
        photos_dir = data_root() / "static" / "photos"
        photos_dir.mkdir(parents=True, exist_ok=True)
        fname = f"{prefix}_{_now_utc().strftime('%Y%m%d_%H%M%S_%f')}.jpg"
        (photos_dir / fname).write_bytes(img_bytes)
        return f"static/photos/{fname}"
    except Exception as e:
        logger.warning("Photo save error: %s", e)
        return None


def _b64_to_tempfile(b64: str) -> "str | None":
    """Decode a base64 data-URI to a temp JPEG file. Caller must delete."""
    if not b64 or not b64.startswith("data:"):
        return None
    try:
        _, b64data = b64.split(",", 1)
        img_bytes = _b64.b64decode(b64data)
        fd, path = _tmp.mkstemp(suffix=".jpg")
        with _os.fdopen(fd, "wb") as f:
            f.write(img_bytes)
        return path
    except Exception:
        return None


async def _upload_to_tempfile(upload) -> "str | None":
    """Save an UploadFile to a temp JPEG. Caller must delete."""
    if not upload or not upload.filename:
        return None
    try:
        data = await upload.read()
        if len(data) < 200:
            return None
        fd, path = _tmp.mkstemp(suffix=".jpg")
        with _os.fdopen(fd, "wb") as f:
            f.write(data)
        return path
    except Exception:
        return None


def _encode_multi_angle(paths: list) -> "bytes | None":
    """
    Encode a multi-angle face vector from a list of image file paths.

    THREAD SAFETY: Acquires _FACE_ML_LOCK before calling into the face
    recognition ML backend (MediaPipe + SFace). The access control loop
    also calls MediaPipe continuously; without this lock, simultaneous
    invocations from different threads cause a C++ segfault that kills
    the entire Python process.
    """
    valid = [p for p in paths if p]
    if not valid:
        return None
    with _FACE_ML_LOCK:
        try:
            from services.face_recognition_ml import face_recognition_ml as _fr_ml
            result = _fr_ml.encode_multi_image_files(valid)
            if result:
                logger.info("Multi-angle face encoded from %d image(s)", len(valid))
            else:
                logger.warning("encode_multi_image_files returned None for %d images", len(valid))
            return result
        except Exception as e:
            logger.warning("Face encoding error: %s", e)
            return None


def _validate_face(path: str) -> "tuple[bool, str]":
    """Validate a face capture for quality (blur, brightness, face presence)."""
    try:
        from services.face_recognition_ml import face_recognition_ml as _fr_ml
        return _fr_ml.validate_face_capture(path)
    except Exception:
        return True, "ok"  # don't block on validation failure


def _invalidate_face_roster():
    """Force the face recognition service to rebuild its in-memory roster.
    Called after any face vector is saved/deleted in the database.

    Note: We do NOT hold _FACE_ML_LOCK here. invalidate_roster_cache() is
    a simple flag-set (sets _roster_valid = False) that is thread-safe on its
    own.  Holding _FACE_ML_LOCK would block the web request for up to 300ms
    while waiting for the face extraction loop to release it — causing the
    input lag users experience after staff creation/deletion.
    """
    try:
        from services.face_recognition import face_service
        face_service.invalidate_roster_cache()
        logger.info("Face roster cache invalidated")
    except Exception as e:
        logger.warning("Roster invalidation failed: %s", e)


def _cleanup(*paths):
    """Delete temp files silently."""
    for p in paths:
        if p:
            try:
                _os.unlink(p)
            except Exception:
                pass


async def _collect_face_paths(
    front_b64="", left_b64="", right_b64="",
    front_file=None, left_file=None, right_file=None
) -> "tuple[str, list[str], list[str]]":
    """
    Collect face images from b64 fields and/or uploaded files.
    Returns (front_temp_path_for_photo, [all_angle_paths], [temp_paths_to_delete])
    The front image is used as the profile photo.
    """
    temps = []
    angle_paths = []

    for b64, upload in [(front_b64, front_file), (left_b64, left_file), (right_b64, right_file)]:
        path = None
        if b64 and b64.startswith("data:"):
            path = _b64_to_tempfile(b64)
        elif upload and upload.filename:
            path = await _upload_to_tempfile(upload)
        if path:
            angle_paths.append(path)
            temps.append(path)

    front_path = angle_paths[0] if angle_paths else None
    return front_path, angle_paths, temps


# ═══════════════════════════════════════════════════════════════════
# STAFF CREATION — override compiled route with multi-angle face encoding
# ═══════════════════════════════════════════════════════════════════

def _remove_all_routes(path: str, methods=("POST", "GET")):
    from starlette.routing import Route
    removed = []
    i = 0
    while i < len(app.routes):
        r = app.routes[i]
        if isinstance(r, Route) and r.path == path:
            if any(m.upper() in (r.methods or set()) for m in methods):
                removed.append(app.routes.pop(i))
                continue
        i += 1
    return removed


_remove_all_routes("/admin/staff", ["POST"])


@app.post("/admin/staff")
async def create_staff_multi_angle(
    request: Request,
    username:         str = Form(...),
    password:         str = Form(...),
    display_name:     str = Form(""),
    role:             str = Form("staff"),
    rfid_uid:         str = Form(""),
    face_photo_b64:   str = Form(""),   # single-angle fallback
    face_front_b64:   str = Form(""),
    face_left_b64:    str = Form(""),
    face_right_b64:   str = Form(""),
    face_photo_file:  UploadFile = File(None),
    face_front_file:  UploadFile = File(None),
    face_left_file:   UploadFile = File(None),
    face_right_file:  UploadFile = File(None),
    db: Session = Depends(get_db),
):
    role_val = request.session.get("role")
    if role_val != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    username = username.strip()
    uid = rfid_uid.strip().upper() or None

    # Uniqueness checks
    if db.execute(text("SELECT 1 FROM staff WHERE username=:u"), {"u": username}).fetchone():
        return RedirectResponse("/admin/staff?error=exists", status_code=303)
    if uid and db.execute(text(
        "SELECT 1 FROM staff WHERE uid=:u UNION ALL SELECT 1 FROM members WHERE uid=:u "
        "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
    ), {"u": uid}).fetchone():
        return RedirectResponse("/admin/staff?error=rfid_taken", status_code=303)

    # Hash password
    import bcrypt as _bcrypt
    pw_hash = _bcrypt.hashpw(password.encode(), _bcrypt.gensalt()).decode()

    # Prefer 3-angle fields; fall back to single-angle
    front_b64 = face_front_b64 or face_photo_b64
    left_b64  = face_left_b64
    right_b64 = face_right_b64
    f_file    = face_front_file if (face_front_file and face_front_file.filename) else face_photo_file

    front_tmp, all_paths, temps = await _collect_face_paths(
        front_b64, left_b64, right_b64,
        f_file, face_left_file, face_right_file,
    )

    photo_path  = None
    face_vector = None

    try:
        if front_tmp:
            photo_path = _save_b64_photo(front_b64 or face_photo_b64, f"staff_{username}")
            if not photo_path and front_tmp:
                # Came from file upload — copy to photos dir
                import shutil as _sh
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                fname = f"staff_{username}_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
                _sh.copy2(front_tmp, str(photos_dir / fname))
                photo_path = f"static/photos/{fname}"

        if all_paths:
            face_vector = await __import__('asyncio').to_thread(_encode_multi_angle, all_paths) if all_paths else None
    finally:
        _cleanup(*temps)

    now = _now_utc()
    res = db.execute(text(
        "INSERT INTO staff (username, password_hash, display_name, role, is_active, uid, "
        "face_vector, photo_path, created_at) "
        "VALUES (:u,:pw,:dn,:role,1,:uid,:fv,:pp,:now)"
    ), {
        "u": username, "pw": pw_hash,
        "dn": display_name.strip() or username,
        "role": role.strip() or "staff",
        "uid": uid, "fv": face_vector, "pp": photo_path, "now": now,
    })
    new_id = res.lastrowid
    db.commit()

    if face_vector:
        _invalidate_face_roster()

    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'create_staff','staff',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": new_id,
            "det": username, "ts": now})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/admin/staff", status_code=303)


_remove_all_routes("/admin/staff/{staff_id}/update", ["POST"])


@app.post("/admin/staff/{staff_id}/update")
async def update_staff_multi_angle(
    staff_id: int,
    request: Request,
    display_name:    str = Form(""),
    role:            str = Form("staff"),
    rfid_uid:        str = Form(""),
    face_photo_b64:  str = Form(""),
    face_front_b64:  str = Form(""),
    face_left_b64:   str = Form(""),
    face_right_b64:  str = Form(""),
    face_photo_file: UploadFile = File(None),
    face_front_file: UploadFile = File(None),
    face_left_file:  UploadFile = File(None),
    face_right_file: UploadFile = File(None),
    db: Session = Depends(get_db),
):
    role_val = request.session.get("role")
    if role_val != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    uid = rfid_uid.strip().upper() or None
    if uid:
        dup = db.execute(text(
            "SELECT 1 FROM staff WHERE uid=:u AND id!=:id "
            "UNION ALL SELECT 1 FROM members WHERE uid=:u "
            "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
        ), {"u": uid, "id": staff_id}).fetchone()
        if dup:
            return RedirectResponse("/admin/staff?error=rfid_taken", status_code=303)

    front_b64 = face_front_b64 or face_photo_b64
    f_file    = face_front_file if (face_front_file and face_front_file.filename) else face_photo_file

    front_tmp, all_paths, temps = await _collect_face_paths(
        front_b64, face_left_b64, face_right_b64,
        f_file, face_left_file, face_right_file,
    )

    photo_path  = None
    face_vector = None

    try:
        if front_tmp:
            photo_path = _save_b64_photo(front_b64 or face_photo_b64, f"staff_{staff_id}")
            if not photo_path and front_tmp:
                import shutil as _sh
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                fname = f"staff_{staff_id}_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
                _sh.copy2(front_tmp, str(photos_dir / fname))
                photo_path = f"static/photos/{fname}"

        if all_paths:
            face_vector = await __import__('asyncio').to_thread(_encode_multi_angle, all_paths) if all_paths else None
    finally:
        _cleanup(*temps)

    # Build update SQL
    sets = ["display_name=:dn", "role=:role"]
    params: dict = {"dn": display_name.strip(), "role": role.strip() or "staff", "id": staff_id}

    if uid is not None:
        sets.append("uid=:uid"); params["uid"] = uid
    if photo_path:
        sets.append("photo_path=:pp"); params["pp"] = photo_path
    if face_vector:
        sets.append("face_vector=:fv"); params["fv"] = face_vector

    db.execute(text(f"UPDATE staff SET {', '.join(sets)} WHERE id=:id"), params)
    db.commit()

    if face_vector:
        _invalidate_face_roster()

    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'update_staff','staff',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": staff_id,
            "det": display_name, "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/admin/staff", status_code=303)


@app.get("/api/live-feed")
async def live_feed_local_time(request: Request, limit: int = 20,
                               db: Session = Depends(get_db)):
    """Live feed with local PH time — covers members, staff, and familiars."""
    rows = db.execute(text(
        "SELECT a.id, a.member_id, a.familiar_id, a.staff_id, "
        "a.direction, a.method, a.timestamp, "
        "m.name        AS mname,  m.photo_path  AS mphoto, "
        "f.name        AS fname,  f.photo_path  AS fphoto, "
        "s.display_name AS sname, s.photo_path  AS sphoto "
        "FROM attendance a "
        "LEFT JOIN members  m ON a.member_id  = m.id "
        "LEFT JOIN familiars f ON a.familiar_id = f.id "
        "LEFT JOIN staff    s ON a.staff_id   = s.id "
        "ORDER BY a.id DESC LIMIT :lim"
    ), {"lim": limit}).fetchall()
    # Column index map (0-based):
    #  0  a.id          1  member_id     2  familiar_id   3  staff_id
    #  4  direction     5  method        6  timestamp
    #  7  mname         8  mphoto
    #  9  fname        10  fphoto
    # 11  sname        12  sphoto
    events = []
    for r in rows:
        is_staff    = r[3] is not None
        is_familiar = r[2] is not None
        if is_staff:
            name  = r[11] or "Staff"
            photo = r[12] or ""
            etype = "staff"
        elif is_familiar:
            name  = r[9]  or "Familiar"
            photo = r[10] or ""
            etype = "familiar"
        else:
            name  = r[7]  or "Unknown"
            photo = r[8]  or ""
            etype = "ok" if (r[4] or "").upper() == "IN" else "exit"
        events.append({
            "type":        etype,
            "member_name": name,
            "direction":   r[4] or "",
            "method":      r[5] or "",
            "photo":       photo,
            "time":        _to_local(r[6]),
            "alert":       "green",
        })
    return JSONResponse(events)


# ══════════════════════════════════════════════════════════════════
# FACE-DETECT OVERRIDE — extends compiled endpoint to cover
# staff and familiars in addition to members.
#
# The compiled face_detect_snapshot only matches against the members
# table.  This override keeps that behaviour for members and adds a
# second pass for staff + familiars: any face that the compiled path
# marks "Unknown" is re-checked against the staff/familiar vector
# roster using the same SFace cosine-similarity pipeline used by
# _start_staff_familiar_face_loop.
#
# This is DISPLAY-ONLY — it never sends UNLOCK or logs attendance.
# Actual door entry is still handled by the background daemon threads.
# ══════════════════════════════════════════════════════════════════

@app.get("/api/face-detect")
async def face_detect_all_persons(request: Request,
                                  db: Session = Depends(get_db)):
    """
    Override of compiled face_detect_snapshot.
    Returns face detection results that cover members, staff, AND familiars.

    Flow:
      1. Auth check (staff/admin session required)
      2. Grab latest cam1 frame from the access-control camera stream
      3. Run compiled detect_and_match_detailed() → member matches
      4. Serialize ORM member objects → JSON-safe dicts
      5. If any face has no member match: extract a live SFace vector
         and run cosine-similarity against the staff+familiar roster
         (threshold 0.55, same as the background entry loop)
      6. Return combined results in the exact JSON shape the UI expects
    """
    if not request.session.get("user_id"):
        return JSONResponse({"status": "error", "message": "Not authenticated"},
                            status_code=401)

    # ── 1. Get the latest cam1 frame ──────────────────────────────
    frame = None
    try:
        from services.access_control import access_control as _ac_fd
        frame = _ac_fd.cam1.get_latest_frame()
    except Exception:
        pass
    if frame is None:
        return JSONResponse({"status": "no_camera", "matches": []})

    fh, fw = frame.shape[:2]

    # ── 2. Member detection via compiled face_service ─────────────
    import asyncio as _aio_fd
    raw_results = []
    try:
        from services.face_recognition import face_service as _fs_fd
        if _fs_fd.available:
            raw_results = await _aio_fd.to_thread(
                _fs_fd.detect_and_match_detailed, frame, db
            ) or []
    except Exception as _e_fd:
        logger.debug("face_detect member path error: %s", _e_fd)

    # ── 3. Serialize ORM objects → JSON dicts ─────────────────────
    matches = []
    has_unmatched = False
    for r in raw_results:
        mo = r.get("member")
        if mo:
            member_info = {
                "id":     mo.id,
                "name":   mo.name,
                "status": mo.status.value if hasattr(mo.status, "value")
                          else str(mo.status),
                "photo":  mo.photo_path or "",
                "type":   "member",
                "expiry": str(mo.expiry_date)
                          if getattr(mo, "expiry_date", None) else None,
            }
        else:
            member_info = None
            has_unmatched = True
        matches.append({
            "box":        list(r.get("face_box",  [0, 0, 0, 0])),
            "confidence": round(float(r.get("yunet_confidence", 0)), 3),
            "landmarks":  r.get("landmarks", []),
            "match_score": round(float(r.get("match_score", 0)), 3),
            "member":     member_info,
        })

    # ── 4. No-match faces → try staff + familiar roster ───────────
    # Only bother extracting the SFace vector if at least one face was
    # detected but not matched to a member.
    if has_unmatched:
        live_vec = None
        try:
            import cv2 as _cv2_fd
            import numpy as _np_fd
            import tempfile as _tmp_fd
            import os as _os_fd
            fd_, tmp_ = _tmp_fd.mkstemp(suffix=".jpg")
            _os_fd.close(fd_)
            _cv2_fd.imwrite(tmp_, frame)
            # Hold the ML lock so we don't race with the live entry loop
            with _FACE_ML_LOCK:
                from services.face_recognition_ml import face_recognition_ml as _frml_fd
                vec_res = _frml_fd.extract_face_vector(tmp_)
            try:
                _os_fd.unlink(tmp_)
            except Exception:
                pass
            if vec_res:
                # extract_face_vector returns EITHER:
                #   tuple: (ndarray, meta_dict)  — current API
                #   dict:  {"vector": ndarray, ...} — alternate format
                if isinstance(vec_res, (tuple, list)):
                    live_vec = _np_fd.array(vec_res[0], dtype=_np_fd.float32).flatten()
                elif isinstance(vec_res, dict):
                    live_vec = _np_fd.array(
                        vec_res["vector"], dtype=_np_fd.float32).flatten()
                else:
                    live_vec = _np_fd.array(vec_res, dtype=_np_fd.float32).flatten()
                if len(live_vec) != 128:
                    live_vec = None
        except Exception as _ve_fd:
            logger.debug("face_detect sf vector extraction error: %s", _ve_fd)

        if live_vec is not None:
            sf_roster = _face_detect_build_sf_roster(
                os.path.join(project_root, "gym.db"))
            if sf_roster:
                best_key_fd, best_score_fd = None, 0.0
                for key_, (nm_, sv_, pt_, did_, ph_) in sf_roster.items():
                    try:
                        sc_ = _cosine_sim(live_vec, sv_)
                        if sc_ > 0.65 and sc_ > best_score_fd:
                            best_score_fd, best_key_fd = sc_, key_
                    except Exception:
                        continue

                if best_key_fd is not None:
                    nm_, sv_, pt_, did_, ph_ = sf_roster[best_key_fd]
                    sf_info = {
                        "id":     did_,
                        "name":   nm_,
                        "status": "active",
                        "photo":  ph_,
                        "type":   pt_,    # "staff" or "familiar"
                        "expiry": None,
                    }
                    logger.info(
                        "face_detect: %s recognised as %s (score=%.3f)",
                        nm_, pt_, best_score_fd
                    )
                    # Assign to the first unmatched slot
                    for m_ in matches:
                        if m_["member"] is None:
                            m_["member"]       = sf_info
                            m_["match_score"]  = round(best_score_fd, 3)
                            break

    # ── 5. Return combined result ──────────────────────────────────
    return JSONResponse({
        "status":         "ok",
        "frame_width":    fw,
        "frame_height":   fh,
        "faces_detected": len(matches),
        "matches":        matches,
    })


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# PHASE 2: Hard Delete for Members
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.post("/members/{member_id}/delete")
async def hard_delete_member(member_id: int, request: Request,
                             db: Session = Depends(get_db)):
    """Hard-delete a member: removes all associated records permanently."""
    user_id = request.session.get("user_id")
    if not user_id:
        return RedirectResponse("/login", status_code=303)

    member = db.execute(
        text("SELECT id, name, photo_path FROM members WHERE id = :id"),
        {"id": member_id}
    ).fetchone()
    if not member:
        return RedirectResponse("/members?error=not_found", status_code=303)

    member_name = member[1]
    photo_path = member[2]

    # Delete dependent records
    db.execute(text("DELETE FROM attendance WHERE member_id = :id"), {"id": member_id})
    db.execute(text("DELETE FROM sales WHERE member_id = :id"), {"id": member_id})
    db.execute(text("DELETE FROM freezes WHERE member_id = :id"), {"id": member_id})
    db.execute(text("DELETE FROM coach_assignments WHERE member_id = :id"), {"id": member_id})
    db.execute(text("UPDATE security_incidents SET member_id = NULL WHERE member_id = :id"),
               {"id": member_id})
    # Delete the member
    db.execute(text("DELETE FROM members WHERE id = :id"), {"id": member_id})
    db.commit()

    # Delete photo files
    if photo_path:
        try:
            from paths import data_root
            full = data_root() / photo_path if not os.path.isabs(photo_path) else Path(photo_path)
            if full.exists():
                full.unlink()
        except Exception:
            pass

    # Log activity
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id, action, target_type, target_id, details, timestamp) "
            "VALUES (:sid, 'hard_delete_member', 'member', :tid, :det, :ts)"
        ), {"sid": user_id, "tid": member_id, "det": member_name, "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/members?deleted=1", status_code=303)


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# PHASE 3: Hard Delete for Staff
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.post("/admin/staff/{staff_id}/delete")
async def hard_delete_staff(staff_id: int, request: Request,
                            db: Session = Depends(get_db)):
    """Hard-delete a staff member (admin only). Cannot delete yourself."""
    user_id = request.session.get("user_id")
    role = request.session.get("role")
    if role != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    if staff_id == user_id:
        return RedirectResponse("/admin/staff?error=self_delete", status_code=303)

    staff = db.execute(
        text("SELECT id, username, photo_path FROM staff WHERE id = :id"),
        {"id": staff_id}
    ).fetchone()
    if not staff:
        return RedirectResponse("/admin/staff?error=not_found", status_code=303)

    staff_name = staff[1]
    photo_path = staff[2]

    # Delete dependent records
    db.execute(text("DELETE FROM attendance WHERE staff_id = :id"), {"id": staff_id})
    db.execute(text("DELETE FROM staff_activities WHERE staff_id = :id"), {"id": staff_id})
    db.execute(text("DELETE FROM coach_assignments WHERE coach_id = :id"), {"id": staff_id})
    db.execute(text("UPDATE sales SET cashier_id = NULL WHERE cashier_id = :id"), {"id": staff_id})
    db.execute(text("UPDATE expenses SET staff_id = NULL WHERE staff_id = :id"), {"id": staff_id})
    db.execute(text("UPDATE security_incidents SET staff_id = NULL WHERE staff_id = :id"),
               {"id": staff_id})
    db.execute(text("UPDATE manual_overrides SET staff_id = NULL WHERE staff_id = :id"),
               {"id": staff_id})
    db.execute(text("UPDATE familiars SET created_by = NULL WHERE created_by = :id"),
               {"id": staff_id})
    # Delete the staff record
    db.execute(text("DELETE FROM staff WHERE id = :id"), {"id": staff_id})
    db.commit()

    # Immediately invalidate face roster so the deleted staff's vector
    # is purged from the in-memory cache — prevents ghost matches.
    _invalidate_face_roster()

    # Delete photo
    if photo_path:
        try:
            from paths import data_root
            full = data_root() / photo_path if not os.path.isabs(photo_path) else Path(photo_path)
            if full.exists():
                full.unlink()
        except Exception:
            pass

    # Log (use a direct insert since the deleted staff can't be referenced)
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id, action, target_type, target_id, details, timestamp) "
            "VALUES (:sid, 'hard_delete_staff', 'staff', :tid, :det, :ts)"
        ), {"sid": user_id, "tid": staff_id, "det": staff_name, "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/admin/staff?deleted=1", status_code=303)


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# PHASE 4: Familiar CRUD
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.get("/admin/familiars")
async def familiars_page(request: Request, db: Session = Depends(get_db)):
    """Admin page for managing Familiars."""
    user_id = request.session.get("user_id")
    role = request.session.get("role")
    if role != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    rows = db.execute(text(
        "SELECT f.id, f.uid, f.photo_path, f.name, f.phone, f.notes, "
        "f.is_active, f.created_at, s.display_name AS created_by_name "
        "FROM familiars f LEFT JOIN staff s ON f.created_by = s.id "
        "ORDER BY f.created_at DESC"
    )).fetchall()

    familiars = []
    for r in rows:
        familiars.append({
            "id": r[0], "uid": r[1], "photo_path": r[2], "name": r[3],
            "phone": r[4], "notes": r[5], "is_active": r[6],
            "created_at": r[7], "created_by_name": r[8],
        })

    return templates.TemplateResponse(request, "admin/familiars.html", {
        "familiars": familiars,
    })


@app.post("/admin/familiars")
async def create_familiar(
    request: Request,
    name:            str = Form(...),
    phone:           str = Form(""),
    notes:           str = Form(""),
    rfid_uid:        str = Form(""),
    face_photo_b64:  str = Form(""),   # single-angle fallback
    face_front_b64:  str = Form(""),
    face_left_b64:   str = Form(""),
    face_right_b64:  str = Form(""),
    face_photo_file: UploadFile = File(None),
    face_front_file: UploadFile = File(None),
    face_left_file:  UploadFile = File(None),
    face_right_file: UploadFile = File(None),
    db: Session = Depends(get_db),
):
    """Create a Familiar with multi-angle face vector encoding."""
    user_id = request.session.get("user_id")
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    uid = rfid_uid.strip().upper() or None
    if uid:
        dup = db.execute(text(
            "SELECT 1 FROM members WHERE uid=:u UNION ALL SELECT 1 FROM staff WHERE uid=:u "
            "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
        ), {"u": uid}).fetchone()
        if dup:
            return RedirectResponse("/admin/familiars?error=rfid_taken", status_code=303)

    # Prefer 3-angle fields; fall back to single-angle
    front_b64 = face_front_b64 or face_photo_b64
    f_file    = face_front_file if (face_front_file and face_front_file.filename) else face_photo_file

    front_tmp, all_paths, temps = await _collect_face_paths(
        front_b64, face_left_b64, face_right_b64,
        f_file, face_left_file, face_right_file,
    )

    photo_path = face_vector = None
    try:
        if front_tmp:
            photo_path = _save_b64_photo(front_b64 or face_photo_b64, f"familiar_{name.strip()[:12]}")
            if not photo_path:
                import shutil as _sh
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                fname = f"familiar_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
                _sh.copy2(front_tmp, str(photos_dir / fname))
                photo_path = f"static/photos/{fname}"
        if all_paths:
            face_vector = await __import__('asyncio').to_thread(_encode_multi_angle, all_paths) if all_paths else None
    finally:
        _cleanup(*temps)

    now = _now_utc()
    res = db.execute(text(
        "INSERT INTO familiars (uid, face_vector, photo_path, name, phone, notes, "
        "is_active, created_by, created_at) VALUES (:uid,:fv,:pp,:name,:phone,:notes,1,:cb,:ca)"
    ), {"uid": uid, "fv": face_vector, "pp": photo_path, "name": name.strip(),
        "phone": phone.strip(), "notes": notes.strip(), "cb": user_id, "ca": now})
    db.commit()

    if face_vector:
        _invalidate_face_roster()

    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'create_familiar','familiar',NULL,:det,:ts)"
        ), {"sid": user_id, "det": name.strip(), "ts": now})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/admin/familiars?created=1", status_code=303)


@app.post("/admin/familiars/{fam_id}/update")
async def update_familiar(
    fam_id: int,
    request: Request,
    name:            str = Form(...),
    phone:           str = Form(""),
    notes:           str = Form(""),
    rfid_uid:        str = Form(""),
    face_photo_b64:  str = Form(""),
    face_front_b64:  str = Form(""),
    face_left_b64:   str = Form(""),
    face_right_b64:  str = Form(""),
    face_photo_file: UploadFile = File(None),
    face_front_file: UploadFile = File(None),
    face_left_file:  UploadFile = File(None),
    face_right_file: UploadFile = File(None),
    db: Session = Depends(get_db),
):
    """Update a Familiar's details and optionally re-encode face vector."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    uid = rfid_uid.strip().upper() or None
    if uid:
        dup = db.execute(text(
            "SELECT 1 FROM members WHERE uid=:u "
            "UNION ALL SELECT 1 FROM staff WHERE uid=:u "
            "UNION ALL SELECT 1 FROM familiars WHERE uid=:u AND id!=:fid"
        ), {"u": uid, "fid": fam_id}).fetchone()
        if dup:
            return RedirectResponse("/admin/familiars?error=rfid_taken", status_code=303)

    front_b64 = face_front_b64 or face_photo_b64
    f_file    = face_front_file if (face_front_file and face_front_file.filename) else face_photo_file

    front_tmp, all_paths, temps = await _collect_face_paths(
        front_b64, face_left_b64, face_right_b64,
        f_file, face_left_file, face_right_file,
    )

    photo_path = face_vector = None
    try:
        if front_tmp:
            photo_path = _save_b64_photo(front_b64 or face_photo_b64, f"familiar_{fam_id}")
            if not photo_path:
                import shutil as _sh
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                fname = f"familiar_{fam_id}_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
                _sh.copy2(front_tmp, str(photos_dir / fname))
                photo_path = f"static/photos/{fname}"
        if all_paths:
            face_vector = await __import__('asyncio').to_thread(_encode_multi_angle, all_paths) if all_paths else None
    finally:
        _cleanup(*temps)

    sets = ["name=:name", "phone=:phone", "notes=:notes"]
    params: dict = {"name": name.strip(), "phone": phone.strip(),
                    "notes": notes.strip(), "id": fam_id}
    if uid is not None:
        sets.append("uid=:uid"); params["uid"] = uid
    if photo_path:
        sets.append("photo_path=:pp"); params["pp"] = photo_path
    if face_vector:
        sets.append("face_vector=:fv"); params["fv"] = face_vector

    db.execute(text(f"UPDATE familiars SET {', '.join(sets)} WHERE id=:id"), params)
    db.commit()

    if face_vector:
        _invalidate_face_roster()

    return RedirectResponse("/admin/familiars?updated=1", status_code=303)


@app.post("/admin/familiars/{fam_id}/toggle")
async def toggle_familiar(fam_id: int, request: Request,
                          db: Session = Depends(get_db)):
    """Enable/disable a Familiar."""
    role = request.session.get("role")
    if role != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    db.execute(text(
        "UPDATE familiars SET is_active = CASE WHEN is_active = 1 THEN 0 ELSE 1 END "
        "WHERE id = :id"
    ), {"id": fam_id})
    db.commit()
    return RedirectResponse("/admin/familiars", status_code=303)


@app.post("/admin/familiars/{fam_id}/delete")
async def delete_familiar(fam_id: int, request: Request,
                          db: Session = Depends(get_db)):
    """Hard-delete a Familiar and their attendance records."""
    user_id = request.session.get("user_id")
    role = request.session.get("role")
    if role != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    fam = db.execute(
        text("SELECT name, photo_path FROM familiars WHERE id = :id"),
        {"id": fam_id}
    ).fetchone()
    if not fam:
        return RedirectResponse("/admin/familiars?error=not_found", status_code=303)

    fam_name, photo_path = fam[0], fam[1]

    db.execute(text("DELETE FROM attendance WHERE familiar_id = :id"), {"id": fam_id})
    db.execute(text("DELETE FROM familiars WHERE id = :id"), {"id": fam_id})
    db.commit()

    # Purge deleted familiar from face roster cache immediately
    _invalidate_face_roster()

    if photo_path:
        try:
            from paths import data_root
            full = data_root() / photo_path if not os.path.isabs(photo_path) else Path(photo_path)
            if full.exists():
                full.unlink()
        except Exception:
            pass

    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id, action, target_type, target_id, details, timestamp) "
            "VALUES (:sid, 'delete_familiar', 'familiar', :tid, :det, :ts)"
        ), {"sid": user_id, "tid": fam_id, "det": fam_name, "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/admin/familiars?deleted=1", status_code=303)


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# PHASE 5: RFID Scan Override â€” check familiars before "unknown"
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# We wrap the existing /api/rfid-scan endpoint. The compiled handler
# returns JSON with {"status": "unknown"} when the UID isn't in the
# members table. We intercept that and check the familiars table.

_original_rfid_handler = None

def _find_and_remove_route(path: str, method: str = "POST"):
    """Find a route by path, remove it, return the old endpoint."""
    from starlette.routing import Route
    for i, route in enumerate(app.routes):
        if isinstance(route, Route) and route.path == path:
            if method.upper() in (route.methods or set()):
                old = route.endpoint
                app.routes.pop(i)
                return old
    return None

_original_rfid_handler = _find_and_remove_route("/api/rfid-scan", "POST")

@app.post("/api/rfid-scan")
async def rfid_scan_with_familiars(request: Request, db: Session = Depends(get_db)):
    """
    RFID scan = EXIT only.

    Flow:
        Face scan IN  → _face_cooldown[id] = now+86400  (inside, blocked)
        RFID scan OUT → state check: _face_cooldown[id] > now+60  ← inside?
                       → YES: _set_exit(5.0) → cooldown = now+5
                               compiled handler logs OUT + UNLOCK + LCD Goodbye
        5s later      → face scan re-arms
        Face scan IN  → _face_cooldown[id] = now+86400  (inside again)

    State check uses _face_cooldown directly — NOT the attendance table.
    This eliminates all race conditions between the face recognition loop
    (which sets _face_cooldown atomically) and our DB queries.

    Fallback: if _face_cooldown has no entry for this member (e.g. first
    scan ever, or after server restart), fall back to attendance table.
    """
    form = await request.form()
    uid_raw = form.get("uid", "")
    uid = uid_raw.strip().upper() if uid_raw else ""
    if not uid:
        return JSONResponse({"status": "error", "message": "Empty UID"})

    # ── Helper: deny when person is not inside ────────────────────────
    def _deny_not_inside(name: str):
        logger.info("RFID DENY — %s: not inside, face scan required to enter", name)
        try:
            from services.serial_bridge import serial_bridge as _sb_d
            _sb_d.send_command("DENY:Face scan first")
            _sb_d.send_command("BEEP")
        except Exception:
            pass
        return JSONResponse({
            "status": "denied",
            "message": f"{name}: face scan to enter first",
            "member_name": name,
            "alert": "red",
        })

    # ── Check familiars first ────────────────────────────────────────
    fam = db.execute(text(
        "SELECT id, name, is_active, photo_path FROM familiars WHERE uid = :u"
    ), {"u": uid}).fetchone()

    if fam:
        fam_id, fam_name, is_active, photo_path = fam[0], fam[1], fam[2], fam[3]
        if not is_active:
            return JSONResponse({"status": "denied",
                                 "message": f"{fam_name}: familiar disabled"})

        now_fam = _now_utc()
        first_name = fam_name.split()[0] if fam_name else "Familiar"

        # Log OUT
        _rfid_db = os.path.join(project_root, "gym.db")
        if not _robust_attendance_insert(_rfid_db, familiar_id=fam_id, direction="OUT", method="RFID"):
            logger.warning("Failed to log familiar RFID OUT for familiar_id=%d", fam_id)
        _face_rearm_until[-fam_id] = _time_module.time() + 5.0
        try:
            from services.access_control import access_control as _ac_fcf
            _ac_fcf._face_cooldown.pop(-fam_id, None)
        except Exception:
            pass
        try:
            from services.serial_bridge import serial_bridge
            serial_bridge.send_command("UNLOCK")
            serial_bridge.send_command(f"LCD:Goodbye!|{first_name}")
        except Exception:
            pass
        try:
            from services.access_control import access_control as _ac_evt
            _ac_evt._push_event({
                "type": "exit", "message": f"Goodbye, {fam_name}",
                "member_name": fam_name, "photo": photo_path or "",
                "alert": "green", "direction": "OUT", "method": "RFID",
                "time": _to_local(now_fam),
            })
        except Exception:
            pass
        return JSONResponse({"status": "ok", "message": f"Goodbye, {fam_name}",
                             "member_name": fam_name, "type": "familiar",
                             "direction": "OUT", "photo": photo_path or "", "alert": "green"})

    # ── Staff path (non-admin only) ──────────────────────────────────
    staff_row = db.execute(text(
        "SELECT id, username, display_name, is_active, photo_path "
        "FROM staff WHERE uid = :u AND role != 'admin'"
    ), {"u": uid}).fetchone()

    if staff_row:
        staff_id    = staff_row[0]
        staff_uname = staff_row[1]
        staff_dname = staff_row[2]
        staff_active = staff_row[3]
        staff_photo  = staff_row[4]
        staff_name   = staff_dname or staff_uname

        if not staff_active:
            try:
                from services.serial_bridge import serial_bridge as _sb_s
                _sb_s.send_command("DENY:Staff disabled")
            except Exception:
                pass
            return JSONResponse({"status": "denied",
                                  "message": f"{staff_name}: staff account disabled",
                                  "member_name": staff_name, "alert": "red"})

        now_st = _now_utc()
        first_name = staff_name.split()[0] if staff_name else "Staff"

        # Log OUT
        _rfid_db2 = os.path.join(project_root, "gym.db")
        if not _robust_attendance_insert(_rfid_db2, staff_id=staff_id, direction="OUT", method="RFID"):
            logger.warning("Failed to log staff RFID OUT for staff_id=%d", staff_id)
        _face_rearm_until[10000 + staff_id] = _time_module.time() + 5.0
        try:
            from services.access_control import access_control as _ac_fcs
            _ac_fcs._face_cooldown.pop(10000 + staff_id, None)
        except Exception:
            pass
        try:
            from services.serial_bridge import serial_bridge
            serial_bridge.send_command("UNLOCK")
            serial_bridge.send_command(f"LCD:Goodbye!|{first_name}")
        except Exception:
            pass
        try:
            from services.access_control import access_control as _ac_se
            _ac_se._push_event({
                "type": "exit", "message": f"Goodbye, {staff_name}",
                "member_name": staff_name, "photo": staff_photo or "",
                "alert": "green", "direction": "OUT", "method": "RFID",
                "time": _to_local(now_st),
            })
        except Exception:
            pass
        return JSONResponse({"status": "ok", "message": f"Goodbye, {staff_name}",
                             "member_name": staff_name, "type": "staff",
                             "direction": "OUT", "photo": staff_photo or "", "alert": "green"})

    # ── Member path ──────────────────────────────────────────────────
    member = db.execute(text(
        "SELECT id, name, status, expiry_date, photo_path, member_type "
        "FROM members WHERE uid = :u"
    ), {"u": uid}).fetchone()

    if not member:
        return JSONResponse({"status": "unknown", "message": f"Unknown badge {uid}"})

    member_id, member_name, member_status = member[0], member[1], member[2]
    member_type = member[5] or "regular"
    member_expiry = member[3]

    # Real-time expiry check — if status is active/frozen but expiry_date has passed,
    # update status immediately and re-fetch
    if member_status in ("active", "frozen") and member_expiry:
        try:
            from datetime import date as _d2
            exp_str = str(member_expiry)[:10]
            if exp_str < _d2.today().isoformat():
                db.execute(text(
                    "UPDATE members SET status='expired' WHERE id=:id"
                ), {"id": member_id})
                db.commit()
                member_status = "expired"
                logger.info("RFID scan: %s (id=%s) auto-marked expired (expiry=%s)",
                            member_name, member_id, exp_str)
        except Exception:
            pass

    # ── Walk-in members: bidirectional RFID with 3-in/3-out limit ──
    # Walk-ins don't face-scan — their RFID card works for both entry
    # AND exit, up to 3 times each direction per day.
    if member_type == "walkin":
        if member_status in ("deleted",):
            try:
                from services.serial_bridge import serial_bridge as _sb_wi
                _sb_wi.send_command("DENY:Walk-in expired")
            except Exception:
                pass
            return JSONResponse({"status": "denied",
                                 "message": f"{member_name}: Walk-in no longer active",
                                 "alert": "red"})

        now_wi = _now_utc()
        today_wi = now_wi.strftime("%Y-%m-%d")
        first_name = member_name.split()[0] if member_name else "Walk-in"

        # Count today's INs and OUTs
        counts = db.execute(text(
            "SELECT direction, COUNT(*) FROM attendance "
            "WHERE member_id=:id AND date(timestamp)=:d "
            "GROUP BY direction"
        ), {"id": member_id, "d": today_wi}).fetchall()
        cmap = {r[0].upper(): r[1] for r in counts}
        ins_used  = cmap.get("IN",  0)
        outs_used = cmap.get("OUT", 0)

        # Determine current state (inside or outside)
        last_att = db.execute(text(
            "SELECT direction FROM attendance WHERE member_id=:id "
            "AND date(timestamp)=:d ORDER BY id DESC LIMIT 1"
        ), {"id": member_id, "d": today_wi}).fetchone()
        currently_inside = last_att and last_att[0].upper() == "IN"

        if currently_inside:
            # RFID OUT — check limit
            if outs_used >= 3:
                try:
                    from services.serial_bridge import serial_bridge as _sb_wi2
                    _sb_wi2.send_command("DENY:Max exits reached")
                except Exception:
                    pass
                return JSONResponse({"status": "denied",
                                     "message": f"{member_name}: Max 3 exits reached for today",
                                     "alert": "red"})
            _rfid_db3 = os.path.join(project_root, "gym.db")
            if not _robust_attendance_insert(_rfid_db3, member_id=member_id, direction="OUT", method="RFID"):
                logger.warning("Failed to log walk-in RFID OUT for member_id=%d", member_id)
            try:
                from services.serial_bridge import serial_bridge as _sb_wi3
                _sb_wi3.send_command("UNLOCK")
                _sb_wi3.send_command(f"LCD:Goodbye!|{first_name}")
            except Exception:
                pass
            try:
                from services.access_control import access_control as _ac_wo
                _ac_wo._push_event({
                    "type": "exit", "message": f"Goodbye, {member_name}",
                    "member_name": member_name, "photo": member[4] or "",
                    "alert": "green", "direction": "OUT", "method": "RFID",
                    "time": _to_local(now_wi),
                })
            except Exception:
                pass
            return JSONResponse({"status": "ok", "message": f"Goodbye, {member_name}",
                                 "direction": "OUT", "member_name": member_name,
                                 "type": "walkin", "alert": "green"})
        else:
            # RFID IN — check limit
            if ins_used >= 3:
                try:
                    from services.serial_bridge import serial_bridge as _sb_wi4
                    _sb_wi4.send_command("DENY:Max entries reached")
                except Exception:
                    pass
                return JSONResponse({"status": "denied",
                                     "message": f"{member_name}: Max 3 entries reached for today",
                                     "alert": "red"})
            _rfid_db4 = os.path.join(project_root, "gym.db")
            if not _robust_attendance_insert(_rfid_db4, member_id=member_id, direction="IN", method="RFID"):
                logger.warning("Failed to log walk-in RFID IN for member_id=%d", member_id)
            _arm_tailgate(7.0)
            try:
                from services.serial_bridge import serial_bridge as _sb_wi5
                _sb_wi5.send_command("UNLOCK")
                _sb_wi5.send_command(f"LCD:Welcome!|{first_name}")
            except Exception:
                pass
            try:
                from services.access_control import access_control as _ac_wi
                _ac_wi._push_event({
                    "type": "unlock", "message": f"Welcome, {member_name}",
                    "member_name": member_name, "photo": member[4] or "",
                    "alert": "green", "direction": "IN", "method": "RFID",
                    "time": _to_local(now_wi),
                })
            except Exception:
                pass
            return JSONResponse({"status": "ok", "message": f"Welcome, {member_name}",
                                 "direction": "IN", "member_name": member_name,
                                 "type": "walkin", "alert": "green"})

    # Status / expiry checks for regular members
    if member_status not in ("active",):
        reason_map = {"frozen": "Membership Frozen",
                      "deleted": "Membership Removed",
                      "expired": "Membership Expired",
                      "cancelled": "Membership Cancelled"}
        lcd_msg = reason_map.get(member_status, member_status.title())
        try:
            from services.serial_bridge import serial_bridge as _sb3
            _sb3.send_command(f"DENY:{lcd_msg}")
        except Exception:
            pass
        return JSONResponse({"status": "denied", "message": f"{member_name}: {lcd_msg}",
                              "member_name": member_name, "alert": "red"})

    # Also deny if expiry_date has passed (status may not have been updated yet)
    expiry_date = member[3]
    if expiry_date:
        from datetime import date as _date_check
        try:
            exp = expiry_date if isinstance(expiry_date, str) else str(expiry_date)
            if exp[:10] < _date_check.today().isoformat():
                # Auto-update status to expired in DB
                db.execute(text(
                    "UPDATE members SET status='expired' WHERE id=:id"
                ), {"id": member_id})
                db.commit()
                try:
                    from services.serial_bridge import serial_bridge as _sb_exp
                    _sb_exp.send_command("DENY:Membership Expired")
                except Exception:
                    pass
                return JSONResponse({"status": "denied",
                                     "message": f"{member_name}: Membership Expired",
                                     "member_name": member_name, "alert": "red"})
        except Exception:
            pass

    # ── Regular member: RFID = EXIT (always log OUT) ──
    # Every RFID scan logs an OUT record. The face scan logs an IN record.
    # The cycle is: FACE→IN, RFID→OUT, FACE→IN, RFID→OUT, ...
    now_m = _now_utc()
    first_name = member_name.split()[0] if member_name else "Member"
    logger.info("RFID: member %d (%s) scanning → logging OUT", member_id, member_name)

    # Log OUT
    _rfid_db5 = os.path.join(project_root, "gym.db")
    if not _robust_attendance_insert(_rfid_db5, member_id=member_id, direction="OUT", method="RFID"):
        logger.warning("Failed to log member RFID OUT for member_id=%d", member_id)
    else:
        logger.info("RFID: member %d OUT logged", member_id)

    # Set 5s re-arm delay so face scan won't immediately re-trigger
    _face_rearm_until[member_id] = _time_module.time() + 5.0
    # Clear the face cooldown so the compiled _entry_loop will match again
    try:
        from services.access_control import access_control as _ac_fc
        _ac_fc._face_cooldown.pop(member_id, None)
        logger.info("RFID: member %d face cooldown cleared", member_id)
    except Exception as e:
        logger.warning("RFID: failed to clear face cooldown for member %d: %s", member_id, e)
    try:
        from services.serial_bridge import serial_bridge
        serial_bridge.send_command("UNLOCK")
        serial_bridge.send_command(f"LCD:Goodbye!|{first_name}")
    except Exception:
        pass
    try:
        from services.access_control import access_control as _ac_me
        _ac_me._push_event({
            "type": "exit", "message": f"Goodbye, {member_name}",
            "member_name": member_name, "photo": member[4] or "",
            "alert": "green", "direction": "OUT", "method": "RFID",
            "time": _to_local(now_m),
        })
    except Exception:
        pass
    return JSONResponse({"status": "ok", "message": f"Goodbye, {member_name}",
                         "direction": "OUT", "member_name": member_name, "alert": "green"})


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# STORE: Admin Product CRUD
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.get("/admin/store")
async def admin_store_page(request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    rows = db.execute(text(
        "SELECT id, name, category, description, price, stock, low_stock_threshold, is_active "
        "FROM store_products ORDER BY name"
    )).fetchall()
    products = [{"id": r[0], "name": r[1], "category": r[2] or "General",
                 "description": r[3] or "", "price": r[4], "stock": r[5],
                 "low_stock_threshold": r[6] or 5, "is_active": r[7]}
                for r in rows]
    return templates.TemplateResponse(request, "admin/store.html", {"products": products})


@app.post("/admin/store/products")
async def create_product(request: Request,
                         name: str = Form(...), category: str = Form("General"),
                         description: str = Form(""), price: float = Form(...),
                         stock: int = Form(0), low_stock_threshold: int = Form(5),
                         db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    if price < 0 or stock < 0 or low_stock_threshold < 0:
        return RedirectResponse("/admin/store?error=invalid", status_code=303)
    price = round(float(price), 2)
    stock = int(stock)
    dup = db.execute(text("SELECT id FROM store_products WHERE name = :n"), {"n": name.strip()}).fetchone()
    if dup:
        return RedirectResponse("/admin/store?error=dup", status_code=303)
    now = _now_utc()
    res = db.execute(text(
        "INSERT INTO store_products (name,category,description,price,stock,low_stock_threshold,is_active,created_at,updated_at) "
        "VALUES (:n,:cat,:desc,:p,:s,:t,1,:now,:now)"
    ), {"n": name.strip(), "cat": category.strip() or "General", "desc": description.strip(),
        "p": price, "s": stock, "t": low_stock_threshold, "now": now})
    new_id = res.lastrowid
    db.commit()
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'create_product','store_product',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": new_id,
            "det": f"Created product: {name.strip()} (₱{price:,.2f})",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse("/admin/store?created=1", status_code=303)


@app.post("/admin/store/products/{pid}/update")
async def update_product(pid: int, request: Request,
                         name: str = Form(...), category: str = Form("General"),
                         description: str = Form(""), price: float = Form(...),
                         low_stock_threshold: int = Form(5),
                         db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    if price < 0 or low_stock_threshold < 0:
        return RedirectResponse("/admin/store?error=invalid", status_code=303)
    price = round(float(price), 2)
    old = db.execute(text("SELECT name FROM store_products WHERE id=:id"), {"id": pid}).fetchone()
    old_name = old[0] if old else "Unknown"
    db.execute(text(
        "UPDATE store_products SET name=:n, category=:cat, description=:desc, "
        "price=:p, low_stock_threshold=:t, updated_at=:now WHERE id=:id"
    ), {"n": name.strip(), "cat": category.strip() or "General", "desc": description.strip(),
        "p": price, "t": low_stock_threshold, "now": _now_utc(), "id": pid})
    db.commit()
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'update_product','store_product',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": pid,
            "det": f"Updated product: {name.strip()} (was: {old_name})",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse("/admin/store?updated=1", status_code=303)


@app.post("/admin/store/products/{pid}/restock")
async def restock_product(pid: int, request: Request,
                          qty: int = Form(...), db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    if qty == 0:
        return RedirectResponse("/admin/store?error=invalid_qty", status_code=303)
    qty = max(-1000, min(qty, 1000))
    # atomic adjust: stock=stock+qty only if result >=0
    res = db.execute(text(
        "UPDATE store_products SET stock = stock + :qty, updated_at = :now WHERE id = :id AND stock + :qty >= 0"
    ), {"qty": qty, "now": _now_utc(), "id": pid})
    if res.rowcount == 0:
        db.rollback()
        # check existence
        exists = db.execute(text("SELECT 1 FROM store_products WHERE id=:id"), {"id": pid}).fetchone()
        if not exists:
            return RedirectResponse("/admin/store?error=not_found", status_code=303)
        return RedirectResponse("/admin/store?error=negative_stock", status_code=303)
    db.commit()
    row = db.execute(text("SELECT name, stock FROM store_products WHERE id=:id"), {"id": pid}).fetchone()
    pname = row[0] if row else str(pid)
    nstock = row[1] if row else 0
    action = "added" if qty > 0 else "removed"
    logger.info("Stock %s %d for %s (id=%d): new stock %d", action, abs(qty), pname, pid, nstock)
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'restock_product','store_product',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": pid,
            "det": f"{action.title()} {abs(qty)} stock for {pname}: new {nstock}",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse("/admin/store?restocked=1", status_code=303)


@app.post("/admin/store/products/{pid}/toggle")
async def toggle_product(pid: int, request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    old = db.execute(text("SELECT name, is_active FROM store_products WHERE id=:id"), {"id": pid}).fetchone()
    if not old:
        return RedirectResponse("/admin/store?error=not_found", status_code=303)
    product_name, was_active = old[0], old[1]
    db.execute(text(
        "UPDATE store_products SET is_active = CASE WHEN is_active=1 THEN 0 ELSE 1 END WHERE id=:id"
    ), {"id": pid})
    db.commit()
    new_status = "activated" if not was_active else "deactivated"
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'toggle_product','store_product',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": pid,
            "det": f"{new_status.title()} product: {product_name}",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse("/admin/store", status_code=303)


@app.post("/admin/store/products/{pid}/delete")
async def delete_product(pid: int, request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    has_sales = db.execute(text("SELECT 1 FROM store_sales WHERE product_id=:id LIMIT 1"), {"id": pid}).fetchone()
    if has_sales:
        return RedirectResponse("/admin/store?error=in_use", status_code=303)
    old = db.execute(text("SELECT name FROM store_products WHERE id=:id"), {"id": pid}).fetchone()
    product_name = old[0] if old else "Unknown"
    db.execute(text("DELETE FROM store_products WHERE id=:id"), {"id": pid})
    db.commit()
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'delete_product','store_product',:tid,:det,:ts)"
        ), {"sid": request.session.get("user_id"), "tid": pid,
            "det": f"Deleted product: {product_name}",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse("/admin/store?deleted=1", status_code=303)


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# STORE: Staff POS
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.get("/api/store/products")
async def api_store_products(request: Request, db: Session = Depends(get_db)):
    if not request.session.get("user_id"):
        return JSONResponse({"error": "unauthorized"}, status_code=401)
    rows = db.execute(text(
        "SELECT id, name, category, description, price, stock FROM store_products "
        "WHERE is_active=1 ORDER BY name"
    )).fetchall()
    return JSONResponse([{"id": r[0], "name": r[1], "category": r[2] or "General",
                          "description": r[3] or "", "price": r[4], "stock": r[5]} for r in rows])


@app.get("/store")
async def store_pos_page(request: Request):
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    return templates.TemplateResponse(request, "store.html", {})


_store_sell_lock = _threading.Lock()


@app.post("/store/sell")
async def store_sell(request: Request,
                     product_id: int = Form(...), quantity: int = Form(1),
                     payment_method: str = Form("cash"), notes: str = Form(""),
                     db: Session = Depends(get_db)):
    user_id = request.session.get("user_id")
    if not user_id:
        return RedirectResponse("/login", status_code=303)

    qty = max(1, quantity)

    # Use a lock to prevent race conditions on concurrent sales.
    # SQLite has coarse-grained locking — without this, two simultaneous
    # sales of the same product could both pass the stock check before
    # either decrements, resulting in negative stock.
    # validate inputs
    if payment_method not in ("cash", "gcash", "bank"):
        payment_method = "cash"
    qty = max(1, min(qty, 1000))
    with _store_sell_lock:
        product = db.execute(text(
            "SELECT id, name, price, stock, is_active FROM store_products WHERE id=:id"
        ), {"id": product_id}).fetchone()
        if not product or not product[4]:
            return RedirectResponse("/store?error=inactive", status_code=303)
        # atomic stock guard: decrement only if enough stock
        now = _now_utc()
        res = db.execute(text(
            "UPDATE store_products SET stock=stock-:q, updated_at=:now WHERE id=:id AND stock>=:q"
        ), {"q": qty, "now": now, "id": product_id})
        if res.rowcount == 0:
            db.rollback()
            return RedirectResponse("/store?error=stock", status_code=303)

        staff = db.execute(text("SELECT display_name FROM staff WHERE id=:id"), {"id": user_id}).fetchone()
        staff_name = staff[0] if staff else ""
        total = product[2] * qty

        db.execute(text(
            "INSERT INTO store_sales (product_id,product_name,quantity,unit_price,total_amount,"
            "payment_method,staff_id,staff_name,notes,created_at) "
            "VALUES (:pid,:pname,:qty,:up,:tot,:pm,:sid,:sname,:notes,:now)"
        ), {"pid": product[0], "pname": product[1], "qty": qty, "up": product[2],
            "tot": total, "pm": payment_method, "sid": user_id,
            "sname": staff_name, "notes": notes.strip()[:200], "now": now})
        db.commit()

    return RedirectResponse("/store?sold=1", status_code=303)


@app.get("/store/history")
async def store_history_page(request: Request,
                              date_from: str = "", date_to: str = "",
                              q: str = "", method: str = "",
                              db: Session = Depends(get_db)):
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    from datetime import date
    df = date_from or date.today().isoformat()
    dt = date_to or date.today().isoformat()
    filters = "WHERE date(created_at) BETWEEN :df AND :dt"
    params: dict = {"df": df, "dt": dt}
    if q:
        filters += " AND product_name LIKE :q"
        params["q"] = f"%{q}%"
    if method:
        filters += " AND payment_method = :m"
        params["m"] = method
    rows = db.execute(text(
        f"SELECT id, product_name, quantity, unit_price, total_amount, payment_method, "
        f"staff_name, notes, created_at FROM store_sales {filters} ORDER BY created_at DESC"
    ), params).fetchall()
    sales = [{"id": r[0], "product_name": r[1], "quantity": r[2], "unit_price": r[3],
              "total_amount": r[4], "payment_method": r[5], "staff_name": r[6],
              "notes": r[7], "created_at": str(r[8])[:16] if r[8] else ""} for r in rows]
    total_revenue = sum(s["total_amount"] for s in sales)
    total_qty = sum(s["quantity"] for s in sales)
    return templates.TemplateResponse(request, "store_history.html", {
        "sales": sales, "total_revenue": total_revenue, "total_qty": total_qty,
        "date_from": df, "date_to": dt, "q": q, "method": method,
    })


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# STORE: Analytics + Reports
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

def _store_revenue_30d(db):
    from datetime import date, timedelta
    today = date.today()
    labels, values = [], []
    for i in range(29, -1, -1):
        d = today - timedelta(days=i)
        row = db.execute(text(
            "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at)=:d"
        ), {"d": d.isoformat()}).fetchone()
        labels.append(d.strftime("%b %d"))
        values.append(float(row[0]))
    return labels, values


@app.get("/admin/store-analytics")
async def store_analytics_page(request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    today = date.today()
    rev_today = db.execute(text(
        "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at)=:d"
    ), {"d": today.isoformat()}).fetchone()[0]
    txn_today = db.execute(text(
        "SELECT COUNT(*) FROM store_sales WHERE date(created_at)=:d"
    ), {"d": today.isoformat()}).fetchone()[0]
    week_start = (today - timedelta(days=7)).isoformat()
    rev_week = db.execute(text(
        "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at) >= :d"
    ), {"d": week_start}).fetchone()[0]
    month_start = (today - timedelta(days=30)).isoformat()
    rev_month = db.execute(text(
        "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at) >= :d"
    ), {"d": month_start}).fetchone()[0]
    total_products = db.execute(text("SELECT COUNT(*) FROM store_products WHERE is_active=1")).fetchone()[0]
    low_stock = db.execute(text(
        "SELECT id,name,category,stock,low_stock_threshold FROM store_products "
        "WHERE is_active=1 AND stock <= low_stock_threshold ORDER BY stock"
    )).fetchall()
    low_stock_items = [{"id": r[0], "name": r[1], "category": r[2] or "General",
                        "stock": r[3], "low_stock_threshold": r[4]} for r in low_stock]
    # Top products
    top = db.execute(text(
        "SELECT product_name, SUM(total_amount) AS rev, SUM(quantity) AS qty "
        "FROM store_sales WHERE date(created_at) >= :d "
        "GROUP BY product_name ORDER BY rev DESC LIMIT 8"
    ), {"d": month_start}).fetchall()
    top_product_labels = [r[0] for r in top]
    top_product_values = [float(r[1]) for r in top]
    # Category breakdown
    cats = db.execute(text(
        "SELECT sp.category, COALESCE(SUM(ss.total_amount),0) "
        "FROM store_sales ss JOIN store_products sp ON ss.product_id=sp.id "
        "WHERE date(ss.created_at) >= :d GROUP BY sp.category"
    ), {"d": month_start}).fetchall()
    category_labels = [r[0] or "General" for r in cats]
    category_values = [float(r[1]) for r in cats]
    chart_labels, chart_values = _store_revenue_30d(db)
    return templates.TemplateResponse(request, "admin/store_analytics.html", {
        "revenue_today": float(rev_today), "txn_today": txn_today,
        "revenue_week": float(rev_week), "revenue_month": float(rev_month),
        "total_products": total_products, "low_stock_count": len(low_stock_items),
        "low_stock_items": low_stock_items,
        "chart_labels": chart_labels, "chart_values": chart_values,
        "top_products": bool(top), "top_product_labels": top_product_labels,
        "top_product_values": top_product_values,
        "category_data": bool(cats), "category_labels": category_labels,
        "category_values": category_values,
    })


@app.get("/admin/store-reports")
async def store_reports_page(request: Request,
                              start: str = "", end: str = "",
                              db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    if not start:
        start = (date.today() - timedelta(days=30)).isoformat()
    if not end:
        end = date.today().isoformat()
    rows = db.execute(text(
        "SELECT date(created_at) as d, COALESCE(SUM(total_amount),0) "
        "FROM store_sales WHERE date(created_at) BETWEEN :s AND :e GROUP BY d ORDER BY d"
    ), {"s": start, "e": end}).fetchall()
    label_dict = {r[0]: float(r[1]) for r in rows}
    from datetime import datetime as dt
    labels, values = [], []
    cur = dt.strptime(start, "%Y-%m-%d").date()
    end_d = dt.strptime(end, "%Y-%m-%d").date()
    while cur <= end_d:
        labels.append(cur.strftime("%b %d"))
        values.append(label_dict.get(cur.isoformat(), 0.0))
        from datetime import timedelta as td
        cur += td(days=1)
    grand_total = sum(values)
    # Top products
    top = db.execute(text(
        "SELECT product_name, SUM(total_amount) AS rev, SUM(quantity) AS qty "
        "FROM store_sales WHERE date(created_at) BETWEEN :s AND :e "
        "GROUP BY product_name ORDER BY rev DESC LIMIT 10"
    ), {"s": start, "e": end}).fetchall()
    top_products = [{"name": r[0], "qty": r[2], "revenue": float(r[1])} for r in top]
    # Payment breakdown
    pay = db.execute(text(
        "SELECT payment_method, COUNT(*), COALESCE(SUM(total_amount),0) "
        "FROM store_sales WHERE date(created_at) BETWEEN :s AND :e GROUP BY payment_method"
    ), {"s": start, "e": end}).fetchall()
    payment_breakdown = [{"method": r[0], "count": r[1], "total": float(r[2])} for r in pay]
    return templates.TemplateResponse(request, "admin/store_reports.html", {
        "start": start, "end": end, "labels": labels, "values": values,
        "grand_total": grand_total, "top_products": top_products,
        "payment_breakdown": payment_breakdown,
    })


@app.get("/admin/store-reports/csv")
async def store_reports_csv(request: Request,
                             start: str = "", end: str = "",
                             db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    import csv, io
    if not start: start = (date.today() - timedelta(days=30)).isoformat()
    if not end: end = date.today().isoformat()
    rows = db.execute(text(
        "SELECT created_at, product_name, quantity, unit_price, total_amount, payment_method, staff_name, notes "
        "FROM store_sales WHERE date(created_at) BETWEEN :s AND :e ORDER BY created_at"
    ), {"s": start, "e": end}).fetchall()
    buf = io.StringIO()
    w = csv.writer(buf)
    w.writerow(["Date", "Product", "Qty", "Unit Price", "Total", "Payment", "Staff", "Notes"])
    for r in rows:
        w.writerow(list(r))
    from fastapi.responses import Response
    return Response(content=buf.getvalue(), media_type="text/csv",
                    headers={"Content-Disposition": f"attachment; filename=store_sales_{start}_{end}.csv"})


@app.get("/admin/store-reports/pdf")
async def store_reports_pdf(request: Request,
                             start: str = "", end: str = "",
                             db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    import io
    if not start: start = (date.today() - timedelta(days=30)).isoformat()
    if not end: end = date.today().isoformat()
    rows = db.execute(text(
        "SELECT created_at, product_name, quantity, unit_price, total_amount, payment_method "
        "FROM store_sales WHERE date(created_at) BETWEEN :s AND :e ORDER BY created_at"
    ), {"s": start, "e": end}).fetchall()
    total = sum(float(r[4]) for r in rows)
    try:
        from reportlab.pdfgen import canvas as rlcanvas
        from reportlab.lib.pagesizes import letter
        buf = io.BytesIO()
        c = rlcanvas.Canvas(buf, pagesize=letter)
        c.setFont("Helvetica-Bold", 14)
        c.drawString(72, 750, "Solo Leveling Gym â€” Store Sales Report")
        c.setFont("Helvetica", 10)
        c.drawString(72, 732, f"Period: {start} to {end}")
        y = 710
        c.setFont("Helvetica-Bold", 9)
        c.drawString(72, y, "Date"); c.drawString(175, y, "Product"); c.drawString(330, y, "Qty")
        c.drawString(370, y, "Unit"); c.drawString(420, y, "Total"); c.drawString(480, y, "Method")
        y -= 14
        c.setFont("Helvetica", 9)
        for r in rows:
            if y < 60: c.showPage(); y = 750; c.setFont("Helvetica", 9)
            c.drawString(72, y, str(r[0])[:16]); c.drawString(175, y, str(r[1])[:22])
            c.drawString(330, y, str(r[2])); c.drawString(370, y, f"{float(r[3]):.2f}")
            c.drawString(420, y, f"{float(r[4]):.2f}"); c.drawString(480, y, str(r[5]))
            y -= 12
        y -= 6
        c.setFont("Helvetica-Bold", 10)
        c.drawString(370, y, f"TOTAL: â‚±{total:,.2f}")
        c.save()
        from fastapi.responses import Response
        return Response(content=buf.getvalue(), media_type="application/pdf",
                        headers={"Content-Disposition": f"attachment; filename=store_report_{start}_{end}.pdf"})
    except Exception as e:
        return JSONResponse({"error": str(e)}, status_code=500)


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# GYM ANALYTICS (moved from /admin/dashboard)
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

_remove_route("/admin/gym-analytics", "GET")

@app.get("/admin/gym-analytics")
async def gym_analytics_page(request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    today = date.today()
    month_start = (today - timedelta(days=30)).isoformat()
    week_start = (today - timedelta(days=7)).isoformat()
    rev_today = db.execute(text(
        "SELECT COALESCE(SUM(amount_paid),0) FROM sales WHERE date(created_at)=:d AND payment_status='PAID'"
    ), {"d": today.isoformat()}).fetchone()[0]
    rev_week = db.execute(text(
        "SELECT COALESCE(SUM(amount_paid),0) FROM sales WHERE date(created_at)>=:d AND payment_status='PAID'"
    ), {"d": week_start}).fetchone()[0]
    rev_month = db.execute(text(
        "SELECT COALESCE(SUM(amount_paid),0) FROM sales WHERE date(created_at)>=:d AND payment_status='PAID'"
    ), {"d": month_start}).fetchone()[0]
    # 30-day chart
    chart_labels, chart_values = [], []
    for i in range(29, -1, -1):
        d = today - timedelta(days=i)
        r = db.execute(text(
            "SELECT COALESCE(SUM(amount_paid),0) FROM sales WHERE date(created_at)=:d AND payment_status='PAID'"
        ), {"d": d.isoformat()}).fetchone()
        chart_labels.append(d.strftime("%b %d"))
        chart_values.append(float(r[0]))
    # Heatmap
    heatmap = {}
    for h in range(6, 23):
        r = db.execute(text(
            "SELECT COUNT(*) FROM attendance WHERE strftime('%H',timestamp)=:h "
            "AND timestamp >= datetime(:d)"
        ), {"h": f"{h:02d}", "d": month_start + " 00:00:00"}).fetchone()
        heatmap[h] = r[0] if r else 0
    # Demographics
    reg = db.execute(text("SELECT COUNT(*) FROM members WHERE member_type='regular' AND status!='deleted'")).fetchone()[0]
    stu = db.execute(text("SELECT COUNT(*) FROM members WHERE member_type='student' AND status!='deleted'")).fetchone()[0]
    wal = db.execute(text(
        "SELECT COUNT(*) FROM members WHERE member_type='walkin' AND date(created_at)>=:d AND status!='deleted'"
    ), {"d": month_start}).fetchone()[0]
    total_m = db.execute(text("SELECT COUNT(*) FROM members WHERE status!='deleted'")).fetchone()[0]
    active_m = db.execute(text("SELECT COUNT(*) FROM members WHERE status='active'")).fetchone()[0]
    expired_m = db.execute(text("SELECT COUNT(*) FROM members WHERE status='expired'")).fetchone()[0]
    return templates.TemplateResponse(request, "admin/gym_analytics.html", {
        "revenue_today": float(rev_today), "revenue_week": float(rev_week),
        "revenue_month": float(rev_month), "chart_labels": chart_labels,
        "chart_values": chart_values, "heatmap": heatmap,
        "total_members": total_m, "active_count": active_m, "expired_count": expired_m,
        "regular_count": reg, "student_count": stu, "walkin_count": wal,
    })


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# COMBINED DASHBOARD â€” override /admin/dashboard
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

def _find_and_remove_all_routes(path: str, method: str = "GET"):
    from starlette.routing import Route
    removed = []
    i = 0
    while i < len(app.routes):
        route = app.routes[i]
        if isinstance(route, Route) and route.path == path:
            if method.upper() in (route.methods or set()):
                removed.append(app.routes.pop(i))
                continue
        i += 1
    return removed


def _remove_routes_containing(needle: str, method: str = "GET"):
    """Remove any route whose path contains the given substring, including inside Mounts."""
    from starlette.routing import Route, Mount
    removed = []
    i = 0
    while i < len(app.routes):
        route = app.routes[i]
        if isinstance(route, Route) and needle.lower() in route.path.lower():
            if method.upper() in (route.methods or set()):
                removed.append(app.routes.pop(i))
                continue
        elif isinstance(route, Mount):
            # Check inside Mounts too
            if hasattr(route, 'routes'):
                j = 0
                while j < len(route.routes):
                    sub = route.routes[j]
                    if isinstance(sub, Route) and needle.lower() in sub.path.lower():
                        if method.upper() in (sub.methods or set()):
                            removed.append(route.routes.pop(j))
                            continue
                    j += 1
        i += 1
    return removed


_find_and_remove_all_routes("/admin/dashboard", "GET")

# ── Remove compiled /sales/end-of-day route ──
# Dump ALL routes for debugging
for _all_r in app.routes:
    from starlette.routing import Route as _allRoute, Mount as _allMount
    if isinstance(_all_r, _allRoute):
        logger.info("Route: %s %s", _all_r.methods, _all_r.path)
    elif isinstance(_all_r, _allMount):
        if hasattr(_all_r, 'routes'):
            for _all_sr in _all_r.routes:
                if isinstance(_all_sr, _allRoute):
                    logger.info("Mount(%s) Route: %s %s", _all_r.path, _all_sr.methods, _all_sr.path)

# Aggressively remove any route containing "end-of-day" or "end_of_day" in the path
_eod_removed = _remove_routes_containing("end-of-day", "GET")
_eod_removed += _remove_routes_containing("end_of_day", "GET")
logger.info("Removed %d end-of-day routes", len(_eod_removed))

@app.get("/admin/dashboard")
async def combined_dashboard(request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from datetime import date, timedelta
    today = date.today()
    month_start = (today - timedelta(days=30)).isoformat()
    week_start = (today - timedelta(days=7)).isoformat()

    def gym_rev(period_start):
        return float(db.execute(text(
            "SELECT COALESCE(SUM(amount_paid),0) FROM sales "
            "WHERE date(created_at)>=:d AND payment_status='PAID'"
        ), {"d": period_start}).fetchone()[0])

    def store_rev(period_start):
        return float(db.execute(text(
            "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at)>=:d"
        ), {"d": period_start}).fetchone()[0])

    gym_today = float(db.execute(text(
        "SELECT COALESCE(SUM(amount_paid),0) FROM sales "
        "WHERE date(created_at)=:d AND payment_status='PAID'"
    ), {"d": today.isoformat()}).fetchone()[0])
    store_today = float(db.execute(text(
        "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at)=:d"
    ), {"d": today.isoformat()}).fetchone()[0])

    active_members = db.execute(text("SELECT COUNT(*) FROM members WHERE status='active'")).fetchone()[0]

    # 30-day chart for both
    gym_values, store_values, chart_labels = [], [], []
    for i in range(29, -1, -1):
        d = today - timedelta(days=i)
        ds = d.isoformat()
        gv = float(db.execute(text(
            "SELECT COALESCE(SUM(amount_paid),0) FROM sales WHERE date(created_at)=:d AND payment_status='PAID'"
        ), {"d": ds}).fetchone()[0])
        sv = float(db.execute(text(
            "SELECT COALESCE(SUM(total_amount),0) FROM store_sales WHERE date(created_at)=:d"
        ), {"d": ds}).fetchone()[0])
        chart_labels.append(d.strftime("%b %d"))
        gym_values.append(gv)
        store_values.append(sv)

    return templates.TemplateResponse(request, "admin/dashboard.html", {
        "gym_today": gym_today, "store_today": store_today,
        "gym_week": gym_rev(week_start), "store_week": store_rev(week_start),
        "gym_month": gym_rev(month_start), "store_month": store_rev(month_start),
        "active_members": active_members,
        "chart_labels": chart_labels, "gym_values": gym_values, "store_values": store_values,
    })


# -- Override /admin/overrides - always latest-to-oldest --
_find_and_remove_all_routes("/admin/overrides", "GET")

@app.get("/admin/overrides")
async def admin_overrides_page(request: Request, db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from database.models import ManualOverride as _MO
    overrides = db.query(_MO).order_by(_MO.timestamp.desc()).all()
    return templates.TemplateResponse(request, "admin/overrides.html", {
        "overrides": overrides,
    })


# -- Override /admin/activity - always latest-to-oldest --
_find_and_remove_all_routes("/admin/activity", "GET")

@app.get("/admin/activity")
async def admin_activity_page(request: Request,
                              staff_filter: int = 0,
                              action_filter: str = "",
                              db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    from database.models import StaffActivity as _SA, Staff as _Staff
    q = db.query(_SA)
    if staff_filter > 0:
        q = q.filter(_SA.staff_id == staff_filter)
    if action_filter:
        af_esc = action_filter.replace("\\","\\\\").replace("%","\\%").replace("_","\\_")[:100]
        q = q.filter(_SA.action.like(f"%{af_esc}%", escape="\\"))
    activities = q.order_by(_SA.timestamp.desc()).limit(500).all()
    staff_list = db.query(_Staff).order_by(_Staff.display_name, _Staff.username).all()
    return templates.TemplateResponse(request, "admin/activity.html", {
        "activities": activities,
        "staff_list": staff_list,
        "staff_filter": staff_filter,
        "action_filter": action_filter,
    })


@app.get("/admin/inter-branch")
async def inter_branch_page(request: Request,
                            gym_filter: str = "", date_from: str = "", date_to: str = "",
                            export: str = "", db: Session = Depends(get_db)):
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    try:
        from license.validator import _db_path as _lic_db
        import sqlite3 as _sl
        _dbp = _lic_db(project_root)
        _conn = _sl.connect(_dbp, timeout=5)
        _row = _conn.execute("SELECT owner_email, gym_id FROM cloud_licenses LIMIT 1").fetchone()
        _conn.close()
        owner_email = _row[0] if _row else ""
        current_gym = _row[1] if _row else "default"
    except Exception:
        owner_email, current_gym = "", "default"
    if not owner_email:
        return templates.TemplateResponse(request, "admin/inter_branch.html", {"visits": [], "gyms": [], "owner_email": "", "gym_filter": gym_filter, "date_from": date_from, "date_to": date_to })
    try:
        _log_activity(db, request.session.get("user_id") or 0, "view_inter_branch", "attendance", 0, f"owner={owner_email} gym={current_gym} filter={gym_filter}")
    except Exception:
        pass
    gyms = []
    try:
        import sqlite3 as _s2
        _c2 = _s2.connect(_lic_db(project_root), timeout=5)
        gyms = [{"gym_id": r[0]} for r in _c2.execute("SELECT gym_id FROM gyms WHERE owner_email=?", (owner_email,)).fetchall()]
        if not gyms:
            gyms = [{"gym_id": current_gym}]
        _c2.close()
    except Exception:
        gyms = [{"gym_id": current_gym}]
    q = "SELECT a.member_id, a.gym_id, a.visitor_home_gym_id, a.timestamp, a.method, m.name as member_name, a.visitor_home_gym_id as home_gym, m.gym_id as home_gym2 FROM attendance a LEFT JOIN members m ON m.id=a.member_id WHERE (a.is_interbranch=1 OR (m.gym_id IS NOT NULL AND a.gym_id != m.gym_id)) AND (a.visitor_home_owner=:oe OR m.owner_email=:oe)"
    params = {"oe": owner_email}
    if gym_filter:
        q += " AND a.gym_id=:gf"
        params["gf"] = gym_filter
    if date_from:
        q += " AND date(a.timestamp) >= :df"
        params["df"] = date_from
    if date_to:
        q += " AND date(a.timestamp) <= :dt"
        params["dt"] = date_to
    q += " ORDER BY a.timestamp DESC LIMIT 500"
    try:
        rows = db.execute(text(q), params).fetchall()
        visits = [dict(r._mapping) if hasattr(r, "_mapping") else dict(r) for r in rows]
    except Exception:
        visits = []
    if export == "csv":
        import csv, io
        from starlette.responses import StreamingResponse
        out = io.StringIO()
        w = csv.writer(out)
        w.writerow(["time","member_id","member_name","home_gym","visited_gym","method"])
        for v in visits:
            w.writerow([v.get("timestamp"), v.get("member_id"), v.get("member_name"), v.get("visitor_home_gym_id"), v.get("gym_id"), v.get("method")])
        out.seek(0)
        return StreamingResponse(iter([out.getvalue()]), media_type="text/csv", headers={"Content-Disposition": "attachment; filename=inter_branch.csv"})
    return templates.TemplateResponse(request, "admin/inter_branch.html", {"visits": visits, "gyms": gyms, "owner_email": owner_email, "gym_filter": gym_filter, "date_from": date_from, "date_to": date_to})


@app.on_event("startup")
async def _ensure_audit_append_only():
    try:
        from database.connection import SessionLocal as _SAL
        _db = _SAL()
        _db.execute(text("CREATE TRIGGER IF NOT EXISTS audit_no_delete BEFORE DELETE ON staff_activities BEGIN SELECT RAISE(ABORT, 'audit append-only'); END"))
        _db.execute(text("CREATE TRIGGER IF NOT EXISTS audit_no_update BEFORE UPDATE ON staff_activities BEGIN SELECT RAISE(ABORT, 'audit append-only'); END"))
        _db.commit()
        _db.close()
    except Exception:
        pass


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# END OF DAY SUMMARY — override compiled route, add Store Sales
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•

@app.get("/sales/end-of-day")
async def end_of_day_summary(request: Request,
                              day: str = "",
                              db: Session = Depends(get_db)):
    """End of Day summary — Gym sales + Store sales + Expenses."""
    try:
        if not request.session.get("user_id"):
            return RedirectResponse("/login", status_code=303)

        from datetime import date as _date2
        target = day or _date2.today().isoformat()
        logger.info("End-of-day summary for target date: %s", target)

        # ── Gym sales (income) — only PAID sales ──
        gym_sales = db.execute(text(
            "SELECT s.id, s.receipt_no, s.member_id, s.plan_id, "
            "s.amount_paid, s.payment_method, s.payment_status, "
            "s.cashier_id, s.cashier_name, s.note, s.created_at, "
            "m.name as member_name, m.member_type FROM sales s "
            "LEFT JOIN members m ON s.member_id = m.id "
            "WHERE date(s.created_at) = :d AND s.payment_status = 'PAID' "
            "ORDER BY s.created_at DESC"
        ), {"d": target}).fetchall()

        gym_income_rows = []
        cash_income = 0.0
        gcash_income = 0.0
        bank_income = 0.0
        other_income = 0.0
        walkin_count = 0
        renewal_count = 0
        reg_count = 0

        for r in gym_sales:
            paid = float(r[4] or 0)
            method = (r[5] or "").lower()
            member_type = r[12] or ""
            member_name = r[11] or ""
            note = (r[9] or "").lower()

            receipt_no = r[1] or ""

            if "-W" in receipt_no:
                walkin_count += 1
            elif note.startswith("new registration"):
                reg_count += 1
            else:
                renewal_count += 1

            if method == "cash":
                cash_income += paid
            elif method in ("gcash",):
                gcash_income += paid
            elif method == "bank":
                bank_income += paid
            else:
                other_income += paid

            gym_income_rows.append(_NS(
                id=r[0], receipt_no=r[1], member_id=r[2], plan_id=r[3],
                amount_paid=paid, payment_method=r[5], payment_status=r[6],
                cashier_id=r[7], cashier_name=r[8], note=r[9], created_at=r[10],
                member=_NS(name=member_name) if member_name else None,
            ))

        total_income = cash_income + gcash_income + bank_income

        # ── Store sales (income) ──
        store_sales = db.execute(text(
            "SELECT id, product_id, product_name, quantity, unit_price, "
            "total_amount, payment_method, staff_id, staff_name, notes, created_at "
            "FROM store_sales WHERE date(created_at) = :d ORDER BY created_at DESC"
        ), {"d": target}).fetchall()

        store_income_rows = []
        store_cash = 0.0
        store_gcash = 0.0
        store_bank = 0.0
        store_other = 0.0

        for r in store_sales:
            amt = float(r[5] or 0)
            method = (r[6] or "").lower()
            if method == "cash":
                store_cash += amt
            elif method == "gcash":
                store_gcash += amt
            elif method == "bank":
                store_bank += amt
            else:
                store_other += amt

            store_income_rows.append(_NS(
                id=r[0], product_id=r[1], product_name=r[2], quantity=r[3],
                unit_price=r[4], total_amount=amt, payment_method=r[6],
                staff_id=r[7], staff_name=r[8], notes=r[9],
                created_at=str(r[10])[:16] if r[10] else "",
            ))

        total_store_income = store_cash + store_gcash + store_bank + store_other
        cash_income += store_cash
        gcash_income += store_gcash
        bank_income += store_bank
        other_income += store_other
        total_income += total_store_income

        # ── Expenses ──
        expenses = db.execute(text(
            "SELECT id, amount, category, description, payment_method, "
            "staff_id, created_at "
            "FROM expenses WHERE date(created_at) = :d ORDER BY created_at DESC"
        ), {"d": target}).fetchall()

        expense_rows = []
        cash_expenses = 0.0
        gcash_expenses = 0.0
        total_expenses = 0.0

        for r in expenses:
            amt = float(r[1] or 0)
            method = (r[4] or "").lower()
            total_expenses += amt
            if method == "cash":
                cash_expenses += amt
            elif method == "gcash":
                gcash_expenses += amt

            expense_rows.append(_NS(
                id=r[0], amount=amt, category=r[2], description=r[3],
                payment_method=r[4], staff_id=r[5], created_at=r[6],
            ))

        net_revenue = total_income - total_expenses
        total_transactions = len(gym_sales) + len(store_sales)

        return templates.TemplateResponse(request, "end_of_day.html", {
            "target_date": target,
            "total_income": total_income,
            "cash_income": cash_income,
            "gcash_income": gcash_income,
            "bank_income": bank_income,
            "other_income": other_income,
            "sales": gym_income_rows,
            "gym_sales_count": len(gym_income_rows),
            "store_sales": store_income_rows,
            "store_sales_count": len(store_income_rows),
            "total_store_income": total_store_income,
            "expenses": expense_rows,
            "total_expenses": total_expenses,
            "cash_expenses": cash_expenses,
            "gcash_expenses": gcash_expenses,
            "net_revenue": net_revenue,
            "total_transactions": total_transactions,
            "walkin_count": walkin_count,
            "renewal_count": renewal_count,
            "reg_count": reg_count,
        })
    except Exception as _eod_e:
        logger.error("End-of-day error: %s", _eod_e, exc_info=True)
        from datetime import date as _date3
        return templates.TemplateResponse(request, "end_of_day.html", {
            "target_date": day or _date3.today().isoformat(),
            "total_income": 0, "cash_income": 0, "gcash_income": 0, "bank_income": 0,
            "other_income": 0, "sales": [], "gym_sales_count": 0,
            "store_sales": [], "store_sales_count": 0, "total_store_income": 0,
            "expenses": [], "total_expenses": 0, "cash_expenses": 0,
            "gcash_expenses": 0, "net_revenue": 0, "total_transactions": 0,
            "walkin_count": 0, "renewal_count": 0, "reg_count": 0,
        })

logger.info("End-of-day route registered at /sales/end-of-day")


# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# Server entry point
# â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•â•
# ══════════════════════════════════════════════════════════════════
# TIER 2 OVERRIDES — Renewal, Walk-in, Toggle-entry, Coach/Plan Delete,
# Admin Password Change
# ══════════════════════════════════════════════════════════════════

# ── Remove compiled routes we're overriding ──
# Dump ALL routes before removal for debugging
for _wr in app.routes:
    from starlette.routing import Route as _wrRoute, Mount as _wrMount
    if isinstance(_wr, _wrRoute):
        if 'walkin' in _wr.path.lower():
            logger.info("Walkin route BEFORE removal: %s %s", _wr.methods, _wr.path)
    elif isinstance(_wr, _wrMount):
        if hasattr(_wr, 'routes'):
            for _wsr in _wr.routes:
                if isinstance(_wsr, _wrRoute):
                    if 'walkin' in _wsr.path.lower():
                        logger.info("Walkin route in Mount(%s) BEFORE removal: %s %s", _wr.path, _wsr.methods, _wsr.path)

_walkin_removed = _remove_all_routes("/sales/walkin", ["GET", "POST"])
logger.info("Removed %d walkin routes", len(_walkin_removed))

# Also remove any route that contains "walkin" in the path
# But exclude our own routes that we're about to register
_walkin_removed2 = _remove_routes_containing("walkin", "POST")
logger.info("Also removed %d walkin-containing POST routes", len(_walkin_removed2))

# Dump routes after removal
for _wr2 in app.routes:
    from starlette.routing import Route as _wrRoute2, Mount as _wrMount2
    if isinstance(_wr2, _wrRoute2):
        if 'walkin' in _wr2.path.lower():
            logger.info("Walkin route AFTER removal: %s %s", _wr2.methods, _wr2.path)
    elif isinstance(_wr2, _wrMount2):
        if hasattr(_wr2, 'routes'):
            for _wsr2 in _wr2.routes:
                if isinstance(_wsr2, _wrRoute2):
                    if 'walkin' in _wsr2.path.lower():
                        logger.info("Walkin route in Mount(%s) AFTER removal: %s %s", _wr2.path, _wsr2.methods, _wsr2.path)

_remove_all_routes("/sales/renew/{member_id}", ["GET", "POST"])
_remove_all_routes("/sales/history", ["GET"])
_remove_all_routes("/members/walkins/list", ["GET"])  # redirect to /walkins
_remove_all_routes("/members/{member_id}/toggle-entry", ["POST"])
_remove_all_routes("/members/{member_id}/update", ["POST"])
_remove_all_routes("/expenses", ["GET"])
_remove_all_routes("/expenses/", ["GET"])
_remove_route("/members/", "GET")
_remove_route("/members", "GET")


@app.get("/members/walkins/list")
async def walkins_list_redirect(request: Request):
    """Redirect compiled walkins/list to our /walkins override."""
    return RedirectResponse("/walkins", status_code=302)


@app.get("/sales/renew/{member_id}")
async def renew_get(member_id: int, request: Request,
                    db: Session = Depends(get_db)):
    """Render the membership renewal form."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    member = db.execute(text(
        "SELECT id, name, expiry_date, is_student, status, student_id_photo, discount_type, voucher_code, discount_id_number FROM members WHERE id=:id"
    ), {"id": member_id}).fetchone()
    if not member:
        return RedirectResponse("/members?error=not_found", status_code=303)
    plans = db.execute(text(
        "SELECT id, name, duration_days, regular_price, student_price FROM plans "
        "WHERE is_active=1 ORDER BY duration_days"
    )).fetchall()
    m = {"id": member[0], "name": member[1], "expiry_date": member[2],
         "is_student": member[3], "status": member[4],
         "student_id_photo": member[5] or "",
         "discount_type": member[6] or "",
         "voucher_code": member[7] or "",
         "discount_id_number": member[8] or ""}
    p_list = [{"id": p[0], "name": p[1], "duration_days": p[2],
               "regular_price": p[3], "student_price": p[4]} for p in plans]
    
    from datetime import date
    today_str = date.today().isoformat()
    
    voucher_titles = _get_available_voucher_titles(db, member_id)
    
    # Voucher error messages
    voucher_error = request.query_params.get("error", "")
    error_msgs = {
        "voucher_unavailable": "No available codes for that voucher type.",
        "voucher_already_used": "You have already used this voucher type.",
        "discount_photo_required": "Discount ID photo is required for this discount type.",
    }

    return templates.TemplateResponse(request, "renew.html", {
        "member": type("M", (), m)(), "plans": [type("P", (), pp)() for pp in p_list],
        "today": today_str,
        "voucher_titles": voucher_titles,
        "error": error_msgs.get(voucher_error, request.query_params.get("error", "")),
    })


@app.post("/sales/renew/{member_id}")
async def renew_post(member_id: int, request: Request,
                     plan_id: int = Form(...), amount: float = Form(...),
                     method: str = Form("cash"),
                     start_date: str = Form(""),
                     end_date: str = Form(""),
                     discount_type: str = Form(""),
                     discount_id_file: UploadFile = File(None),
                     discount_id_number: str = Form(""),
                     voucher_title: str = Form(""),
                     voucher_id_file: UploadFile = File(None),
                     remove_discount_photo: str = Form(""),
                     db: Session = Depends(get_db)):
    """Process membership renewal: create sale + extend expiry."""
    user_id = request.session.get("user_id")
    if not user_id:
        return RedirectResponse("/login", status_code=303)

    # Real-time expiry check before renewal
    today_str = _now_utc().strftime("%Y-%m-%d")
    db.execute(text(
        "UPDATE members SET status='expired' "
        "WHERE expiry_date IS NOT NULL AND expiry_date < :today "
        "AND status IN ('active', 'frozen')"
    ), {"today": today_str})
    db.commit()

    member = db.execute(text("SELECT id, name, expiry_date, is_student, student_id_photo FROM members WHERE id=:id"),
                        {"id": member_id}).fetchone()
    if not member:
        return RedirectResponse("/members?error=not_found", status_code=303)
    plan = db.execute(text("SELECT id, name, duration_days FROM plans WHERE id=:id"),
                      {"id": plan_id}).fetchone()
    if not plan:
        return RedirectResponse(f"/sales/renew/{member_id}?error=plan", status_code=303)

    from datetime import date, timedelta, datetime
    current_exp = member[2]
    if current_exp:
        try:
            if isinstance(current_exp, str):
                current_exp = date.fromisoformat(current_exp)
        except Exception:
            current_exp = date.today()
    else:
        current_exp = date.today()
    
    # Use provided start_date if given, otherwise use max(current_exp, today)
    if start_date.strip():
        try:
            base = date.fromisoformat(start_date.strip())
        except Exception:
            base = max(current_exp, date.today())
    else:
        base = max(current_exp, date.today())
    
    # Use provided end_date if given, otherwise calculate from plan duration
    if end_date.strip():
        try:
            new_expiry = date.fromisoformat(end_date.strip())
        except Exception:
            new_expiry = base + timedelta(days=plan[2])
    else:
        new_expiry = base + timedelta(days=plan[2])

    now = _now_utc()
    staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                           {"id": user_id}).fetchone()
    cashier_name = staff_row[0] if staff_row else ""
    
    # Get plan name for sale note
    plan_name = plan[1]
    
    # Handle discount type
    discount_type = discount_type.strip().lower() if discount_type.strip() else ""
    has_discount = discount_type in ("student", "senior", "pwd", "voucher")
    new_is_student = 1 if discount_type == "student" else 0

    # Read discount ID photo early for validation
    discount_photo_file_data = None
    file_to_read = discount_id_file
    if file_to_read is None:
        try:
            from starlette.datastructures import UploadFile as _UPF2
            all_form_data = await request.form()
            maybe_f = all_form_data.get("discount_id_file")
            if isinstance(maybe_f, _UPF2):
                file_to_read = maybe_f
        except Exception:
            pass
    if file_to_read:
        try:
            if file_to_read.file:
                file_to_read.file.seek(0)
            discount_photo_file_data = await file_to_read.read()
        except Exception:
            discount_photo_file_data = None

    # Validate discount ID photo for student/senior/pwd (unless member already has one)
    if discount_type in ("student", "senior", "pwd"):
        existing_photo = member[4] if len(member) > 4 else None
        has_new_photo = discount_photo_file_data and len(discount_photo_file_data) >= 50
        if not existing_photo and not has_new_photo:
            return RedirectResponse(f"/sales/renew/{member_id}?error=discount_photo_required", status_code=303)

    # Validate voucher title and assign a code
    voucher_code = ""
    voucher_assigned_title = ""
    voucher_photo_renew_data = None
    if discount_type == "voucher":
        voucher_title = voucher_title.strip()
        if not voucher_title:
            return RedirectResponse(f"/sales/renew/{member_id}?error=voucher_unavailable", status_code=303)
        # Check if member already used this voucher title
        used = db.execute(text(
            "SELECT 1 FROM voucher_usage WHERE voucher_title=:vt AND member_id=:mid"
        ), {"vt": voucher_title, "mid": member_id}).fetchone()
        if used:
            return RedirectResponse(f"/sales/renew/{member_id}?error=voucher_already_used", status_code=303)
        # Read voucher photo
        file_to_read_renew = voucher_id_file
        if file_to_read_renew is None:
            try:
                all_form_rn = await request.form()
                from starlette.datastructures import UploadFile as _UPF_RN
                maybe_rn = all_form_rn.get("voucher_id_file")
                if isinstance(maybe_rn, _UPF_RN):
                    file_to_read_renew = maybe_rn
            except Exception:
                pass
        if file_to_read_renew:
            try:
                if file_to_read_renew.file:
                    file_to_read_renew.file.seek(0)
                voucher_photo_renew_data = await file_to_read_renew.read()
            except Exception:
                pass
        vid, code = _assign_voucher_code(db, voucher_title, member_id)
        if not code:
            return RedirectResponse(f"/sales/renew/{member_id}?error=voucher_unavailable", status_code=303)
        voucher_code = code
        voucher_assigned_title = voucher_title

    # 1) If remove_discount_photo flag is set, clear discount photo
    if remove_discount_photo == "1":
        db.execute(text(
            "UPDATE members SET student_id_photo=NULL WHERE id=:id"
        ), {"id": member_id})

    # 2) Upload new discount ID photo if provided (reuse already-read data)
    discount_photo_path = None
    if discount_type == "voucher":
        if voucher_photo_renew_data and len(voucher_photo_renew_data) >= 50:
            try:
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                member_name = member[1]
                fname = f"voucher_{member_name.replace(' ', '_')}_{now.strftime('%Y%m%d_%H%M%S')}.jpg"
                (photos_dir / fname).write_bytes(voucher_photo_renew_data)
                discount_photo_path = f"static/photos/{fname}"
            except Exception as e:
                logger.warning("Voucher photo save error: %s", e)
    elif discount_photo_file_data and len(discount_photo_file_data) >= 50:
        try:
            from paths import data_root
            photos_dir = data_root() / "static" / "photos"
            photos_dir.mkdir(parents=True, exist_ok=True)
            member_name = member[1]
            fname = f"{discount_type}_id_{member_name.replace(' ', '_')}_{now.strftime('%Y%m%d_%H%M%S')}.jpg"
            (photos_dir / fname).write_bytes(discount_photo_file_data)
            discount_photo_path = f"static/photos/{fname}"
        except Exception as e:
            logger.warning("Discount ID photo save error: %s", e)

    # 3) Save discount ID number in dedicated column
    if discount_id_number.strip() and has_discount:
        db.execute(text(
            "UPDATE members SET discount_id_number=:idnum WHERE id=:id"
        ), {"idnum": discount_id_number.strip(), "id": member_id})

    receipt = f"R{now.strftime('%Y%m%d')}-RNW{now.strftime('%H%M%S')}"
    db.execute(text(
        "INSERT INTO sales (receipt_no, member_id, plan_id, amount_paid, payment_method, "
        "payment_status, cashier_id, cashier_name, note, created_at) "
        "VALUES (:rn,:mid,:pid,:amt,:pm,'PAID',:cid,:cn,:note,:now)"
    ), {"rn": receipt, "mid": member_id, "pid": plan_id, "amt": amount, "pm": method,
        "cid": user_id, "cn": cashier_name, "now": now,
        "note": f"Renewal - {plan_name}"})

    if discount_photo_path:
        db.execute(text(
            "UPDATE members SET student_id_photo=:photo WHERE id=:id"
        ), {"photo": discount_photo_path, "id": member_id})

    db.execute(text(
        "UPDATE members SET expiry_date=:exp, status='active', plan_id=:pid, "
        "is_student=:is_student, member_type=:member_type, "
        "discount_type=:discount_type, voucher_code=:voucher_code WHERE id=:id"
    ), {"exp": new_expiry.isoformat(), "pid": plan_id, "id": member_id,
        "is_student": new_is_student,
        "member_type": "student" if discount_type == "student" else "regular",
        "discount_type": discount_type or None,
        "voucher_code": voucher_code.strip() if discount_type == "voucher" else None})
    db.commit()

    # Voucher is one-time use — auto-clear after renewal
    if discount_type == "voucher":
        db.execute(text(
            "UPDATE members SET discount_type=NULL, voucher_code=NULL WHERE id=:id"
        ), {"id": member_id})
        db.commit()

    # Record voucher usage
    if discount_type == "voucher" and voucher_code.strip():
        vc = voucher_code.strip().upper()
        vrow = db.execute(text("SELECT id FROM vouchers WHERE code=:code"), {"code": vc}).fetchone()
        if vrow:
            db.execute(text(
                "INSERT OR IGNORE INTO voucher_usage (voucher_id, member_id, voucher_title, used_at) "
                "VALUES (:vid, :mid, :vt, :now)"
            ), {"vid": vrow[0], "mid": member_id, "vt": voucher_assigned_title, "now": _now_utc()})
            db.execute(text(
                "UPDATE vouchers SET used_count = (SELECT COUNT(*) FROM voucher_usage WHERE voucher_id=:vid) WHERE id=:vid"
            ), {"vid": vrow[0]})
            db.commit()

    return RedirectResponse(f"/members/{member_id}?renewed=1", status_code=303)


@app.get("/sales/walkin")
async def walkin_get(request: Request, db: Session = Depends(get_db)):
    """Render the walk-in entry form."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    rate_row = db.execute(text(
        "SELECT walkin_price FROM plans WHERE walkin_price > 0 AND is_active=1 LIMIT 1"
    )).fetchone()
    default_rate = rate_row[0] if rate_row else 100.0
    return templates.TemplateResponse(request, "walkin.html", {
        "default_rate": default_rate,
    })


@app.post("/sales/walkin")
async def walkin_post(request: Request,
                      amount: float = Form(...),
                      payment_method: str = Form("Cash"),
                      note: str = Form(""),
                      rfid_uid: str = Form(""),
                      db: Session = Depends(get_db)):
    """Process walk-in payment: create walk-in member + sale + RFID + unlock gate."""
    logger.info("=== WALKIN POST ENTRY POINT HIT ===")
    try:
        logger.info("Walk-in POST received: amount=%s, method=%s, note=%s, rfid=%s",
                    amount, payment_method, note, rfid_uid)
        user_id = request.session.get("user_id")
        if not user_id:
            logger.warning("Walk-in POST: no user_id in session")
            return RedirectResponse("/login", status_code=303)

        now = _now_utc()
        logger.info("Walk-in POST: querying staff info")
        staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                               {"id": user_id}).fetchone()
        cashier_name = staff_row[0] if staff_row else ""
        receipt = f"R{now.strftime('%Y%m%d')}-W{now.strftime('%H%M%S')}"
        walkin_name = note.strip() or "Walk-in"
        uid = rfid_uid.strip().upper() or None

        # Validate RFID uniqueness if provided
        if uid:
            logger.info("Walk-in POST: validating RFID uniqueness")
            dup = db.execute(text(
                "SELECT 1 FROM members WHERE uid=:u "
                "UNION ALL SELECT 1 FROM staff WHERE uid=:u "
                "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
            ), {"u": uid}).fetchone()
            if dup:
                return RedirectResponse("/sales/walkin?error=rfid_taken", status_code=303)

        # Create a walk-in member record (so they appear in walkins page + can RFID out)
        # Walk-ins no longer expire after 8 hours — they persist until manually logged out
        # or auto-logged out at midnight.
        logger.info("Walk-in POST: creating member record")
        res = db.execute(text(
            "INSERT INTO members (name, member_type, status, uid, created_at) "
            "VALUES (:name, 'walkin', 'active', :uid, :now)"
        ), {"name": walkin_name, "uid": uid, "now": now})
        walkin_member_id = res.lastrowid
        logger.info("Walk-in member created: id=%d, name=%s", walkin_member_id, walkin_name)

        # Create sale record linked to the walk-in member
        logger.info("Walk-in POST: creating sale record")
        db.execute(text(
            "INSERT INTO sales (receipt_no, member_id, amount_paid, payment_method, payment_status, "
            "cashier_id, cashier_name, note, created_at) "
            "VALUES (:rn, :mid, :amt, :pm, 'PAID', :cid, :cn, :note, :now)"
        ), {"rn": receipt, "mid": walkin_member_id, "amt": amount, "pm": payment_method,
            "cid": user_id, "cn": cashier_name, "note": f"Day Pass — {walkin_name}", "now": now})
        logger.info("Walk-in sale created: receipt=%s", receipt)

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Commit all database changes
        logger.info("Walk-in POST: committing database changes")
        db.commit()

        # Log attendance IN for the walk-in
        # method must be a valid AttendanceMethod enum value: FACE, RFID, MANUAL
        _rfid_db6 = os.path.join(project_root, "gym.db")
        logger.info("Walk-in POST: logging attendance IN")
        if not _robust_attendance_insert(_rfid_db6, member_id=walkin_member_id, direction="IN", method="MANUAL"):
            logger.warning("Failed to log walk-in attendance IN for member_id=%d", walkin_member_id)

        logger.info("Walk-in POST: sending UNLOCK command")
        try:
            from services.serial_bridge import serial_bridge
            serial_bridge.send_command("UNLOCK")
            serial_bridge.send_command(f"LCD:Walk-in|{walkin_name[:16]}")
        except Exception as e:
            logger.warning("Walk-in serial_bridge unlock failed: %s", e)

        logger.info("Walk-in POST: pushing event")
        try:
            from services.access_control import access_control as _ac_wi
            _ac_wi._push_event({
                "type": "unlock", "message": f"Walk-in: {walkin_name}",
                "member_name": walkin_name,
                "alert": "green", "direction": "IN", "method": "WALKIN",
                "time": _to_local(now),
            })
        except Exception as e:
            logger.warning("Walk-in push_event failed: %s", e)

        logger.info("Walk-in POST completed: member_id=%d, redirecting to /walkins", walkin_member_id)
        return RedirectResponse("/walkins", status_code=303)
    except Exception as e:
        logger.error("Walk-in POST error: %s", e, exc_info=True)
        return RedirectResponse("/sales/walkin?error=server_error", status_code=303)


@app.get("/sales/history")
async def sales_history_page(request: Request,
                              date_from: str = "", date_to: str = "",
                              member_name: str = "", payment_method: str = "",
                              payment_status: str = "",
                              db: Session = Depends(get_db)):
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    from datetime import date
    df = date_from or date.today().isoformat()
    dt = date_to or date.today().isoformat()
    q = "SELECT s.*, m.name as member_name, m.member_type as member_type, p.name as plan_name FROM sales s LEFT JOIN members m ON s.member_id=m.id LEFT JOIN plans p ON s.plan_id=p.id WHERE date(s.created_at) BETWEEN :df AND :dt"
    params = {"df": df, "dt": dt}
    if member_name:
        q += " AND m.name LIKE :mn"
        params["mn"] = f"%{member_name}%"
    if payment_method:
        q += " AND s.payment_method = :pm"
        params["pm"] = payment_method
    if payment_status:
        q += " AND s.payment_status = :ps"
        params["ps"] = payment_status
    q += " ORDER BY s.created_at DESC"
    rows = db.execute(text(q), params).fetchall()
    sales = []
    for r in rows:
        sales.append(_NS(
            id=r[0], receipt_no=r[1], member_id=r[2], plan_id=r[3],
            amount_paid=r[4], payment_method=r[5], payment_status=r[6],
            cashier_id=r[7], cashier_name=r[8], notes=r[9], created_at=r[10],
            member_name=r[12], member_type=r[13], plan_name=r[14],
            member={"id": r[2], "name": r[12]} if r[2] else None,
            plan={"name": r[14]} if r[14] else None,
        ))
    # Filtered total — sum all rows returned by the filtered query
    total_amount = sum(s.amount_paid for s in sales)
    # Also compute paid-only subtotal for reference
    paid_amount = sum(s.amount_paid for s in sales
                      if (s.payment_status or "").upper() == "PAID")
    return templates.TemplateResponse(request, "sales_history.html", {
        "sales": sales,
        "total_amount": total_amount,
        "paid_amount": paid_amount,
        "filters": _NS(
            date_from=df, date_to=dt,
            member_name=member_name, payment_method=payment_method,
            payment_status=payment_status,
        ),
    })


@app.get("/expenses")
async def expenses_page(request: Request,
                         date_from: str = "", date_to: str = "",
                         category: str = "",
                         db: Session = Depends(get_db)):
    """Expenses page with default today filter."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    from datetime import date
    df = date_from or date.today().isoformat()
    dt = date_to or date.today().isoformat()
    q = "SELECT id, amount, category, description, payment_method, staff_id, created_at FROM expenses WHERE date(created_at) BETWEEN :df AND :dt"
    params = {"df": df, "dt": dt}
    if category:
        q += " AND category = :cat"
        params["cat"] = category
    q += " ORDER BY created_at DESC"
    rows = db.execute(text(q), params).fetchall()
    expenses = []
    for r in rows:
        staff = None
        if r[5]:
            sr = db.execute(text("SELECT id, username, display_name FROM staff WHERE id=:id"), {"id": r[5]}).fetchone()
            if sr:
                staff = _NS(id=sr[0], username=sr[1], display_name=sr[2])
        expenses.append(_NS(
            id=r[0], amount=r[1], category=r[2], description=r[3],
            payment_method=r[4], staff=staff, created_at=r[6],
        ))
    total = sum(e.amount for e in expenses)
    categories = ["Utilities", "Supplies", "Maintenance", "Rent", "Other"]
    return templates.TemplateResponse(request, "expenses.html", {
        "expenses": expenses,
        "total": total,
        "categories": categories,
        "filters": _NS(date_from=df, date_to=dt, category=category),
    })


@app.post("/members/{member_id}/toggle-entry")
async def toggle_entry(member_id: int, request: Request,
                       db: Session = Depends(get_db)):
    """Manual toggle for walk-ins: max 3 INs and 3 OUTs per day to prevent abuse."""
    if not request.session.get("user_id"):
        return JSONResponse({"status": "error"}, status_code=401)

    member = db.execute(text(
        "SELECT id, name, member_type FROM members WHERE id=:id"
    ), {"id": member_id}).fetchone()
    if not member:
        return JSONResponse({"status": "error", "message": "Member not found"})

    name = member[1]
    member_type = member[2] or "regular"
    now = _now_utc()
    today_str = now.strftime("%Y-%m-%d")

    _MAX_TOGGLES = 3   # max 3 INs and 3 OUTs per walk-in per day

    # Count today's MANUAL IN and OUT records for this member
    counts = db.execute(text(
        "SELECT direction, COUNT(*) FROM attendance "
        "WHERE member_id=:id AND date(timestamp)=:d AND method='MANUAL' "
        "GROUP BY direction"
    ), {"id": member_id, "d": today_str}).fetchall()
    count_map = {r[0].upper(): r[1] for r in counts}
    ins_used  = count_map.get("IN",  0)
    outs_used = count_map.get("OUT", 0)

    # Get last direction to know which toggle to apply
    last = db.execute(text(
        "SELECT direction FROM attendance WHERE member_id=:id AND date(timestamp)=:d "
        "ORDER BY id DESC LIMIT 1"
    ), {"id": member_id, "d": today_str}).fetchone()
    currently_inside = last and last[0].upper() == "IN"

    if currently_inside:
        # Want to log OUT
        if member_type == "walkin" and outs_used >= _MAX_TOGGLES:
            return JSONResponse({
                "status": "limit_reached",
                "message": f"{name} has reached the max {_MAX_TOGGLES} exits for today.",
                "direction": "OUT", "ins_used": ins_used, "outs_used": outs_used,
                "max": _MAX_TOGGLES,
            })
        _rfid_db7 = os.path.join(project_root, "gym.db")
        if not _robust_attendance_insert(_rfid_db7, member_id=member_id, direction="OUT", method="MANUAL"):
            logger.warning("Failed to log manual OUT for member_id=%d", member_id)
        outs_used += 1
        try:
            from services.serial_bridge import serial_bridge
            serial_bridge.send_command("UNLOCK")
        except Exception:
            pass
        return JSONResponse({
            "status": "ok", "direction": "OUT", "name": name,
            "ins_used": ins_used, "outs_used": outs_used, "max": _MAX_TOGGLES,
        })
    else:
        # Want to log IN
        if member_type == "walkin" and ins_used >= _MAX_TOGGLES:
            return JSONResponse({
                "status": "limit_reached",
                "message": f"{name} has reached the max {_MAX_TOGGLES} entries for today.",
                "direction": "IN", "ins_used": ins_used, "outs_used": outs_used,
                "max": _MAX_TOGGLES,
            })
        _rfid_db8 = os.path.join(project_root, "gym.db")
        if not _robust_attendance_insert(_rfid_db8, member_id=member_id, direction="IN", method="MANUAL"):
            logger.warning("Failed to log manual IN for member_id=%d", member_id)
        ins_used += 1
        try:
            from services.serial_bridge import serial_bridge
            serial_bridge.send_command("UNLOCK")
        except Exception:
            pass
        return JSONResponse({
            "status": "ok", "direction": "IN", "name": name,
            "ins_used": ins_used, "outs_used": outs_used, "max": _MAX_TOGGLES,
        })


@app.post("/members/{member_id}/update")
async def member_update_override(
    member_id: int,
    request: Request,
    name: str = Form(""),
    expiry_date: str = Form(""),
    uid: str = Form(""),
    discount_type: str = Form(""),
    discount_id_number: str = Form(""),
    db: Session = Depends(get_db),
):
    """Override compiled member_update to also recalculate status from expiry_date.
    When admin sets expiry_date via the profile date picker:
      - expiry_date > today  → status = 'active'
      - expiry_date < today  → status = 'expired'
      - expiry_date = today  → status = 'active' (valid until end of day)
      - expiry_date empty    → keep current status
    """
    user_id = request.session.get("user_id")
    if not user_id:
        return RedirectResponse("/login", status_code=303)

    # Validate member exists
    member = db.execute(text("SELECT id, name, status FROM members WHERE id=:id"),
                        {"id": member_id}).fetchone()
    if not member:
        return RedirectResponse(f"/members/{member_id}?error=not_found", status_code=303)

    # Check RFID uniqueness if provided
    uid_val = uid.strip().upper() or None
    if uid_val:
        dup = db.execute(text(
            "SELECT 1 FROM members WHERE uid=:u AND id!=:id "
            "UNION ALL SELECT 1 FROM staff WHERE uid=:u "
            "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
        ), {"u": uid_val, "id": member_id}).fetchone()
        if dup:
            return RedirectResponse(f"/members/{member_id}?error=rfid_taken", status_code=303)

    # Build UPDATE fields
    sets = []
    params = {"id": member_id}

    if name.strip():
        sets.append("name=:name")
        params["name"] = name.strip()

    if uid_val is not None:
        sets.append("uid=:uid")
        params["uid"] = uid_val

    discount_type = discount_type.strip().lower() if discount_type.strip() else ""
    has_discount = discount_type in ("student", "senior", "pwd", "voucher")
    sets.append("is_student=:is_student")
    params["is_student"] = 1 if discount_type == "student" else 0
    sets.append("discount_type=:discount_type")
    params["discount_type"] = discount_type or None
    sets.append("member_type=:member_type")
    params["member_type"] = "student" if discount_type == "student" else "regular"

    # Recalculate status from new expiry_date
    if expiry_date.strip():
        from datetime import date as _d
        params["expiry_date"] = expiry_date.strip()
        sets.append("expiry_date=:expiry_date")
        try:
            exp = _d.fromisoformat(expiry_date.strip())
            today = _d.today()
            new_status = "active" if exp >= today else "expired"
            sets.append("status=:status")
            params["status"] = new_status
        except ValueError:
            pass  # invalid date format — keep existing status

    if sets:
        db.execute(text(f"UPDATE members SET {', '.join(sets)} WHERE id=:id"), params)
        db.commit()

    # Save/update discount ID number in dedicated column
    discount_id_number = discount_id_number.strip()
    if discount_id_number and discount_type in ("student", "senior", "pwd"):
        db.execute(text(
            "UPDATE members SET discount_id_number=:idnum WHERE id=:id"
        ), {"idnum": discount_id_number, "id": member_id})
    elif discount_type in ("", "voucher"):
        db.execute(text(
            "UPDATE members SET discount_id_number=NULL WHERE id=:id"
        ), {"id": member_id})
    db.commit()

    # Log activity
    try:
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'update_member','member',:tid,:det,:ts)"
        ), {"sid": user_id, "tid": member_id,
            "det": name.strip() or member[1], "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    return RedirectResponse(f"/members/{member_id}", status_code=303)


# ── Face Re-scan for existing members ─────────────────────────────
@app.get("/members/{member_id}/re-scan")
async def rescan_face_page(member_id: int, request: Request,
                           db: Session = Depends(get_db)):
    """Render face re-scan page for an existing member."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    member = db.execute(text(
        "SELECT id, name FROM members WHERE id=:id"
    ), {"id": member_id}).fetchone()
    if not member:
        return RedirectResponse("/members?error=not_found", status_code=303)
    return templates.TemplateResponse(request, "rescan_face.html", {
        "member": {"id": member[0], "name": member[1]},
    })


@app.post("/members/{member_id}/re-scan")
async def rescan_face_submit(member_id: int, request: Request,
                             face_front_b64: str = Form(""),
                             face_left_b64: str = Form(""),
                             face_right_b64: str = Form(""),
                             db: Session = Depends(get_db)):
    """Process 3-angle face capture and update member's face data."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    if not face_front_b64 and not face_left_b64 and not face_right_b64:
        return RedirectResponse(f"/members/{member_id}/re-scan?error=no_faces", status_code=303)
    import asyncio as _asyncio
    # Save photos
    front_path = _save_b64_photo(face_front_b64, f"member_{member_id}")
    left_path = _save_b64_photo(face_left_b64, f"member_{member_id}_l")
    right_path = _save_b64_photo(face_right_b64, f"member_{member_id}_r")
    all_paths = [p for p in (front_path, left_path, right_path) if p]
    if not all_paths:
        logger.warning("rescan_face_submit: no valid photos for member %s", member_id)
        return RedirectResponse(f"/members/{member_id}/re-scan?error=save_failed", status_code=303)
    # Encode face vector (runs in thread pool to avoid blocking)
    face_vector = await _asyncio.to_thread(_encode_multi_angle, all_paths)
    # Update member record
    photo = front_path or all_paths[0]
    db.execute(text(
        "UPDATE members SET face_vector=:fv, photo_path=:pp WHERE id=:id"
    ), {"fv": face_vector, "pp": photo, "id": member_id})
    db.commit()
    # Invalidate face roster so the new vector is picked up immediately
    try:
        _invalidate_face_roster()
    except Exception:
        pass
    logger.info("Face re-scanned for member %s (photo=%s)", member_id, photo)
    return RedirectResponse(f"/members/{member_id}", status_code=303)


@app.post("/admin/pricing/{plan_id}/delete")
async def delete_plan(plan_id: int, request: Request,
                      db: Session = Depends(get_db)):
    """Delete a gym plan (admin only)."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    has_sales = db.execute(text(
        "SELECT 1 FROM sales WHERE plan_id=:id LIMIT 1"
    ), {"id": plan_id}).fetchone()
    if has_sales:
        return RedirectResponse("/admin/pricing?error=in_use", status_code=303)
    db.execute(text("DELETE FROM plans WHERE id=:id"), {"id": plan_id})
    db.commit()
    return RedirectResponse("/admin/pricing?deleted=1", status_code=303)


# ── Override compiled /coaches routes ──
# Log all routes containing "coach" to debug
for _cr in app.routes:
    from starlette.routing import Route as _cRoute, Mount as _cMount
    if isinstance(_cr, _cRoute):
        if "coach" in _cr.path.lower():
            logger.info("Found coach route BEFORE removal: %s %s", _cr.methods, _cr.path)
    elif isinstance(_cr, _cMount):
        if hasattr(_cr, 'routes'):
            for _sr in _cr.routes:
                if isinstance(_sr, _cRoute) and "coach" in _sr.path.lower():
                    logger.info("Found mounted coach route BEFORE removal: %s %s", _sr.methods, _sr.path)

# Remove ALL routes that match /coaches patterns (handle trailing slashes and variations)
for _cp in ["/coaches", "/coaches/", "/coaches/add", "/coaches/add/",
            "/coaches/{coach_id}/toggle", "/coaches/{coach_id}/toggle/",
            "/coaches/{coach_id}/delete", "/coaches/{coach_id}/delete/",
            "/coaches/{coach_id}/students", "/coaches/{coach_id}/students/",
            "/coaches/{coach_id}/enroll", "/coaches/{coach_id}/enroll/",
            "/coaches/{coach_id}/enroll-new", "/coaches/{coach_id}/enroll-new/",
            "/coaches/assignment/{assignment_id}/delete", "/coaches/assignment/{assignment_id}/delete/",
            "/coaches/assignment/{assignment_id}/toggle", "/coaches/assignment/{assignment_id}/toggle/",
            "/coaches/assignment/{assignment_id}/renew", "/coaches/assignment/{assignment_id}/renew/",
            "/coaches/assignment/{assignment_id}/edit", "/coaches/assignment/{assignment_id}/edit/"]:
    _remove_all_routes(_cp, ["GET", "POST"])

# Also aggressively remove any remaining route with "coach" in the path
from starlette.routing import Route as _cRoute2
_coach_routes_before = [r for r in app.routes if isinstance(r, _cRoute2) and "coach" in r.path.lower()]
logger.info("Coach routes remaining after removal: %d", len(_coach_routes_before))
for _rr in _coach_routes_before:
    logger.info("  Remaining: %s %s", _rr.methods, _rr.path)


@app.get("/coaches")
async def coaches_page(request: Request,
                        status: str = "",
                        session_date: str = "",
                        db: Session = Depends(get_db)):
    """Coaches list with embedded coaching sessions and date filter."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    try:
        today_str = _now_utc().strftime("%Y-%m-%d")

        # Deactivate expired coaching assignments only (does NOT change member status)
        db.execute(text(
            "UPDATE coach_assignments SET is_active=0 "
            "WHERE expiry_date IS NOT NULL AND expiry_date < :today "
            "AND is_active=1"
        ), {"today": today_str})
        db.commit()

        coaches = db.execute(text(
            "SELECT s.id, s.username, s.display_name, s.is_active "
            "FROM staff s WHERE s.role='coach' ORDER BY s.display_name"
        )).fetchall()

        coach_data = []
        for c in coaches:
            coach_id = c[0]

            total = db.execute(text(
                "SELECT COUNT(*) FROM coach_assignments ca "
                "JOIN members m ON ca.member_id = m.id "
                "WHERE ca.coach_id=:cid AND ca.is_active=1 AND LOWER(m.status) != 'deleted'"
            ), {"cid": coach_id}).fetchone()[0]

            expired = db.execute(text(
                "SELECT COUNT(*) FROM coach_assignments ca "
                "JOIN members m ON ca.member_id = m.id "
                "WHERE ca.coach_id=:cid AND ca.expiry_date IS NOT NULL AND ca.expiry_date < :today AND LOWER(m.status) != 'deleted'"
            ), {"cid": coach_id, "today": today_str}).fetchone()[0]

            active = db.execute(text(
                "SELECT COUNT(*) FROM coach_assignments ca "
                "JOIN members m ON ca.member_id = m.id "
                "WHERE ca.coach_id=:cid AND ca.is_active=1 AND LOWER(m.status) = 'active'"
            ), {"cid": coach_id}).fetchone()[0]

            if status == "expired" and expired == 0:
                continue
            if status == "active" and active == 0:
                continue

            coach_data.append(_NS(
                coach=_NS(
                    id=c[0], username=c[1], display_name=c[2], is_active=c[3]
                ),
                student_count=total,
                active_count=active,
                expired_count=expired,
            ))

        # Load coaching sessions with optional date filter
        sessions = []
        try:
            if session_date:
                rows = db.execute(text(
                    "SELECT cs.id, cs.coach_id, s.display_name, s.username, "
                    "cs.member_name, cs.session_date, cs.notes, cs.created_at "
                    "FROM coaching_sessions cs "
                    "LEFT JOIN staff s ON cs.coach_id = s.id "
                    "WHERE cs.session_date = :sdate "
                    "ORDER BY cs.created_at DESC"
                ), {"sdate": session_date}).fetchall()
            else:
                rows = db.execute(text(
                    "SELECT cs.id, cs.coach_id, s.display_name, s.username, "
                    "cs.member_name, cs.session_date, cs.notes, cs.created_at "
                    "FROM coaching_sessions cs "
                    "LEFT JOIN staff s ON cs.coach_id = s.id "
                    "ORDER BY cs.session_date DESC, cs.created_at DESC"
                )).fetchall()
        except Exception:
            rows = []

        for r in rows:
            sessions.append(_NS(
                id=r[0], coach_id=r[1],
                coach_name=r[2] or r[3] or f"Coach #{r[1]}",
                member_name=r[4],
                session_date=r[5], notes=r[6] or "",
                created_at=r[7]
            ))

        return templates.TemplateResponse(request, "coaches.html", {
            "coach_data": coach_data,
            "status_filter": status,
            "sessions": sessions,
            "session_filter": session_date,
        })
    except Exception as e:
        logger.error("Coaches page error: %s", e, exc_info=True)
        raise


@app.post("/coaches/add-session")
async def add_coach_session_from_coaches(request: Request,
                                          coach_id: int = Form(...),
                                          member_name: str = Form(...),
                                          notes: str = Form(""),
                                          db: Session = Depends(get_db)):
    """Record a coaching session from the coaches page + create gym sale."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    now = _now_utc()
    today_str = now.strftime("%Y-%m-%d")

    # Look up coaching session price, gym share type & value from settings
    session_price = 500.0
    gym_ratio = 60.0
    gym_share_type = "pct"
    gym_share_peso = 0.0
    try:
        row = db.execute(text(
            "SELECT value FROM admin_settings WHERE key='coaching_session_price'"
        )).fetchone()
        if row:
            session_price = float(row[0])
        row = db.execute(text(
            "SELECT value FROM admin_settings WHERE key='coaching_gym_ratio'"
        )).fetchone()
        if row:
            gym_ratio = float(row[0])
        row = db.execute(text(
            "SELECT value FROM admin_settings WHERE key='coaching_gym_share_type'"
        )).fetchone()
        if row and row[0]:
            gym_share_type = row[0].strip().lower()
        row = db.execute(text(
            "SELECT value FROM admin_settings WHERE key='coaching_gym_share_peso'"
        )).fetchone()
        if row:
            gym_share_peso = float(row[0])
    except Exception:
        pass

    # Compute gym amount by share type
    if gym_share_type == "peso":
        gym_amount = round(max(0.0, gym_share_peso), 2)
        gym_audit_value = gym_amount
    else:
        gym_share_type = "pct"
        gym_amount = round(session_price * gym_ratio / 100, 2)
        gym_audit_value = gym_ratio

    # Insert coaching session record (with price & gym share for audit trail)
    db.execute(text(
        "INSERT INTO coaching_sessions "
        "(coach_id, member_name, session_date, price, gym_commission_pct, gym_share_type, notes, created_at, created_by) "
        "VALUES (:cid, :name, :sdate, :price, :gpct, :gtype, :notes, :now, :by)"
    ), {"cid": coach_id, "name": member_name.strip(),
        "sdate": today_str, "price": session_price, "gpct": gym_audit_value,
        "gtype": gym_share_type,
        "notes": notes.strip(),
        "now": now, "by": request.session.get("user_id")})

    # Generate receipt and create gym sale
    receipt = f"R{now.strftime('%Y%m%d')}-COA{now.strftime('%H%M%S')}"
    coach_name_row = db.execute(text(
        "SELECT display_name, username FROM staff WHERE id=:id"
    ), {"id": coach_id}).fetchone()
    coach_display = coach_name_row[0] or coach_name_row[1] or f"Coach #{coach_id}" if coach_name_row else f"Coach #{coach_id}"
    db.execute(text(
        "INSERT INTO sales (receipt_no, member_id, plan_id, amount_paid, payment_method, "
        "payment_status, cashier_id, cashier_name, note, created_at) "
        "VALUES (:rn, NULL, NULL, :amt, 'coaching', 'PAID', :cid, :cn, :note, :now)"
    ), {"rn": receipt, "amt": gym_amount, "cid": request.session.get("user_id"),
        "cn": coach_display, "now": now,
        "note": f"Coaching session — {coach_display} / {member_name.strip()}"})

    db.commit()
    return RedirectResponse("/coaches?session_created=1", status_code=303)


@app.post("/coaches/add")
async def add_coach(request: Request,
                     username: str = Form(...),
                     password: str = Form(...),
                     display_name: str = Form(""),
                     db: Session = Depends(get_db)):
    """Create a new coach account."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)

    dup = db.execute(text(
        "SELECT 1 FROM staff WHERE username=:u"
    ), {"u": username.strip()}).fetchone()
    if dup:
        return RedirectResponse("/coaches?error=dup_username", status_code=303)

    import bcrypt as _bcrypt
    pw_hash = _bcrypt.hashpw(password.encode(), _bcrypt.gensalt()).decode()

    now = _now_utc()
    db.execute(text(
        "INSERT INTO staff (username, password_hash, display_name, role, is_active, created_at) "
        "VALUES (:u, :pw, :dn, 'coach', 1, :now)"
    ), {"u": username.strip(), "pw": pw_hash, "dn": display_name.strip() or username.strip(), "now": now})
    db.commit()

    return RedirectResponse("/coaches?created=1", status_code=303)


@app.post("/coaches/{coach_id}/toggle")
async def toggle_coach(coach_id: int, request: Request,
                        db: Session = Depends(get_db)):
    """Toggle coach active/inactive status."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    db.execute(text(
        "UPDATE staff SET is_active = CASE WHEN is_active=1 THEN 0 ELSE 1 END "
        "WHERE id=:id AND role='coach'"
    ), {"id": coach_id})
    db.commit()

    return RedirectResponse("/coaches", status_code=303)


@app.post("/coaches/assignment/{assignment_id}/toggle")
async def toggle_coach_assignment(assignment_id: int, request: Request,
                                   db: Session = Depends(get_db)):
    """Toggle a coach assignment active/inactive."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    row = db.execute(text(
        "SELECT coach_id FROM coach_assignments WHERE id = :id"
    ), {"id": assignment_id}).fetchone()

    if not row:
        return RedirectResponse("/coaches", status_code=303)

    coach_id = row[0]
    db.execute(text(
        "UPDATE coach_assignments SET is_active = CASE WHEN is_active=1 THEN 0 ELSE 1 END "
        "WHERE id = :id"
    ), {"id": assignment_id})
    db.commit()

    return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)


@app.post("/coaches/{coach_id}/delete")
async def delete_coach(coach_id: int, request: Request,
                       db: Session = Depends(get_db)):
    """Remove a coach (admin only) — reverts to regular staff."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    db.execute(text("DELETE FROM coach_assignments WHERE coach_id=:id"), {"id": coach_id})
    db.execute(text("UPDATE staff SET role='staff' WHERE id=:id AND role='coach'"),
               {"id": coach_id})
    db.commit()
    return RedirectResponse("/coaches?deleted=1", status_code=303)


@app.post("/coaches/assignment/{assignment_id}/delete")
async def delete_coach_assignment(assignment_id: int, request: Request,
                                  db: Session = Depends(get_db)):
    """Hard-delete a student from a coach's roster (admin or staff)."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    # Fetch coach_id first so we can redirect back to the correct students page
    row = db.execute(text(
        "SELECT coach_id FROM coach_assignments WHERE id = :id"
    ), {"id": assignment_id}).fetchone()

    if not row:
        return RedirectResponse("/coaches", status_code=303)

    coach_id = row[0]
    db.execute(text("DELETE FROM coach_assignments WHERE id = :id"),
               {"id": assignment_id})
    db.commit()

    return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)


@app.post("/coaches/assignment/{assignment_id}/renew")
async def renew_coach_assignment(assignment_id: int, request: Request,
                                  coaching_plan_id: str = Form(""),
                                  start_date: str = Form(""),
                                  expiry_date: str = Form(""),
                                  notes: str = Form(""),
                                  db: Session = Depends(get_db)):
    """Renew a student's coaching assignment with a new plan and expiry."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    row = db.execute(text(
        "SELECT coach_id, member_id FROM coach_assignments WHERE id = :id"
    ), {"id": assignment_id}).fetchone()

    if not row:
        return RedirectResponse("/coaches", status_code=303)

    coach_id, member_id = row[0], row[1]

    from datetime import datetime, timedelta
    if start_date.strip():
        try:
            renew_date = datetime.fromisoformat(start_date.strip())
        except Exception:
            renew_date = _now_utc()
    else:
        renew_date = _now_utc()

    plan_expiry_date = None
    combined_notes = notes.strip()
    gym_commission = 0
    plan_name = ""

    if coaching_plan_id.strip():
        try:
            cp_id = int(coaching_plan_id)
            cp = db.execute(text(
                "SELECT name, price, duration_days, commission_pct FROM coaching_plans WHERE id=:id"
            ), {"id": cp_id}).fetchone()
            if cp:
                plan_name = f"Plan:{cp[0]}(₱{cp[1]})"
                if plan_name not in combined_notes:
                    combined_notes = f"{combined_notes} | {plan_name}" if combined_notes else plan_name
                if not expiry_date.strip():
                    if start_date.strip():
                        try:
                            start_dt = datetime.fromisoformat(start_date.strip())
                        except Exception:
                            start_dt = renew_date
                    else:
                        start_dt = renew_date
                    plan_expiry_date = (start_dt + timedelta(days=cp[2])).strftime("%Y-%m-%d")
                else:
                    plan_expiry_date = expiry_date.strip()
                gym_commission = cp[1] * (cp[3] / 100) if cp[3] > 0 else 0
        except (ValueError, Exception):
            pass

    if not plan_expiry_date and expiry_date.strip():
        plan_expiry_date = expiry_date.strip()

    db.execute(text(
        "UPDATE coach_assignments SET is_active=1, expiry_date=:exp, notes=:notes, "
        "coaching_plan_id=:plan_id, created_at=:renew_date "
        "WHERE id=:id"
    ), {"exp": plan_expiry_date, "notes": combined_notes,
        "plan_id": coaching_plan_id.strip() if coaching_plan_id.strip() else None,
        "renew_date": renew_date.isoformat(), "id": assignment_id})

    if gym_commission > 0:
        now = _now_utc()
        staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                               {"id": request.session.get("user_id")}).fetchone()
        cashier_name = staff_row[0] if staff_row else ""
        receipt = f"R{now.strftime('%Y%m%d')}-CR{now.strftime('%H%M%S')}"
        db.execute(text(
            "INSERT INTO sales (receipt_no, member_id, amount_paid, payment_method, "
            "payment_status, cashier_id, cashier_name, note, gym_commission, created_at) "
            "VALUES (:rn, :mid, :amt, 'Cash', 'PAID', :cid, :cn, :note, :gc, :now)"
        ), {"rn": receipt, "mid": member_id, "amt": gym_commission,
            "cid": request.session.get("user_id"), "cn": cashier_name,
            "note": f"Coaching Renewal: {plan_name} (Gym commission)",
            "gc": gym_commission, "now": now})

    db.commit()
    return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)


@app.post("/coaches/assignment/{assignment_id}/edit")
async def edit_coach_assignment(assignment_id: int, request: Request,
                                 coaching_plan_id: str = Form(""),
                                 start_date: str = Form(""),
                                 expiry_date: str = Form(""),
                                 notes: str = Form(""),
                                 db: Session = Depends(get_db)):
    """Edit a student's coaching assignment details only — no sales record."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    coach_id = None
    try:
        row = db.execute(text(
            "SELECT coach_id, member_id FROM coach_assignments WHERE id = :id"
        ), {"id": assignment_id}).fetchone()

        if not row:
            return RedirectResponse("/coaches", status_code=303)

        coach_id, member_id = row[0], row[1]

        from datetime import datetime as _dt, timedelta
        if start_date.strip():
            try:
                edit_date = _dt.fromisoformat(start_date.strip())
            except Exception:
                edit_date = _now_utc()
        else:
            edit_date = _now_utc()

        plan_expiry_date = None
        combined_notes = notes.strip()

        if coaching_plan_id.strip():
            try:
                cp_id = int(coaching_plan_id)
                cp = db.execute(text(
                    "SELECT name, price, duration_days FROM coaching_plans WHERE id=:id"
                ), {"id": cp_id}).fetchone()
                if cp:
                    plan_name = f"Plan:{cp[0]}(₱{cp[1]})"
                    if plan_name not in combined_notes:
                        combined_notes = f"{combined_notes} | {plan_name}" if combined_notes else plan_name
                    if not expiry_date.strip():
                        if start_date.strip():
                            try:
                                start_dt = _dt.fromisoformat(start_date.strip())
                            except Exception:
                                start_dt = edit_date
                        else:
                            start_dt = edit_date
                        plan_expiry_date = (start_dt + timedelta(days=cp[2])).strftime("%Y-%m-%d")
                    else:
                        plan_expiry_date = expiry_date.strip()
            except (ValueError, Exception):
                pass

        if not plan_expiry_date and expiry_date.strip():
            plan_expiry_date = expiry_date.strip()

        db.execute(text(
            "UPDATE coach_assignments SET expiry_date=:exp, notes=:notes, "
            "coaching_plan_id=:plan_id, created_at=:edit_date "
            "WHERE id=:id"
        ), {"exp": plan_expiry_date, "notes": combined_notes,
            "plan_id": coaching_plan_id.strip() if coaching_plan_id.strip() else None,
            "edit_date": edit_date.isoformat(), "id": assignment_id})

        db.commit()
        return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)
    except Exception as e:
        logger.error("Edit assignment error: %s", e, exc_info=True)
        return RedirectResponse(f"/coaches/{coach_id}/students?error=edit_failed" if coach_id else "/coaches?error=edit_failed", status_code=303)


# ══════════════════════════════════════════════════════════════════
# COACHING SESSIONS — 1-day session management with coach picker
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/coach/sessions", ["GET", "POST"])


@app.get("/coach/sessions")
async def coach_sessions_redirect():
    """Redirected to consolidated coaches page."""
    return RedirectResponse("/coaches", status_code=301)


@app.post("/coach/sessions/add")
async def coach_sessions_add_redirect():
    """Redirected to consolidated coaches page."""
    return RedirectResponse("/coaches", status_code=307)


@app.post("/coach/sessions/{session_id}/delete")
async def delete_coach_session(session_id: int, request: Request,
                                db: Session = Depends(get_db)):
    """Delete a coaching session."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    db.execute(text("DELETE FROM coaching_sessions WHERE id=:id"), {"id": session_id})
    db.commit()
    return RedirectResponse("/coach/sessions?deleted=1", status_code=303)


@app.post("/api/admin/verify-password")
async def admin_verify_password(request: Request,
                                password: str = Form(...),
                                db: Session = Depends(get_db)):
    """Verify the admin's password — used to gate hardware settings access.
    Returns {ok: true} if the current session user is admin and password matches.
    Accepts BOTH admin and staff sessions — the password must match the ADMIN
    account's password hash, regardless of who is logged in.
    This allows an admin to unlock hardware from any session.
    """
    user_id = request.session.get("user_id")
    role    = request.session.get("role")
    if not user_id:
        return JSONResponse({"ok": False, "reason": "not_logged_in"})

    import bcrypt as _bc

    # Get the admin account's password hash (id=1 or first admin)
    admin_row = db.execute(text(
        "SELECT id, password_hash FROM staff WHERE role='admin' LIMIT 1"
    )).fetchone()
    if not admin_row:
        return JSONResponse({"ok": False, "reason": "no_admin_account"})

    try:
        match = _bc.checkpw(password.encode(), admin_row[1].encode())
    except Exception as e:
        logger.warning("verify-password bcrypt error: %s", e)
        match = False

    logger.info("verify-password: user_id=%s role=%s match=%s", user_id, role, match)
    return JSONResponse({"ok": match})


@app.post("/admin/maintenance/change-password")
async def change_admin_password(request: Request,
                                current_password: str = Form(...),
                                new_password: str = Form(...),
                                db: Session = Depends(get_db)):
    """Change the logged-in admin's password."""
    user_id = request.session.get("user_id")
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    import bcrypt as _bc
    row = db.execute(text("SELECT password_hash FROM staff WHERE id=:id"),
                     {"id": user_id}).fetchone()
    if not row or not _bc.checkpw(current_password.encode(), row[0].encode()):
        return RedirectResponse("/admin/maintenance?error=wrong_password", status_code=303)
    new_hash = _bc.hashpw(new_password.encode(), _bc.gensalt()).decode()
    db.execute(text("UPDATE staff SET password_hash=:pw WHERE id=:id"),
               {"pw": new_hash, "id": user_id})
    db.commit()
    return RedirectResponse("/admin/maintenance?password_changed=1", status_code=303)


# ══════════════════════════════════════════════════════════════════
# ATTENDANCE PAGE OVERRIDE — show staff/familiar with proper names
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/attendance/", ["GET"])
_remove_all_routes("/attendance", ["GET"])


class _NS:
    """Simple namespace for template attribute access (mimics ORM objects)."""
    def __init__(self, **kw):
        self.__dict__.update(kw)


@app.get("/attendance")
@app.get("/attendance/")
async def attendance_page(request: Request, tab: str = "clients",
                          q: str = "", day: str = "",
                          db: Session = Depends(get_db)):
    """Attendance page with proper staff/familiar names in client tab."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    from datetime import date as _date
    filter_day = day or _date.today().isoformat()

    # Occupancy: count ALL person types whose last attendance record today is IN.
    # Uses MAX(id) to find each person's latest record, then checks direction='IN'.
    # Covers members, familiars, and staff independently (mutually exclusive FK cols).
    occ = db.execute(text("""
        SELECT
          (SELECT COUNT(*) FROM attendance a
           WHERE a.member_id IS NOT NULL AND date(a.timestamp)=:d AND a.direction='IN'
             AND a.id=(SELECT MAX(id) FROM attendance
                       WHERE member_id=a.member_id AND date(timestamp)=:d))
        + (SELECT COUNT(*) FROM attendance a
           WHERE a.familiar_id IS NOT NULL AND date(a.timestamp)=:d AND a.direction='IN'
             AND a.id=(SELECT MAX(id) FROM attendance
                       WHERE familiar_id=a.familiar_id AND date(timestamp)=:d))
        + (SELECT COUNT(*) FROM attendance a
           WHERE a.staff_id IS NOT NULL AND date(a.timestamp)=:d AND a.direction='IN'
             AND a.id=(SELECT MAX(id) FROM attendance
                       WHERE staff_id=a.staff_id AND date(timestamp)=:d))
    """), {"d": filter_day}).fetchone()
    occupancy = occ[0] if occ else 0

    if tab == "staff":
        # Staff tab — show staff_activities (clock_in/clock_out)
        # PLUS attendance records with staff_id (face/rfid entries)
        sql = (
            "SELECT a.timestamp, s.display_name, s.username, a.direction, a.method "
            "FROM attendance a LEFT JOIN staff s ON a.staff_id = s.id "
            "WHERE a.staff_id IS NOT NULL AND date(a.timestamp)=:d "
        )
        params = {"d": filter_day}
        if q:
            sql += " AND (s.display_name LIKE :q OR s.username LIKE :q)"
            params["q"] = f"%{q}%"
        sql += " ORDER BY a.id DESC"
        rows = db.execute(text(sql), params).fetchall()

        staff_logs = []
        for r in rows:
            staff_name = r[1] or r[2] or "Unknown"
            direction = r[3] or ""
            method = r[4] or ""
            # FACE = always clock_in (IN), RFID = always clock_out (OUT)
            # MANUAL = use stored direction value
            if method.upper() == "FACE":
                action = "clock_in"
            elif method.upper() == "RFID":
                action = "clock_out"
            else:
                action = "clock_in" if direction.upper() == "IN" else "clock_out"
            staff_logs.append(_NS(
                timestamp=r[0],
                staff=_NS(display_name=staff_name, username=r[2] or ""),
                action=action,
                details=f"{method} {direction}",
            ))

        # Also pull from staff_activities table (clock-in/out buttons)
        act_sql = (
            "SELECT sa.timestamp, s.display_name, s.username, sa.action, sa.details "
            "FROM staff_activities sa LEFT JOIN staff s ON sa.staff_id = s.id "
            "WHERE sa.action IN ('clock_in','clock_out') AND date(sa.timestamp)=:d "
        )
        act_params = {"d": filter_day}
        if q:
            act_sql += " AND (s.display_name LIKE :q OR s.username LIKE :q)"
            act_params["q"] = f"%{q}%"
        act_sql += " ORDER BY sa.timestamp DESC"
        act_rows = db.execute(text(act_sql), act_params).fetchall()
        for r in act_rows:
            staff_logs.append(_NS(
                timestamp=r[0],
                staff=_NS(display_name=r[1] or r[2] or "Unknown", username=r[2] or ""),
                action=r[3],
                details=r[4] or "",
            ))
        # Sort all by timestamp desc
        staff_logs.sort(key=lambda x: x.timestamp or "", reverse=True)

        return templates.TemplateResponse(request, "attendance.html", {
            "tab": tab, "q": q, "day": filter_day, "occupancy": occupancy,
            "staff_logs": staff_logs, "client_logs": [],
        })

    else:
        # Client tab — members + familiars + staff face/rfid entries
        sql = (
            "SELECT a.timestamp, a.member_id, a.familiar_id, a.staff_id, "
            "a.direction, a.method, "
            "m.name AS mname, m.uid AS muid, "
            "f.name AS fname,  f.uid AS fuid, "
            "s.display_name AS sname, s.uid AS suid "
            "FROM attendance a "
            "LEFT JOIN members m ON a.member_id = m.id "
            "LEFT JOIN familiars f ON a.familiar_id = f.id "
            "LEFT JOIN staff s ON a.staff_id = s.id "
            "WHERE date(a.timestamp)=:d "
        )
        params = {"d": filter_day}
        if q:
            sql += " AND (m.name LIKE :q OR f.name LIKE :q OR s.display_name LIKE :q)"
            params["q"] = f"%{q}%"
        sql += " ORDER BY a.id DESC"
        rows = db.execute(text(sql), params).fetchall()

        client_logs = []
        for r in rows:
            ts, mid, fid, sid = r[0], r[1], r[2], r[3]
            direction = r[4] or "IN"
            method = r[5] or ""
            mname, muid = r[6], r[7]
            fname, fuid = r[8], r[9]
            sname, suid = r[10], r[11]

            # Determine name and UID based on which FK is set
            if sid:
                name = sname or "Staff"
                uid = suid or ""
                person_type = "staff"
            elif fid:
                name = fname or "Familiar"
                uid = fuid or ""
                person_type = "familiar"
            elif mid:
                name = mname or "Member"
                uid = muid or ""
                person_type = "member"
            else:
                name = "Walk-in / Manual"
                uid = ""
                person_type = "walkin"

            client_logs.append(_NS(
                timestamp=ts,
                member=_NS(name=name, uid=uid) if name else None,
                direction=_NS(value=(
                    # FACE = always IN, RFID = always OUT (all person types).
                    # MANUAL keeps the stored direction value as-is.
                    "IN"  if method.upper() == "FACE"
                    else "OUT" if method.upper() == "RFID"
                    else direction.upper()
                )),
                method=_NS(value=method),
            ))

        return templates.TemplateResponse(request, "attendance.html", {
            "tab": tab, "q": q, "day": filter_day, "occupancy": occupancy,
            "staff_logs": [], "client_logs": client_logs,
        })


# ══════════════════════════════════════════════════════════════════
# WALK-IN RFID — 8-hour expiry + recycling
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/walkins", ["GET"])


@app.get("/walkins")
async def walkins_page(request: Request, db: Session = Depends(get_db)):
    """Walk-in management page with RFID support.
    Shows active walk-ins (created today), their RFID status, and inside/outside state.
    RFIDs assigned to walk-ins expire after 8 hours — they're automatically cleared
    so the physical card can be recycled for the next walk-in.
    """
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    from datetime import date as _dt

    # Get walk-ins from the last 7 days (or any with RFID still assigned).
    # Names are preserved — no data is deleted after the day ends.
    recent = _dt.today().isoformat()
    rows = db.execute(text(
        "SELECT m.id, m.name, m.photo_path, m.uid, m.created_at, "
        "  (SELECT direction FROM attendance WHERE member_id=m.id "
        "   AND date(timestamp)=:recent ORDER BY id DESC LIMIT 1) AS last_dir, "
        "  (SELECT timestamp FROM attendance WHERE member_id=m.id "
        "   ORDER BY id DESC LIMIT 1) AS last_time "
        "FROM members m "
        "WHERE m.member_type='walkin' AND (date(m.created_at) >= date('now', '-7 days') OR m.uid IS NOT NULL) "
        "ORDER BY m.created_at DESC"
    ), {"recent": recent}).fetchall()

    walkins = []
    for r in rows:
        is_inside = (r[5] or "").upper() == "IN"
        mid = r[0]
        today = _dt.today().isoformat()

        # Count today's MANUAL INs and OUTs for abuse prevention
        counts = db.execute(text(
            "SELECT direction, COUNT(*) FROM attendance "
            "WHERE member_id=:id AND date(timestamp)=:d AND method='MANUAL' "
            "GROUP BY direction"
        ), {"id": mid, "d": today}).fetchall()
        cmap = {c[0].upper(): c[1] for c in counts}

        walkins.append(_NS(
            id=mid, name=r[1] or "Walk-in",
            photo=r[2] or "", uid=r[3] or "",
            is_inside=is_inside,
            last_time=_to_local_time(r[6]) if r[6] else "",
            ins_used=cmap.get("IN", 0),
            outs_used=cmap.get("OUT", 0),
        ))

    # Get default walk-in rate for the renew panel
    rate_row = db.execute(text(
        "SELECT walkin_price FROM plans WHERE walkin_price > 0 AND is_active=1 LIMIT 1"
    )).fetchone()
    default_rate = rate_row[0] if rate_row else 100.0

    return templates.TemplateResponse(request, "walkins.html", {
        "walkins": walkins,
        "default_rate": default_rate,
    })


@app.post("/walkins/{member_id}/assign-rfid")
async def walkin_assign_rfid(member_id: int, request: Request,
                             uid: str = Form(""),
                             db: Session = Depends(get_db)):
    """Assign an RFID card to a walk-in member. Expires in 8 hours automatically."""
    if not request.session.get("user_id"):
        return JSONResponse({"status": "error"}, status_code=401)

    uid = uid.strip().upper()
    if not uid:
        return JSONResponse({"status": "error", "message": "Empty UID"})

    dup = db.execute(text(
        "SELECT 1 FROM members WHERE uid=:u AND id!=:id "
        "UNION ALL SELECT 1 FROM staff WHERE uid=:u "
        "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
    ), {"u": uid, "id": member_id}).fetchone()
    if dup:
        return JSONResponse({"status": "error", "message": "RFID already assigned to another person"})

    db.execute(text("UPDATE members SET uid=:u WHERE id=:id AND member_type='walkin'"),
               {"u": uid, "id": member_id})
    db.commit()
    return JSONResponse({"status": "ok", "uid": uid})


@app.post("/walkins/{member_id}/renew")
async def walkin_renew(member_id: int, request: Request,
                       new_uid: str = Form(""),
                       amount: float = Form(100.0),
                       db: Session = Depends(get_db)):
    """Renew a walk-in: new sale + new RFID + reset today's attendance counter."""
    user_id = request.session.get("user_id")
    if not user_id:
        return RedirectResponse("/login", status_code=303)

    new_uid = new_uid.strip().upper()
    if not new_uid:
        return RedirectResponse("/walkins?error=empty_uid", status_code=303)

    # Validate UID not already taken
    dup = db.execute(text(
        "SELECT 1 FROM members WHERE uid=:u AND id!=:id "
        "UNION ALL SELECT 1 FROM staff WHERE uid=:u "
        "UNION ALL SELECT 1 FROM familiars WHERE uid=:u"
    ), {"u": new_uid, "id": member_id}).fetchone()
    if dup:
        return RedirectResponse("/walkins?error=rfid_taken", status_code=303)

    # Confirm this is a walk-in member
    member = db.execute(text(
        "SELECT id, name FROM members WHERE id=:id AND member_type='walkin'"
    ), {"id": member_id}).fetchone()
    if not member:
        return RedirectResponse("/walkins", status_code=303)

    now = _now_utc()
    today = now.strftime("%Y-%m-%d")
    staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                           {"id": user_id}).fetchone()
    cashier_name = staff_row[0] if staff_row else ""
    receipt = f"R{now.strftime('%Y%m%d')}-W{now.strftime('%H%M%S')}-R"
    from datetime import timedelta
    new_expiry = (now + timedelta(hours=8)).strftime("%Y-%m-%d")

    # Create new sale record
    db.execute(text(
        "INSERT INTO sales (receipt_no, member_id, amount_paid, payment_method, "
        "payment_status, cashier_id, cashier_name, note, created_at) "
        "VALUES (:rn, :mid, :amt, 'Cash', 'PAID', :cid, :cn, :note, :now)"
    ), {"rn": receipt, "mid": member_id, "amt": amount,
        "cid": user_id, "cn": cashier_name,
        "note": f"Day Pass Renewal — {member[1]}", "now": now})

    # Update member: new UID (no expiry — walk-ins persist until logged out)
    db.execute(text(
        "UPDATE members SET uid=:uid WHERE id=:id AND member_type='walkin'"
    ), {"uid": new_uid, "id": member_id})

    # Reset today's attendance counter (delete today's records)
    db.execute(text(
        "DELETE FROM attendance WHERE member_id=:id AND date(timestamp)=:d"
    ), {"id": member_id, "d": today})

    # Log fresh entry IN
    _rfid_db9 = os.path.join(project_root, "gym.db")
    _robust_attendance_insert(_rfid_db9, member_id=member_id, direction="IN", method="MANUAL")

    db.commit()

    try:
        from services.serial_bridge import serial_bridge
        first = member[1].split()[0] if member[1] else "Walk-in"
        serial_bridge.send_command("UNLOCK")
        serial_bridge.send_command(f"LCD:Renewed!|{first}")
    except Exception:
        pass

    return RedirectResponse("/walkins", status_code=303)


@app.post("/walkins/{member_id}/update-name")
async def walkin_update_name(member_id: int, request: Request,
                              name: str = Form(""),
                              db: Session = Depends(get_db)):
    """Update walk-in client name. Sales and attendance records keep the original name."""
    logger.info("Walk-in name update: member_id=%d, name=%s", member_id, name)
    if not request.session.get("user_id"):
        logger.warning("Walk-in name update: no user_id in session")
        return JSONResponse({"status": "error"}, status_code=401)
    
    new_name = name.strip()
    if not new_name:
        return JSONResponse({"status": "error", "message": "Name cannot be empty"})
    
    db.execute(text(
        "UPDATE members SET name=:name WHERE id=:id AND member_type='walkin'"
    ), {"name": new_name, "id": member_id})
    db.commit()
    logger.info("Walk-in name updated: member_id=%d, new_name=%s", member_id, new_name)
    return JSONResponse({"status": "ok", "name": new_name})


@app.post("/walkins/{member_id}/logout")
async def walkin_logout(member_id: int, request: Request,
                        db: Session = Depends(get_db)):
    """Log out a walk-in: clear UID + log attendance OUT. Member record kept for history."""
    if not request.session.get("user_id"):
        return JSONResponse({"status": "error"}, status_code=401)

    # Log attendance OUT for the walk-in
    now = _now_utc()
    _rfid_db10 = os.path.join(project_root, "gym.db")
    if not _robust_attendance_insert(_rfid_db10, member_id=member_id, direction="OUT", method="MANUAL"):
        logger.warning("Failed to log walk-in logout OUT for member_id=%d", member_id)

    # Clear the RFID UID so card can be reused
    db.execute(text(
        "UPDATE members SET uid=NULL "
        "WHERE id=:id AND member_type='walkin'"
    ), {"id": member_id})
    db.commit()
    return JSONResponse({"status": "ok", "message": "Logged out — RFID cleared"})


@app.post("/walkins/{member_id}/delete")
async def walkin_delete(member_id: int, request: Request,
                        db: Session = Depends(get_db)):
    """Hard delete a walk-in: removes member + all attendance. Sales records are kept intact."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    # Get member name for logging
    member = db.execute(text(
        "SELECT name FROM members WHERE id=:id AND member_type='walkin'"
    ), {"id": member_id}).fetchone()
    
    if not member:
        return RedirectResponse("/walkins", status_code=303)

    # Delete attendance records
    db.execute(text("DELETE FROM attendance WHERE member_id=:id"), {"id": member_id})
    
    # NULL out sales references instead of deleting them (preserves accounting records)
    db.execute(text(
        "UPDATE sales SET member_id=NULL WHERE member_id=:id AND receipt_no LIKE '%-W%'"
    ), {"id": member_id})
    
    # Delete the member
    db.execute(text(
        "DELETE FROM members WHERE id=:id AND member_type='walkin'"
    ), {"id": member_id})
    db.commit()
    
    # Log activity
    try:
        user_id = request.session.get("user_id")
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'delete_walkin','member',:tid,:det,:ts)"
        ), {"sid": user_id, "tid": member_id,
            "det": f"Deleted walk-in: {member[0]}", "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    
    return RedirectResponse("/walkins", status_code=303)


# ══════════════════════════════════════════════════════════════════
# RESET-DATA OVERRIDE — fixes SQLite FK constraint blocking DROP TABLE
# The compiled handler uses Base.metadata.drop_all() which fails when
# foreign_keys=ON is active (SQLite blocks DROP on referenced tables).
# This override disables FK checks, drops all tables via raw SQL,
# then lets the compiled handler recreate them cleanly.
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/admin/maintenance/reset-data", ["POST"])


@app.post("/admin/maintenance/reset-data")
async def reset_data_override(request: Request,
                              confirm: str = Form(""),
                              db: Session = Depends(get_db)):
    """Factory reset — drops all tables with FK checks disabled, then recreates.
    Requires the confirmation field to equal 'RESET'.
    """
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    if confirm != "RESET":
        return RedirectResponse("/admin/maintenance?err=type_reset", status_code=303)

    try:
        import sqlite3 as _sq3, shutil as _sh, glob as _gl

        db_path = os.path.join(project_root, "gym.db")

        # ── 1. Backup first ──────────────────────────────────────
        from datetime import datetime as _dt_r
        ts = _now_utc().strftime("%Y%m%d_%H%M%S")
        backup_dir = os.path.join(project_root, "static", "backups")
        os.makedirs(backup_dir, exist_ok=True)
        backup_path = os.path.join(backup_dir, f"pre_reset_{ts}.db")
        _sh.copy2(db_path, backup_path)
        logger.info("Pre-reset backup saved: %s", backup_path)

        # ── 2. Disconnect all SQLAlchemy connections ─────────────
        try:
            from database.connection import engine as _engine
            _engine.dispose()
        except Exception as _e:
            logger.debug("engine.dispose: %s", _e)

        # ── 3. Drop all tables with FK disabled (raw sqlite3) ────
        conn = _sq3.connect(db_path, timeout=15)
        conn.execute("PRAGMA foreign_keys = OFF")
        conn.execute("PRAGMA journal_mode = DELETE")  # leave WAL mode for reset

        # Get all table names
        tables = [r[0] for r in conn.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'"
        ).fetchall()]
        logger.info("Dropping %d tables: %s", len(tables), tables)
        for tbl in tables:
            conn.execute(f"DROP TABLE IF EXISTS [{tbl}]")
        conn.execute("PRAGMA foreign_keys = ON")
        conn.execute("PRAGMA journal_mode = WAL")
        conn.commit()
        conn.close()
        logger.info("All tables dropped cleanly")

        # ── 4. Recreate schema via SQLAlchemy ORM ────────────────
        try:
            from database.connection import engine as _engine2, SessionLocal as _SL
            from database.models import Base as _Base
            _Base.metadata.create_all(_engine2)
            logger.info("Schema recreated via Base.metadata.create_all")
        except Exception as _ce:
            logger.warning("Schema recreate error: %s", _ce)

        # ── 4b. Create extension tables not in compiled ORM Base ─
        # store_products, store_sales, familiars are not in models.pyc
        # Also add extension columns to attendance (familiar_id, staff_id)
        _ext_tables = [
            """CREATE TABLE IF NOT EXISTS familiars (
                id INTEGER NOT NULL PRIMARY KEY, uid VARCHAR(32),
                face_vector BLOB, photo_path VARCHAR(256),
                name VARCHAR(128) NOT NULL, phone VARCHAR(20),
                notes TEXT, is_active BOOLEAN DEFAULT 1,
                created_by INTEGER REFERENCES staff(id), created_at DATETIME
            )""",
            """CREATE TABLE IF NOT EXISTS store_products (
                id INTEGER NOT NULL PRIMARY KEY, name VARCHAR(128) NOT NULL,
                category VARCHAR(64) DEFAULT 'General', description TEXT,
                price FLOAT NOT NULL DEFAULT 0, stock INTEGER NOT NULL DEFAULT 0,
                low_stock_threshold INTEGER DEFAULT 5, is_active BOOLEAN DEFAULT 1,
                created_at DATETIME, updated_at DATETIME
            )""",
            """CREATE TABLE IF NOT EXISTS store_sales (
                id INTEGER NOT NULL PRIMARY KEY,
                product_id INTEGER NOT NULL REFERENCES store_products(id),
                product_name VARCHAR(128) NOT NULL,
                quantity INTEGER NOT NULL DEFAULT 1,
                unit_price FLOAT NOT NULL, total_amount FLOAT NOT NULL,
                payment_method VARCHAR(32) NOT NULL DEFAULT 'cash',
                staff_id INTEGER REFERENCES staff(id),
                staff_name VARCHAR(128), notes TEXT, created_at DATETIME
            )""",
            """CREATE TABLE IF NOT EXISTS attendance_daily (
                id INTEGER NOT NULL PRIMARY KEY,
                member_id INTEGER, staff_id INTEGER, familiar_id INTEGER,
                date TEXT NOT NULL,
                total_ins INTEGER DEFAULT 0, total_outs INTEGER DEFAULT 0,
                first_in TEXT, last_out TEXT, created_at TEXT
            )""",
            """CREATE TABLE IF NOT EXISTS coaching_plans (
                id INTEGER NOT NULL PRIMARY KEY,
                name VARCHAR(128) NOT NULL,
                duration_days INTEGER NOT NULL DEFAULT 30,
                price FLOAT NOT NULL DEFAULT 0,
                expiry_date TEXT,
                is_active BOOLEAN DEFAULT 1,
                commission_pct FLOAT DEFAULT 0,
                created_at DATETIME
            )""",
            """CREATE TABLE IF NOT EXISTS coaching_sessions (
                id INTEGER NOT NULL PRIMARY KEY,
                coach_id INTEGER NOT NULL REFERENCES staff(id),
                member_name VARCHAR(128) NOT NULL,
                price FLOAT NOT NULL DEFAULT 0,
                gym_commission_pct FLOAT DEFAULT 0,
                gym_share_type TEXT DEFAULT 'pct',
                session_date TEXT NOT NULL,
                notes TEXT, created_at DATETIME,
                created_by INTEGER REFERENCES staff(id)
            )""",
            """CREATE TABLE IF NOT EXISTS vouchers (
                id INTEGER NOT NULL PRIMARY KEY,
                title VARCHAR(128) NOT NULL,
                code VARCHAR(64) NOT NULL UNIQUE,
                quantity INTEGER NOT NULL DEFAULT 0,
                used_count INTEGER NOT NULL DEFAULT 0,
                is_active BOOLEAN DEFAULT 1,
                created_at DATETIME
            )""",
            """CREATE TABLE IF NOT EXISTS voucher_usage (
                id INTEGER NOT NULL PRIMARY KEY,
                voucher_id INTEGER NOT NULL REFERENCES vouchers(id),
                member_id INTEGER NOT NULL REFERENCES members(id),
                voucher_title VARCHAR(128) NOT NULL DEFAULT '',
                used_at DATETIME
            )""",
        ]
        try:
            _conn_ext = _sq3.connect(db_path, timeout=10)
            _conn_ext.execute("PRAGMA foreign_keys = OFF")

            for _sql in _ext_tables:
                _conn_ext.execute(_sql)
            
            # Create unique index for attendance_daily
            _conn_ext.execute("""
                CREATE UNIQUE INDEX IF NOT EXISTS idx_attendance_daily_unique 
                ON attendance_daily(COALESCE(member_id,0), COALESCE(staff_id,0), COALESCE(familiar_id,0), date)
            """)
            # Create unique index for voucher_usage (one-time per member per voucher title)
            _conn_ext.execute("""
                CREATE UNIQUE INDEX IF NOT EXISTS idx_vu_title_member
                ON voucher_usage(voucher_title, member_id)
            """)

            # Add extension columns to attendance if missing
            _att_cols = [r[1] for r in _conn_ext.execute("PRAGMA table_info(attendance)").fetchall()]
            if 'familiar_id' not in _att_cols:
                _conn_ext.execute("ALTER TABLE attendance ADD COLUMN familiar_id INTEGER REFERENCES familiars(id)")
            if 'staff_id' not in _att_cols:
                _conn_ext.execute("ALTER TABLE attendance ADD COLUMN staff_id INTEGER REFERENCES staff(id)")

             # Add member_type to members if missing (for walk-ins)
            _mem_cols = [r[1] for r in _conn_ext.execute("PRAGMA table_info(members)").fetchall()]
            if 'member_type' not in _mem_cols:
                _conn_ext.execute("ALTER TABLE members ADD COLUMN member_type VARCHAR(16) DEFAULT 'regular'")
            if 'is_student' not in _mem_cols:
                _conn_ext.execute("ALTER TABLE members ADD COLUMN is_student BOOLEAN DEFAULT 0")

            # Fix existing students whose member_type didn't get set to 'student'
            _conn_ext.execute(
                "UPDATE members SET member_type='student' "
                "WHERE is_student=1 AND member_type NOT IN ('student', 'walkin')"
            )

            # Add discount_type to members (student/senior/voucher or NULL for none)
            if 'discount_type' not in _mem_cols:
                _conn_ext.execute("ALTER TABLE members ADD COLUMN discount_type VARCHAR(16) DEFAULT NULL")
            if 'voucher_code' not in _mem_cols:
                _conn_ext.execute("ALTER TABLE members ADD COLUMN voucher_code VARCHAR(64) DEFAULT NULL")

            # Migrate existing is_student=1 to discount_type='student'
            _conn_ext.execute(
                "UPDATE members SET discount_type='student' "
                "WHERE is_student=1 AND (discount_type IS NULL OR discount_type='')"
            )
            _conn_ext.commit()

            # Add commission_pct to plans if missing (gym/coach revenue split)
            _plan_cols = [r[1] for r in _conn_ext.execute("PRAGMA table_info(plans)").fetchall()]
            if 'commission_pct' not in _plan_cols:
                _conn_ext.execute("ALTER TABLE plans ADD COLUMN commission_pct FLOAT DEFAULT 0")

            # Add gym_share_type to coaching_sessions if missing (gym share = pct or peso)
            _cs_cols = [r[1] for r in _conn_ext.execute("PRAGMA table_info(coaching_sessions)").fetchall()]
            if 'gym_share_type' not in _cs_cols:
                _conn_ext.execute("ALTER TABLE coaching_sessions ADD COLUMN gym_share_type TEXT DEFAULT 'pct'")

            # Seed gym-share settings defaults if missing
            _conn_ext.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_gym_share_type', 'pct')")
            _conn_ext.execute("INSERT OR IGNORE INTO admin_settings (key, value) VALUES ('coaching_gym_share_peso', '0')")

            _conn_ext.execute("PRAGMA foreign_keys = ON")
            _conn_ext.commit()
            _conn_ext.close()
            logger.info("Extension tables + columns created after reset")
        except Exception as _ete:
            logger.warning("Extension tables create error: %s", _ete)

        # ── 5. Seed default admin account ────────────────────────
        try:
            import bcrypt as _bc_r
            _db2 = _SL()
            from sqlalchemy import text as _text2
            existing = _db2.execute(_text2(
                "SELECT 1 FROM staff WHERE username='admin'"
            )).fetchone()
            if not existing:
                _raw_pw = secrets.token_urlsafe(12)
                pw_hash = _bc_r.hashpw(_raw_pw.encode(), _bc_r.gensalt()).decode()
                _db2.execute(_text2(
                    "INSERT INTO staff (username, password_hash, display_name, role, is_active, created_at) "
                    "VALUES ('admin', :pw, 'Administrator', 'admin', 1, :now)"
                ), {"pw": pw_hash, "now": _now_utc()})
                _db2.commit()
                logger.warning("Default admin seeded — password: %s — CHANGE IMMEDIATELY via /admin/maintenance", _raw_pw)
            _db2.close()
        except Exception as _se:
            logger.warning("Seed admin error: %s", _se)

        # ── 5b. Drop dedup trigger (replaced by _AttendanceGateDict) ─
        try:
            _apply_dedup_trigger()
            logger.info("Dedup trigger dropped after reset")
        except Exception as _dte:
            logger.debug("Dedup trigger after reset: %s", _dte)

        # ── 6. Delete user photos and snapshots ──────────────────
        for pattern in ["static/photos/*", "static/snapshots/*", "static/clips/*"]:
            for f in _gl.glob(os.path.join(project_root, pattern)):
                try:
                    if os.path.isfile(f):
                        os.unlink(f)
                except Exception:
                    pass

        logger.info("Factory reset complete — restart recommended")
        # Restart the Electron shell to reinitialize all background threads cleanly
        try:
            import threading as _thr_r
            def _restart():
                import time as _t_r
                _t_r.sleep(2)
                import sys
                os.execv(sys.executable, [sys.executable] + sys.argv)
            _thr_r.Thread(target=_restart, daemon=True, name="post-reset-restart").start()
        except Exception:
            pass

        return RedirectResponse("/admin/maintenance?ok=reset", status_code=303)

    except Exception as _re:
        logger.error("reset_data_override error: %s", _re, exc_info=True)
        return RedirectResponse(f"/admin/maintenance?err=reset_failed", status_code=303)


@app.get("/members/")
@app.get("/members")
async def members_list_override(
    request: Request,
    q: str = "", status: str = "", plan_id: int = 0,
    date_from: str = "", date_to: str = "",
    db: Session = Depends(get_db)
):
    """Members list — excludes walk-in members (member_type='walkin')."""
    import traceback as _tb
    from starlette.responses import HTMLResponse
    try:
        if not request.session.get("user_id"):
            return RedirectResponse("/login", status_code=303)

        from database.models import Member, Plan
        from datetime import date as _date, timedelta as _td

        today = _date.today()

        # Real-time expiry check — update any member whose expiry_date has passed
        # before rendering the list. This ensures the UI always shows correct status.
        today_str = today.isoformat()
        db.execute(text(
            "UPDATE members SET status='expired' "
            "WHERE expiry_date IS NOT NULL AND expiry_date < :today "
            "AND status IN ('active', 'frozen')"
        ), {"today": today_str})
        db.commit()

        # Base query — always exclude walk-ins
        query = db.query(Member).filter(
            Member.member_type != "walkin",
            Member.status != "deleted"
        )

        if q:
            # escape %/_ and bind param to prevent LIKE injection
            q_esc = q.replace("\\","\\\\").replace("%","\\%").replace("_","\\_")[:100]
            like_q = f"%{q_esc}%"
            query = query.filter(
                Member.name.ilike(like_q, escape="\\") |
                Member.uid.ilike(like_q, escape="\\")
            )

        if status == "active":
            query = query.filter(Member.status == "active")
        elif status == "expired":
            query = query.filter(Member.status == "expired")
        elif status == "frozen":
            query = query.filter(Member.status == "frozen")
        elif status == "at-risk":
            cutoff = (today + _td(days=7)).isoformat()
            query = query.filter(
                Member.status == "active",
                Member.expiry_date <= cutoff,
                Member.expiry_date >= today.isoformat()
            )

        if plan_id:
            query = query.filter(Member.plan_id == plan_id)
        if date_from:
            query = query.filter(Member.created_at >= date_from)
        if date_to:
            query = query.filter(Member.created_at <= f"{date_to}T23:59:59")

        members = query.order_by(Member.name).all()

        # Fetch discount_type for each member (not a mapped column on compiled ORM)
        member_ids = [m.id for m in members]
        if member_ids:
            import sqlite3 as _sq3
            _disc_db = _sq3.connect(os.path.join(project_root, "gym.db"), timeout=10)
            placeholders = ",".join("?" for _ in member_ids)
            discount_rows = _disc_db.execute(
                f"SELECT id, discount_type FROM members WHERE id IN ({placeholders})",
                member_ids
            ).fetchall()
            _disc_db.close()
            dt_map = {row[0]: row[1] or "" for row in discount_rows}
            for m in members:
                m.discount_type = dt_map.get(m.id, "")

        plans = db.query(Plan).filter(Plan.is_active == True).all()

        # Summary counts (unfiltered, always exclude walk-ins and deleted)
        base = db.query(Member).filter(
            Member.member_type != "walkin",
            Member.status != "deleted"
        )
        total_members = base.count()
        active_members = base.filter(Member.status == "active").count()
        expired_members = base.filter(Member.status == "expired").count()

        if request.headers.get("HX-Request"):
            return templates.TemplateResponse(request, "components/member_rows.html", {
                "members": members, "today": today.isoformat(),
            })

        return templates.TemplateResponse(request, "members.html", {
            "members": members, "plans": plans,
            "q": q, "status_filter": status,
            "plan_filter": plan_id, "date_from": date_from,
            "date_to": date_to, "today": today.isoformat(),
            "total_members": total_members,
            "active_members": active_members,
            "expired_members": expired_members,
        })
    except Exception as _exc:
        _tb.print_exc()
        logger.exception("members_list_override crashed")
        return HTMLResponse(
            f"<html><body><h1>Error</h1><pre>{_tb.format_exc()}</pre></body></html>",
            status_code=500
        )


# ══════════════════════════════════════════════════════════════════
# COACH STUDENTS OVERRIDE — fix deleted members, duplicates, add search
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/coaches/{coach_id}/students", ["GET"])
_remove_all_routes("/coaches/{coach_id}/enroll", ["POST"])


@app.get("/coaches/{coach_id}/students")
async def coach_students_page(coach_id: int, request: Request,
                               q: str = "",
                               member_status: str = "",
                               db: Session = Depends(get_db)):
    """Coach students page with search, status filter, no deleted members, no duplicates."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    today_str = _now_utc().strftime("%Y-%m-%d")
    
    # Get coach info
    coach = db.execute(text(
        "SELECT id, username, display_name, is_active FROM staff WHERE id=:id AND role='coach'"
    ), {"id": coach_id}).fetchone()
    
    if not coach:
        return RedirectResponse("/coaches?error=not_found", status_code=303)
    
    coach_obj = _NS(id=coach[0], username=coach[1], display_name=coach[2], is_active=coach[3])
    
    # Get assignments with member info — exclude deleted members, no duplicates
    sql = (
        "SELECT ca.id, ca.member_id, ca.notes, ca.is_active, ca.created_at, ca.expiry_date, ca.coaching_plan_id, "
        "m.name, m.status, m.member_type, m.expiry_date "
        "FROM coach_assignments ca "
        "JOIN members m ON ca.member_id = m.id "
        "WHERE ca.coach_id=:cid AND m.status != 'deleted' "
    )
    params = {"cid": coach_id}
    
    if q:
        sql += " AND m.name LIKE :q"
        params["q"] = f"%{q}%"
    
    if member_status:
        sql += " AND m.status = :ms"
        params["ms"] = member_status
    
    sql += " ORDER BY ca.created_at DESC"
    
    rows = db.execute(text(sql), params).fetchall()
    
    assignments = []
    for r in rows:
        ca_expiry = r[5] or ""
        m_expiry = r[9] or ""
        is_expired = (ca_expiry and ca_expiry < today_str) or (m_expiry and m_expiry < today_str)
        computed_status = "expired" if is_expired else "active"
        assignments.append(_NS(
            id=r[0], member_id=r[1], notes=r[2], is_active=r[3], created_at=r[4],
            expiry_date=ca_expiry or m_expiry, is_expired=is_expired,
            coaching_plan_id=r[6],
            member=_NS(name=r[7], status=_NS(value=computed_status), member_type=r[9])
        ))
    
    # Get available members for enrollment — active + expired only, no walk-ins
    members = db.execute(text(
        "SELECT DISTINCT m.id, m.name, m.status, m.member_type, m.discount_type "
        "FROM members m "
        "WHERE m.status IN ('active', 'expired') "
        "AND m.member_type IN ('regular', 'student') "
        "AND m.id NOT IN (SELECT member_id FROM coach_assignments WHERE coach_id=:cid) "
        "ORDER BY m.name"
    ), {"cid": coach_id}).fetchall()
    
    # Get coaching plans for enrollment dropdown
    coaching_plans = db.execute(text(
        "SELECT id, name, duration_days, price, expiry_date, is_active, commission_pct "
        "FROM coaching_plans WHERE is_active=1 ORDER BY duration_days"
    )).fetchall()
    
    coaching_list = []
    for cp in coaching_plans:
        coaching_list.append(_NS(
            id=cp[0], name=cp[1], duration_days=cp[2],
            price=cp[3], expiry_date=cp[4] or "", is_active=cp[5],
            commission_pct=cp[6] or 0
        ))
    
    member_list = [_NS(id=r[0], name=r[1], status=_NS(value=r[2]), member_type=r[3], discount_type=r[4] or "") for r in members]
    
    from datetime import date
    today_str = date.today().isoformat()
    
    return templates.TemplateResponse(request, "coach_students.html", {
        "coach": coach_obj,
        "assignments": assignments,
        "members": member_list,
        "coaching_plans": coaching_list,
        "q": q,
        "member_status": member_status,
        "today": today_str,
    })


@app.post("/coaches/{coach_id}/enroll")
async def coach_enroll_student(coach_id: int, request: Request,
                                member_id: int = Form(...),
                                coaching_plan_id: str = Form(""),
                                start_date: str = Form(""),
                                expiry_date: str = Form(""),
                                notes: str = Form(""),
                                db: Session = Depends(get_db)):
    """Enroll a member to a coach — prevents duplicates."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    
    # Check if already enrolled
    existing = db.execute(text(
        "SELECT id FROM coach_assignments WHERE coach_id=:cid AND member_id=:mid"
    ), {"cid": coach_id, "mid": member_id}).fetchone()
    
    if existing:
        return RedirectResponse(f"/coaches/{coach_id}/students?error=already_enrolled", status_code=303)
    
    # Verify member is not deleted
    member = db.execute(text(
        "SELECT id, name, status FROM members WHERE id=:id"
    ), {"id": member_id}).fetchone()
    
    if not member or member[2] == 'deleted':
        return RedirectResponse(f"/coaches/{coach_id}/students?error=invalid_member", status_code=303)
    
    # Use start_date if provided, otherwise use current time
    from datetime import datetime
    if start_date.strip():
        try:
            enroll_date = datetime.fromisoformat(start_date.strip())
        except Exception:
            enroll_date = _now_utc()
    else:
        enroll_date = _now_utc()
    
    # Build notes with coaching plan info
    combined_notes = notes.strip()
    coaching_plan_name = ""
    plan_expiry_date = None
    if coaching_plan_id.strip():
        try:
            cp_id = int(coaching_plan_id)
            cp = db.execute(text(
                "SELECT name, price, duration_days, commission_pct FROM coaching_plans WHERE id=:id"
            ), {"id": cp_id}).fetchone()
            if cp:
                coaching_plan_name = f"Plan:{cp[0]}(₱{cp[1]})"
                combined_notes = f"{combined_notes} | {coaching_plan_name}" if combined_notes else coaching_plan_name
                # Calculate expiry date from duration_days if not manually set
                if not expiry_date.strip():
                    from datetime import timedelta
                    if start_date.strip():
                        try:
                            start_dt = datetime.fromisoformat(start_date.strip())
                        except Exception:
                            start_dt = enroll_date
                    else:
                        start_dt = enroll_date
                    plan_expiry_date = (start_dt + timedelta(days=cp[2])).strftime("%Y-%m-%d")
                else:
                    plan_expiry_date = expiry_date.strip()
                # Create gym commission sale
                gym_commission = cp[1] * (cp[3] / 100) if cp[3] > 0 else 0
                if gym_commission > 0:
                    now = _now_utc()
                    staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                                           {"id": request.session.get("user_id")}).fetchone()
                    cashier_name = staff_row[0] if staff_row else ""
                    receipt = f"R{now.strftime('%Y%m%d')}-C{now.strftime('%H%M%S')}"
                    db.execute(text(
                        "INSERT INTO sales (receipt_no, member_id, amount_paid, payment_method, "
                        "payment_status, cashier_id, cashier_name, note, gym_commission, created_at) "
                        "VALUES (:rn, :mid, :amt, 'Cash', 'PAID', :cid, :cn, :note, :gc, :now)"
                    ), {"rn": receipt, "mid": member_id, "amt": gym_commission,
                        "cid": request.session.get("user_id"), "cn": cashier_name,
                        "note": f"Coaching: {cp[0]} (Gym {cp[3]}% commission)",
                        "gc": gym_commission, "now": now})
        except (ValueError, Exception):
            pass
    
    # Use manual expiry date if provided and no plan calculated it
    if not plan_expiry_date and expiry_date.strip():
        plan_expiry_date = expiry_date.strip()
    
    db.execute(text(
        "INSERT INTO coach_assignments (coach_id, member_id, notes, is_active, created_at, expiry_date, coaching_plan_id) "
        "VALUES (:cid, :mid, :notes, 1, :enroll_date, :expiry_date, :plan_id)"
    ), {"cid": coach_id, "mid": member_id, "notes": combined_notes, "enroll_date": enroll_date, "expiry_date": plan_expiry_date, "plan_id": coaching_plan_id.strip() if coaching_plan_id.strip() else None})
    db.commit()
    
    return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)


@app.post("/coaches/{coach_id}/enroll-new")
async def coach_enroll_new_student(coach_id: int, request: Request,
                                    name: str = Form(...),
                                    phone: str = Form(""),
                                    member_type: str = Form("regular"),
                                    student_id: str = Form(""),
                                    coaching_plan_id: str = Form(""),
                                    start_date: str = Form(""),
                                    expiry_date: str = Form(""),
                                    notes: str = Form(""),
                                    db: Session = Depends(get_db)):
    """Create a brand new member and enroll them directly to the coach."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    
    name = name.strip()
    if not name:
        return RedirectResponse(f"/coaches/{coach_id}/students?error=empty_name", status_code=303)
    
    # Use start_date if provided, otherwise use current time
    from datetime import datetime
    if start_date.strip():
        try:
            enroll_date = datetime.fromisoformat(start_date.strip())
        except Exception:
            enroll_date = _now_utc()
    else:
        enroll_date = _now_utc()
    
    is_student = 1 if member_type == "student" else 0
    
    # Build notes field — include student ID if provided
    combined_notes = notes.strip()
    if student_id.strip():
        combined_notes = f"{combined_notes} | StudentID:{student_id.strip()}" if combined_notes else f"StudentID:{student_id.strip()}"
    
    # Add coaching plan info
    coaching_plan_name = ""
    plan_expiry_date = None
    if coaching_plan_id.strip():
        try:
            cp_id = int(coaching_plan_id)
            cp = db.execute(text(
                "SELECT name, price, duration_days, commission_pct FROM coaching_plans WHERE id=:id"
            ), {"id": cp_id}).fetchone()
            if cp:
                coaching_plan_name = f"Plan:{cp[0]}(₱{cp[1]})"
                combined_notes = f"{combined_notes} | {coaching_plan_name}" if combined_notes else coaching_plan_name
                # Calculate expiry date from duration_days if not manually set
                if not expiry_date.strip():
                    from datetime import timedelta
                    if start_date.strip():
                        try:
                            start_dt = datetime.fromisoformat(start_date.strip())
                        except Exception:
                            start_dt = enroll_date
                    else:
                        start_dt = enroll_date
                    plan_expiry_date = (start_dt + timedelta(days=cp[2])).strftime("%Y-%m-%d")
                else:
                    plan_expiry_date = expiry_date.strip()
                # Create gym commission sale
                gym_commission = cp[1] * (cp[3] / 100) if cp[3] > 0 else 0
                if gym_commission > 0:
                    now = _now_utc()
                    staff_row = db.execute(text("SELECT display_name FROM staff WHERE id=:id"),
                                           {"id": request.session.get("user_id")}).fetchone()
                    cashier_name = staff_row[0] if staff_row else ""
                    receipt = f"R{now.strftime('%Y%m%d')}-C{now.strftime('%H%M%S')}"
                    # Will use new_member_id after insert
                    sale_data = {"rn": receipt, "amt": gym_commission,
                                 "cid": request.session.get("user_id"), "cn": cashier_name,
                        "note": f"Coaching Enrollment: {cp[0]} (Gym {cp[3]}% commission)",
                                 "gc": gym_commission, "now": now}
        except (ValueError, Exception):
            pass
    
    # Use manual expiry date if provided and no plan calculated it
    if not plan_expiry_date and expiry_date.strip():
        plan_expiry_date = expiry_date.strip()
    
    # Create new member record
    res = db.execute(text(
        "INSERT INTO members (name, phone, member_type, status, is_student, notes, created_at) "
        "VALUES (:name, :phone, :mtype, 'active', :is_student, :notes, :enroll_date)"
    ), {"name": name, "phone": phone.strip(), "mtype": member_type,
        "is_student": is_student, "notes": combined_notes, "enroll_date": enroll_date})
    new_member_id = res.lastrowid
    
    # Create gym commission sale if applicable
    if coaching_plan_id.strip() and 'sale_data' in locals():
        sale_data["mid"] = new_member_id
        db.execute(text(
            "INSERT INTO sales (receipt_no, member_id, amount_paid, payment_method, "
            "payment_status, cashier_id, cashier_name, note, gym_commission, created_at) "
            "VALUES (:rn, :mid, :amt, 'Cash', 'PAID', :cid, :cn, :note, :gc, :now)"
        ), sale_data)
    
    # Enroll to coach with expiry tracking
    db.execute(text(
        "INSERT INTO coach_assignments (coach_id, member_id, notes, is_active, created_at, expiry_date, coaching_plan_id) "
        "VALUES (:cid, :mid, :notes, 1, :enroll_date, :expiry_date, :plan_id)"
    ), {"cid": coach_id, "mid": new_member_id, "notes": combined_notes, "enroll_date": enroll_date, "expiry_date": plan_expiry_date, "plan_id": coaching_plan_id.strip() if coaching_plan_id.strip() else None})
    db.commit()
    
    return RedirectResponse(f"/coaches/{coach_id}/students", status_code=303)


@app.post("/sales/{sale_id}/delete")
async def delete_gym_sale(sale_id: int, request: Request,
                          db: Session = Depends(get_db)):
    """Delete a gym sale record (admin only)."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/login", status_code=303)
    
    # Get sale info for logging
    sale = db.execute(text(
        "SELECT id, receipt_no, amount_paid, member_id FROM sales WHERE id=:id"
    ), {"id": sale_id}).fetchone()
    
    if not sale:
        return RedirectResponse("/sales/history?error=not_found", status_code=303)
    
    # Delete the sale
    db.execute(text("DELETE FROM sales WHERE id=:id"), {"id": sale_id})
    db.commit()
    
    # Log activity
    try:
        user_id = request.session.get("user_id")
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'delete_sale','sale',:tid,:det,:ts)"
        ), {"sid": user_id, "tid": sale_id,
            "det": f"Deleted sale {sale[1] or ''} (₱{sale[2]:,.2f})", "ts": _now_utc()})
        db.commit()
    except Exception:
        pass
    
    return RedirectResponse("/sales/history?deleted=1", status_code=303)


@app.post("/store/sales/{sale_id}/delete")
async def delete_store_sale(sale_id: int, request: Request,
                            db: Session = Depends(get_db)):
    """Delete a store sale record (admin only). Restores stock to the product."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/login", status_code=303)

    # Fetch full sale record including product_id and quantity for stock restoration
    sale = db.execute(text(
        "SELECT id, product_id, product_name, quantity, total_amount "
        "FROM store_sales WHERE id=:id"
    ), {"id": sale_id}).fetchone()

    if not sale:
        return RedirectResponse("/store/history?error=not_found", status_code=303)

    sale_id_val, product_id, product_name, quantity, total_amount = sale

    # Restore stock to the product
    db.execute(text(
        "UPDATE store_products SET stock = stock + :qty, updated_at = :now "
        "WHERE id = :pid"
    ), {"qty": quantity, "now": _now_utc(), "pid": product_id})

    # Delete the sale record
    db.execute(text("DELETE FROM store_sales WHERE id=:id"), {"id": sale_id_val})
    db.commit()

    # Log activity
    try:
        user_id = request.session.get("user_id")
        db.execute(text(
            "INSERT INTO staff_activities (staff_id,action,target_type,target_id,details,timestamp) "
            "VALUES (:sid,'delete_store_sale','store_sale',:tid,:det,:ts)"
        ), {"sid": user_id, "tid": sale_id_val,
            "det": f"Deleted store sale: {product_name} qty={quantity} (₱{total_amount:,.2f}) — stock restored",
            "ts": _now_utc()})
        db.commit()
    except Exception:
        pass

    return RedirectResponse("/store/history?deleted=1", status_code=303)


# ══════════════════════════════════════════════════════════════════
# COACHING PLANS — CRUD for coach enrollment plans
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/admin/pricing", ["GET"])
_remove_all_routes("/admin/pricing", ["POST"])
_remove_all_routes("/admin/pricing/{plan_id}/update", ["POST"])


@app.get("/admin/pricing")
async def admin_pricing_override(request: Request,
                                  db: Session = Depends(get_db)):
    """Admin pricing page with coaching plans section."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    
    # Check if commission_pct column exists (migration may not have run yet)
    try:
        plans = db.execute(text(
            "SELECT id, name, duration_days, regular_price, student_price, walkin_price, is_active, commission_pct "
            "FROM plans ORDER BY duration_days"
        )).fetchall()
        has_comm_col = True
    except Exception:
        # Auto-create the column on first access if startup migration missed it
        try:
            db.execute(text("ALTER TABLE plans ADD COLUMN commission_pct FLOAT DEFAULT 0"))
            db.commit()
        except Exception:
            pass
        plans = db.execute(text(
            "SELECT id, name, duration_days, regular_price, student_price, walkin_price, is_active "
            "FROM plans ORDER BY duration_days"
        )).fetchall()
        has_comm_col = False
    
    try:
        walkin_rate_row = db.execute(text(
            "SELECT walkin_price FROM plans WHERE walkin_price > 0 AND is_active=1 LIMIT 1"
        )).fetchone()
        walkin_rate = walkin_rate_row[0] if walkin_rate_row else 100.0
    except Exception:
        walkin_rate = 100.0
    
    # Calculate student discount percentage from first plan
    try:
        plans_list = [p for p in plans if p[3] and p[3] > 0]
        if plans_list:
            p0 = plans_list[0]
            discount_pct = round((1 - p0[4] / p0[3]) * 100) if p0[3] and p0[4] else 0
        else:
            discount_pct = 0
    except Exception:
        discount_pct = 0
    
    # Get coaching plans (with fallback if table not yet created)
    coaching_list = []
    try:
        coaching_plans = db.execute(text(
            "SELECT id, name, duration_days, price, expiry_date, is_active, commission_pct "
            "FROM coaching_plans ORDER BY duration_days"
        )).fetchall()
        for cp in coaching_plans:
            coaching_list.append(_NS(
                id=cp[0], name=cp[1], duration_days=cp[2],
                price=cp[3], expiry_date=cp[4] or "", is_active=cp[5],
                commission_pct=cp[6] or 0
            ))
    except Exception:
        pass
    
    from datetime import date
    today_str = date.today().isoformat()
    
    # Load coaching session settings (with fallback)
    csettings = {}
    try:
        for row in db.execute(text("SELECT key, value FROM admin_settings")).fetchall():
            csettings[row[0]] = row[1]
    except Exception:
        csettings = {"coaching_session_price": "500", "coaching_gym_ratio": "60"}
    
    # Load vouchers — grouped by title with aggregate info
    vouchers = []
    try:
        for row in db.execute(text(
            "SELECT MIN(id) as id, title, COUNT(*) as cnt, "
            "MIN(code) as code_from, MAX(code) as code_to, "
            "SUM(used_count) as total_used, MIN(is_active) as all_active "
            "FROM vouchers GROUP BY title ORDER BY MIN(created_at) DESC"
        )).fetchall():
            vouchers.append(_NS(
                id=row[0], title=row[1], count=row[2],
                code_from=row[3], code_to=row[4],
                total_used=row[5], all_active=row[6]
            ))
    except Exception:
        pass

    return templates.TemplateResponse(request, "admin/pricing.html", {
        "plans": [_NS(id=p[0], name=p[1], duration_days=p[2], regular_price=p[3],
                       student_price=p[4], walkin_price=p[5], is_active=p[6],
                       commission_pct=p[7] if has_comm_col else 0) for p in plans],
        "walkin_rate": walkin_rate,
        "student_discount_pct": discount_pct,
        "coaching_plans": coaching_list,
        "today": today_str,
        "coaching_session_price": float(csettings.get("coaching_session_price", "500")),
        "coaching_gym_ratio": float(csettings.get("coaching_gym_ratio", "60")),
        "coaching_gym_share_type": (csettings.get("coaching_gym_share_type") or "pct").strip().lower() or "pct",
        "coaching_gym_share_peso": float(csettings.get("coaching_gym_share_peso", "0") or "0"),
        "vouchers": vouchers,
    })


@app.post("/admin/pricing/settings")
async def update_pricing_settings(request: Request,
                                   coaching_session_price: float = Form(0),
                                   coaching_gym_ratio: float = Form(0),
                                   coaching_gym_share_type: str = Form("pct"),
                                   coaching_gym_share_peso: float = Form(0),
                                   db: Session = Depends(get_db)):
    """Update coaching session pricing settings."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    # Normalize share type: only 'pct' or 'peso' are valid
    share_type = "peso" if (coaching_gym_share_type or "").strip().lower() == "peso" else "pct"
    if share_type == "peso":
        coaching_gym_share_peso = max(0.0, float(coaching_gym_share_peso))
    db.execute(text(
        "INSERT OR REPLACE INTO admin_settings (key, value) VALUES ('coaching_session_price', :v)"
    ), {"v": str(coaching_session_price)})
    db.execute(text(
        "INSERT OR REPLACE INTO admin_settings (key, value) VALUES ('coaching_gym_ratio', :v)"
    ), {"v": str(coaching_gym_ratio)})
    db.execute(text(
        "INSERT OR REPLACE INTO admin_settings (key, value) VALUES ('coaching_gym_share_type', :v)"
    ), {"v": share_type})
    db.execute(text(
        "INSERT OR REPLACE INTO admin_settings (key, value) VALUES ('coaching_gym_share_peso', :v)"
    ), {"v": str(coaching_gym_share_peso)})
    db.commit()
    return RedirectResponse("/admin/pricing?settings_updated=1", status_code=303)


@app.post("/admin/pricing")
async def create_plan(request: Request,
                      name: str = Form(...),
                      duration_days: int = Form(...),
                      regular_price: float = Form(...),
                      student_price: float = Form(...),
                      walkin_price: float = Form(0),
                      commission_pct: float = Form(0),
                      db: Session = Depends(get_db)):
    """Create a new subscription plan with commission ratio."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    now = _now_utc()
    db.execute(text(
        "INSERT INTO plans (name, duration_days, regular_price, student_price, "
        "walkin_price, commission_pct, is_active, created_at) "
        "VALUES (:n, :d, :rp, :sp, :wp, :cp, 1, :now)"
    ), {"n": name.strip(), "d": duration_days, "rp": regular_price,
        "sp": student_price, "wp": walkin_price, "cp": commission_pct, "now": now})
    db.commit()
    return RedirectResponse("/admin/pricing?created=1", status_code=303)


@app.post("/admin/pricing/{plan_id}/update")
async def update_plan(plan_id: int, request: Request,
                      name: str = Form(...),
                      duration_days: int = Form(...),
                      regular_price: float = Form(...),
                      student_price: float = Form(...),
                      walkin_price: float = Form(0),
                      is_active: bool = Form(False),
                      commission_pct: float = Form(0),
                      db: Session = Depends(get_db)):
    """Update a subscription plan including commission ratio."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    db.execute(text(
        "UPDATE plans SET name=:n, duration_days=:d, regular_price=:rp, "
        "student_price=:sp, walkin_price=:wp, is_active=:act, commission_pct=:cp WHERE id=:id"
    ), {"n": name.strip(), "d": duration_days, "rp": regular_price,
        "sp": student_price, "wp": walkin_price,
        "act": 1 if is_active else 0, "cp": commission_pct, "id": plan_id})
    db.commit()
    return RedirectResponse("/admin/pricing?updated=1", status_code=303)


@app.post("/admin/coaching-plans")
async def create_coaching_plan(request: Request,
                                name: str = Form(...),
                                duration_days: int = Form(...),
                                price: float = Form(...),
                                expiry_date: str = Form(""),
                                is_active: str = Form(""),
                                commission_pct: float = Form(0),
                                db: Session = Depends(get_db)):
    """Create a new coaching plan."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    
    now = _now_utc()
    active = 1 if is_active.lower() in ('1', 'on', 'true', 'yes') else 0
    
    db.execute(text(
        "INSERT INTO coaching_plans (name, duration_days, price, expiry_date, is_active, commission_pct, created_at) "
        "VALUES (:name, :days, :price, :exp, :active, :comm, :now)"
    ), {"name": name.strip(), "days": duration_days, "price": price,
        "exp": expiry_date.strip() or None, "active": active, "comm": commission_pct, "now": now})
    db.commit()
    
    return RedirectResponse("/admin/pricing?coaching_plan_created=1", status_code=303)


@app.post("/admin/coaching-plans/{plan_id}/update")
async def update_coaching_plan(plan_id: int, request: Request,
                                field: str = Form(""),
                                value: str = Form(""),
                                db: Session = Depends(get_db)):
    """Update a single field of a coaching plan (inline edit)."""
    if request.session.get("role") != "admin":
        return JSONResponse({"status": "error"}, status_code=401)
    
    if not field or field not in ('name', 'duration_days', 'price', 'expiry_date', 'is_active', 'commission_pct'):
        return JSONResponse({"status": "error", "message": "Invalid field"})
    
    if field == 'is_active':
        val = 1 if value.lower() in ('true', '1', 'on', 'yes') else 0
    elif field in ('duration_days', 'price', 'commission_pct'):
        try:
            val = float(value)
            if field == 'duration_days':
                val = int(val)
        except ValueError:
            return JSONResponse({"status": "error", "message": "Invalid number"})
    else:
        val = value.strip()
    
    db.execute(text(
        f"UPDATE coaching_plans SET {field}=:val WHERE id=:id"
    ), {"val": val, "id": plan_id})
    db.commit()
    
    return JSONResponse({"status": "ok"})


@app.post("/admin/coaching-plans/{plan_id}/delete")
async def delete_coaching_plan(plan_id: int, request: Request,
                                db: Session = Depends(get_db)):
    """Delete a coaching plan."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    
    db.execute(text("DELETE FROM coaching_plans WHERE id=:id"), {"id": plan_id})
    db.commit()
    
    return RedirectResponse("/admin/pricing?coaching_plan_deleted=1", status_code=303)


# ══════════════════════════════════════════════════════════════════
# VOUCHER MANAGEMENT
# ══════════════════════════════════════════════════════════════════

def _validate_voucher(db, code: str, member_id: int):
    """Validate a voucher code and record usage. Returns (ok, message)."""
    if not code.strip():
        return False, "Voucher code is required."
    code = code.strip().upper()
    row = db.execute(text(
        "SELECT id, title, quantity, used_count, is_active FROM vouchers WHERE code=:code"
    ), {"code": code}).fetchone()
    if not row:
        return False, "Voucher code not found."
    vid, title, qty, used, active = row
    if not active:
        return False, f"Voucher '{title}' is no longer active."
    if qty > 0 and used >= qty:
        return False, f"Voucher '{title}' has been fully redeemed."
    existing = db.execute(text(
        "SELECT 1 FROM voucher_usage WHERE voucher_id=:vid AND member_id=:mid"
    ), {"vid": vid, "mid": member_id}).fetchone()
    if existing:
        return False, f"You have already used voucher '{title}'."
    return True, title


def _get_available_voucher_titles(db, member_id=None):
    """Return all distinct voucher titles registered in the system.

    If member_id is given, excludes titles this member has already used.
    The availability check (active/quantity) happens in _assign_voucher_code.
    """
    sql = "SELECT DISTINCT title FROM vouchers"
    if member_id:
        sql += """
          WHERE title NOT IN (
              SELECT DISTINCT voucher_title FROM voucher_usage
              WHERE member_id = :mid AND voucher_title != ''
          )
        """
        rows = db.execute(text(sql), {"mid": member_id}).fetchall()
    else:
        rows = db.execute(text(sql)).fetchall()
    return [r[0] for r in rows]


def _assign_voucher_code(db, title, member_id=None):
    """Find and return the first available (id, code) for a voucher title.

    Returns (voucher_id, code) tuple or (None, None) if none available.
    If member_id is given, excludes codes this member has already used.
    """
    sql = """
        SELECT id, code FROM vouchers
        WHERE title = :title AND is_active = 1
          AND (quantity = 0 OR used_count < quantity)
    """
    if member_id:
        sql += """
          AND title NOT IN (
              SELECT DISTINCT voucher_title FROM voucher_usage
              WHERE member_id = :mid AND voucher_title != ''
          )
        """
    sql += " ORDER BY code ASC LIMIT 1"
    params = {"title": title}
    if member_id:
        params["mid"] = member_id
    row = db.execute(text(sql), params).fetchone()
    if row:
        return (row[0], row[1])
    return (None, None)


# ── Voucher routes (remove compiled originals first) ──
_remove_all_routes("/admin/vouchers/create", ["POST"])
_remove_all_routes("/admin/vouchers/{voucher_id}/toggle", ["POST"])
_remove_all_routes("/admin/vouchers/{voucher_id}/delete", ["POST"])


@app.post("/admin/vouchers/create")
async def create_voucher(request: Request,
                         title: str = Form(...),
                         code: str = Form(""),
                         quantity: int = Form(0),
                         db: Session = Depends(get_db)):
    """Create voucher(s) — quantity controls how many individual codes to generate."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    title = title.strip()
    if not title:
        return RedirectResponse("/admin/pricing?voucher_error=missing_fields", status_code=303)
    qty = max(1, quantity) if quantity > 0 else 1
    count = db.execute(text("SELECT COUNT(*) FROM vouchers")).fetchone()[0]
    pad = len(str(count + qty))
    now = _now_utc()
    for i in range(qty):
        seq = count + i + 1
        c = f"{seq:0{pad}d}"
        db.execute(text(
            "INSERT INTO vouchers (title, code, quantity, used_count, is_active, created_at) "
            "VALUES (:title, :code, 1, 0, 1, :now)"
        ), {"title": title, "code": c, "now": now})
    db.commit()
    return RedirectResponse("/admin/pricing?voucher_created=1", status_code=303)


@app.post("/admin/vouchers/toggle")
async def toggle_voucher(request: Request,
                         title: str = Form(...),
                         db: Session = Depends(get_db)):
    """Toggle all vouchers with given title active/inactive."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    db.execute(text(
        "UPDATE vouchers SET is_active = CASE WHEN is_active THEN 0 ELSE 1 END WHERE title=:title"
    ), {"title": title})
    db.commit()
    return RedirectResponse("/admin/pricing?voucher_toggled=1", status_code=303)


@app.post("/admin/vouchers/delete-batch")
async def delete_voucher_batch(request: Request,
                               title: str = Form(...),
                               db: Session = Depends(get_db)):
    """Delete all vouchers with given title and their usage records."""
    if request.session.get("role") != "admin":
        return RedirectResponse("/admin/login", status_code=303)
    ids = [r[0] for r in db.execute(text("SELECT id FROM vouchers WHERE title=:title"), {"title": title}).fetchall()]
    if ids:
        import sqlite3 as _sq3
        _del_db = _sq3.connect(os.path.join(project_root, "gym.db"), timeout=10)
        placeholders = ",".join("?" for _ in ids)
        _del_db.execute(f"DELETE FROM voucher_usage WHERE voucher_id IN ({placeholders})", ids)
        _del_db.execute("DELETE FROM vouchers WHERE title=?", (title,))
        _del_db.commit()
        _del_db.close()
        # Refresh the SQLAlchemy session's view
        db.commit()
    return RedirectResponse("/admin/pricing?voucher_deleted=1", status_code=303)


# ══════════════════════════════════════════════════════════════════
# REGISTRATION STEP 1 OVERRIDE — discount type + voucher title
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/members/register/step1", ["GET", "POST"])


@app.get("/members/register/step1")
async def register_step1_get(request: Request,
                              db: Session = Depends(get_db)):
    """Render registration step 1 with voucher titles dropdown."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    voucher_titles = _get_available_voucher_titles(db)
    return templates.TemplateResponse(request, "register_step1.html", {
        "plans": db.execute(text(
            "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
        )).fetchall(),
        "voucher_titles": voucher_titles,
    })


@app.post("/members/register/step1")
async def register_step1_override(
    request: Request,
    name: str = Form(...),
    start_date: str = Form(""),
    plan_id: int = Form(...),
    discount_type: str = Form(""),
    discount_id_file: UploadFile = File(None),
    discount_id_number: str = Form(""),
    voucher_title: str = Form(""),
    voucher_id_file: UploadFile = File(None),
    payment_method: str = Form("Cash"),
    db: Session = Depends(get_db),
):
    """Process step 1 of member registration — handles discount type picker + voucher title."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)
    
    name = name.strip()
    if not name:
        return templates.TemplateResponse(request, "register_step1.html", {
            "plans": db.execute(text(
                "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
            )).fetchall(),
            "voucher_titles": _get_available_voucher_titles(db),
            "error": "Name is required."
        })
    
    # Get plan details
    plan = db.execute(text(
        "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE id=:id"
    ), {"id": plan_id}).fetchone()
    
    if not plan:
        return templates.TemplateResponse(request, "register_step1.html", {
            "plans": db.execute(text(
                "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
            )).fetchall(),
            "voucher_titles": _get_available_voucher_titles(db),
            "error": "Invalid plan selected."
        })
    
    discount_type = discount_type.strip().lower() if discount_type.strip() else ""
    has_discount = discount_type in ("student", "senior", "pwd", "voucher")
    
    # Validate discount ID photo for student/senior/pwd
    needs_photo = discount_type in ("student", "senior", "pwd")
    if needs_photo:
        uploaded_data = None
        # Try DI-bound file first, then fall back to request.form()
        file_to_read = discount_id_file
        if file_to_read is None:
            try:
                all_form = await request.form()
                from starlette.datastructures import UploadFile as _UPF
                maybe = all_form.get("discount_id_file")
                if isinstance(maybe, _UPF):
                    file_to_read = maybe
            except Exception:
                pass
        if file_to_read:
            try:
                # Ensure we're at the start of the file
                if file_to_read.file:
                    file_to_read.file.seek(0)
                uploaded_data = await file_to_read.read()
            except Exception:
                uploaded_data = None
        if not uploaded_data or len(uploaded_data) < 50:
            return templates.TemplateResponse(request, "register_step1.html", {
                "plans": db.execute(text(
                    "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
                )).fetchall(),
                "voucher_titles": _get_available_voucher_titles(db),
                "error": "Discount ID photo is required for this discount type."
            })
        discount_photo_save_data = uploaded_data
    else:
        discount_photo_save_data = None
    
    # Validate voucher title and assign a code
    voucher_code = ""
    voucher_photo_data = None
    if discount_type == "voucher":
        voucher_title = voucher_title.strip()
        if not voucher_title:
            return templates.TemplateResponse(request, "register_step1.html", {
                "plans": db.execute(text(
                    "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
                )).fetchall(),
                "voucher_titles": _get_available_voucher_titles(db),
                "error": "Please select a voucher type."
            })
        # Read voucher photo
        file_to_read = voucher_id_file
        if file_to_read is None:
            try:
                all_form = await request.form()
                from starlette.datastructures import UploadFile as _UPF_V
                maybe = all_form.get("voucher_id_file")
                if isinstance(maybe, _UPF_V):
                    file_to_read = maybe
            except Exception:
                pass
        if file_to_read:
            try:
                if file_to_read.file:
                    file_to_read.file.seek(0)
                voucher_photo_data = await file_to_read.read()
            except Exception:
                pass
        vid, code = _assign_voucher_code(db, voucher_title)
        if not code:
            return templates.TemplateResponse(request, "register_step1.html", {
                "plans": db.execute(text(
                    "SELECT id, name, duration_days, regular_price, student_price FROM plans WHERE is_active=1 ORDER BY duration_days"
                )).fetchall(),
                "voucher_titles": _get_available_voucher_titles(db),
                "error": f"No available codes for voucher '{voucher_title}'."
            })
        voucher_code = code

    price = plan[4] if has_discount else plan[3]  # student_price or regular_price
    
    # Calculate expiry date
    from datetime import date, timedelta
    if start_date:
        try:
            start = date.fromisoformat(start_date)
        except Exception:
            start = date.today()
    else:
        start = date.today()
    
    expiry = start + timedelta(days=plan[2])
    
    # Save discount ID photo if provided (reuse already-read data)
    discount_photo_path = None
    if discount_type == "voucher":
        if voucher_photo_data and len(voucher_photo_data) >= 50:
            try:
                from paths import data_root
                photos_dir = data_root() / "static" / "photos"
                photos_dir.mkdir(parents=True, exist_ok=True)
                fname = f"voucher_{name.replace(' ', '_')}_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
                (photos_dir / fname).write_bytes(voucher_photo_data)
                discount_photo_path = f"static/photos/{fname}"
            except Exception as e:
                logger.warning("Voucher photo save error: %s", e)
    elif discount_photo_save_data and len(discount_photo_save_data) >= 50:
        try:
            from paths import data_root
            photos_dir = data_root() / "static" / "photos"
            photos_dir.mkdir(parents=True, exist_ok=True)
            fname = f"{discount_type}_id_{name.replace(' ', '_')}_{_now_utc().strftime('%Y%m%d_%H%M%S')}.jpg"
            (photos_dir / fname).write_bytes(discount_photo_save_data)
            discount_photo_path = f"static/photos/{fname}"
        except Exception as e:
            logger.warning("Discount ID photo save error: %s", e)
    
    # Store session data for next steps
    import json
    session_data = json.dumps({
        "name": name,
        "start_date": start.isoformat(),
        "plan_id": plan_id,
        "plan_name": plan[1],
        "price": price,
        "discount_type": discount_type,
        "discount_id_number": discount_id_number,
        "discount_photo_path": discount_photo_path,
        "voucher_code": voucher_code.strip() if discount_type == "voucher" else "",
        "voucher_title": voucher_title if discount_type == "voucher" else "",
        "expiry": expiry.isoformat(),
        "payment_method": payment_method,
    })
    
    return templates.TemplateResponse(request, "register_step2.html", {
        "session_data": session_data,
        "member_name": name,
    })


# ══════════════════════════════════════════════════════════════════
# REGISTRATION STEP 3 OVERRIDE — use payment_method from form
# ══════════════════════════════════════════════════════════════════

_remove_all_routes("/members/register/step3", ["POST"])


@app.post("/members/register/step3")
async def register_step3_override(
    request: Request,
    session_data: str = Form(...),
    rfid_uid: str = Form(""),
    db: Session = Depends(get_db),
):
    """Override for step 3 — payment_method read from step 1 session data."""
    if not request.session.get("user_id"):
        return RedirectResponse("/login", status_code=303)

    import json
    try:
        data = json.loads(session_data)
    except Exception:
        return HTMLResponse("<p style='color:red'>Error: Invalid session data</p>")

    rfid_uid = rfid_uid.strip() or None
    plan_id = data.get("plan_id")

    from database.models import Member, Plan, Sale, MemberStatus, MemberType, PaymentStatus
    from datetime import date, timedelta

    # Check if RFID is already taken
    if rfid_uid:
        existing = db.query(Member).filter(Member.uid == rfid_uid).first()
        if existing:
            plan = db.query(Plan).filter(Plan.id == plan_id).first()
            discount_type = data.get("discount_type", "")
            has_discount = discount_type in ("student", "senior", "pwd", "voucher")
            price = plan.student_price if (has_discount and plan) else (plan.regular_price if plan else 0)
            return templates.TemplateResponse(request, "register_step3.html", {
                "error": f"RFID UID '{rfid_uid}' is already assigned to {existing.name}.",
                "session_data": session_data,
                "member_name": data.get("name", "Member"),
                "photo_front": data.get("photo_front"),
                "plan_name": plan.name if plan else "N/A",
                "price": price,
                "discount_type": discount_type,
            })

    # Get plan
    plan = db.query(Plan).filter(Plan.id == plan_id).first()

    # Parse start date
    start_date_str = data.get("start_date") or date.today().isoformat()
    try:
        start = date.fromisoformat(start_date_str)
    except ValueError:
        start = date.today()

    # Calculate expiry
    expiry_date = (start + timedelta(days=plan.duration_days)) if plan else None

    # Member creation
    discount_type = data.get("discount_type", "")
    has_discount = discount_type in ("student", "senior", "pwd", "voucher")
    is_student = 1 if discount_type == "student" else 0
    face_vector_hex = data.get("face_vector", "")
    member = Member(
        name=data["name"],
        uid=rfid_uid,
        photo_path=data.get("photo_front"),
        face_vector=bytes.fromhex(face_vector_hex) if face_vector_hex else None,
        status=MemberStatus.active,
        member_type=MemberType.student if discount_type == "student" else MemberType.regular,
        is_student=is_student,
        student_id_photo=data.get("discount_photo_path"),
        plan_id=plan_id,
        expiry_date=expiry_date,
        discount_type=discount_type or None,
        voucher_code=data.get("voucher_code") if discount_type == "voucher" else None,
    )
    db.add(member)
    db.commit()
    db.refresh(member)

    # Record voucher usage
    if discount_type == "voucher" and data.get("voucher_code"):
        vc = data["voucher_code"].strip().upper()
        vrow = db.execute(text(
            "SELECT id FROM vouchers WHERE code=:code"
        ), {"code": vc}).fetchone()
        if vrow:
            vt = data.get("voucher_title", "")
            db.execute(text(
                "INSERT OR IGNORE INTO voucher_usage (voucher_id, member_id, voucher_title, used_at) "
                "VALUES (:vid, :mid, :vt, :now)"
            ), {"vid": vrow[0], "mid": member.id, "vt": vt, "now": _now_utc()})
            db.execute(text(
                "UPDATE vouchers SET used_count = (SELECT COUNT(*) FROM voucher_usage WHERE voucher_id=:vid) WHERE id=:vid"
            ), {"vid": vrow[0]})
            db.commit()

    # Invalidate face recognition roster cache
    try:
        from services.face_recognition import face_service
        face_service.invalidate_roster_cache()
    except Exception:
        pass

    # Calculate amount
    amount = plan.student_price if (has_discount and plan) else (plan.regular_price if plan else 0)

    # Sale creation with user-selected payment_method from step 1
    now_str = _now_utc()
    receipt = f"R{now_str.strftime('%Y%m%d')}-REG{now_str.strftime('%H%M%S')}"
    sale = Sale(
        receipt_no=receipt,
        member_id=member.id,
        plan_id=plan_id,
        amount_paid=amount,
        payment_method=data.get("payment_method", "Cash"),
        payment_status=PaymentStatus.PAID,
        cashier_id=request.session.get("user_id"),
        cashier_name=request.session.get("display_name", "System"),
        note=f"New registration - {plan.name}" if plan else "New registration",
        created_at=now_str,
    )
    db.add(sale)
    db.commit()

    # Save discount ID number in dedicated column (student/senior/pwd only; voucher uses voucher_code column)
    discount_id_number = data.get("discount_id_number", "") or ""
    if discount_id_number.strip() and discount_type in ("student", "senior", "pwd"):
        db.execute(text(
            "UPDATE members SET discount_id_number=:idnum WHERE id=:id"
        ), {"idnum": discount_id_number.strip(), "id": member.id})
        db.commit()

    # Log activity
    staff_id = request.session.get("user_id")
    if staff_id:
        _log_activity(db, staff_id, "register", "member", member.id, f"Registered {member.name}")

    return RedirectResponse(f"/members/{member.id}?registered=1", status_code=303)


# ── Final route verification ──
_eod_final = [r for r in app.routes if hasattr(r, 'path') and 'end-of-day' in (r.path or '').lower()]
logger.info("Final end-of-day routes in app: %d", len(_eod_final))
for _ef in _eod_final:
    logger.info("  -> %s %s", getattr(_ef, 'methods', set()), _ef.path)

if __name__ == "__main__":
    import uvicorn
    host = os.environ.get("SOLO_HOST", "0.0.0.0")
    port = int(os.environ.get("SOLO_PORT", "8000"))
    uvicorn.run(app, host=host, port=port, log_level="info")








