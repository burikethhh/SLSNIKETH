"""Run once: python -m license.migrate"""
import os, sqlite3, sys
project_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
if project_root not in sys.path:
    sys.path.insert(0, project_root)
from license.validator import _ensure_license_tables
from pathlib import Path

db = os.path.join(project_root, "gym.db")
if not os.path.exists(db):
    # also try SOLO_DATA_DIR
    d = os.environ.get("SOLO_DATA_DIR")
    if d:
        db = os.path.join(d, "gym.db")
print(f"Migrating {db}")
_ensure_license_tables(os.path.dirname(db) if os.path.isdir(os.path.dirname(db)) else project_root)
# Actually call with correct project_root
from license.validator import _ensure_license_tables as _e
_e(db if os.path.exists(os.path.dirname(db)) else db)
print("Done: cloud_licenses, gyms, members.gym_id, attendance.gym_id")
