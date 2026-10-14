#!/usr/bin/env python3
"""E2: era (model-capability) x size-bucket outcome analysis with confound controls."""
import json, sqlite3, math, statistics
from collections import defaultdict

REPO = "/home/leo/src/dark-factory"
SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"
RUNS = f"{REPO}/data/orchestrator/runs.db"

def q(db, sql, params=()):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

rows = json.load(open(f"{SCRATCH}/task_rows.json"))

besc_rows = q(RUNS, """SELECT e.task_id AS tid, count(*) AS n
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='dark_factory' AND e.event_type='escalation_created'
      AND coalesce(json_extract(e.data,'$.severity'),'') NOT IN ('info')
      AND coalesce(json_extract(e.data,'$.category'),'') != 'review_suggestions'
    GROUP BY e.task_id""")
besc = {r["tid"]: r["n"] for r in besc_rows}
for r in rows:
    r["besc"] = besc.get(r["tid"], 0)

# eras by merge date (opus-alias / sonnet-alias serving versions)
ERAS = [
    ("M1 O4.6+S4.6", "2026-03-01", "2026-04-16"),
    ("M2 O4.7+S4.6", "2026-04-16", "2026-05-28"),
    ("M3 O4.8+S4.6", "2026-05-28", "2026-06-30"),
    ("M4 O4.8+S5",   "2026-06-30", "2026-07-24"),
    ("M5 O5+S5",     "2026-07-24", "2026-08-24"),
    ("M6 O5+S5 rst", "2026-08-24", "2026-12-31"),
]
def era(r):
    d = r["date"][:10]
    for name, a, b in ERAS:
        if a <= d < b:
            return name
    return None

CUTS = [100, 300, 1000]
NAMES = ["<=100", "101-300", "301-1000", ">1000"]
def bucket(r):
    v = r["ins"] + r["dele"]
    for i, c in enumerate(CUTS):
        if v <= c: return i
    return len(CUTS)

def wilson(k, n, z=1.96):
    if n == 0: return (0.0, 0.0, 1.0)
    p = k / n
    den = 1 + z*z/n
    c = (p + z*z/(2*n)) / den
    h = z * math.sqrt(p*(1-p)/n + z*z/(4*n*n)) / den
    return (p, max(0, c-h), min(1, c+h))

def pct(v, p):
    v = sorted(v)
    if not v: return float("nan")
    k = (len(v)-1)*p; f = int(k); c = min(f+1, len(v)-1)
    return v[f] + (v[c]-v[f])*(k-f)

landed = [r for r in rows if r.get("cost") and r["cost"] > 0]
for r in landed:
    r["era"] = era(r)
    r["bk"] = bucket(r)
landed = [r for r in landed if r["era"]]

print("===== era x LOC-bucket: n / besc% / blocked% / rework(disp>1)% / med $/kLOC / drain-cancel% =====")
grid = defaultdict(list)
for r in landed:
    grid[(r["era"], r["bk"])].append(r)
for name, a, b in ERAS:
    line = f"{name:14s}"
    for i in range(4):
        rs = grid.get((name, i), [])
        if not rs:
            line += f" | {NAMES[i]}: n=0"
            continue
        n = len(rs)
        be = sum(1 for x in rs if x["besc"] > 0)
        bl = sum(1 for x in rs if x["blocked_bounces"] > 0)
        rw = sum(1 for x in rs if x["dispatches"] > 1)
        cn = sum(1 for x in rs if x["cancels"] > 0)
        cpk = pct([x["cost"]/max(x["ins"]+x["dele"],1)*1000 for x in rs], .5)
        line += f" | {NAMES[i]}: n={n} be={be/n:.0%} bl={bl/n:.0%} rw={rw/n:.0%} $k={cpk:.1f} cx={cn/n:.0%}"
    print(line)

print("\n===== size->risk slope per era: besc rate top bucket (>1000) minus bottom (<=100), Wilson CIs =====")
for name, a, b in ERAS:
    lo = grid.get((name, 0), []); hi = grid.get((name, 3), [])
    if len(lo) < 8 or len(hi) < 8:
        print(f"{name:14s}  insufficient n (lo={len(lo)}, hi={len(hi)})")
        continue
    plo = wilson(sum(1 for x in lo if x["besc"] > 0), len(lo))
    phi = wilson(sum(1 for x in hi if x["besc"] > 0), len(hi))
    print(f"{name:14s}  lo={plo[0]:.0%} [{plo[1]:.0%},{plo[2]:.0%}] n={len(lo)}   "
          f"hi={phi[0]:.0%} [{phi[1]:.0%},{phi[2]:.0%}] n={len(hi)}   diff={phi[0]-plo[0]:+.0%}")

