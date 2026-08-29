"""
GymPOS License Validator — hardware-bound, offline, 7-day heartbeat, 3-day grace
Implements Goal.md picks: 7d heartbeat, 1 Device 1 Key (HWID), global dedup=1, tamper=immediate lock
"""
from __future__ import annotations
import base64, hashlib, json, os, time, uuid
import sqlite3
from datetime import datetime, timezone, timedelta
from pathlib import Path

try:
    import zoneinfo
    PHT = zoneinfo.ZoneInfo("Asia/Manila")
except Exception:
    PHT = timezone(timedelta(hours=8))

TIER_CAPS = {"Basic": 200, "Pro": 500, "Ultra": 1000}
GRACE_SECONDS = 3 * 24 * 3600
HEARTBEAT_SECONDS = 7 * 24 * 3600

def _now_pht_naive() -> datetime:
    return datetime.now(PHT).replace(tzinfo=None)

def get_hwid() -> str:
    """Hardened fingerprint: MachineGuid + disk + baseboard + validated MAC + hostname"""
    anchors = []
    # MachineGuid (Windows)
    try:
        import winreg
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography") as k:
            guid, _ = winreg.QueryValueEx(k, "MachineGuid")
            if guid:
                anchors.append(guid)
    except Exception:
        pass
    # disk serial via wmic
    try:
        import subprocess
        out = subprocess.check_output(["wmic","diskdrive","get","SerialNumber"], text=True, timeout=3)
        for line in out.splitlines():
            s = line.strip()
            if s and s.lower() != "serialnumber":
                anchors.append(s)
                break
    except Exception:
        pass
    # validated MAC (skip random)
    try:
        node = uuid.getnode()
        if (node >> 40) & 0x01 == 0:  # not random
            anchors.append(hex(node)[2:])
    except Exception:
        pass
    try:
        hostname = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
        anchors.append(hostname)
    except Exception:
        pass
    if len(anchors) < 2:
        anchors.append("fallback-" + str(uuid.getnode()))
    raw = "|".join(sorted(set(anchors))).encode()
    return hashlib.sha256(raw).hexdigest()[:32]

def encrypt_vector(vec_bytes: bytes, key_b64: str = "") -> bytes:
    """AES-GCM encrypt face vector (or return plaintext if no key)"""
    if not key_b64:
        return vec_bytes
    try:
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        import os as _os
        key = base64.b64decode(key_b64)
        aesgcm = AESGCM(key)
        nonce = _os.urandom(12)
        ct = aesgcm.encrypt(nonce, vec_bytes, None)
        return nonce + ct
    except Exception:
        return vec_bytes

def decrypt_vector(blob: bytes, key_b64: str = "") -> bytes:
    if not key_b64 or len(blob) < 13:
        return blob
    try:
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        key = base64.b64decode(key_b64)
        aesgcm = AESGCM(key)
        nonce, ct = blob[:12], blob[12:]
        return aesgcm.decrypt(nonce, ct, None)
    except Exception:
        return blob

def _db_path(project_root: str) -> str:
    # honor SOLO_DATA_DIR (packaged) else project_root/gym.db
    d = os.environ.get("SOLO_DATA_DIR")
    if d:
        return os.path.join(d, "gym.db")
    return os.path.join(project_root, "gym.db")

