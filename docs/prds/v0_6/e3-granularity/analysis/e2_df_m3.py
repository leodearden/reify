#!/usr/bin/env python3
"""Composition of the anomalous flat-slope M3 (Jun) >1000-LOC cohort vs M4/M5 cohorts."""
import json
from collections import Counter

SCRATCH = "/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad"
rows = json.load(open(f"{SCRATCH}/task_rows.json"))

def era(d):
    if "2026-05-28" <= d < "2026-06-30": return "M3"
    if "2026-06-30" <= d < "2026-07-24": return "M4"
    if "2026-07-24" <= d < "2026-08-24": return "M5"
    return None

for e in ("M3", "M4", "M5"):
    big = [r for r in rows if r.get("cost") and r["cost"] > 0 and era(r["date"][:10]) == e
           and r["ins"] + r["dele"] > 1000]
    prds = Counter((r.get("prd_path") or "none").split("/")[-1] for r in big)
    print(f"{e}: n={len(big)}  top prds: {prds.most_common(5)}")
    src = Counter(r.get("source") or "?" for r in big)
    print(f"    sources: {src.most_common(5)}")
