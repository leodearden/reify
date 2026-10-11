# E3 analysis records

Each directory here is one run of `../e3_analysis.py`, named by its `--as-of`
instant (`%Y%m%dT%H%M%SZ`, UTC):

- `report.txt`: the human-readable report, also printed on stdout;
- `endpoints.json`: the same figures as structured data;
- `tasks-snapshot.json`: the task-store read. It is written only by a live
  run, and it is what makes the record replayable.

A record is never overwritten. The script refuses an out-dir that exists and
is not empty.

## Replaying a record

Replay a record from the repository root, into a scratch directory:

```bash
RECORD=docs/prds/v0_6/e3-granularity/analysis/results/20261010T200000Z
python3 docs/prds/v0_6/e3-granularity/analysis/e3_analysis.py \
    --as-of 2026-10-10T20:00:00Z \
    --tasks-snapshot "$RECORD/tasks-snapshot.json" \
    --out-dir /tmp/e3-replay
cmp "$RECORD/report.txt" /tmp/e3-replay/report.txt
cmp "$RECORD/endpoints.json" /tmp/e3-replay/endpoints.json
```

A replay re-reads the live `data/orchestrator/runs.db` and the `main` ref, but
every source is cut at the `--as-of` instant. Activity after that instant
therefore cannot change the record. Only a change to the script's output can.

`tests/infra/test_e3_analysis.py` (`CommittedRecordTests`) checks the
hermetic half of this. Every committed `tasks-snapshot.json` must still read,
and must still attribute under the registered protocol.
