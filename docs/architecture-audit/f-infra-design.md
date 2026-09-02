# F-infra — audit cadence + tracking infrastructure (design)

**Date:** 2026-05-12
**Status:** design landed; implementation session follows
**Scope marker:** complementary to `/prd` (upstream-of-orchestrator gates A/D/E/H + grammar) and to G (corpus-level reviewer lint). F-infra is the **downstream** half — catches incomplete/ill-formed implementation chains that slip past `/prd` at any later lifecycle stage. F operates at task-state-transition time and on a periodic sweep, not at PRD-authoring time.

Audit terminology in this doc follows `preferences_implementation_chain_naming.md`: "incomplete/ill-formed implementation chain". Sub-shapes (Type A producer-orphan, Type B consumer-with-stub, Type C both-built-not-bridged) per `phase-3-scaffold-pattern-critique.md` §1.3.

## 1. Scope

**First-slice patterns** (Q-F-8 answer): **P1 + P2 + P5**.

| Pattern | What | Source |
|---|---|---|
| **P1** | Type-A producer-orphan: public symbol introduced; only test callers; no downstream consumer task queued. | `phase-3-scaffold-pattern-critique.md` §1.3 Type A; cluster C-02, C-04, C-10, C-43 endemics. |
| **P2** | Type-B consumer-with-stub: consumer-side code calls into a deliberate placeholder (TODO marker, `Value::Undef` arm, `unimplemented!()`, `task_X_pending` warn). | Type B; cluster C-25 build_doc_model, C-44 stress alias, C-39 Manifold hook. |
| **P5** | Phantom-done: `set_task_status(done)` accepted on a task whose `metadata.files` don't match the diff on main, OR `done_provenance.kind=found_on_main` on a branch with zero commits diff vs. main. | `procedural_runs_db_forensics.md`; six already-witnessed incidents may05–may11. |

**Deferred to follow-up F-infra slices** (NOT in first-implement scope):

| Pattern | Why deferred |
|---|---|
| **P3** Type-C both-built-not-bridged | Needs cross-PRD seam metadata that `/prd`'s `consumer_ref` only partly captures. Revisit once `/prd`-decomposed PRDs accumulate. |
| **P4** Grammar fiction | Already enforced by `/prd` G3 (tree-sitter parse gate at PRD-authoring time). No downstream re-check needed. |
| **P6** Contested seam ownership | Already enforced by `/prd` G4. Manual gap-register curation handles legacy unresolved seams. |
| **P7** PRD-vs-shipped drift | Requires test-band analysis or per-PRD assertion checking. Out of scope without a separate language-level invariant. |

The portfolio (`preferences_implementation_chain_portfolio.md`) frames F as "audit cadence + tracking infra". This first slice realizes that framing for the three highest-signal patterns; it is **not** a re-implementation of the full Phase-2 sweep.

## 2. Trigger surface (Q-F-1)

Three triggers, escalating in confidence:

1. **Pre-done gate via fused-memory MCP** (primary). All `set_task_status(done)` calls route through fused-memory (per CLAUDE.md task-routing policy). A pre-write validator hook calls F's fast subset before the state transition. **The hook does not exist yet** — see §11 dependency D-1: implement session queues a dark-factory task to add it.
   - F's pre-done check runs **only P5** in-line (cheap: bounded SQL query + a `git log` + a `metadata.files` diff). P5 is the only pattern whose evidence is local to the just-closing task and whose false-positive rate is low enough to gate state transition.
   - On P5 detection: hook returns Err, set_task_status raises, the orchestrator workflow's done-transition aborts. F emits an `mcp__escalation__escalate_info` ticket with evidence and the offending task id.
2. **Periodic sweep via `/audit`** (secondary). The `/audit` skill (§3) runs the full P1+P2+P5 pass over a configurable window (default: last 14 days of `done` task flips + workspace symbol delta). Cadence is human-driven (weekly or per-release), not cron-automated in slice 1 — Reify is solo OSS; a cron daemon adds infra without commensurate value. Cron-isation is a v2 follow-up.
3. **`/review`-invoked** (tertiary). The existing `/review` skill's Phase 2 (architectural coherence) calls `/audit` as one of its passes (Q-F-4 answer). F's findings become Phase-2 evidence; `/review`'s Phase 3 triage routes them per F's severity ladder.

Pre-done is the only state-blocking trigger. The other two are advisory and produce follow-up tasks / log entries.

## 3. Architecture (Q-F-4)

F-infra ships as **two artifacts**:

1. **`reify-audit` library** (Rust crate at `crates/reify-audit/`) — the detector core. Three modules: `p1_producer_orphan`, `p2_consumer_stub`, `p5_phantom_done`. Each exposes `fn check(ctx: AuditContext) -> Vec<Finding>`. `AuditContext` carries: project root, target task id (Option), time window, jcodemunch client handle, fused-memory client handle, runs.db path. The crate is pure logic; no scheduler, no MCP server.
2. **`/audit` skill** (`.claude/skills/audit/`) — the human entry point + glue. Invokes the library, formats findings, files follow-ups via fused-memory, escalates via `mcp__escalation__*`, writes time-series JSON. Modes:
   - `/audit` (no args) — full sweep over default window.
   - `/audit --task <id>` — single-task spot-check (matches pre-done-hook semantics; debugging aid).
   - `/audit --since <date>` — bounded sweep.
   - `/audit --pattern P1|P2|P5` — restrict patterns.

The pre-done hook (D-1) calls `reify-audit::check_pre_done(task_id)` directly — no skill invocation. The skill exists for human-driven sweeps and `/review` composition.

**Why a Rust library, not a Python script:** the pre-done hook lives inside fused-memory's MCP request path. Whichever language the hook is written in, it must be callable from the same process. Rust is the language of the Reify side of the boundary; a shared library callable both from a small CLI (which `/audit` shells out to) and from a dark-factory FFI shim (or subprocess) is the cleanest split. Slice 1 may implement the library + CLI; the dark-factory shim is a sub-task of D-1.

