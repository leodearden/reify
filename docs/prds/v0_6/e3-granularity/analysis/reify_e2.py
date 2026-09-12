#!/usr/bin/env python3
"""E2 for reify: exact-model map from transcripts, era assignment, size x era interaction, confounds."""
import json, os, re, sqlite3, statistics, subprocess
from collections import defaultdict

SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"
TD = "/home/leo/src/reify/data/orchestrator/agent-transcripts"
RUNS = "/home/leo/src/reify/data/orchestrator/runs.db"
J = "/home/leo/src/dark-factory/data/reconciliation/write_journal.db"

def q(db, sql, params=()):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    con.row_factory = sqlite3.Row
    try:
        return [dict(r) for r in con.execute(sql, params).fetchall()]
    finally:
        con.close()

# ---------- 1. transcript exact-model map ----------
pat = re.compile(rb'"model":"(claude-[a-z0-9.-]+)"')
tspat = re.compile(rb'"timestamp":"(20[0-9-]{8}T[0-9:.]+)')
task_models = defaultdict(set)          # task -> set(exact model)
model_span = defaultdict(lambda: [None, None])  # model -> [min_ts, max_ts]
nfiles = 0
for tid in os.listdir(TD):
    tdir = os.path.join(TD, tid)
    if not os.path.isdir(tdir):
        continue
    for root, _, fs in os.walk(tdir):
        for f in fs:
            if not f.endswith(".jsonl"):
                continue
            nfiles += 1
            p = os.path.join(root, f)
            models, ts_first = set(), None
            try:
                with open(p, "rb") as fh:
                    for i, line in enumerate(fh):
                        m = pat.search(line)
                        if m:
                            models.add(m.group(1).decode())
                        if ts_first is None:
                            t = tspat.search(line)
                            if t:
                                ts_first = t.group(1).decode()
                        if i > 400 and models:
                            break
            except OSError:
                continue
            ts = ts_first or None
            for mod in models:
                task_models[tid].add(mod)
                if ts:
                    sp = model_span[mod]
                    if sp[0] is None or ts < sp[0]: sp[0] = ts
                    if sp[1] is None or ts > sp[1]: sp[1] = ts

print(f"transcript files scanned: {nfiles}; tasks with exact models: {len(task_models)}")
print("\n===== exact model observation spans (transcript corpus) =====")
for mod, (lo, hi) in sorted(model_span.items(), key=lambda x: x[1][0] or ""):
    n = sum(1 for t, ms in task_models.items() if mod in ms)
    print(f"  {mod:>24s}  first={str(lo)[:16]}  last={str(hi)[:16]}  tasks={n}")

# ---------- 2. era assignment for all landed tasks ----------
rows = json.load(open(f"{SCRATCH}/reify_task_rows.json"))
OPUS = [("2026-02-17", "opus-4.6"), ("2026-04-16", "opus-4.7"),
        ("2026-05-28", "opus-4.8"), ("2026-07-24", "opus-5")]
SONNET = [("2026-02-17", "sonnet-4.6"), ("2026-06-30", "sonnet-5")]
def era(date, alias):
    tbl = OPUS if alias == "opus" else SONNET if alias == "sonnet" else []
    cur = f"{alias}-?"
    for d, name in tbl:
        if date >= d:
            cur = name
    return cur

landed = [r for r in rows if r.get("cost") and r["cost"] > 0]
# exact model where transcripts cover the task
exact_hits = agree = 0
for r in landed:
    ms = task_models.get(r["tid"])
    r["exact_models"] = sorted(m.decode() if isinstance(m, bytes) else m for m in ms) if ms else None
    r["era"] = era(r["date"][:10], r.get("main_model") or "?")
    if ms:
        exact_hits += 1
print(f"\nlanded tasks with transcript-exact models: {exact_hits}/{len(landed)}")

# blocking escalations
besc_rows = q(RUNS, """SELECT e.task_id AS tid, count(*) AS n
    FROM events e JOIN runs r ON r.run_id=e.run_id
    WHERE r.project_id='reify' AND e.event_type='escalation_created'
      AND coalesce(json_extract(e.data,'$.severity'),'') NOT IN ('info')
      AND coalesce(json_extract(e.data,'$.category'),'') != 'review_suggestions'
    GROUP BY e.task_id""")