def _ensure_license_tables(project_root: str):
    db_path = _db_path(project_root)
    conn = sqlite3.connect(db_path, timeout=10)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("""
    CREATE TABLE IF NOT EXISTS cloud_licenses (
        gym_id TEXT PRIMARY KEY,
        owner_email TEXT,
        tier TEXT,
        max_members INT,
        exp_unix INT,
        grace_until INT,
        hwid TEXT,
        status TEXT,
        last_verify INT,
        last_seen INT,
        license_b64 TEXT
    )""")
    conn.execute("""
    CREATE TABLE IF NOT EXISTS gyms (
        gym_id TEXT PRIMARY KEY,
        owner_email TEXT,
        tier TEXT,
        hwid TEXT,
        last_ip TEXT,
        last_sync_at TEXT
    )""")
    # add gym_id + owner_email to members/attendance if missing (idempotent) + backfill + visitor cols
    for tbl in ("members", "attendance"):
        cols = [r[1] for r in conn.execute(f"PRAGMA table_info({tbl})").fetchall()]
        for col in ("gym_id", "owner_email"):
            if col not in cols:
                try:
                    conn.execute(f"ALTER TABLE {tbl} ADD COLUMN {col} TEXT")
                except Exception:
                    pass
        try:
            conn.execute(f"UPDATE {tbl} SET gym_id='default' WHERE gym_id IS NULL")
        except Exception:
            pass
    # visitor cols for inter-branch
    for col in ("is_interbranch", "visitor_home_gym_id", "visitor_home_owner"):
        cols = [r[1] for r in conn.execute("PRAGMA table_info(attendance)").fetchall()]
        if col not in cols:
            try:
                typ = "INTEGER DEFAULT 0" if col == "is_interbranch" else "TEXT"
                conn.execute(f"ALTER TABLE attendance ADD COLUMN {col} {typ}")
            except Exception:
                pass
    # indexes
    for sql in [
        "CREATE INDEX IF NOT EXISTS idx_members_gym ON members(gym_id, status)",
        "CREATE INDEX IF NOT EXISTS idx_attendance_gym ON attendance(gym_id, date(timestamp))",
        "CREATE INDEX IF NOT EXISTS idx_attendance_visitor ON attendance(is_interbranch, visitor_home_owner)",
    ]:
        try:
            conn.execute(sql)
        except Exception:
            pass
    # audit cols for staff_activities
    cols = [r[1] for r in conn.execute("PRAGMA table_info(staff_activities)").fetchall()]
    for col in ("gym_id", "ip"):
        if col not in cols:
            try:
                conn.execute(f"ALTER TABLE staff_activities ADD COLUMN {col} TEXT")
            except Exception:
                pass
    conn.commit()
    conn.close()

def _verify_sig(claims: dict, signature_b64: str, pubkey_pem: str) -> bool:
    """RSA-PSS SHA256 verify. Require pubkey in hardened mode."""
    if not pubkey_pem or pubkey_pem.strip() == "":
        import os as _os2, logging as _lg2
        if _os2.environ.get("SOLO_DEV") == "1" or _os2.environ.get("ENV") == "dev":
            _lg2.getLogger(__name__).warning("LICENSE_PUBKEY empty — dev mode accepts unsigned license")
            return True
        _lg2.getLogger(__name__).error("LICENSE_PUBKEY missing — hardened mode rejects unsigned license")
        return False
    try:
        from cryptography.hazmat.primitives import hashes
        from cryptography.hazmat.primitives.asymmetric import padding
        from cryptography.hazmat.primitives.serialization import load_pem_public_key
        pub = load_pem_public_key(pubkey_pem.encode())
        payload = json.dumps(claims, sort_keys=True, separators=(",", ":")).encode()
        sig = base64.b64decode(signature_b64)
        pub.verify(sig, payload, padding.PSS(mgf=padding.MGF1(hashes.SHA256()), salt_length=padding.PSS.MAX_LENGTH), hashes.SHA256())
        return True
    except Exception:
        return False