## 4. Data substrate (Q-F-2 / Q-F-6)

Hybrid (Q-F-2 answer): jcodemunch + task-metadata + runs.db SQL.

| Source | Used for | Notes |
|---|---|---|
| **jcodemunch MCP** (`mcp__jcodemunch__find_unused_paths`, `find_references`, `get_changed_symbols`, `get_symbol_provenance`) | P1: identify newly-introduced public symbols + check for non-test callers. P2 (assist): trace stub-marker symbols back to declarations. | Requires repo index to be fresh. F's invocation does `mcp__jcodemunch__index_repo` if last-indexed mtime > 24h. |
| **Task metadata** (fused-memory `get_task`, `get_tasks`, `get_statuses`) | P1: filter orphan candidates by checking whether a downstream task with `consumer_ref` pointing at this task's PRD/symbol exists. P5: read `metadata.files`, `done_provenance`. | `/prd`-decomposed tasks carry `consumer_ref` + `user_observable_signal` + `grammar_confirmed`. Pre-`/prd` legacy tasks lack these; F gracefully degrades to symbol-only signal. |
| **`data/orchestrator/runs.db` SQL** | P5: `events`, `task_results`, `done_provenance` tables. SQL templates in `procedural_runs_db_forensics.md`. | Reify-local, append-only, well-indexed. |
| **`git log main --grep "<task-id>"` + `git diff main..task-branch`** | P5 corroboration: when `metadata.files` claim mismatches diff on main. | Bounded queries; cheap. |
| **Workspace grep** (`rg`) | P2: scan for TODO/unimplemented/`Value::Undef`/`task_X_pending` markers in just-touched files. | Filtered to `metadata.files` set + extensions `*.rs`/`*.ts`/`*.ri`. |

Graph-walk invariant (Q-F-6) is **not** an explicit dependency-DAG walk in slice 1 — `/prd`'s `consumer_ref` metadata is the cheaper proxy. A full task-DAG walk is a slice-2 addition once enough `/prd`-decomposed PRDs exist that the graph is dense.

## 5. Invariants enforced (per pattern)

### P1 — Type-A producer-orphan

**Invariant:** For every public symbol introduced by a `done` task's `metadata.files` diff, at least one of:
- a non-test caller exists in the workspace (jcodemunch `find_references` filtered to non-`*/tests/*` paths), OR
- a `pending`, `in-progress`, or `review` task with `consumer_ref` referencing the producing PRD exists in fused-memory, OR
- the symbol carries `#[allow(dead_code)]` or `#[cfg(test)]`.

