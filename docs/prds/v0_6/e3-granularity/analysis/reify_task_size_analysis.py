#!/usr/bin/env python3
"""Task size vs outcome analysis for dark-factory. Read-only."""
import json, re, sqlite3, subprocess, statistics, sys
from collections import defaultdict

REPO = "/home/leo/src/reify"

def q(db, sql, params=()):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

# ---------- 1. Git: per-landed-task size ----------
raw = subprocess.run(
    ["git", "log", "--first-parent", "--merges", "-m", "--numstat",
     "--format=%x01%H|%P|%cI|%s"],
    cwd=REPO, capture_output=True, text=True).stdout

merges = {}  # task_id -> dict
cur = None
for chunk in raw.split("\x01"):
    if not chunk.strip():
        continue
    head, *lines = chunk.split("\n")
    h, parents, date, subj = head.split("|", 3)
    m = re.match(r"Merge task/(\d+) into main", subj)
    if not m:
        cur = None
        continue
    tid = m.group(1)
    files = ins = dele = 0
    test_files = 0
    for ln in lines:
        parts = ln.split("\t")
        if len(parts) != 3:
            continue
        a, d, path = parts
        files += 1
        if "test" in path.lower():
            test_files += 1
        if a != "-":
            ins += int(a)
        if d != "-":
            dele += int(d)
    ps = parents.split()
    e = merges.setdefault(tid, dict(tid=tid, files=0, ins=0, dele=0,
                                    test_files=0, n_merges=0, dates=[], pairs=[]))
    e["files"] += files; e["ins"] += ins; e["dele"] += dele
    e["test_files"] += test_files; e["n_merges"] += 1
    e["dates"].append(date)
    if len(ps) >= 2:
        e["pairs"].append((ps[0], ps[1]))

# commit counts per merge (batched rev-list calls)
for tid, e in merges.items():
    c = 0
    for p1, p2 in e["pairs"]:
        try:
            out = subprocess.run(["git", "rev-list", "--count", f"{p1}..{p2}"],
                                 cwd=REPO, capture_output=True, text=True, timeout=30).stdout.strip()
            c += int(out or 0)
        except Exception:
            pass
    e["commits"] = c
    e["date"] = max(e["dates"])

print(f"landed task merges parsed: {len(merges)}", file=sys.stderr)

# ---------- 2. runs.db ----------
RUNS = f"{REPO}/data/orchestrator/runs.db"
tr = q(RUNS, """SELECT task_id, outcome, cost_usd, duration_ms, agent_invocations,
                execute_iterations, verify_attempts, review_cycles, steward_cost_usd, completed_at
                FROM task_results WHERE project_id='reify'""")
per_task_runs = defaultdict(lambda: dict(cost=0.0, dur=0, attempts=0, outcomes=[],
                                         exec_it=0, verify=0, review=0, steward=0.0))
for r in tr:
    t = per_task_runs[r["task_id"]]
    t["cost"] += r["cost_usd"] or 0; t["dur"] += r["duration_ms"] or 0
    t["attempts"] += 1; t["outcomes"].append(r["outcome"])
    t["exec_it"] += r["execute_iterations"] or 0
    t["verify"] += r["verify_attempts"] or 0; t["review"] += r["review_cycles"] or 0
    t["steward"] += r["steward_cost_usd"] or 0

# phase costs per task (df runs only)
phase_rows = q(RUNS, """SELECT e.task_id AS tid, e.phase AS phase, sum(e.cost_usd) AS c
                        FROM events e JOIN runs r ON r.run_id=e.run_id
                        WHERE r.project_id='reify' AND e.cost_usd IS NOT NULL
                        GROUP BY e.task_id, e.phase""")
phase_cost = defaultdict(dict)
for r in phase_rows:
    phase_cost[r["tid"]][r["phase"] or "other"] = r["c"]

