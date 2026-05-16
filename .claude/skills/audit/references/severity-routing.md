# Severity Routing

Per-finding action ladder. Apply this logic to each `Finding` in the parsed JSON array, in order of severity (High → Medium → Low), after a successful CLI run. Design foundation: `docs/architecture-audit/f-infra-design.md` §6.

---

## §1 Severity table

| Severity | Action | Tool | Parameters |
|----------|--------|------|------------|
| **High** | Escalate (advisory, non-blocking) | `mcp__escalation__escalate_info` | `category="risk_identified"`, `summary="[P<n>] task <id>: <finding.summary>"`, `detail=<json-snippet of finding.evidence>` |
| **Medium** | File deferred follow-up task (with dedupe) | `mcp__fused-memory__submit_task` | `planning_mode=True` (synchronous, curator-bypassing); see §2 for title template and metadata |
| **Low** | Log into per-run JSON only | _(none)_ | No side effects; `action_taken: "logged"` |

### High severity — escalation details

```python
mcp__escalation__escalate_info(
    category="risk_identified",
    summary=f"[{finding.pattern}] task {finding.task_id}: {finding.summary}",
    detail=json.dumps(finding.evidence),   # JSON-serialized evidence list
)
```

The escalation lands in the same queue `/unblock` drains. The skill is **advisory** here — pre-done blocking is D-1 hook territory. High findings raise visibility without mutating task state.

### Medium severity — follow-up task details

Before calling `submit_task`, perform the **dedupe check** (§3). If the dedupe key already exists in `data/audit-runs/index.json`, skip filing and record `action_taken: "deduped"`.

If no prior entry found:

```python
mcp__fused-memory__submit_task(
    planning_mode=True,          # synchronous, returns task_id directly
    title=<title-from-template>, # see §2
    description=f"Audit finding: {finding.summary}\n\nEvidence: {finding.evidence}",
    metadata={
        "audit_cluster": finding.pattern,        # e.g. "P1", "P2"
        "audit_origin": "<run-timestamp>",       # ISO timestamp of this run
        "parent_task": finding.task_id,          # the offending task
        "policy_ref": "feedback_task_chain_user_observable",
    },
    project_root="/home/leo/src/reify",
)
```

`planning_mode=True` is **synchronous** (curator-bypassing) and returns `task_id` directly — no `resolve_ticket` round trip. This is the same pattern used by `/prd` decompose mode (see `.claude/skills/prd/references/decompose-mode.md` Step 3); the contract is captured in fused-memory entity `feedback_planning_mode_scope`. Tasks are filed as `deferred` (not `pending`), awaiting human triage. The skill does **not** call `set_task_status` to flip them to `pending`.

---

## §2 Per-pattern follow-up task title templates

| Pattern | Title template |
|---------|---------------|
| **P1** (producer-orphan) | `Wire <symbol> consumer (P1 orphan introduced by task <id>)` |
| **P2** (consumer-stub) | `Wire <symbol> consumer (P2 stub introduced in task <id>)` |
| **P5** (phantom-done) | _(P5 cannot reach Medium — see note below)_ |

**P1/P2 templates:** Substitute `<symbol>` with the symbol name from `finding.evidence` (first reference that names the symbol, or fall back to `finding.summary` if not available). Substitute `<id>` with `finding.task_id`.

**P5 severity note:** P5 (phantom-done) findings are **High-only or Low** in the periodic sweep:
- High: verified phantom-done (task status=done + missing metadata evidence).
- Low: Cargo.lock-only change or sibling-absorbed downgrade (CLI classifies these as Low directly).

P5 findings never reach Medium in the periodic sweep context, so no Medium title template is needed for P5. (In the D-1 pre-done hook context P5 findings exit non-zero, but that context does not go through this skill's severity routing.)

---

## §3 Dedupe contract

**Key definition:** `(parent_task_id, audit_cluster, symbol_or_path)`

- `parent_task_id` = `finding.task_id`
- `audit_cluster` = `finding.pattern` (e.g. `"P1"`, `"P2"`, `"P5"`)
- `symbol_or_path` = the primary symbol or file path from `finding.evidence` (first evidence string; use `finding.summary` as fallback)

**Lookup procedure (before filing any medium finding):**

1. Read `data/audit-runs/index.json`. If the file does not exist, treat as empty (`{entries: []}`).
2. Search `entries` for a record matching the key `(parent_task_id, audit_cluster, symbol_or_path)`.
3. **On hit** (prior entry found):
   - Skip `submit_task`.
   - Set `action_taken: "deduped"` in the per-run finding record.
   - Set `prior_finding_id: <found-entry.finding_id>` in the per-run finding record.
4. **On miss** (no prior entry):
   - Call `submit_task` (§1), receive `task_id`.
   - Set `action_taken: "filed"` in the per-run finding record.
   - Set `task_id_filed: <returned-task-id>` in the per-run finding record.
   - Append a new entry to `data/audit-runs/index.json` (see `output-format.md` §3 for the entry schema).

The `index.json` file is **append-only within a run** (entries from prior runs are preserved) and is **rewritten in full** at the end of each run (so the file always reflects the current state of all known dedupe keys).

**Atomic rewrite:** Write the updated contents to `data/audit-runs/index.json.tmp`, then `rename()` over `data/audit-runs/index.json`. Without atomicity, an interrupted rewrite (Ctrl-C, OOM, host crash) leaves a truncated or corrupt `index.json`. The next run would then fail to parse it and silently re-file every duplicate finding — the exact failure dedupe was designed to prevent.

**Recovery from corrupt index:** If `data/audit-runs/index.json` exists but fails to parse (e.g. left truncated by an interrupted atomic rewrite), the skill must **surface the parse error to the user and stop** — do NOT silently treat a parse failure as an empty index. This preserves the user's ability to inspect and manually repair the file rather than losing dedupe history silently.

---

## §4 Do-not-flip-status invariant

The `/audit` skill **never** calls:
- `set_task_status` on any task (not the offending task, not the filed follow-up)
- `mcp__fused-memory__update_task` to alter the offending task's status
- Any operation that transitions a `done` task to `deferred`, `pending`, or `blocked`

The skill is **advisory** in periodic-sweep context:
- **High** findings raise an escalation alert. A human (Leo) decides whether to act.
- **Medium** findings file a new `deferred` task for triage. The new task is a proposal, not an instruction.
- **Low** findings are logged only — no follow-up at all.

State-blocking is exclusively D-1 hook territory (non-zero exit from `--pre-done` prevents the orchestrator from marking the task done). Outside the hook context, the skill observes and reports; it does not mutate.

This invariant is intentional: auto-unwinding a `done` task on a phantom-done finding would be a heavier intervention than the design authorizes (design §3, §6).
