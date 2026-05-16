# Output Format

All on-disk artifacts written by the `/audit` skill. Design foundation: `docs/architecture-audit/f-infra-design.md` §7.

---

## §1 Per-run JSON path

```
data/audit-runs/<YYYY-MM-DDTHH-MM-SSZ>.json
```

**Filename convention:** ISO-8601 with colons replaced by dashes (filesystem-safe, lexicographically sortable):

```
2026-05-16T07-30-45Z.json    ✓ (colon-free, portable)
2026-05-16T07:30:45Z.json    ✗ (colons invalid on Windows/FAT, inconvenient in shell paths)
```

Use UTC (`Z` suffix). `ls data/audit-runs/` produces chronological order because the timestamps sort lexicographically.

**Gitignore:** `data/audit-runs/` is gitignored (added as a prerequisite in this task). Per-run files are local-only time-series data; they are not committed to the repo.

---

## §2 Per-run JSON schema

```jsonc
{
  "timestamp": "2026-05-16T07:30:45Z",     // ISO-8601 UTC, full colons (not filename-safe form)
  "scope": {
    // Exactly one of the following, matching the invocation mode:
    "window": "14d",                         // default or --since 14d
    "window": "2026-05-02..now",             // --since <date>
    "task": "3242",                          // --task <id>
    "patterns": ["P1"]                       // --pattern P1 (or ["P2"], ["P5"])
  },
  "cli": {
    "argv": ["--since", "2026-05-02", "--tasks-file", "…"],  // exact argv passed to reify-audit
    "exit_code": 0                           // 0, 1-254, or 125
  },
  "findings": [
    {
      "finding_id": "f-<timestamp>-<n>",     // e.g. "f-20260516T073045Z-0" (0-indexed within run)
      "severity": "High",                    // "High" | "Medium" | "Low"
      "pattern": "P5PhantomDone",            // from Finding.pattern (CLI value)
      "task_id": "3242",                     // from Finding.task_id
      "summary": "task marked done but …",   // from Finding.summary
      "evidence_refs": ["…"],                // from Finding.evidence (list of strings)

      // Action taken by the skill for this finding:
      "action_taken": "escalated",           // "escalated" | "filed" | "deduped" | "logged"

      // Present only when action_taken == "filed":
      "task_id_filed": "3901",               // task_id returned by submit_task

      // Present only when action_taken == "escalated":
      "escalation_id": "esc-abc123",         // ID returned by escalate_info (if available)

      // Present only when action_taken == "deduped":
      "prior_finding_id": "f-20260510T120000Z-1"  // finding_id from the prior run
    }
  ]
}
```

### Worked example — mixed-severity run

```json
{
  "timestamp": "2026-05-16T07:30:45Z",
  "scope": { "window": "14d" },
  "cli": {
    "argv": ["--since", "2026-05-02", "--tasks-file", ".taskmaster/tasks/tasks.json",
             "--runs-db", "data/orchestrator/runs.db", "--project-root", "."],
    "exit_code": 1
  },
  "findings": [
    {
      "finding_id": "f-20260516T073045Z-0",
      "severity": "High",
      "pattern": "P5PhantomDone",
      "task_id": "3242",
      "summary": "task marked done but metadata files missing",
      "evidence_refs": ["task status=done; files=[]; done_provenance=null"],
      "action_taken": "escalated",
      "escalation_id": "esc-7f4a2b"
    },
    {
      "finding_id": "f-20260516T073045Z-1",
      "severity": "Medium",
      "pattern": "P2ConsumerStub",
      "task_id": "3301",
      "summary": "stub AuditReporter introduced but no consumer wired",
      "evidence_refs": ["src/audit/reporter.rs:AuditReporter (stub, introduced task 3301)"],
      "action_taken": "filed",
      "task_id_filed": "3902"
    },
    {
      "finding_id": "f-20260516T073045Z-2",
      "severity": "Medium",
      "pattern": "P1ProducerOrphan",
      "task_id": "3288",
      "summary": "SchedulePolicy exported but no downstream consumer",
      "evidence_refs": ["crates/reify-planner/src/schedule.rs:SchedulePolicy"],
      "action_taken": "deduped",
      "prior_finding_id": "f-20260510T120000Z-1"
    },
    {
      "finding_id": "f-20260516T073045Z-3",
      "severity": "Low",
      "pattern": "P1ProducerOrphan",
      "task_id": "3210",
      "summary": "Cargo.lock-only change, no symbol orphan",
      "evidence_refs": ["Cargo.lock"],
      "action_taken": "logged"
    }
  ]
}
```