esc_rows = q(RUNS, """SELECT e.task_id AS tid, count(*) AS n
                      FROM events e JOIN runs r ON r.run_id=e.run_id
                      WHERE r.project_id='reify' AND e.event_type='escalation_created'
                      GROUP BY e.task_id""")
esc = {r["tid"]: r["n"] for r in esc_rows}

# model mix per task
inv_rows = q(RUNS, """SELECT task_id AS tid, model, sum(cost_usd) AS c
                      FROM invocations WHERE project_id='reify' GROUP BY task_id, model""")
task_models = defaultdict(dict)
for r in inv_rows:
    task_models[r["tid"]][r["model"]] = r["c"]

# ---------- 3. tasks.db metadata ----------
TASKS = f"{REPO}/.taskmaster/tasks/tasks.db"
trows = q(TASKS, "SELECT id, status, priority, metadata FROM tasks WHERE tag='master'")
meta = {}
for r in trows:
    md = json.loads(r["metadata"] or "{}")
    meta[str(r["id"])] = dict(
        status=r["status"],
        task_kind=md.get("task_kind"),
        complexity=md.get("complexity"),
        source=md.get("source"),
        human_decomposed=md.get("human_decomposed"),
        prd_path=md.get("prd_path"),
        declared_files=len(md.get("files") or []),
        declared_modules=len(md.get("modules") or []),
    )

# ---------- 4. journal transitions ----------
J = "/home/leo/src/dark-factory/data/reconciliation/write_journal.db"
jr = q(J, """SELECT params, created_at FROM write_ops
             WHERE project_id='reify' AND operation='set_task_status' AND success=1""")
jt = defaultdict(lambda: defaultdict(int))
for r in jr:
    try:
        p = json.loads(r["params"])
    except Exception:
        continue
    tid = str(p.get("task_id")); jt[tid][p.get("status")] += 1

# ---------- 5. join & analyze ----------
def pct(v, p):
    v = sorted(v)
    if not v: return None
    k = (len(v)-1) * p
    f = int(k); c = min(f+1, len(v)-1)
    return v[f] + (v[c]-v[f]) * (k-f)

rows = []
for tid, g in merges.items():
    r = dict(tid=tid, **{k: g[k] for k in ("files", "ins", "dele", "commits", "n_merges", "date")})
    pr = per_task_runs.get(tid)
    r.update(cost=pr["cost"] if pr else None,
             dur_h=(pr["dur"]/3.6e6) if pr else None,
             attempts=pr["attempts"] if pr else 0,
             review=pr["review"] if pr else 0,
             exec_it=pr["exec_it"] if pr else 0)
    r["esc"] = esc.get(tid, 0)
    m = meta.get(tid, {})
    r.update(m)
    r["blocked_bounces"] = jt.get(tid, {}).get("blocked", 0)
    r["dispatches"] = jt.get(tid, {}).get("in-progress", 0)
    r["cancels"] = jt.get(tid, {}).get("cancelled", 0)
    r["phases"] = phase_cost.get(tid, {})
    models = task_models.get(tid, {})
    r["main_model"] = max(models, key=models.get) if models else None
    rows.append(r)

landed = [r for r in rows if r["cost"] is not None and r["cost"] > 0]
recent = [r for r in landed if r["date"] >= "2026-06-01"]

def summarize(rs, label):
    print(f"\n===== {label} (n={len(rs)}) =====")
    for k in ("files", "ins", "commits", "cost", "dur_h"):
        v = sorted(x[k] for x in rs if x[k] is not None)
        if not v: continue
        print(f"{k:8s} p10={pct(v,.1):.1f} p25={pct(v,.25):.1f} p50={pct(v,.5):.1f} "
              f"p75={pct(v,.75):.1f} p90={pct(v,.9):.1f} p99={pct(v,.99):.1f} mean={statistics.mean(v):.1f}")

summarize(landed, "ALL landed tasks with cost data")
summarize(recent, "landed since 2026-06-01")

