# Severity Routing

Per-finding action ladder. Apply this logic to each `Finding` in the parsed JSON array, in order of severity (High → Medium → Low), after a successful CLI run. Design foundation: `docs/architecture-audit/f-infra-design.md` §6.

---

## §0 Pattern registry

One row per `reify-audit --pattern` token, the CLI vocabulary defined in `reify_audit::pattern_flag::TOKENS`. The JSON carries `Finding.pattern`, and this table maps it back to its token; a new token needs a row here, enforced by `crates/reify-audit/tests/skill_registration_parity.rs`.

| Token | `Finding.pattern` value(s) | Default sweep | `Finding.task_id` carries | Routing notes |
|---|---|---|---|---|
| `P1` | `P1ProducerOrphan` | yes | task id | §2 P1 template |
| `P2` | `P2ConsumerStub` | yes | task id | §2 P2 template |
| `P5` | `P5PhantomDone`, `P5MetadataFilesGitignored`, `P5TestsAssertEmpty`, `P5LivePathStranded` | yes | task id | §2 P5 note |
| `PTODO` | `PTodo` | yes | repo path for the structural and liveness lanes, which include every High kind; task id for the inverse lane (`task-cites-deleted-path` / `task-cites-renamed-path`) | §2 PTODO |
| `PDSSENTINEL` | `PDsSentinel` | yes | repo path | §2 PDSSENTINEL |
| `PDEAD` | `PDeadCode` | no | empty string | §2 P-* note |
| `PUNTESTED` | `PUntested` | no | empty string | §2 P-* note |
| `PLAYER` | `PLayerViolation` | no | empty string | §2 P-* note |
| `PDIAG` | `PDiag` | no | repo path: the swept file, or `crates/reify-audit/pdiag-baseline.txt` for baseline/census faults | §2 PDIAG |
| `PDOCCOVER` | `PDocCover` | no | repo path: `crates/reify-compiler/src/units.rs`, `crates/reify-audit/pdoccover-baseline.txt`, or a `crates/reify-mcp/src/tools/chunks/*.md` | §2 PDOCCOVER (batched) |
| `PDCHECK` | `PDeliveredCheckPath` | no | task id: the owning non-terminal task | §2 PDCHECK |

---

## §1 Severity table

