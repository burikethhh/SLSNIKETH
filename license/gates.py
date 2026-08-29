"""Tier gate: block new registration at cap, allow renewal (global dedup =1)"""
import sqlite3, os, hashlib

def _db(project_root: str):
    d = os.environ.get("SOLO_DATA_DIR")
    return os.path.join(d, "gym.db") if d else os.path.join(project_root, "gym.db")

def _face_hash(face_vector_blob: bytes) -> str:
    if not face_vector_blob:
        return ""
    return hashlib.sha256(face_vector_blob).hexdigest()[:16]

def can_register(project_root: str, max_members: int, owner_email: str = "") -> dict:
    """Check tier cap using global dedup (face_hash/phone) =1"""
    db = _db(project_root)
    conn = sqlite3.connect(db, timeout=10)
    try:
        # Use DISTINCT face_hash if available, else count active members
        # Global dedup: if owner_email grouping exists, count distinct face_hash across gyms for that owner
        if owner_email:
            cols = [r[1] for r in conn.execute("PRAGMA table_info(members)").fetchall()]
            has_owner = "owner_email" in cols
            if "face_vector" in cols and has_owner:
                cnt = conn.execute("SELECT COUNT(DISTINCT face_vector) FROM members WHERE status='active' AND face_vector IS NOT NULL AND owner_email=?", (owner_email,)).fetchone()[0]
                if cnt is None:
                    cnt = 0
            elif has_owner:
                cnt = conn.execute("SELECT COUNT(*) FROM members WHERE status='active' AND owner_email=?", (owner_email,)).fetchone()[0]
            elif "face_vector" in cols:
                cnt = conn.execute("SELECT COUNT(DISTINCT face_vector) FROM members WHERE status='active' AND face_vector IS NOT NULL").fetchone()[0]
                if cnt is None:
                    cnt = 0
            else:
                cnt = conn.execute("SELECT COUNT(*) FROM members WHERE status='active'").fetchone()[0]
        else:
            cnt = conn.execute("SELECT COUNT(*) FROM members WHERE status='active'").fetchone()[0]
        conn.close()
        if cnt >= max_members:
            return {"allowed": False, "reason": f"tier_cap {cnt}/{max_members}", "count": cnt}
        return {"allowed": True, "count": cnt}
    except Exception as e:
        try: conn.close()
        except: pass
        return {"allowed": False, "reason": str(e)}

def can_renew(project_root: str, member_id: int) -> dict:
    """Renewals always allowed (never block)"""
    return {"allowed": True, "reason": "renewal_always_allowed", "member_id": member_id}