def bucket_analysis(rs, key, cuts, label):
    print(f"\n===== outcomes by {key} bucket — {label} =====")
    buckets = defaultdict(list)
    for r in rs:
        v = r[key]
        if v is None: continue
        for i, c in enumerate(cuts):
            if v <= c:
                buckets[i].append(r); break
        else:
            buckets[len(cuts)].append(r)
    names = [f"<={c}" for c in cuts] + [f">{cuts[-1]}"]
    hdr = f"{'bucket':>8s} {'n':>4s} {'med_cost':>8s} {'med_dur_h':>9s} {'esc_rate':>8s} {'blk_rate':>8s} {'med_disp':>8s} {'rework%':>8s} {'$/100LOC':>9s} {'med_LOC':>8s}"
    print(hdr)
    for i in range(len(cuts)+1):
        rs_ = buckets.get(i, [])
        if not rs_: continue
        med_cost = pct([r["cost"] for r in rs_], .5)
        med_dur = pct([r["dur_h"] for r in rs_], .5)
        esc_rate = sum(1 for r in rs_ if r["esc"] > 0) / len(rs_)
        blk = sum(1 for r in rs_ if r["blocked_bounces"] > 0) / len(rs_)
        med_disp = pct([r["dispatches"] for r in rs_], .5)
        rework = sum(1 for r in rs_ if r["attempts"] > 1) / len(rs_)
        loc = [max(r["ins"] + r["dele"], 1) for r in rs_]
        eff = pct([r["cost"] / max(r["ins"]+r["dele"], 1) * 100 for r in rs_], .5)
        print(f"{names[i]:>8s} {len(rs_):>4d} {med_cost:>8.2f} {med_dur:>9.2f} {esc_rate:>8.0%} "
              f"{blk:>8.0%} {med_disp:>8.1f} {rework:>8.0%} {eff:>9.2f} {pct(loc,.5):>8.0f}")

for rs_, lbl in ((landed, "all"), (recent, "since 06-01")):
    bucket_analysis(rs_, "ins", [50, 150, 400, 1000], lbl)
    bucket_analysis(rs_, "files", [2, 4, 8, 15], lbl)

# phase overhead vs size
print("\n===== median phase cost by size (ins+dele LOC) bucket — since 06-01 =====")
cuts = [100, 300, 1000]
names = ["<=100", "101-300", "301-1000", ">1000"]
buckets = defaultdict(list)
for r in recent:
    v = r["ins"] + r["dele"]
    for i, c in enumerate(cuts):
        if v <= c:
            buckets[i].append(r); break
    else:
        buckets[len(cuts)].append(r)
phases = ["plan", "execute", "review", "verify", "merge", "escalated", "blocked", "other"]
print(f"{'bucket':>9s} {'n':>4s} " + " ".join(f"{p:>8s}" for p in phases) + f" {'total':>8s}")
for i in range(len(cuts)+1):
    rs_ = buckets.get(i, [])
    if not rs_: continue
    meds = []
    for p in phases:
        vals = [r["phases"].get(p, 0.0) for r in rs_]
        meds.append(pct(vals, .5) or 0)
    tot = pct([sum(r["phases"].values()) for r in rs_], .5)
    print(f"{names[i]:>9s} {len(rs_):>4d} " + " ".join(f"{m:>8.2f}" for m in meds) + f" {tot:>8.2f}")

# model confound
print("\n===== dominant model by LOC bucket (since 06-01) =====")
for i in range(len(cuts)+1):
    rs_ = buckets.get(i, [])
    if not rs_: continue
    mc = defaultdict(int)
    for r in rs_:
        if r["main_model"]: mc[r["main_model"]] += 1
    top = sorted(mc.items(), key=lambda x: -x[1])[:4]
    print(f"{names[i]:>9s}: " + ", ".join(f"{m}={n}" for m, n in top))