**Detector:** `get_changed_symbols(branch=ctx.producer_branch.unwrap_or("main"), since=<task done timestamp>)` → for each new pub symbol, `find_references(symbol)` (scoped to the symbol's declaring file to avoid same-name conflation) → filter to non-test → check task-graph for downstream consumer.

**False-positive guards:**
- Grace window: producer-orphan flagged only if **>14 days** have passed since done-flip with no consumer landing AND no consumer task pending. Inside the window: log only (low severity).
- Foundation tasks: if the task's PRD section header matches `## Phase N (foundation)` or task metadata has `audit_foundation=true`, suppress with note.
- Stdlib `.ri` definitions are scope-excluded (every `structure_def` is technically "orphan" until something calls it).

**Severity:** medium (after grace window) → file follow-up. low (within grace window) → log.

### P2 — Type-B consumer-with-stub

**Invariant:** For every file in a just-closing task's `metadata.files`, no NEW marker matching the stub pattern is introduced relative to `main` pre-task-branch:
- `TODO\(.*pending\)` / `TODO\(post-\w+\)` / `TODO\(.*later\)` / `TODO\(task_\d+\)`
- `unimplemented!\(`
- `panic!\(.*not yet`
- `tracing::warn!\(reason="task_\w+_pending"`
- explicit `Value::Undef` arms with comment containing `pending|stub|placeholder`
- `// stub`, `// placeholder`, `// fixme` (line-comment form)

**Detector:** `git diff main..task-branch` filtered to `metadata.files`; grep for stub patterns on **added** lines (`^+` in diff); report each match with file:line + matched pattern.

**False-positive guards:**
- Test files (`*/tests/*`, `*_test.rs`, `__tests__/`) excluded.
- Stub-pattern matches that were present in pre-task `main` (i.e. moved code) are excluded — diff-based detection handles this.
- Tasks whose description explicitly names "stub" or "placeholder" in the title (e.g. "Add stub for X") are flagged but with severity downgraded to low.

**Severity:** medium → file follow-up task (carries `audit_cluster=P2`, `audit_origin=2026-05-12+`, `parent_task=<id>`). The follow-up task's title template: `Wire <symbol> consumer (P2 stub introduced in task <id>)`.

### P5 — Phantom-done

**Invariant:** A `done` task's evidence is self-consistent:
- If `done_provenance.kind=merged`: a commit exists on `main` whose tree touches every path in `metadata.files`, AND a `task_completed` event exists for the task in `runs.db`.
- If `done_provenance.kind=found_on_main`: `git log main --grep "<task-id>"` returns at least one commit, AND `git diff main pre-found..main` touches at least one path in `metadata.files`.
- `metadata.files` does not contain gitignored entries (per `project_steward_metadata_files_gitignore_falsepositive.md`).

**Detector:**
- SQL on `runs.db`:
  ```sql
  SELECT t.task_id, t.done_provenance, t.metadata
  FROM task_results t
  WHERE t.status='done'
    AND t.updated_at > <window_start>
    AND t.task_id IN (<candidate-ids>);
  ```
- Per row, run the corroboration checks above.
- For pre-done invocations: candidate-ids = `[<incoming task id>]`; for periodic: candidate-ids = all done-flips in window.

**False-positive guards:**
- Convergent fast-forward (sibling-absorbed): if metadata.files diff is empty on main BUT `git log main --grep <id> OR --grep <prd-slug>` returns a sibling-task commit covering the same files, downgrade to low (matches `project_unblock_convergent_ff_worktree_reap.md` pattern).
- Equivalence false-positives on Cargo.lock (per `project_post_merge_equivalence_false_positive_cargo_lock.md`): if the only mismatched file is `Cargo.lock` AND task files exist on main AND tests pass, downgrade to low.

**Severity:** high → escalate via `mcp__escalation__escalate_info` (or block via pre-done hook). The Phase-2 may10 incident catalog (`project_phantom_done_at_reap_premature_followup.md`, `project_phantom_done_metadata_files_strip_may09.md`, etc.) shows ~6 known incidents in the past 2 weeks — this is the highest-signal pattern.

## 6. Intervention vocabulary (Q-F-3)

Severity ladder:

| Severity | Action | Patterns | Reversibility |
|---|---|---|---|
| **high** | Block done-flip via pre-done hook return-Err + escalate via `mcp__escalation__escalate_info`. Outside hook context: escalate only (task remains done; manual unblock or `done→deferred` flip needed). | P5 verified phantom-done. | None: state transition refused. |
| **medium** | File a deferred follow-up task via `submit_task(planning_mode=True)` + `resolve_ticket`, carrying `audit_cluster=P1\|P2`, `audit_origin=<date>`, `parent_task=<id>`, `policy_ref=feedback_task_chain_user_observable.md`. Title template per pattern. Dedupe: skip if a task with the same `parent_task` + `audit_cluster` exists. | P1 (post-grace), P2. | Reversible: Leo can cancel the follow-up. |
| **low** | Log to `data/audit-runs/<ts>.json` only. No state change. Appears in `/audit` summary report. | P1 (in-grace), P2 (with stub-in-title), P5 (Cargo.lock-only / sibling-absorbed). | n/a. |

Dedupe key for medium follow-ups: `(parent_task_id, audit_cluster, symbol_or_path)`. Stored in `data/audit-runs/index.json` to survive across runs.

## 7. Storage (Q-F-7)

| Artifact | Path | Lifecycle |
|---|---|---|
| Per-run JSON | `data/audit-runs/<iso-timestamp>.json` | Append per `/audit` invocation. Contains: timestamp, scope (window/task-id/pattern filter), findings list (each with severity, pattern, evidence refs, action taken, task-id-filed-if-any). Gitignored (under `data/`). |
| Findings markdown index | `docs/architecture-audit/audit-findings/<run>/` | Per-run, human-readable; one `<finding-id>.md` per medium-or-high finding for browsability. Slice-1 v1: omit; slice-2 add if `data/audit-runs/*.json` proves unbrowsable. **Slice 1 keeps only the JSON time-series; markdown is on-demand via `/audit --format markdown`.** |
| `gap-register.md` | `docs/architecture-audit/gap-register.md` | Phase-3 manual curation only. F-infra does **not** auto-promote findings into GR-IDs. Leo or a synthesis session reviews `data/audit-runs/*.json`, promotes the load-bearing ones to GR-IDs. |
| Dedupe index | `data/audit-runs/index.json` | `(parent_task_id, audit_cluster, symbol_or_path) → finding_id` map. Append-only; rewrite on each run. |
| Follow-up task back-reference | task `metadata.audit_origin`, `metadata.audit_cluster`, `metadata.parent_task` | Standard task metadata; queryable via fused-memory `get_task`. |

Time-series JSON enables "regression rate going up or down?" queries (Q-F-7 prompt). Slice-1 leaves the analysis to Leo + ad-hoc `jq`; a `/audit --trend` summarisation is a slice-2 nice-to-have.

## 8. Interaction with existing infra

### 8.1 `/prd` (upstream)

F **consumes** the metadata `/prd` decompose writes: `user_observable_signal`, `consumer_ref`, `grammar_confirmed`. P1's "downstream consumer task exists" check inspects `consumer_ref`. Pre-`/prd` legacy tasks have no `consumer_ref`; F degrades to symbol-only call-graph signal for those (higher false-positive rate is the cost).

F does **not** rewrite `/prd`-written metadata; it only reads.

### 8.2 `/review` (peer)

`/review`'s Phase 2 ("Architectural Coherence") historically does its own stub-pattern scanning ad-hoc. After F lands, `/review` Phase 2 invokes `/audit --pattern P1,P2,P5 --since <last-review>` and folds findings into Phase 2's report. `/review` Phase 3 triage continues to do task creation; F's medium-severity finder also creates tasks, so dedupe via the index in §7 prevents double-filing.

`/review`'s briefing.yaml gains an optional `audit.window_days` field; default 14.

### 8.3 Orchestrator (downstream)

The orchestrator does **not** read F's metadata or findings directly. The only orchestrator-side change required is the **pre-done MCP hook** (D-1) — the orchestrator's task-workflow continues to call `set_task_status(done)` exactly as today; the hook intercepts at fused-memory layer.

If the pre-done hook returns Err, `set_task_status` raises an exception; the orchestrator's task-workflow handles this the same as any other set_task_status failure (workflow retries / escalates per its existing logic). F's escalation parallel-publishes the evidence so the human escalation-watcher loop has full context.

### 8.4 Other adjacent skills

- `/unblock` — F's escalations land in the same queue `/unblock` already drains. No coupling change; `/unblock` becomes one of the manual remediation paths for F-flagged phantom-dones.
- `/orchestrate` — unchanged.
- `/reflect` — F can optionally summarise the session's audit-run deltas if `data/audit-runs/` has new entries.

## 9. Q-F-* resolution table

| Question | Decision |
|---|---|
| **Q-F-1** Trigger granularity | Pre-done MCP hook (primary, escalates **before** state flip; D-1 dependency); periodic `/audit` (human-driven); `/review` Phase-2 calls `/audit`. |
| **Q-F-2** Detector mechanism | Hybrid: jcodemunch (call-graph) + task-metadata (consumer_ref, metadata.files) + runs.db SQL (P5). |
| **Q-F-3** Intervention per pattern | Severity ladder: P5 → high (escalate, block); P1 (post-grace)/P2 → medium (file follow-up); P1 (in-grace)/P2 (stub-in-title)/P5 (Cargo.lock-only) → low (log). |
| **Q-F-4** /review relationship | Separate `/audit` skill at `.claude/skills/audit/`; `/review` Phase 2 invokes it. |
| **Q-F-5** Task metadata schema additions | None new on the producer side: `/prd` already writes `user_observable_signal`, `consumer_ref`, `grammar_confirmed`. F adds **consumer-side** metadata on its filed follow-up tasks: `audit_cluster`, `audit_origin`, `parent_task`, `policy_ref`. |
| **Q-F-6** Graph-walk invariants | Slice 1: `consumer_ref` proxy + jcodemunch `find_references`. Full task-DAG walk deferred to slice 2 once `/prd`-decomposed PRDs accumulate. |
| **Q-F-7** Storage of detected gaps | `data/audit-runs/<ts>.json` time-series (gitignored); follow-up tasks carry back-references in metadata; `gap-register.md` stays human-curated. |
| **Q-F-8** First-slice scope | P1 + P2 + P5. P3/P4/P6/P7 deferred (P4/P6 already covered by `/prd` G3/G4; P7 too hard; P3 needs more `/prd` adoption first). |

## 10. First-slice DAG (what the implement session ships)

```
                 ┌──────────────────────────────────────────────┐
                 │ D-1 dark-factory: pre-done MCP hook surface  │
                 │      (dependency — separate dark-factory PR) │
                 └──────────────────┬───────────────────────────┘
                                    │
       ┌────────────────────────────┼────────────────────────────┐
       ▼                            ▼                            ▼
 T-1 reify-audit              T-2 reify-audit              T-3 reify-audit
     ::p5_phantom_done            ::p2_consumer_stub          ::p1_producer_orphan
     library + tests              library + tests             library + tests
       │                            │                            │
       └────────────────┬───────────┴────────────┬───────────────┘
                        ▼                        ▼
                  T-4 reify-audit-cli       T-5 /audit skill
                  binary + JSON output       (.claude/skills/audit/
                  (callable from D-1         SKILL.md + references/)
                  hook + /audit skill)
                        │                        │
                        └───────────┬────────────┘
                                    ▼
                         T-6 integration smoke:
                         seed three known incidents
                         (one per pattern) from
                         project_phantom_done_*
                         memories; assert detector
                         flags each correctly
                                    │
                                    ▼
                         T-7 /review Phase-2 wires
                         in /audit invocation; smoke
                         test on a curated subset
```

**User-observable signals per leaf** (per `feedback_task_chain_user_observable.md`):

| Task | Signal |
|---|---|
| T-1 P5 | `cargo test -p reify-audit p5::tests` passes; one of the tests seeds a synthetic `runs.db` fixture matching the may09 task 3242 incident and asserts P5 fires. |
| T-2 P2 | `cargo test -p reify-audit p2::tests` passes; fixture asserts the seven canonical stub patterns are detected, the seven non-stub patterns are not. |
| T-3 P1 | `cargo test -p reify-audit p1::tests` passes; fixture asserts producer-orphan detected after grace window expiry, suppressed inside window. |
| T-4 CLI | `target/debug/reify-audit --task 3242` exits non-zero and prints structured JSON-on-stderr matching the P5 expected shape. |
| T-5 `/audit` skill | `/audit --task 3242` (or equivalent test fixture) emits a Markdown report and files no follow-up (because already-done). `/audit` (no args) over a seeded run produces a finding list. |
| T-6 integration | A scripted seed-and-replay test in `tests/audit_integration.rs` reproduces three known incidents and verifies the detector flags each. |
| T-7 `/review` wiring | `/review --phase architecture` smoke-test on a curated commit range produces a Phase 2 report whose architectural-findings section includes /audit's output. |

**D-1 (dependency, NOT in slice 1):** dark-factory task to add a pre-write validator hook to fused-memory's `set_task_status` MCP entry point. The hook calls a configurable subprocess (in Reify's case: `reify-audit --task <id> --pre-done`) and propagates its exit code. The implement session for F-infra **queues this dark-factory task at decomposition time** (per Leo's Q-F-1b answer); slice-1 lands T-1..T-7 against the **hookless** path (periodic + `/review` triggers work; pre-done remains aspirational until D-1 lands).

## 11. Dependencies (pre-implementation)

| ID | Description | Who lands it | Blocking? |
|---|---|---|---|
| D-1 | dark-factory: pre-write validator hook on `set_task_status(done)` in fused-memory MCP. Configurable per-project via env var: `FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/.cargo/bin/reify-audit --task {id} --pre-done`. On exit-code ≠ 0, the MCP call raises and the done-flip is refused. Landed upstream as `fused_memory.middleware.pre_done_hook`. | dark-factory side; implement session queues the task. | **Done 2026-05-16:** D-1 shipped upstream; activated on Reify host via T-8. Subsequently rewired 2026-05-16+ to flow through `scripts/reify-audit-predone-wrapper.sh` (task 3731) after the Taskmaster removal (2026-05-12) left the CLI's dead default pointing at a non-existent path. |
| T-8 | Reify-side activation: set `Environment=FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done` in `/home/leo/.config/systemd/user/fused-memory.service`; reload + restart fused-memory; verify via `bash scripts/smoke-predone-hook.sh`. Hook invocation flows through `scripts/reify-audit-predone-wrapper.sh`, which materializes a TaskMetadata snapshot from `mcp__fused-memory__get_tasks` before invoking `reify-audit --tasks-file <tempfile>`. | Reify side; this task (3675); rewired by task 3731. | **Done 2026-05-16.** Operator action required: rewire systemd env var to wrapper path (see §11.1). |
| D-2 | jcodemunch repo index reasonably fresh (≤24h). F's invocation triggers `mcp__jcodemunch__index_repo` if stale. | F itself manages this. | Non-blocking. |
| D-3 | Confirm `runs.db` schema (task_results, events tables) stable enough to pin SQL queries. | Verify during implementation. | Non-blocking; SQL embedded in T-1. |
| D-4 | `/prd`-decomposed tasks already carry consumer_ref / user_observable_signal / grammar_confirmed. | Already shipped (per `procedural_prd_skill.md`). | Done. |

### 11.1 Activation status (2026-05-16; updated post-task-3731)

The pre-done gating loop is **active** on the Reify host as of 2026-05-16 (F-infra T-8, task 3675). The hook command was subsequently rewired to flow through a snapshot-materializer wrapper (task 3731, 2026-05-16+) after the Taskmaster removal (2026-05-12) left the direct binary invocation pointing at a non-existent default path.

- **Systemd unit:** `/home/leo/.config/systemd/user/fused-memory.service`
- **Env var (target, NOT the live state — see §11.1.3):** `FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done`
- **Wrapper (snapshot + invoke):** `/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh` — materializes a TaskMetadata JSON snapshot from `mcp__fused-memory__get_tasks`, then invokes `reify-audit` with `--tasks-file <tempfile>` (snapshot cleaned up on EXIT). → uses `scripts/reify-audit-snapshot-filter.jq`; see §11.2 for the `done_at` proxy rationale.
- **Binary:** `/home/leo/.cargo/bin/reify-audit` (invoked by wrapper; installed via `cargo install --path crates/reify-audit --root ~/.cargo --force`). The binary requires an explicit `--tasks-file`; there is no default path (removed in task 3731 after the Taskmaster deletion made the old default non-existent).
- **Smoke test:** `bash scripts/smoke-predone-hook.sh` (exits 0 when wiring AND wrapper round-trip both succeed; assertion 4 catches re-introduction of the dead default).
- **Reload command:** `systemctl --user daemon-reload && systemctl --user restart fused-memory`
- **Operator action required:** rewire the systemd `Environment=` line to point at the wrapper: `Environment=FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done`. Then reload and verify via `bash scripts/smoke-predone-hook.sh`.
- **Procedural memory:** entry keyed `FUSED_MEMORY_PREDONE_HOOK_REIFY systemd activation` in fused-memory memory store

#### 11.1.1 Why the snapshot wrapper? (task 3731)

The `reify-audit` binary is a pure-logic library (no MCP client, no scheduler). Before task 3731, the CLI defaulted `--tasks-file` to `.taskmaster/tasks/tasks.json`, which was deleted in commit `1402b46c63` (Taskmaster removal, 2026-05-12). Any invocation without an explicit `--tasks-file` silently exited 125 ("infrastructure error") and blocked done-flips. The fix makes `--tasks-file` required (no default) and concentrates fused-memory coupling at the wrapper boundary: the wrapper materializes a fresh TaskMetadata snapshot via `mcp__fused-memory__get_tasks` before each invocation, keeping the audit crate dependency-free. See design decisions in `.task/plan.json` for the rationale for Option 1 over Options 2 (new `--from-fused-memory` flag) and 3 (auto-write snapshot on state change).

#### 11.1.2 What the hook subprocess actually receives (task 6345)

Verified read-only against dark-factory. `middleware/pre_done_hook.py`
substitutes exactly one placeholder, `{id}`, over the `shlex.split` tokens
(docstring lines 8-10 and the `run_hook` docstring both state it is the ONLY
one; there is no `.format(`, `%`, or `string.Template` in the module). Launch
is `asyncio.create_subprocess_exec` with no `env=` kwarg, no stdin written,
stdout captured-and-discarded, a 30 s timeout, and `cwd=project_root`.

`task_interceptor.py` calls the hook at step "2d", BEFORE the write, and
accumulates `done_provenance` in an in-memory `audit_fields` dict that is
persisted only at write time. So the subprocess sees the PRE-transition status
and NO persisted provenance, and receives no task state beyond the id.

Consequence: the `--pre-done` leg is necessarily provenance-free. It
corroborates landing from `task_id` + `metadata.files`. Passing pending
provenance instead would require a cross-repo dark-factory change (a new
placeholder, or an env export) and is out of scope here.

Second correction from the same task: `git diff main..<commit>` is DEGENERATE
once `<commit>` is an ancestor of main. `main..X` is a two-point TREE diff, so
the paths the two trees agree on — post-merge, exactly the paths `X` introduced
— are excluded by construction, and what comes back is the reverse-delta of
whatever landed after `X`. P5 now selects the diff base by ancestry
(`changed_paths_for_claim`): `<commit>^1..<commit>` for a landed commit,
`main..<commit>` for an un-landed branch tip. That is also what makes a
deletion visible, which the pre-done gate's removal/rename rescue depends on.

**Fail-safe direction is INVERTED on this path, and must be corrected for.**
Every git seam in `reify-audit` fail-safes to `false`/empty on error. In the
sweep that converges on "no finding" — the safe direction. On the `--pre-done`
path the same defaults converge on a `High` that REFUSES a state transition, so
an infrastructure hiccup inside the 30 s hook subprocess is otherwise
indistinguishable from a genuine phantom-done. Two guards restore the safe
direction: a one-fork probe that `main` resolves at all
(`git merge-base --is-ancestor main main`), run only once something is already
absent so the healthy flip pays nothing for it; and a truncation flag on the
`PRE_DONE_SIBLING_SCAN_CAP` (50) break, since the corroborating commit may be
one the capped scan never inspected. Either downgrades the refusal to an
advisory `Low` carrying its reason — still emitted and visible, but exit 0, so
it cannot block the flip. **A refusal must rest on evidence actually gathered.**

One consequence for readers of the corroboration legs: `log_grep` matches the
whole commit message but `LOG_GREP_FORMAT` (`%H%x09%s`) returns only the
subject, so the digit-boundary collision filter can only adjudicate hits whose
subject contains the id. A hit matched on the body or a trailer is KEPT
unchanged — dropping it would silently narrow "reject digit collisions" into
"reject every body-only reference".

#### 11.1.3 Live wiring as measured (2026-08-28, task 6345)

The **Env var** bullet in §11.1 records the *target* wiring. Measured on the
Reify host, the live unit does not match it —
`/home/leo/.config/systemd/user/fused-memory.service:54` reads:

```
Environment="FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/.cargo/bin/reify-audit --task {id} --pre-done"
```

— the RAW binary, not the wrapper. The **Operator action required** rewire at the
end of §11.1 is therefore still outstanding as of this task.

- **The wrapper's freshness guard is bypassed on the live hook path.** Invoking
  the binary directly skips `scripts/reify-audit-predone-wrapper.sh`, and with it
  the REFUSE-mode freshness guard that would exit 125 rather than run a stale
  detector; a stale install is served silently instead. (SUPERSEDED as to policy
  by §11.1.5 — the wrapper no longer refuses on staleness, it falls open with an
  alarm. The bypass observation itself stands.) Measured:
  `/home/leo/.cargo/bin/reify-audit` has mtime `2026-06-09 23:32`, while the last
  commit touching `crates/reify-audit/` on `main` is `d8e36e3e4c` (2026-08-25) —
  ~77 days newer, i.e. stale by the guard's own freshness reference.
- **This task's armed gate does not reach the live hook until a reinstall.** The
  deployed binary predates every commit on this branch, so it cannot contain the
  `--pre-done` gate armed here. Post-merge operator action:
  `cargo install --path crates/reify-audit --root ~/.cargo --force`. (Measured
  separately, for readers of the **Binary** bullet above: the unit's exact
  command run from the repo root completes — `0 findings`, exit 0 — it does not
  error out for want of a `--tasks-file`.)
- **The freshness check cannot move into the binary to close this.** See the
  "WHY THE GUARD IS EXTERNAL" block in `scripts/reify-audit-freshness.sh`: the
  staleness to catch is precisely a binary built before any guard existed, so a
  Rust self-check can never fire from it. The guard must stay in the caller,
  which leaves the rewire plus the reinstall — both operator actions outside this
  repo — as the only fix.
- **Why the drift went unrecorded.** `scripts/smoke-predone-hook.sh` asserts that
  the env var is set, that its first token is executable and survives `--help`,
  and that the value carries `--task` / `{id}` / `--pre-done` — but never that the
  first token is the *wrapper*, and assertion 4 deliberately round-trips the
  binary directly. The smoke test is therefore green under the raw-binary wiring
  and cannot be used as evidence that the rewire happened.

#### 11.1.4 Arming the gate: rollout, and why a warn-only soak is silent (2026-08-29, task 6345)

Until this task the `--pre-done` path returned `[]` unconditionally — `check_task`'s
`status == "done"` guard made it structurally unable to fire on the one transition it
exists to gate (§11.1.2). This task converts it, in one step and with no soak, into a
fail-closed blocking gate that is **ARMED by default**. Two properties of the deployment
are worth planning the rollout around; both are measured, not inferred.

- **The break-glass costs the very restart it exists to avoid.**
  `REIFY_AUDIT_PREDONE_WARN_ONLY=1` is read from the hook subprocess's environment, which
  it inherits from fused-memory (§11.1.2: `create_subprocess_exec` with no `env=` kwarg).
  Setting it therefore means editing `~/.config/systemd/user/fused-memory.service` and
  running `systemctl --user daemon-reload && systemctl --user restart fused-memory` — the
  red-tier restart. An operator hit by a misfire mid-incident cannot apply the break-glass
  without the outage it was meant to prevent, so the decision to set it belongs *before*
  the reinstall, not after a misfire.
- **Warn-only makes the gate SILENT on the live hook path, not advisory.** Measured
  read-only against dark-factory
  `fused-memory/src/fused_memory/middleware/pre_done_hook.py`: the subprocess is launched
  with `stdout=PIPE, stderr=PIPE` (so neither stream reaches fused-memory's journal), and
  on `returncode == 0` the function returns `None` immediately — the captured
  `stderr_bytes` is decoded and surfaced only on a NON-zero exit. Warn-only downgrades
  every refusal to `Low` and so exits 0 by construction, which means its
  `[warn-only] pre-done gate: …` line is captured and discarded. The same is true of the
  advisory `Low` the fail-safe guards emit. **A soak run through the live hook observes
  nothing.**

Recommended sequence — all operator actions, outside this repo:

1. **Soak out-of-band, before the reinstall.** The only observational soak available is to
   run the gate directly and read its findings: build the crate on `main` and invoke
   `--task <id> --pre-done --project-root /home/leo/src/reify` against the tasks that are
   about to flip (or run the `/audit` sweep, whose P5 lane shares the corroboration legs).
   Reading exit codes and findings here is what the hook path cannot give you.
2. **If that run is clean**, `cargo install --path crates/reify-audit --root ~/.cargo --force`
   (§11.1.3) and let the gate arm.
3. **If it is not clean**, add `Environment="REIFY_AUDIT_PREDONE_WARN_ONLY=1"` to the unit
   and `daemon-reload && restart fused-memory` *before* the reinstall, so the first live
   exposure cannot block a flip — accepting that it is silent, and that the soak signal
   must still come from step 1.
4. **Disarming is a second restart.** Removing the `Environment=` line to arm the gate for
   real needs another `daemon-reload && restart`; budget it rather than discovering it.

Residual risk is reduced but not removed. The guards in §11.1.2, plus the fallible
`try_path_tracked_on` / `try_log_grep` seams added in this task, mean a git failure now
downgrades to an advisory `Low` rather than refusing — so the misfire surface is a task
whose `metadata.files` genuinely does not correspond to what landed, which is the gate
working as designed. And note the exposure is currently **deferred, not removed**: per
§11.1.3 the live hook still runs the stale 2026-06-09 binary, so the armed gate has zero
live effect until step 2 above is performed.

#### 11.1.5 The freshness guard fails OPEN (2026-09-02, task 7139)

§11.1.3 and §11.1.4 above describe the guard as REFUSE-mode: a stale
`REIFY_AUDIT_BIN` exits 125 "rather than run a stale detector". That policy is
reversed as of this task. A stale-but-runnable binary now emits a
self-describing `E_AUDIT_BIN_STALE` advisory to stderr and the detector runs
anyway; only an **unrunnable** binary (`E_AUDIT_BIN_MISSING`), or an operator
who armed `REIFY_AUDIT_FRESHNESS_STRICT=1`, still exits 125.

**Why: refuse mode was a project-wide outage, not a safeguard.** The wrapper is
a synchronous pre-done hook. Measured read-only against dark-factory
`fused-memory/src/fused_memory/middleware/pre_done_hook.py`: `returncode == 0`
returns `None` and ALLOWS the flip (:222-223); any non-zero rc becomes
`pre_done_hook_rejected` and BLOCKS it. So one stale binary blocked **every**
done-flip in the project until a human ran `cargo install`.

That is not a rare edge. `git log --since=90.days main -- crates/reify-audit`
counts 361 commits — ~4/day — and the freshness reference is the last commit
epoch of that path, so it advances several times a day. Each advance re-wedges
the project. The recorded instance ran **~15.7h** over 2026-08-30/31 (crate
epoch `617a053837` at 17:41 BST; reinstall at 09:25:51 BST the next morning),
and no automated repair path exists. Measured 2026-09-02: the only
`cargo install --path crates/reify-audit` invocation anywhere in `hooks/`,
`scripts/` or `dark-factory-orchestrator.yaml` is in the one-shot, operator-run
`scripts/deploy-reify-audit-predone-hook.sh`. Every other `cargo install` in
the tree installs a DIFFERENT tool (`setup-dev.sh`: sccache, cargo-nextest,
tree-sitter-cli) or is a hint string inside a diagnostic message
(`reify-audit-freshness.sh`, `reify-audit-predone-wrapper.sh`,
`smoke-predone-hook.sh`, `tree-sitter-generate.sh`). And `hooks/` contains only
`main-gate-lib.sh`, `pre-commit`, `pre-merge-commit`, `project-checks` and
`reference-transaction` — there is no post-merge hook to hang a reinstall on.

**Why the tokens are self-describing.** The outage produced three escalations —
esc-7042-2, esc-6315-2, esc-6120-5 — and all three misattributed it: they
blamed stale `metadata.files` and the `done_provenance` ancestor check, neither
of which was involved. The 125 was cryptic enough that competent triage went to
the wrong subsystem three times. Both message forms therefore now lead with a
stable machine token, state explicitly that this is *infrastructure* and NOT an
audit finding about `metadata.files` or `done_provenance`, carry the exact
one-line remedy, and report the two observed numbers (binary mtime, crate
epoch) — all inside `pre_done_hook.py`'s `_STDERR_CLIP = 2000` (:51), which is
what bounds what a triager actually sees.

**Why not auto-reinstall instead.** The hook runs INSIDE fused-memory's
per-project write lock with a 30s timeout (:25-31, :151), while
`cargo install --path crates/reify-audit` pulls the whole reify compiler stack
via `reify-test-support`. Inline reinstall would convert a refusal into a
*timeout* refusal and serialize every task mutation on the project behind a 30s
stall per flip. `scripts/release-sensitive-crates.txt` is likewise the wrong
lever: it declares debug-vs-release TEST scope, never invokes `cargo install`,
and is set-equality-gated by `tests/infra/test_release_scoped_scope.sh`.

**Consumers diverge deliberately.** Fail-open is right for the unattended
done-flip hot path. It is wrong for an ATTENDED deploy that just claimed to
have installed a fresh binary, so `deploy-reify-audit-predone-hook.sh` step 6
now treats an `E_AUDIT_BIN_STALE` advisory as fatal *regardless of rc* — without
that, a fail-open probe would have reported a green deploy over a stale fleet,
and that script is the `before_done` action of deterministic task #6939.

**Not closed by this task:** sibling task 6642 replaces the mtime PREDICATE with
a content-derived one (the case where a stale binary looks FRESH). This task
changes the POLICY applied once staleness is detected. They are orthogonal and
compose.

### 11.2 Snapshot filter and the `updatedAt`→`done_at` proxy

`scripts/reify-audit-predone-wrapper.sh` and the `/audit` skill both materialize their TaskMetadata snapshots through a single canonical jq filter at `scripts/reify-audit-snapshot-filter.jq`. The filter takes a fused-memory `tools/call get_tasks` JSON-RPC response on stdin and emits a JSON array of TaskMetadata-shaped objects (matching `crates/reify-audit/src/lib.rs:127-158`).

**The `done_at` derivation.** Fused-memory MCP does NOT currently expose an explicit done-flip timestamp on its task records (probed 2026-05-16; only `updatedAt` is available). P1's orphan-export grace window (see §5 P1) compares `ctx.now - done_at` against 14 days — so without a `done_at` value P1 silently skips every done task and becomes a no-op (this was the reviewer-blocking bug uncovered in task 3731 review cycle 1).

The filter uses `updatedAt` as a proxy: for tasks with `status=="done"`, it parses the ISO-8601 string (stripping the `.NNN` millisecond suffix that jq 1.7's `fromdateiso8601` rejects) and emits epoch-seconds. For non-done tasks `done_at` is always `null` (P1 skips them by status anyway — see `p1_producer_orphan.rs:79`).

Priority rule: the filter checks `.metadata.done_at` first (via jq `//` fallback). If fused-memory ever exposes an explicit done-flip timestamp on the task record, the filter picks it up automatically and the `updatedAt` fallback becomes unreachable. This makes the filter forward-compatible without requiring a code change.

**Approximation skew.** `updatedAt` is "when the task record was last written," which equals the done-flip time only when nothing further has been written to that task (status, metadata, etc.) after the flip. In practice this is true for most done tasks; the typical skew is hours-to-days, well inside P1's 14-day grace window.

**Loud-fail mode.** The wrapper post-checks the snapshot for any task with `status=="done"` and `done_at==null` (rows where the fallback also failed, e.g. legacy entries with no `updatedAt`) and emits a WARNING line to stderr listing the affected task ids. The wrapper does NOT block in this case — the warning is observability, not gating — so legacy data flows through but the silent-skip mode of the original bug is no longer reachable.

**Single point of truth.** The filter is referenced from both the wrapper and the audit-skill references (`references/cli-invocation.md` §2, `references/modes.md` §§1-4). This prevents copy-paste drift: fixing the filter in one place fixes all consumers. The regression-guard assertion (5e) in `tests/infra/test_reify_audit_predone_wrapper.sh` ensures the wrapper continues referencing the sidecar rather than an inlined copy.

**Follow-up action (upstream fused-memory).** A future enhancement to the fused-memory task store can stamp an explicit `metadata.done_at` field at the moment a task flips to `status=="done"`, reading from the existing `TaskInterceptor` reconciliation event stream. Once that field is exposed by `get_tasks`, the filter's `.metadata.done_at //` precedence picks it up automatically and the `updatedAt` fallback can be deleted. Action item logged via memory (`add_memory` category `procedural_knowledge`, keyed "fused-memory done_at proxy"). Track on the Reify side until upstream lands.

## 12. Implementation cost budget

**Total estimate for slice 1 (T-1..T-7, excluding D-1):** ~1 implementation session of 90–150 minutes interactive, OR ~2-3 orchestrated tasks if priority-graded high enough to land in reasonable time under the narrow-lock scheduler (per `feedback_orchestrator_narrow_locks_favor_upfront_design.md`).

| Task | Estimate | Lock scope |
|---|---|---|
| T-1 P5 library | 25–35 min | `crates/reify-audit/src/p5_phantom_done.rs` + tests. Single-crate; narrow-lock-friendly. |
| T-2 P2 library | 20–30 min | `crates/reify-audit/src/p2_consumer_stub.rs` + tests. Single-crate. |
| T-3 P1 library | 30–45 min | `crates/reify-audit/src/p1_producer_orphan.rs` + tests. Needs jcodemunch handle plumbing. Single-crate. |
| T-4 CLI | 15–25 min | `crates/reify-audit/src/bin/reify-audit.rs` + Cargo.toml workspace add. Single-crate. |
| T-5 `/audit` skill | 15–25 min | `.claude/skills/audit/SKILL.md` + `references/*.md`. Outside Cargo; no lock contention. |
| T-6 integration smoke | 20–30 min | `crates/reify-audit/tests/`. Single-crate. |
| T-7 `/review` wiring | 5–10 min | `.claude/skills/review/SKILL.md` (or the dark-factory copy — confirm in implement session). Cross-skill but small. |

Per `feedback_orchestrator_narrow_locks_favor_upfront_design.md`: the slice is structured so every task is single-crate or single-skill-file. The cross-crate concern (D-1 dark-factory hook) is *separate* and not bundled into slice 1. F-infra's implementation itself does not exhibit the failure mode it's designed to detect — every task in T-1..T-7 has a user-observable signal, and T-6/T-7 are the integration-gate leaves that prove the chain end-to-end.

**Priority recommendation:** T-1 (P5) at **high** priority — it addresses the most-witnessed failure mode (6 incidents in 2 weeks). T-2/T-3 at medium. T-4/T-5/T-6/T-7 follow the prereq chain.

## 13. Out of scope for this design

- D-1 itself (dark-factory PR) — scoped only as a queued task that the implement session will file.
- Slice 2: P3 (Type-C), full task-DAG walk, markdown-rendering of `data/audit-runs/`, `--trend` summarisation, cron daemon.
- G (corpus-level reviewer lint) — separate session pair per portfolio.
- Modification to `/prd`, `/review`, `/orchestrate`, or `/unblock` beyond T-7's small read-in.
- gap-register auto-promotion. F-infra deliberately does not write to `gap-register.md` — that stays Phase-3 human-curated.

## 14. Next session: implement

Implement-session hand-off:

> Implement F-infra slice 1 per `docs/architecture-audit/f-infra-design.md`. Ship T-1..T-7 (P5/P2/P1 library + CLI + `/audit` skill + integration smoke + `/review` wiring). Queue D-1 (dark-factory pre-done hook) as a separate deferred task in dark-factory at decomposition time; F-infra slice 1 lands hookless and activates pre-done gating when D-1 follows. Use `/prd` decompose mode on this design doc to generate the task batch (the doc is structured to satisfy G1/G2/G3/G4/G5/META). Expected implement-session length: 90–150 minutes interactive, or ~6–7 orchestrated tasks high-medium priority.
>
> Test plan: seed the three known incidents (may09 task 3242 for P5, a synthetic Type-A producer-orphan from C-04, a synthetic Type-B stub from C-39) and assert the detector flags each at the correct severity. Cross-check that none of the seven existing pre-`/prd` legacy tasks the slice touches gets a false positive.

---

**End of design.** No implementation in this session.