def validate_license(project_root: str, pubkey_pem: str = "") -> dict:
    """
    Returns {status: ACTIVE|GRACE|LOCKED, reason, claims}
    Enforces: sig, expiry+3d grace, 7d heartbeat, hwid binding, clock tamper
    """
    db = _db_path(project_root)
    _ensure_license_tables(project_root)
    conn = sqlite3.connect(db, timeout=10)
    conn.row_factory = sqlite3.Row
    row = conn.execute("SELECT * FROM cloud_licenses LIMIT 1").fetchone()
    if not row:
        conn.close()
        return {"status": "LOCKED", "reason": "no_license", "claims": None}

    try:
        lic = json.loads(base64.b64decode(row["license_b64"]).decode())
        claims = lic["claims"]
        sig = lic["sig"]
    except Exception as e:
        conn.close()
        return {"status": "LOCKED", "reason": f"corrupt:{e}", "claims": None}

    # sig
    if not _verify_sig(claims, sig, pubkey_pem):
        conn.close()
        return {"status": "LOCKED", "reason": "bad_sig", "claims": claims}

    # hwid binding: 1 Device 1 Key
    expected_hwid = claims.get("hwid", "")
    if expected_hwid and expected_hwid != get_hwid():
        conn.close()
        return {"status": "LOCKED", "reason": "hwid_mismatch", "claims": claims}

    now_unix = int(time.time())
    exp = int(claims.get("exp_unix", 0))
    grace = int(claims.get("grace_until", exp + GRACE_SECONDS))

    # heartbeat 7d: must have verified within 7d (only when pubkey set = prod)
    last_verify = int(row["last_verify"] or 0)
    if pubkey_pem and last_verify and (now_unix - last_verify) > HEARTBEAT_SECONDS:
        conn.close()
        return {"status": "LOCKED", "reason": "heartbeat_expired", "claims": claims}

    # clock tamper: now < last_seen → immediate lock
    last_seen = int(row["last_seen"] or 0)
    if last_seen and now_unix < last_seen - 60:
        conn.execute("UPDATE cloud_licenses SET status='LOCKED' WHERE gym_id=?", (claims.get("gym_id"),))
        conn.commit()
        conn.close()
        return {"status": "LOCKED", "reason": "clock_tamper", "claims": claims}

    # update last_seen
    conn.execute("UPDATE cloud_licenses SET last_seen=? WHERE gym_id=?", (now_unix, claims.get("gym_id")))
    conn.commit()
    conn.close()

    if now_unix < exp:
        return {"status": "ACTIVE", "reason": "ok", "claims": claims}
    if now_unix < grace:
        return {"status": "GRACE", "reason": "in_grace", "claims": claims}
    return {"status": "LOCKED", "reason": "expired", "claims": claims}

def install_license(project_root: str, license_b64: str, pubkey_pem: str = "") -> dict:
    """Install/activate a license; binds hwid on first install"""
    db = _db_path(project_root)
    _ensure_license_tables(project_root)
    try:
        lic = json.loads(base64.b64decode(license_b64).decode())
        claims = lic["claims"]
    except Exception as e:
        return {"ok": False, "error": f"bad_license:{e}"}
    if not _verify_sig(claims, lic.get("sig",""), pubkey_pem):
        return {"ok": False, "error": "bad_sig"}
    # if hwid empty in claims, bind to this device
    if not claims.get("hwid"):
        claims["hwid"] = get_hwid()
        lic["claims"] = claims
        license_b64 = base64.b64encode(json.dumps(lic).encode()).decode()
    exp = int(claims.get("exp_unix", 0))
    grace = int(claims.get("grace_until", exp + GRACE_SECONDS))
    now = int(time.time())
    conn = sqlite3.connect(db, timeout=10)
    conn.execute("INSERT OR REPLACE INTO cloud_licenses (gym_id, owner_email, tier, max_members, exp_unix, grace_until, hwid, status, last_verify, last_seen, license_b64) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
                 (claims.get("gym_id"), claims.get("owner_email"), claims.get("tier"), claims.get("max_members"), exp, grace, claims.get("hwid"), "ACTIVE", now, now, license_b64))
    conn.execute("INSERT OR REPLACE INTO gyms (gym_id, owner_email, tier, hwid, last_sync_at) VALUES (?,?,?,?,?)",
                 (claims.get("gym_id"), claims.get("owner_email"), claims.get("tier"), claims.get("hwid"), _now_pht_naive().isoformat()))
    conn.commit()
    conn.close()
    return {"ok": True, "claims": claims}

def heartbeat_ok(project_root: str, gym_id: str = "") -> bool:
    """Call after successful cloud verify to refresh last_verify"""
    db = _db_path(project_root)
    now = int(time.time())
    try:
        conn = sqlite3.connect(db, timeout=10)
        if gym_id:
            conn.execute("UPDATE cloud_licenses SET last_verify=? WHERE gym_id=?", (now, gym_id))
        else:
            conn.execute("UPDATE cloud_licenses SET last_verify=? WHERE 1", (now,))
        conn.commit()
        conn.close()
        return True
    except Exception:
        return False