# PRD-level
print("\n===== PRD-level: tasks per PRD vs outcomes =====")
prd = defaultdict(list)
for r in rows:
    if r.get("prd_path"):
        prd[r["prd_path"]].append(r)
prd_stats = []
for p, rs_ in prd.items():
    landed_ = [r for r in rs_ if r["cost"]]
    if len(landed_) < 2: continue
    prd_stats.append(dict(prd=p.split("/")[-1], n=len(landed_),
                          med_loc=pct([r["ins"]+r["dele"] for r in landed_], .5),
                          esc=sum(1 for r in landed_ if r["esc"] > 0)/len(landed_),
                          blk=sum(1 for r in landed_ if r["blocked_bounces"] > 0)/len(landed_),
                          cost=sum(r["cost"] for r in landed_),
                          loc=sum(r["ins"]+r["dele"] for r in landed_)))
prd_stats.sort(key=lambda x: -x["n"])
print(f"{'prd':>45s} {'n':>3s} {'medLOC':>7s} {'esc%':>5s} {'blk%':>5s} {'$':>8s} {'$/kLOC':>7s}")
for s in prd_stats[:20]:
    print(f"{s['prd'][:45]:>45s} {s['n']:>3d} {s['med_loc']:>7.0f} {s['esc']:>5.0%} {s['blk']:>5.0%} "
          f"{s['cost']:>8.0f} {1000*s['cost']/max(s['loc'],1):>7.0f}")

# tasks-per-prd size vs escalation: split PRDs into few-task vs many-task
few = [s for s in prd_stats if s["n"] <= 6]
many = [s for s in prd_stats if s["n"] > 6]
for grp, lbl in ((few, "PRDs with <=6 landed tasks"), (many, "PRDs with >6 landed tasks")):
    if not grp: continue
    print(f"{lbl}: n_prds={len(grp)}, mean esc_rate={statistics.mean([s['esc'] for s in grp]):.0%}, "
          f"mean blk_rate={statistics.mean([s['blk'] for s in grp]):.0%}, "
          f"med $/kLOC={pct(sorted(1000*s['cost']/max(s['loc'],1) for s in grp), .5):.0f}")

# survivorship: tasks dispatched but never landed
dispatched = {t for t, d in jt.items() if d.get("in-progress", 0) > 0}
landed_ids = set(merges)
never = dispatched - landed_ids
never_meta = [meta.get(t, {}) for t in never]
print(f"\nsurvivorship: dispatched tasks={len(dispatched)}, landed={len(dispatched & landed_ids)}, "
      f"never landed={len(never)}")
sc = defaultdict(int)
for t in never:
    sc[meta.get(t, {}).get("status", "?")] += 1
print("never-landed by current status:", dict(sorted(sc.items(), key=lambda x: -x[1])))

# spearman-ish: correlation of LOC with cost & escalations (recent)
def rank(v):
    s = sorted(range(len(v)), key=lambda i: v[i])
    r = [0]*len(v)
    for i, idx in enumerate(s): r[idx] = i
    return r
if len(recent) > 10:
    loc = [r["ins"]+r["dele"] for r in recent]
    cost = [r["cost"] for r in recent]
    escs = [r["esc"] for r in recent]
    def corr(a, b):
        ra, rb = rank(a), rank(b)
        ma, mb = statistics.mean(ra), statistics.mean(rb)
        num = sum((x-ma)*(y-mb) for x, y in zip(ra, rb))
        den = (sum((x-ma)**2 for x in ra) * sum((y-mb)**2 for y in rb)) ** .5
        return num/den if den else 0
    print(f"\nspearman (since 06-01, n={len(recent)}): LOC~cost={corr(loc,cost):.2f}, "
          f"LOC~escalations={corr(loc,escs):.2f}")

json.dump([{k: v for k, v in r.items() if k != 'phases'} for r in rows],
          open("/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad/reify_task_rows.json", "w"))
print("\nrows dumped to scratchpad/reify_task_rows.json")