| Severity | Action | Tool | Parameters |
|----------|--------|------|------------|
| **High** | Escalate (advisory, non-blocking) | `mcp__escalation__escalate_info` | `task_id=<subject>` (see below), `agent_role="audit"`, `category="risk_identified"`, `summary="[<finding.pattern>] <finding.task_id>: <finding.summary>"`, `detail=<json of finding.evidence>`, `terminal_state_is_the_bug=True` |
| **Medium** | File deferred follow-up task (with dedupe) | `mcp__fused-memory__submit_task` | `planning_mode=True` (synchronous, curator-bypassing); see §2 for title template and metadata |
| **Low** | Log into per-run JSON only | _(none)_ | No side effects; `action_taken: "logged"`. **PDEAD, PUNTESTED, and PLAYER findings are always Low** — they are never escalated, never auto-filed, and never promoted to Medium. **PTODO findings are severity-split (task η, #4559):** `untracked`/`orphaned`/`bare-ignore` → High (escalate); `malformed-cite`/`phantom-tracking`/`unknown-id` → Medium (file task); `task-cites-deleted-path` / `task-cites-renamed-path` → Medium (advisory, file task). See §2 for PTODO title template and per-kind routing. |

### High severity — escalation details

One template serves every pattern:

```python
mcp__escalation__escalate_info(
    task_id=subject,                       # see the subject rule below
    agent_role="audit",
    category="risk_identified",
    summary=f"[{finding.pattern}] {finding.task_id}: {finding.summary}",
    detail=json.dumps(finding.evidence),   # JSON-serialized list of EvidenceRef tagged-enum objects (not bare strings)
    terminal_state_is_the_bug=True,
)
```

**Subject rule:** `subject = finding.task_id` when §0 says that finding carries a task id (P1, P2, P5, PDCHECK); otherwise the fixed subject `"audit"`. Every High from PTODO, PDIAG and PDOCCOVER is path-keyed, so it takes `"audit"`. The two parameters beyond the obvious ones are load-bearing:

- **The subject, not the raw `task_id`:** the escalation server mints the escalation id from `task_id` (`make_id` names its counter files `esc-<task_id>.seq…`), and a repo path cannot mint one. Measured 2026-09-23 against a scratch queue: `make_id('crates/reify-compiler/src/units.rs')` raises `FileNotFoundError`, while `make_id('audit')` mints `esc-audit-1`.
- **`terminal_state_is_the_bug=True`:** without it the server auto-resolves, on arrival, any filing whose task is done or cancelled — and every `P5PhantomDone` is about a done task.

PDOCCOVER is the one batched pattern: one escalation per run, not one per finding (§2).

**Source:** `Finding` struct and `EvidenceRef` enum in `crates/reify-audit/src/lib.rs`.

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

(Note: `planning_mode=True` implies `deferred` status — the tool flips to `deferred` automatically. Do NOT add an explicit `status=` kwarg. See the `submit_task` docstring in `fused-memory/src/fused_memory/server/tools.py`.)

`planning_mode=True` is **synchronous** (curator-bypassing) and returns `task_id` directly — no `resolve_ticket` round trip. This is the same pattern used by `/prd` decompose mode (see `.claude/skills/prd/references/decompose-mode.md` Step 3); the contract is captured in fused-memory entity `feedback_planning_mode_scope`. Tasks are filed as `deferred` (not `pending`), awaiting human triage. The skill does **not** call `set_task_status` to flip them to `pending`.

---

## §2 Per-pattern follow-up task title templates

| Pattern | Title template |
|---------|---------------|
| **P1** (producer-orphan) | `Wire <symbol> consumer (P1 orphan introduced by task <id>)` |
| **P2** (consumer-stub) | `Wire <symbol> consumer (P2 stub introduced in task <id>)` |
| **P5** (phantom-done) | _(P5 cannot reach Medium — see note below)_ |
| **PTODO** (TODO-tracking invariant) | `Track TODO marker (PTODO <kind> at <path> in task <id>)` |
| **PDSSENTINEL** (ds-sentinel reintroduction) | `Remove ds-sentinel reintroduction (PDSSENTINEL at <path> line <n>)` |
| **PDIAG** (codes-mandatory ratchet) — Medium `pdiag-baseline-stale` only | `Tighten pdiag baseline row (PDIAG pdiag-baseline-stale at <path>)` |
| **PDOCCOVER** (registry ↔ chunk name drift) | _(High only: batched escalation, no Medium template)_ |
| **PDCHECK** (`delivered_checks` dead path) — Medium `delivered-check-vacuous-absent-path` only | `Repair vacuous delivered_check <check_name> (PDCHECK on task <id>)` |

**P1/P2 templates:** Substitute `<symbol>` with the symbol name from `finding.evidence` (first reference that names the symbol, or fall back to `finding.summary` if not available). Substitute `<id>` with `finding.task_id`.

**P5 severity note:** P5 (phantom-done) findings are **High-only or Low** in the periodic sweep:
- High: verified phantom-done (task status=done + missing metadata evidence).
- Low: Cargo.lock-only change or sibling-absorbed downgrade (CLI classifies these as Low directly).

P5 findings never reach Medium in the periodic sweep context, so no Medium title template is needed for P5. (In the D-1 pre-done hook context P5 findings exit non-zero, but that context does not go through this skill's severity routing.)

**PTODO title template:** Substitute `<kind>` with the violation taxonomy kind from `finding.summary` (e.g. `untracked`, `malformed-cite`, `orphaned`, `bare-ignore`, `unknown-id`, `phantom-tracking`, `task-cites-deleted-path`, `task-cites-renamed-path`). Substitute `<path>` with the primary file path from `finding.evidence`. Substitute `<id>` with `finding.task_id`. For `orphaned` violations include the dead task id in the title: `Track orphaned cite (#<dead> at <path> in task <id>)`.

**PTODO taxonomy note (post-η, task #4559):** PTODO is deterministic (grep + read-only sqlite; no jcodemunch) and runs in the default sweep. Severity is split by kind:
- `untracked` / `orphaned` / `bare-ignore` → **High** → escalate per the High row above. These emit a non-zero exit code (= High count). That exit code is **not** what gates the merge — the real-tree gate is the severity-blind fingerprint ratchet in `tests/infra/test_reify_audit_ptodo.sh`, which never observes the High count; see SKILL.md §PTODO ("What actually gates verify") and `docs/prds/reify-audit-ptodo-detector.md` §8.4. The structural High kinds (untracked/bare-ignore) fire everywhere; `orphaned` (liveness) fires only where tasks.db exists.
- `malformed-cite` / `phantom-tracking` / `unknown-id` → **Medium** → file deferred follow-up task per §1. `unknown-id` stays Medium because a DB-sync race (freshly-filed cite not yet in tasks.db) must not raise a High finding.
- `task-cites-deleted-path` → **Medium** (advisory) → file deferred follow-up task per §1.
- `task-cites-renamed-path` → **Medium** (advisory) → file deferred follow-up task per §1. The summary already names the new path, so the follow-up is a repoint of `metadata.files`, not an investigation.

**PDEAD / PUNTESTED / PLAYER severity note:** These three advisory patterns pin `Severity::Low` in the detector implementation and are **never promoted** to Medium or High. No Medium title template exists for them — they always route to the Low/logged path (`action_taken: "logged"`) with no follow-up task filed and no escalation triggered. This is intentional: jcodemunch's Rust accuracy is unproven, so these detectors are advisory/log-only pending validation.

**PDSSENTINEL note:** Medium only, so it never escalates. The follow-up's remedy is to resolve the site per `docs/prds/dimensionless-scalar-sentinel-stampout.md`, or to mark a legitimate KEEP with `// ds-sentinel:allow <reason>`. Substitute `<n>` from the summary (`ds-sentinel: line <n>: …`). The dedupe symbol is the path, so every site in one file shares one follow-up.

**PDIAG note:** the High kinds (`pdiag-ratchet` / `pdiag-baseline-unreadable` / `pdiag-census-empty`) escalate per finding under subject `"audit"`. They are the same verdicts the merge gate `tests/infra/test_reify_audit_pdiag.sh` fails on, so a High on main means that gate was bypassed or skipped its ratchet scenario — or, for `pdiag-census-empty`, that this run's git enumeration came back empty. Medium `pdiag-baseline-stale` files a follow-up whose fix is the regeneration command quoted in its summary.

**PDOCCOVER note:** all five categories (`undocumented-name`, `fabricated-name`, `stale-baseline-entry`, `stale-allow-entry`, `allow-missing-reason`) are High, and they are escalated **once per run**, not per finding:

- `summary=f"[PDocCover] {N} High findings — {k} undocumented-name, {k} fabricated-name, …"`, counting each category present;
- `detail=json.dumps([{"path": f.task_id, "summary": f.summary} for f in pdoccover_findings])`;
- `task_id="audit"`, with every other §1 parameter unchanged.

Every PDOCCOVER finding then records `action_taken: "escalated"` with that one `escalation_id`. Why batched: the findings are one census with one owner — #6931 seeds `crates/reify-audit/pdoccover-baseline.txt` and wires the gate — so a human makes one decision per run, not one per name. The 41 High measured on main on 2026-09-23 would otherwise queue 41 advisories for that one decision.

**PDCHECK note:** the High kind (`delivered-check-unsatisfiable-path`) escalates per finding with `task_id=finding.task_id`, the owning live task. The Medium kind (`delivered-check-vacuous-absent-path`) files a follow-up. Take `<check_name>` from the finding's `DeliveredCheck` evidence, which is also its dedupe symbol (§3). Either repair is a `metadata.delivered_checks` edit a human makes; §4 forbids the skill mutating tasks. A run whose stderr carries the `PDCHECK … lane skipped … NOT a clean bill of health` breadcrumb checked nothing, so its empty PDCHECK result is not evidence of health.

---

## §3 Dedupe contract

**Key definition:** `(parent_task_id, audit_cluster, symbol_or_path)`

- `parent_task_id` = `finding.task_id`
- `audit_cluster` = `finding.pattern` (e.g. `"P1"`, `"P2"`, `"P5"`)
- `symbol_or_path` = the primary symbol or file path from `finding.evidence` (first evidence string; use `finding.summary` as fallback)
- For PDCHECK, `symbol_or_path` is the `DeliveredCheck` evidence's `check_name` (always its first evidence entry), so two stale rows on one task stay distinct.

**The key is kind-agnostic:** `audit_cluster` is the PATTERN (`"PTODO"`), not the finding kind, so two PTODO findings on the same task+path collide on one key regardless of kind. No change is needed for the two inverse kinds — `task-cites-deleted-path` and `task-cites-renamed-path` are mutually exclusive by construction (a cited path either resolves to a rename target still tracked at HEAD, or it does not), so they can never both be emitted for the same task+path. Stated here so a future reader does not have to re-derive it.

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
