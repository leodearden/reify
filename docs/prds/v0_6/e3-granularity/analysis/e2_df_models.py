#!/usr/bin/env python3
"""Sweep agent-transcripts for exact model ids; derive empirical alias->version switch dates."""
import os, re, json
from collections import defaultdict

ROOT = "/home/leo/src/dark-factory/data/orchestrator/agent-transcripts"
SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"
MODEL_RE = re.compile(rb'"model"\s*:\s*"(claude-[a-z0-9.-]+)"')
TS_RE = re.compile(rb'"timestamp"\s*:\s*"([0-9T:.Z+-]+)"')

records = []  # (task_id, file, first_ts, models_set)
n_files = 0
for task_id in os.listdir(ROOT):
    tdir = os.path.join(ROOT, task_id)
    if not os.path.isdir(tdir):
        continue
    for dirpath, _dirs, files in os.walk(tdir):
        for fn in files:
            if not fn.endswith(".jsonl"):
                continue
            p = os.path.join(dirpath, fn)
            n_files += 1
            models = set()
            first_ts = None
            try:
                with open(p, "rb") as f:
                    chunk = f.read(4_000_000)  # 4MB cap: model appears on first assistant msg
                m = TS_RE.search(chunk)
                if m:
                    first_ts = m.group(1).decode()
                for mm in MODEL_RE.finditer(chunk):
                    s = mm.group(1).decode()
                    if "<synthetic" not in s:
                        models.add(s)
            except OSError:
                continue
            if models:
                records.append((task_id, first_ts or "", sorted(models)))

print(f"transcript files scanned: {n_files}; with model ids: {len(records)}")
dates = sorted(r[1][:10] for r in records if r[1])
print(f"retention span: {dates[0]} .. {dates[-1]}  ({len(set(dates))} distinct days)")

# empirical timeline per alias family
FAM = {"opus": re.compile(r"claude-opus"), "sonnet": re.compile(r"claude-sonnet"), "haiku": re.compile(r"claude-haiku")}
by_exact = defaultdict(list)  # exact -> [date]
for tid, ts, models in records:
    if not ts:
        continue
    d = ts[:10]
    for m in models:
        by_exact[m].append(d)

print("\nexact model presence windows (transcript ground truth):")
for m in sorted(by_exact):
    ds = sorted(by_exact[m])
    print(f"  {m:28s} n_files={len(ds):5d}  {ds[0]} .. {ds[-1]}")

# per-task exact model sets + dominant date
task_models = defaultdict(set)
task_dates = {}
for tid, ts, models in records:
    task_models[tid].update(models)
    if ts and (tid not in task_dates or ts < task_dates[tid]):
        task_dates[tid] = ts
out = {tid: {"models": sorted(ms), "first_ts": task_dates.get(tid, "")} for tid, ms in task_models.items()}
json.dump(out, open(f"{SCRATCH}/e2_df_task_models.json", "w"), indent=0)
print(f"\ntasks with transcript model data: {len(out)} -> e2_df_task_models.json")