print("\n===== like-for-like cost & besc: 301-1000 LOC bucket per era =====")
for name, a, b in ERAS:
    rs = grid.get((name, 2), [])
    if len(rs) < 8:
        print(f"{name:14s}  n={len(rs)} (skip)"); continue
    n = len(rs)
    be = wilson(sum(1 for x in rs if x["besc"] > 0), n)
    medc = pct([x["cost"] for x in rs], .5)
    medl = pct([x["ins"]+x["dele"] for x in rs], .5)
    medd = pct([x["dur_h"] for x in rs if x.get("dur_h")], .5)
    print(f"{name:14s}  n={n:4d} med$={medc:6.2f} medLOC={medl:5.0f} medDur={medd:5.2f}h besc={be[0]:.0%} [{be[1]:.0%},{be[2]:.0%}]")

print("\n===== M5/M6 drain control: besc by bucket, cancels==0 vs >0 =====")
for label, pred in [("no-drain", lambda x: x["cancels"] == 0), ("drained", lambda x: x["cancels"] > 0)]:
    line = f"{label:9s}"
    for i in range(4):
        rs = [x for x in landed if x["era"] in ("M5 O5+S5", "M6 O5+S5 rst") and x["bk"] == i and pred(x)]
        if not rs:
            line += f" | {NAMES[i]}: n=0"; continue
        n = len(rs); be = sum(1 for x in rs if x["besc"] > 0)
        line += f" | {NAMES[i]}: n={n} be={be/n:.0%}"
    print(line)

print("\n===== capped-invocation share by month x role (turn/budget saturation proxy) =====")
cap = q(RUNS, """SELECT substr(started_at,1,7) AS mon, role,
       count(*) AS n, sum(capped) AS c
    FROM invocations WHERE project_id='dark_factory'
      AND role IN ('implementer','architect','reviewer_comprehensive','simple_task')
    GROUP BY mon, role ORDER BY mon""")
bymon = defaultdict(dict)
for r in cap:
    bymon[r["mon"]][r["role"]] = (r["c"] or 0, r["n"])
roles = ["architect", "implementer", "reviewer_comprehensive", "simple_task"]
print(f"{'month':8s}" + "".join(f"{ro:>26s}" for ro in roles))
for mon in sorted(bymon):
    line = f"{mon:8s}"
    for ro in roles:
        c, n = bymon[mon].get(ro, (0, 0))
        line += f"{(str(c)+'/'+str(n)+' ('+format(c/n if n else 0,'.0%')+')'):>26s}"
    print(line)

print("\n===== task-mix drift per era (maturity confound check) =====")
for name, a, b in ERAS:
    rs = [r for r in landed if r["era"] == name]
    if not rs: continue
    n = len(rs)
    det = sum(1 for x in rs if x.get("task_kind") == "deterministic")
    smp = sum(1 for x in rs if x.get("complexity") == "simple")
    prd = sum(1 for x in rs if x.get("prd_path"))
    medl = pct([x["ins"]+x["dele"] for x in rs], .5)
    medf = pct([x["files"] for x in rs], .5)
    print(f"{name:14s} n={n:4d} medLOC={medl:5.0f} medFiles={medf:3.0f} determ={det/n:.0%} simple={smp/n:.0%} prd-linked={prd/n:.0%}")

# PRD-level slope check in eras with dense prd_path
print("\n===== PRD-clustered check (eras M3-M6): per-PRD mean LOC vs any-besc share =====")
prds = defaultdict(list)
for r in landed:
    if r["era"] in ("M3 O4.8+S4.6", "M4 O4.8+S5", "M5 O5+S5", "M6 O5+S5 rst") and r.get("prd_path"):
        prds[r["prd_path"]].append(r)
pts = []
for p, rs in prds.items():
    if len(rs) >= 3:
        pts.append((statistics.median(x["ins"]+x["dele"] for x in rs),
                    sum(1 for x in rs if x["besc"] > 0)/len(rs), len(rs), p))
pts.sort()
n = len(pts)
if n >= 8:
    lo_half = pts[:n//2]; hi_half = pts[n//2:]
    print(f"PRDs n={n}; small-task PRDs (medLOC<= {pts[n//2][0]:.0f}): mean besc-share={statistics.mean(x[1] for x in lo_half):.0%}; "
          f"large-task PRDs: mean besc-share={statistics.mean(x[1] for x in hi_half):.0%}")
    def spearman(a, b):
        def rank(v):
            s = sorted(range(len(v)), key=lambda i: v[i]); rk = [0]*len(v)
            for i, j in enumerate(s): rk[j] = i
            return rk
        ra, rb = rank(a), rank(b)
        ma, mb = statistics.mean(ra), statistics.mean(rb)
        num = sum((x-ma)*(y-mb) for x, y in zip(ra, rb))
        da = math.sqrt(sum((x-ma)**2 for x in ra)); db = math.sqrt(sum((y-mb)**2 for y in rb))
        return num/(da*db) if da and db else float("nan")
    print(f"Spearman(PRD median LOC, PRD besc-share) = {spearman([x[0] for x in pts], [x[1] for x in pts]):.2f}")
