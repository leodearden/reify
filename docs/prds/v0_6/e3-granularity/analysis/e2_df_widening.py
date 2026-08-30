#!/usr/bin/env python3
"""Bonus: touched-but-undeclared widening rate by size bucket; drain-record sanity check."""
import json, sqlite3, subprocess, re
from collections import defaultdict

REPO = "/home/leo/src/dark-factory"
SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"

def q(db, sql, params=()):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

# 1. journal cancelled-transition sanity (are drain cancels recorded at all?)
J = f"{REPO}/data/reconciliation/write_journal.db"
tables = q(J, "SELECT name FROM sqlite_master WHERE type='table'")
print("journal tables:", [t["name"] for t in tables])
jt = [t["name"] for t in tables if "journal" in t["name"] or "write" in t["name"]]
if jt:
    t = jt[0]
    cols = [c["name"] for c in q(J, f"PRAGMA table_info({t})")]
    print(f"{t} cols:", cols)
    rows = q(J, f"SELECT substr(created_at,1,10) AS d, count(*) AS n FROM {t} "
                f"WHERE operation='set_task_status' AND params LIKE '%cancelled%' "
                f"AND project_id='dark_factory' "
                f"AND substr(created_at,1,7)='2026-08' GROUP BY d ORDER BY d")
    print("aug cancelled set_task_status by day:", [(r["d"], r["n"]) for r in rows])

# 2. widening: merge numstat file lists vs metadata.files
raw = subprocess.run(
    ["git", "log", "--first-parent", "--merges", "-m", "--numstat",
     "--format=%x01%H|%cI|%s"],
    cwd=REPO, capture_output=True, text=True).stdout
task_files = {}
task_date = {}
cur_tid = None
for chunk in raw.split("\x01"):
    if not chunk.strip():
        continue
    header, _, body = chunk.partition("\n")
    parts = header.split("|")
    if len(parts) < 3:
        continue
    m = re.match(r"Merge task/(\d+) into main", parts[2])
    if not m:
        cur_tid = None
        continue
    tid = m.group(1)
    files = set()
    for ln in body.splitlines():
        cols = ln.split("\t")
        if len(cols) == 3:
            files.add(cols[2])
    if tid not in task_files:
        task_files[tid] = files
        task_date[tid] = parts[1][:10]
    else:
        task_files[tid] |= files

TASKS = f"{REPO}/.taskmaster/tasks/tasks.db"
trows = q(TASKS, "SELECT id, metadata FROM tasks WHERE tag='master'")
declared = {}
for r in trows:
    md = json.loads(r["metadata"] or "{}")
    fl = md.get("files")
    if isinstance(fl, list) and fl:
        declared[str(r["id"])] = set(str(x).strip().lstrip("./") for x in fl if isinstance(x, str))

CUTS = [100, 300, 1000]
NAMES = ["<=100", "101-300", "301-1000", ">1000"]
rows = json.load(open(f"{SCRATCH}/task_rows.json"))
loc = {r["tid"]: r["ins"] + r["dele"] for r in rows}

def bucket(v):
    for i, c in enumerate(CUTS):
        if v <= c: return i
    return len(CUTS)

stats = defaultdict(lambda: dict(n=0, widened=0, wide_files=[], ratio=[]))
NOISE = re.compile(r"(^|/)(\.gitignore|uv\.lock|.*\.lock)$")
for tid, touched in task_files.items():
    if tid not in declared or tid not in loc:
        continue
    dec = declared[tid]
    tch = set(f for f in touched if not NOISE.search(f))
    extra = tch - dec
    b = bucket(loc[tid])
    s = stats[b]
    s["n"] += 1
    if extra:
        s["widened"] += 1
        s["wide_files"].append(len(extra))
    s["ratio"].append(len(extra) / max(len(dec), 1))

print("\n===== touched-but-undeclared widening by LOC bucket (landed tasks with metadata.files) =====")
print(f"{'bucket':>9s} {'n':>5s} {'>=1 undeclared':>15s} {'med extra files':>16s} {'mean extra/declared':>20s}")
import statistics
for i in range(4):
    s = stats[i]
    if not s["n"]: continue
    med_extra = statistics.median(s["wide_files"]) if s["wide_files"] else 0
    print(f"{NAMES[i]:>9s} {s['n']:>5d} {s['widened']/s['n']:>14.0%} {med_extra:>16.0f} {statistics.mean(s['ratio']):>20.2f}")

cov = sum(s["n"] for s in stats.values())
print(f"\ncoverage: {cov} of {len(task_files)} landed merge-tasks have metadata.files declarations")