---

## §3 Dedupe index

```
data/audit-runs/index.json
```

Append-only across all runs (entries from prior runs are never deleted); rewritten in full at the end of each run.

```jsonc
{
  "entries": [
    {
      "key": {
        "parent_task_id": "3288",
        "audit_cluster": "P1",
        "symbol_or_path": "crates/reify-planner/src/schedule.rs:SchedulePolicy"
      },
      "finding_id": "f-20260510T120000Z-1",   // first run that detected this finding
      "filed_at": "2026-05-10T12:00:00Z",
      "follow_up_task_id": "3845"              // task filed on first detection (omit if none was filed)
    }
  ]
}
```

The `key` triple `(parent_task_id, audit_cluster, symbol_or_path)` is the dedupe identity. Before filing any Medium-severity follow-up task, look up this key in `entries`. See `references/severity-routing.md` §3 for the full lookup procedure.

---

## §4 Markdown rendering rules (`--format markdown`)

Emitted to the user when `--format markdown` is given. Not written to disk (the per-run JSON is the on-disk record).

### Slice-1 minimal format (current)

```markdown
# /audit run 2026-05-16T07-30-45Z

3 findings (1 high, 2 medium, 0 low)

## High

| task_id | pattern | summary | action_taken |
|---------|---------|---------|--------------|
| 3242 | P5PhantomDone | task marked done but metadata files missing | escalated |

## Medium

| task_id | pattern | summary | action_taken |
|---------|---------|---------|--------------|
| 3301 | P2ConsumerStub | stub AuditReporter introduced but no consumer wired | filed (→ task 3902) |
| 3288 | P1ProducerOrphan | SchedulePolicy exported but no downstream consumer | deduped (prior: f-20260510T120000Z-1) |
```

**Rules:**
- Open with `# /audit run <timestamp>` (use the filesystem-safe form with dashes, matching the artifact filename).
- Summary line: `N findings (X high, Y medium, Z low)`. Count findings from all three severities; omit zero-count severities from the summary if desired (e.g. "1 high, 2 medium" rather than "1 high, 2 medium, 0 low").
- One `## <Severity>` section per severity with findings; **omit empty sections entirely**.
- Within each section, a markdown table with columns: `task_id | pattern | summary | action_taken`.
- In `action_taken`, expand with context where useful: `filed (→ task <id>)` for filed findings, `deduped (prior: <finding_id>)` for deduped findings.

### Slice-2 (deferred)

Per-finding evidence expansion, links to task URLs, and a markdown index under `docs/architecture-audit/audit-findings/<run>/` are deferred per design §7 v1 callout. Reconsider in slice-2 if `jq` querying of the per-run JSON proves unbrowsable in practice.

---

## §5 Why `data/audit-runs/` and not `docs/architecture-audit/audit-findings/<run>/`

Design §7 explicitly defers the per-finding markdown index to slice-2. The `data/` directory is the natural home for machine-readable time-series data that is:
- Gitignored (not committed to the repo)
- High-frequency (one file per run)
- Consumed by the dedupe logic (not just by humans browsing docs)

The `docs/architecture-audit/` directory is for human-authored design docs and audit artifacts that are committed and reviewed. Mixing gitignored per-run JSON into a committed docs directory would confuse both git status and code review.

If slice-2 adds a human-browsable markdown index, it will live in `docs/architecture-audit/audit-findings/` and be committed, while the machine-readable JSON time-series continues in `data/audit-runs/`.
