import json, sys
S = '/tmp/claude-1000/-home-leo-src-dark-factory/55a263de-1a3f-4728-ad69-78d1d09661a2/scratchpad'
rows = json.load(open(S + '/task_rows.json'))
print(type(rows).__name__, len(rows))
sample = rows[0] if isinstance(rows, list) else dict(list(rows.items())[:1])
print(json.dumps(sample, indent=1, default=str)[:1500])
if isinstance(rows, list):
    keys = set()
    for r in rows:
        keys.update(r.keys())
    print(sorted(keys))
