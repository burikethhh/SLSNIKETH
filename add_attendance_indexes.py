import sqlite3
conn = sqlite3.connect(r"C:\Users\USER\Desktop\STANDALONE\gym.db")
c = conn.cursor()

indexes = [
    "CREATE INDEX IF NOT EXISTS idx_attendance_member_dir_date ON attendance(member_id, direction, date(timestamp))",
    "CREATE INDEX IF NOT EXISTS idx_attendance_staff_dir_date ON attendance(staff_id, direction, date(timestamp))",
    "CREATE INDEX IF NOT EXISTS idx_attendance_familiar_dir_date ON attendance(familiar_id, direction, date(timestamp))",
]

for idx_sql in indexes:
    try:
        c.execute(idx_sql)
        name = idx_sql.split("idx_")[1].split(" ")[0]
        print(f"Created index: idx_{name}")
    except Exception as e:
        print(f"Index error: {e}")

conn.commit()

# Verify
c.execute("SELECT name, sql FROM sqlite_master WHERE type='index' AND name LIKE 'idx_attendance%' ORDER BY name")
for r in c.fetchall():
    print(f"  {r[0]}")

conn.close()
print("Done.")
