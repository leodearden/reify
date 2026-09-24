#!/usr/bin/env python3
"""Orchestrator run cadence + dispatch inflation over time (operational confound dating)."""
import sqlite3
from collections import defaultdict

REPO = "/home/leo/src/dark-factory"
RUNS = f"{REPO}/data/orchestrator/runs.db"

def q(sql, params=()):
    con = sqlite3.connect(f"file:{RUNS}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

cols = [c["name"] for c in q("PRAGMA table_info(runs)")]
print("runs cols:", cols)

rows = q("""SELECT substr(started_at,1,7) AS mon, count(*) AS n_runs,
    sum(julianday(coalesce(completed_at, started_at)) - julianday(started_at)) * 24 AS total_h
    FROM runs WHERE project_id='dark_factory' GROUP BY mon ORDER BY mon""")
print(f"\n{'month':8s} {'runs':>5s} {'runs/day':>9s} {'med run h':>10s}")
for r in rows:
    med = q("""SELECT (julianday(coalesce(completed_at, started_at)) - julianday(started_at)) * 24 AS h
        FROM runs WHERE project_id='dark_factory' AND substr(started_at,1,7)=?
        ORDER BY h LIMIT 1 OFFSET (
          SELECT count(*)/2 FROM runs WHERE project_id='dark_factory' AND substr(started_at,1,7)=?)""",
        (r["mon"], r["mon"]))
    days = 30
    print(f"{r['mon']:8s} {r['n_runs']:>5d} {r['n_runs']/days:>9.1f} {(med[0]['h'] if med else 0):>10.1f}")

# weekly for Jun-Aug detail
rows = q("""SELECT strftime('%Y-%W', started_at) AS wk, count(*) AS n
    FROM runs WHERE project_id='dark_factory' AND started_at >= '2026-06-01'
    GROUP BY wk ORDER BY wk""")
print("\nweekly runs since Jun:", [(r["wk"], r["n"]) for r in rows])

# task_started events per landed task by month (dispatch inflation dating)
rows = q("""SELECT substr(e.timestamp,1,7) AS mon, count(*) AS n
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='dark_factory' AND e.event_type='task_started'
    GROUP BY mon ORDER BY mon""")
print("\ntask_started events by month:", [(r["mon"], r["n"]) for r in rows])
rows = q("""SELECT substr(e.timestamp,1,7) AS mon, count(DISTINCT e.task_id) AS n
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='dark_factory' AND e.event_type='task_started'
    GROUP BY mon ORDER BY mon""")
print("distinct tasks started by month:", [(r["mon"], r["n"]) for r in rows])
