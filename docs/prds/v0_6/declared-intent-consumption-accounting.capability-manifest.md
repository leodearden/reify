# Capability manifest — declared-intent consumption accounting

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/declared-intent-consumption-accounting.md`.
Each binding maps a leaf's asserted capability to **executed** evidence, with a
PASS verdict; any FAIL blocks the batch. Authored at decompose time, 2026-07-24.

**Leaf task IDs:** stamped into the YAML sidecar
(`declared-intent-consumption-accounting.capability-manifest.yaml`) by `commit_planning`.

**Probe environment:** `target/debug/reify` (debug binary built 2026-07-22 —
all cited source anchors re-verified on main `0d70ef1d5b` 2026-07-24 by direct
read); `tree-sitter` from `tree-sitter-reify/`; committed fixtures under
`docs/prds/v0_6/fixtures/dic_*.ri`.

**D3 workflow note (2026-07-24):** the decompose-verify workflow (run
`wf_64a00407-b28`) launched over all seven leaves but 16/22 agents failed on
the platform's weekly subagent limit (resets Jul 27); its `blocks:true` verdict
is entirely spawn-failure artifacts — zero premise FAILs among completed
probes. Every premise the workflow would bind was executed directly by the
decompose session with captured output (this file + PRD §2.2). The partial run
still paid for itself: an Enumerator caught a probe-capture error (a piped
`$?` reading `tail`'s exit, not reify's) — re-probed: `check --strict` on the
inert fixture exits **1** (misattributed reason stands as the δ finding), and
`eval` on the String-auto fixture exits **1** (the misleading residual *text*
is the β finding, not the exit code).

---

## Scope note — substrate vs deliverable (why no FAILs)

Decompose-time verification asserts only the **assumed substrate** (the relate
verify machinery, the registry drop sites, `build_dependent_cells`,
`classify_undef`'s reason strings, the connect generation site, the
`finish_check` spine) plus the **baseline silences** each RED signal repairs —
never the tasks' own deliverables, which by definition do not exist yet. Every
new rejection/diagnostic asserted by a leaf signal (relate violation Error,
envelope refusal, inert-objective error, generation refusal, ledger inert
failure) is that leaf's **own deliverable** (G6 branch 4: the rejection
mechanism is introduced by the task that asserts it), verified at the leaf's
signal, not as a substrate probe.

## Numeric note (G6 branches 1/2)

No accuracy-bound or exactness premises exist in this PRD. The only geometric
assertions are combinatorial/colocation identities in the relate fixtures
(datums either colocated by construction — `dic_relate_static_ok` translates
both subs' datums to the same coordinates — or separated by 30–20 mm,
orders of magnitude beyond assertion tolerance). No floor exposure.

## Grammar evidence (anti-mismatch)

No novel syntax. All 7 committed fixtures parse with 0 ERROR nodes
(`tree-sitter parse --quiet`, 2026-07-24): `dic_relate_static_violated`,
`dic_relate_static_ok`, `dic_string_auto`, `dic_min_no_autos`,
`dic_min_unread`, `dic_min_unconstrained`, `dic_inert_connect`.

---

## Per-leaf capability bindings

### α — relate zero-auto static verification arm

| Capability | Evidence | Verdict |
|---|---|---|
| Verify machinery exists & is production-wired | `grep`: `fn solve_relate_scope` + step-4 remainder verification `relate_solve.rs:635-797`; pipeline entry `solve_scopes` called from `engine_build.rs:3710` (production, not test-only) | **PASS** |
| The zero-auto drop is real (signal premise) | `grep`: the `:836` filter `!s.auto_unknowns.is_empty() && !s.relations.is_empty()`; doc comment `:809-811` confirms the deliberate skip | **PASS** (gap confirmed) |
| Baseline false-green (signal premise) | probes 2026-07-24: `reify check dic_relate_static_violated.ri` → `All constraints satisfied.` exit 0; `reify eval` → zero relate output on both fixtures | **PASS** (bug confirmed) |
| Fixed-placement witness convention | substrate: the solve path already witnesses at identity (`relate_solve.rs:668` seed comment); ok-fixture datums colocated by construction | **PASS** |

### β — solver capability envelope + typed refusal

| Capability | Evidence | Verdict |
|---|---|---|
| No representability check exists (signal premise) | source read 2026-07-24: classifier `String` explicit no-op (`classifier.rs:115`), no-flag → `Dimensional` default (`classifier.rs:22-37`); `DimensionalSolver` builds `Value::Scalar` trial/solved values unconditionally (`solver.rs:138-150`, `:110-129`) | **PASS** (gap confirmed) |
| Baseline misleading residual (signal premise) | probe 2026-07-24: `reify eval dic_string_auto.ri` → `constraints could not be satisfied (max absolute residual: 1.00e0)` + `s undef (solve failed: infeasible)` | **PASS** (bug confirmed) |
| Undef-cause channel for refusals | `grep`: `record_failed_autos` / `capture_undef_causes` `engine_eval.rs:4821-4857` (undef-self-describing, done) — production CLI `note:` loop renders causes | **PASS** |
| §3.5 seam additive-method shape | substrate: `ConstraintSolver` trait + `SolverRegistry` dispatch (`registry.rs:85-92` `solver_for`); additive default method breaks no impl (rustc-guarded) | **PASS** |
| PRD 2 landing-order independence | design property (E3): verdicts consult live registry contents; pinned fixture kind = `String`, outside CpSat's envelope too | **PASS** |

### γ — objective consumption accounting

| Capability | Evidence | Verdict |
|---|---|---|
| The three silent drop sites are real (signal premise) | source read 2026-07-24: `registry.rs:152-162` (no autos), `:182-192` (`components.is_empty()`), `:208-210` (no-match → component 0, comment verbatim) | **PASS** (gap confirmed) |
| Baselines (signal premises) | probes 2026-07-24: `dic_min_no_autos` → total silence; `dic_min_unread` → `a=3.06…`, objective silently useless; `dic_min_unconstrained` → `a undef (awaiting solve)`, objective never mentioned | **PASS** (bugs confirmed) |
| Transitive-reachability builder wired | `grep`: `fn build_dependent_cells` `engine_eval.rs:1553`, called from `build_solver_problem` paths (production; #5188 done) | **PASS** |
| Engine objective-diagnostic site | `grep`: `W_SOLVER_OPTIMALITY_UNPROVEN` emission `engine_eval.rs:4867-4877` + merged `:6281-6291` (#4804 done) — the house pattern γ's diagnostic sits beside | **PASS** |
| Vacuous-healthy severity rule is implementable | substrate: auto-declared-ness and per-instance binding are distinguishable at problem build (unresolved-auto set is what `build_auto_param_list`/the solve path already computes) | **PASS** |

### δ — typed IndeterminateReason + recorded-reason rendering

| Capability | Evidence | Verdict |
|---|---|---|
| Reason knowledge already recorded (signal premise) | `grep`: `classify_undef` `reify-constraints/src/lib.rs:68-102` distinguishes "undefined inputs: …" vs "operator undefined for these operand kinds: …"; GD&T free-text reason `engine_constraints.rs:2100` | **PASS** |
| Carrier gap is real (signal premise) | source read: `ConstraintCheckEntry` `reify-eval/src/lib.rs:1140-1144` = `{id, label, satisfaction}` only; `Satisfaction` bare (`reify-ir/src/value.rs:3912-3919`) | **PASS** (gap confirmed) |
| Misattribution baseline (signal premise) | probe 2026-07-24: `reify check --strict dic_inert_connect.ri` prints the synthesized generic "inputs undefined (e.g. auto-params unresolved or geometry did not realize)" while the recorded warning says "operator undefined for these operand kinds" | **PASS** (bug confirmed) |
| Additive-field compat | design property: `Satisfaction` untouched; entry field is `Option` — MCP/GUI consumers unaffected until they opt in | **PASS** |

### ε — inert-constraint detection (generation refusal + backstop + sweep)

| Capability | Evidence | Verdict |
|---|---|---|
| Generation site known (signal premise) | source read: `connect.rs:587-619` builds `frame_align_{l}_{r}` as `Eq(left_frame, right_frame)`; `@face`/`@edge` placeholder eval `reify-expr/src/lib.rs:1281`; Indeterminate bail `reify-constraints/src/lib.rs:173-206` | **PASS** |
| Inertness of the class (signal premise) | probe 2026-07-24: `dic_inert_connect.ri` → `INDETERMINATE frame_align_a_b`, reason "operator undefined for these operand kinds", green non-strict exit 0 — run-invariant per the investigation (a0d342d4) probe evidence | **PASS** (bug confirmed) |
| `connect_compat` half unaffected | probe: same run reports `OK connect_compat_a_b` — the refusal targets only the frame_align generation | **PASS** |
| Producer upstream (DAG-direction) | δ (reason taxonomy) is a hard upstream dep of ε in this batch | **PASS** |

### ζ — check consumption ledger (integration gate)

| Capability | Evidence | Verdict |
|---|---|---|
| Check spine exists (substrate) | source read: `report_constraint_results` `main.rs:2370-2400` → `ConstraintOutcome` → `check_fails` `main.rs:2299-2305` → `finish_check` `main.rs:2719-2746` | **PASS** |
| Baseline summary strings (signal premise) | probe + source: `All constraints satisfied.` / `No constraints violated (N indeterminate).` — no per-reason breakdown, no objective/relate rows, no inert class | **PASS** (gap confirmed) |
| Producers all upstream (DAG-direction) | ζ ← {α, γ, δ, ε}; every ledger fact class is delivered by a hard upstream dep in this batch (anti-inversion) | **PASS** |
| Inert-fails-check is outcome-native | design property: extends `ConstraintOutcome`/`check_fails`, not the diagnostic-severity exit gate (sibling PRD's seam — G4) | **PASS** |

### η — companion docs

| Capability | Evidence | Verdict |
|---|---|---|
| Spec target exists | `docs/reify-language-spec.md` on main documents `reify check` semantics today | **PASS** |
| Producer upstream | ζ (the shipped ledger) is a hard upstream dep — docs describe landed behaviour, never aspirations | **PASS** |
