import sqlite3
conn = sqlite3.connect(r"C:\Users\USER\Desktop\STANDALONE\gym.db")
c = conn.cursor()

c.execute("SELECT sql FROM sqlite_master WHERE name='coaching_plans'")
print(c.fetchone()[0])
print()

c.execute("SELECT * FROM coaching_plans")
rows = c.fetchall()
print("Columns:", [d[0] for d in c.description])
for r in rows:
    print(r)

conn.close()