besc = {r["tid"]: r["n"] for r in besc_rows}
for r in landed:
    r["besc"] = besc.get(r["tid"], 0)

CUTS = [100, 300, 1000]
NAMES = ["<=100", "101-300", "301-1000", ">1000"]
def bucket(r):
    v = r["ins"] + r["dele"]
    for i, c in enumerate(CUTS):
        if v <= c: return i
    return len(CUTS)

def pct(v, p):
    v = sorted(v)
    if not v: return 0
    k = (len(v)-1)*p; f = int(k); c = min(f+1, len(v)-1)
    return v[f] + (v[c]-v[f])*(k-f)

print("\n===== size x MODEL ERA (opus-dominant landed tasks): besc / blocked-bounce / rework =====")
print(f"{'era':>10s} {'bucket':>9s} {'n':>4s} {'besc%':>6s} {'blk%':>5s} {'rework%':>7s} {'med$':>6s} {'med$/100LOC':>11s}")
grp = defaultdict(list)
for r in landed:
    if r.get("main_model") != "opus":
        continue
    grp[(r["era"], bucket(r))].append(r)
order = ["opus-4.6", "opus-4.7", "opus-4.8", "opus-5"]
for e in order:
    for bi, name in enumerate(NAMES):
        rs = grp.get((e, bi), [])
        if len(rs) < 8: continue
        b = sum(1 for x in rs if x["besc"] > 0)/len(rs)
        blk = sum(1 for x in rs if x["blocked_bounces"] > 0)/len(rs)
        rw = sum(1 for x in rs if x["attempts"] > 1)/len(rs)
        eff = pct([x["cost"]/max(x["ins"]+x["dele"],1)*100 for x in rs], .5)
        print(f"{e:>10s} {name:>9s} {len(rs):>4d} {b:>6.0%} {blk:>5.0%} {rw:>7.0%} "
              f"{pct([x['cost'] for x in rs],.5):>6.2f} {eff:>11.2f}")

# slope summary: large-vs-small besc gap per era
print("\nera slope summary (opus): besc%(>1000) - besc%(<=100), same for blk")
for e in order:
    small = grp.get((e, 0), []); large = grp.get((e, 3), [])
    if len(small) >= 8 and len(large) >= 8:
        bs = sum(1 for x in small if x["besc"] > 0)/len(small)
        bl = sum(1 for x in large if x["besc"] > 0)/len(large)
        ks = sum(1 for x in small if x["blocked_bounces"] > 0)/len(small)
        kl = sum(1 for x in large if x["blocked_bounces"] > 0)/len(large)
        print(f"  {e:>10s}: besc gap {bl-bs:+.0%} ({bs:.0%}->{bl:.0%}, n={len(small)}/{len(large)}), "
              f"blk gap {kl-ks:+.0%} ({ks:.0%}->{kl:.0%})")

# ---------- 3. confounds ----------
print("\n===== restart churn per month (runs table = orchestrator process runs) =====")
rr = q(RUNS, "SELECT substr(started_at,1,7) m, count(*) n FROM runs GROUP BY m ORDER BY m")
dur = q(RUNS, """SELECT substr(started_at,1,7) m,
                 avg((julianday(completed_at)-julianday(started_at))*24) h
                 FROM runs WHERE completed_at IS NOT NULL GROUP BY m""")
dh = {r["m"]: r["h"] for r in dur}
for r in rr:
    print(f"  {r['m']}: {r['n']:>5d} runs, mean run length {dh.get(r['m'], float('nan')):>6.1f}h")

print("\n===== cancellations per month (journal, reify) =====")
jr = q(J, """SELECT substr(created_at,1,7) m, count(*) n FROM write_ops
             WHERE project_id='reify' AND operation='set_task_status' AND success=1
               AND params LIKE '%"cancelled"%' GROUP BY m ORDER BY m""")
for r in jr:
    print(f"  {r['m']}: {r['n']} cancels")

print("\n===== config history: sizing-relevant lines =====")
out = subprocess.run(["git", "log", "--format=%h %cs %s", "--follow", "--",
                      "dark-factory-orchestrator.yaml"],
                     cwd="/home/leo/src/reify", capture_output=True, text=True).stdout
print(out[:2000] or "(no config history)")
