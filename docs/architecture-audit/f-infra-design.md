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
- a `pending`/`in-progress` task with `consumer_ref` referencing the producing PRD exists in fused-memory, OR
- the symbol carries `#[allow(dead_code)]` or `#[cfg(test)]`.

**Detector:** `get_changed_symbols(branch=main, since=<task done timestamp>)` → for each new pub symbol, `find_references` → filter to non-test → check task-graph for downstream consumer.

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
- **Env var:** `FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done`
- **Wrapper (snapshot + invoke):** `/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh` — materializes a TaskMetadata JSON snapshot from `mcp__fused-memory__get_tasks`, then invokes `reify-audit` with `--tasks-file <tempfile>` (snapshot cleaned up on EXIT). → uses `scripts/reify-audit-snapshot-filter.jq`; see §11.2 for the `done_at` proxy rationale.
- **Binary:** `/home/leo/.cargo/bin/reify-audit` (invoked by wrapper; installed via `cargo install --path crates/reify-audit --root ~/.cargo --force`). The binary requires an explicit `--tasks-file`; there is no default path (removed in task 3731 after the Taskmaster deletion made the old default non-existent).
- **Smoke test:** `bash scripts/smoke-predone-hook.sh` (exits 0 when wiring AND wrapper round-trip both succeed; assertion 4 catches re-introduction of the dead default).
- **Reload command:** `systemctl --user daemon-reload && systemctl --user restart fused-memory`
- **Operator action required:** rewire the systemd `Environment=` line to point at the wrapper: `Environment=FUSED_MEMORY_PREDONE_HOOK_REIFY=/home/leo/src/reify/scripts/reify-audit-predone-wrapper.sh --task {id} --pre-done`. Then reload and verify via `bash scripts/smoke-predone-hook.sh`.
- **Procedural memory:** entry keyed `FUSED_MEMORY_PREDONE_HOOK_REIFY systemd activation` in fused-memory memory store

#### 11.1.1 Why the snapshot wrapper? (task 3731)

The `reify-audit` binary is a pure-logic library (no MCP client, no scheduler). Before task 3731, the CLI defaulted `--tasks-file` to `.taskmaster/tasks/tasks.json`, which was deleted in commit `1402b46c63` (Taskmaster removal, 2026-05-12). Any invocation without an explicit `--tasks-file` silently exited 125 ("infrastructure error") and blocked done-flips. The fix makes `--tasks-file` required (no default) and concentrates fused-memory coupling at the wrapper boundary: the wrapper materializes a fresh TaskMetadata snapshot via `mcp__fused-memory__get_tasks` before each invocation, keeping the audit crate dependency-free. See design decisions in `.task/plan.json` for the rationale for Option 1 over Options 2 (new `--from-fused-memory` flag) and 3 (auto-write snapshot on state change).

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
