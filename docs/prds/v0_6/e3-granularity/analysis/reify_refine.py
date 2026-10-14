#!/usr/bin/env python3
"""Refinements: blocking-only escalations, phase fractions, fixed-cost fit, alias/period buckets."""
import json, sqlite3, statistics
from collections import defaultdict

REPO = "/home/leo/src/reify"
SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"

def q(db, sql, params=()):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

rows = json.load(open(f"{SCRATCH}/reify_task_rows.json"))
RUNS = f"{REPO}/data/orchestrator/runs.db"

# blocking escalations per task (exclude review_suggestions / info severity)
besc_rows = q(RUNS, """SELECT e.task_id AS tid, count(*) AS n
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='reify' AND e.event_type='escalation_created'
      AND coalesce(json_extract(e.data,'$.severity'),'') NOT IN ('info')
      AND coalesce(json_extract(e.data,'$.category'),'') != 'review_suggestions'
    GROUP BY e.task_id""")
besc = {r["tid"]: r["n"] for r in besc_rows}

# phase SUM costs per task (plan/execute/review/verify only; task_completed rows are per-attempt totals, excluded)
ph_rows = q(RUNS, """SELECT e.task_id AS tid, e.phase AS phase, sum(e.cost_usd) AS c
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='reify' AND e.cost_usd IS NOT NULL AND e.phase IS NOT NULL
    GROUP BY e.task_id, e.phase""")
ph = defaultdict(dict)
for r in ph_rows:
    ph[r["tid"]][r["phase"]] = r["c"]

for r in rows:
    r["besc"] = besc.get(r["tid"], 0)
    r["ph"] = ph.get(r["tid"], {})

landed = [r for r in rows if r.get("cost") and r["cost"] > 0]
recent = [r for r in landed if r["date"] >= "2026-06-01"]

def pct(v, p):
    v = sorted(v)
    if not v: return 0
    k = (len(v)-1)*p; f = int(k); c = min(f+1, len(v)-1)
    return v[f] + (v[c]-v[f])*(k-f)

CUTS = [100, 300, 1000]
NAMES = ["<=100", "101-300", "301-1000", ">1000"]
def bucket(r):
    v = r["ins"] + r["dele"]
    for i, c in enumerate(CUTS):
        if v <= c: return i
    return len(CUTS)

print("===== since 06-01: blocking-only escalation & phase fractions by LOC bucket =====")
print(f"{'bucket':>9s} {'n':>4s} {'besc%':>6s} {'med$':>6s} {'plan%':>6s} {'exec%':>6s} {'review%':>7s} {'verify%':>7s} {'agg$/task':>9s}")
bk = defaultdict(list)
for r in recent: bk[bucket(r)].append(r)
for i, name in enumerate(NAMES):
    rs = bk.get(i, [])
    if not rs: continue
    besc_rate = sum(1 for r in rs if r["besc"] > 0)/len(rs)
    agg = {p: sum(r["ph"].get(p, 0) for r in rs) for p in ("plan","execute","review","verify")}
    tot = sum(agg.values()) or 1
    aggtask = tot/len(rs)
    print(f"{name:>9s} {len(rs):>4d} {besc_rate:>6.0%} {pct([r['cost'] for r in rs],.5):>6.2f} "
          f"{agg['plan']/tot:>6.0%} {agg['execute']/tot:>6.0%} {agg['review']/tot:>7.0%} {agg['verify']/tot:>7.0%} {aggtask:>9.2f}")

# OLS fixed-cost fit on recent: cost = a + b*LOC
x = [r["ins"]+r["dele"] for r in recent]; y = [r["cost"] for r in recent]
mx, my = statistics.mean(x), statistics.mean(y)
b = sum((xi-mx)*(yi-my) for xi, yi in zip(x, y)) / sum((xi-mx)**2 for xi in x)
a = my - b*mx
print(f"\nOLS since 06-01 (n={len(recent)}): cost = ${a:.2f} + ${1000*b:.2f}/kLOC  "
      f"(median cost of <=50-LOC decile as overhead floor: ${pct([r['cost'] for r in recent if r['ins']+r['dele']<=50],.5):.2f})")

# (alias, month) outcome buckets
print("\n===== outcomes by (dominant alias, merge month) x LOC bucket =====")
print(f"{'month':>8s} {'alias':>7s} {'bucket':>9s} {'n':>4s} {'med$':>6s} {'besc%':>6s} {'blk%':>5s} {'rework%':>7s} {'med$/100LOC':>11s}")
grp = defaultdict(list)
for r in landed:
    if not r.get("main_model"): continue
    month = r["date"][:7]
    if month < "2026-05": month = "<=2026-04"
    grp[(month, r["main_model"], bucket(r))].append(r)
for (month, alias, bi) in sorted(grp):
    rs = grp[(month, alias, bi)]
    if len(rs) < 8: continue
    besc_rate = sum(1 for r in rs if r["besc"] > 0)/len(rs)
    blk = sum(1 for r in rs if r["blocked_bounces"] > 0)/len(rs)
    rw = sum(1 for r in rs if r["attempts"] > 1)/len(rs)
    eff = pct([r["cost"]/max(r["ins"]+r["dele"],1)*100 for r in rs], .5)
    print(f"{month:>8s} {alias:>7s} {NAMES[bi]:>9s} {len(rs):>4d} {pct([r['cost'] for r in rs],.5):>6.2f} "
          f"{besc_rate:>6.0%} {blk:>5.0%} {rw:>7.0%} {eff:>11.2f}")

# invocation timestamp distribution per role/alias by month (for Leo's date cross-ref)
print("\n===== invocations per (month, alias) and per (month, role, alias) [top roles] =====")
inv = q(RUNS, """SELECT substr(completed_at,1,7) AS m, model, role, count(*) AS n
                 FROM invocations WHERE project_id='reify' GROUP BY 1,2,3""")
ma = defaultdict(int)
for r in inv: ma[(r["m"], r["model"])] += r["n"]
for k in sorted(ma): print(f"  {k[0]} {k[1]:>7s} {ma[k]:>6d}")
roles = defaultdict(int)
for r in inv: roles[r["role"]] += r["n"]
top_roles = {k for k, _ in sorted(roles.items(), key=lambda x: -x[1])[:5]}
print("  --- per role (top 5) ---")
for r in sorted(inv, key=lambda r: (r["role"], r["m"])):
    if r["role"] in top_roles and r["n"] >= 20:
        print(f"  {r['m']} {r['role']:>24s} {r['model']:>7s} {r['n']:>6d}")
