# Solver legibility & telemetry — GUI + debug MCP

**Milestone:** v0_6 · **Status:** active — surface PRD · **Approach:** B + H

Authored 2026-08-26 in an interactive `/prd` session (Leo + Claude; groundwork by a
six-agent team). Fourth of four solver-program PRDs (P4). P1
`solver-driver-parity.md` is committed and is this PRD's stated precondition; P2
`geometry-algebra-solver-unification.md` runs in parallel; P3 multimodality is
chartered.

**Code anchors** verified against main `2128c3692cbb88f59b6e9edfd25ee801513423bb`
(2026-08-26); main has since advanced. Main moves fast — cite-by-symbol; re-locate
lines at implementation time.

**Amendment A1 (2026-09-03) — ε's signal restated; it was not achievable as authored.**
ε (#6722) originally asserted "`reify check` … prints the remaining slack per inequality
constraint and `n/a` for an equality". That signal could not be produced from ε's own
scope, for two independent reasons, both established by execution (D3 run
`wf_96ec9b6e-8eb` plus first-hand probes against a `target/release/reify` built
2026-09-01):

1. **No renderer was in scope.** ε's modules are the solver/IR/eval crates; none is a
   CLI file. Per-constraint output is formatted by `report_constraint_results`
   (`crates/reify-cli/src/main.rs`), which writes only `"  {status} {label}"`. **No leaf
   of this PRD owns `reify check`'s per-constraint rendering** — ξ is the only leaf
   scoped to that file and its signal is `reify explain`.
2. **`reify check` does not resolve `auto` params**, so there is no post-solve value map
   to compute slack from on the very fixture class the signal described. Measured on
   `param w : Length = auto` with `w >= 8mm`, `w <= 12mm`, `minimize w`: `reify check`
   returns `INDETERMINATE` for both constraints with *"undefined inputs"* and exit 0,
   while `reify eval` resolves `w = 0.01 m`. Auto resolution runs on the eval/solve
   path, not the check path; making `cmd_check` resolve is **P1-δ #6693**'s deliverable.

This is the G6 branch-3 shape (esc-3436-210): a signal demanding output its own
dependency set cannot produce. The capability manifest scored ε PASS because its
bindings check only the **producer** side; nothing checked that a renderer exists.
**No edge to #6693 was added** — an edge alone changes nothing without a renderer owner.
If `reify check` should print slack, that is a **separate leaf** requiring both a CLI
renderer owner and the #6693 edge. ε's WORK items, honesty contract, lockstep
obligation and #6653 prereq are unchanged.

---

## §1 — Goal

A solve becomes **readable** — by the human in the GUI and by an LLM through the
`reify-debug` MCP — without leaving the surface you are working in.

Today the engine computes a rich solve record and then discards almost all of it
before anything can render it. `reify explain` shows three fields of a five-field
provenance record for value autos only; the GUI shows *none* of it, loses **every**
eval-time diagnostic, and renders its one constraint-verdict badge from a string
whose casing no frontend consumer matches. An LLM editing a `.ri` through the debug
MCP has 61 tools available and **not one** reports a solve outcome.

After this PRD, the same solve is legible on three surfaces that agree:

- **CLI** — `reify explain` (already shipped, #4017), extended to the fields it
  currently drops.
- **GUI** — each datum rendered by the panel that already owns its shape, plus one
  solve-summary chip in `StatusBar` as the single front door.
- **`reify-debug` MCP** — one `solve_report` tool returning the same record as
  structured data, so an agent closes its loop on data rather than on scraped UI
  state.

User-observable on landing:

- Loading a solver-bearing model in the dev GUI shows constraint verdicts that
  **match `reify check`** — today every badge renders `?` and `StatusBar` reports
  `0 satisfied / 0 violated / N indeterminate` regardless of the real verdicts.
- `W_SOLVER_OPTIMALITY_UNPROVEN` appears in the GUI Diagnostics panel under a new
  `solve` source chip — today **no eval-time diagnostic reaches the GUI at all**.
- A `minimize` that parks on an active clearance bound shows the remaining slack on
  that constraint, in the constraint panel and in `reify explain` — today nothing
  anywhere reports slack.
- Each resolved `auto` shows which objective governed it, beside the determinacy and
  freshness chips `PropertyEditor` already renders.
- `mcp__reify-debug__solve_report` returns per-auto provenance, per-constraint
  verdict + slack, optimality status, and the resolution profile that ran — asserted
  by a committed `ValueScenario` against a live `reify-gui`.
- A non-converged, budget-exhausted, refused or **stale-served** solve is visibly
  flagged in the GUI, never only on stderr.

---

## §2 — Background

### §2.1 — The engine computes this and throws it away

Five distinct drops, all verified in-tree:

1. **The `EvalResult` → `CheckResult` narrowing.** `Engine::check`
   (`crates/reify-eval/src/engine_constraints.rs`) calls `self.eval(module)`, which
   **fully populates** `objective_provenance`, then constructs `CheckResult` from
   five fields — provenance is not one of them. `BuildResult` likewise has no such
   field. The data is computed on the build/check path and dropped at a six-line
   type boundary. **The GUI's load path goes through `check()`.**
2. **The warm/edit path never computes it.** `eval_cached`, `edit_param` and
   `edit_source` construct `EvalResult` with `objective_provenance: HashMap::new()`
   and call plain `solve()` rather than `solve_ranked` — a documented, deliberate
   cold/warm divergence (task #5118). The GUI's *edit* path goes here. P1 κ closes
   this; this PRD depends on it.
3. **`W_SOLVER_OPTIMALITY_UNPROVEN` is cold-only.** It is pushed only from the
   `solve_ranked` arm, so a warm re-solve can return an iteration-limited answer
   with no warning. Same P1 κ dependency.
4. **`candidates[1..]` and every `objective_score` are discarded at the engine
   seam** — the engine consumes `candidates[0]` and drops the rest. The largest
   single telemetry drop in the stack.
5. **Per-constraint residual and slack are folded to a scalar inside the solver**
   and embedded in an error-message string on the failure path only.
   `ConstraintCheckEntry` — the only per-constraint record that escapes the engine
   — is `{ id, label, satisfaction }`. No residual, no slack, no active flag.

### §2.2 — Three surfaces that are dead, not merely unwired

Each was verified by execution or by exhaustive grep, and each is owned as a leaf
here (Leo, 2026-08-26).

**The constraint-verdict badge renders nothing true.** `build_constraints`
(`gui/src-tauri/src/engine.rs`, two sites) emits `"Satisfied"` / `"Violated"` /
`"Indeterminate"`. Every frontend consumer compares lower-case — `StatusBar`'s
`c.status === 'satisfied'`, `ConstraintPanel`'s `case 'satisfied'` and its
`isExpandable = status !== 'satisfied'`. There is **no normalisation anywhere**
(the only `toUpperCase` in the tree formats a file extension) and **no lower-case
status literal exists on the Rust side**. `build_constraints` is the production
feed. Consequently every badge renders `?` with the title *"Indeterminate — not yet
evaluated"*, every row is treated as expandable, sort priority is uniform, and the
`StatusBar` counts are structurally false. The entire frontend fixture corpus uses
lower-case, so **the vitest suite is green against a contract the backend has never
satisfied** — the precise shape a two-way boundary test exists to catch, which is
why §4 pins one.

**No eval-time diagnostic reaches the GUI.** `CheckResult.diagnostics` is never read
by `EngineSession`: `build_gui_state` populates `compile_diagnostics` from
`compiled.diagnostics` and `tessellation_diagnostics` from `tess_result.diagnostics`,
and neither is the `CheckResult`. `tessellate_snapshot` cannot back-fill it — it
uses `check_constraints_against_templates` with no `eval()` and accumulates its own
buffer. This loses `W_SOLVER_OPTIMALITY_UNPROVEN`, `"Constraint solver made no
progress"`, the infeasible-solve warnings, and the DFM/GD&T check-pass diagnostics.
The CLI surfaces all of them. **The GUI is the only surface that loses them**, and
the fix is small: the wire type already carries `code`, and `DiagnosticsPanel`
already filters by source.

**`AutoResolvePanel` can never paint.** It is mounted under
`<Show when={engineStore.state.autoResolve.active}>`, but `emit_auto_resolve_if_any`
fires `start → iteration → complete` back-to-back synchronously inside one
`post_engine_call_telemetry` call, and `endAutoResolveLoop` sets `active: false`
**and clears `iterations`**. The panel flashes for at most a frame. Both its charts
are structurally undrawable: `driving_metric_value` is always `None` (the chart
needs ≥2 finite points) and exactly one iteration exists per loop (sparklines need
≥2). Its props-driven vitest builds multi-iteration fixtures by hand and cannot
catch any of this.

### §2.3 — The substrate is richer than the gap suggests

This PRD is mostly *retain, key and render*, not *compute*:

- **Per-inequality slack is already implemented.** `collect_slack_terms`
  (`crates/reify-constraints/src/solver.rs`) builds signed-slack `CompiledExpr`s
  (`Ge`/`Gt` → `left − right`, `Le`/`Lt` → `right − left`, `And` → recurse) to feed
  the centrality objective. What is missing is the **key**: it pushes into a flat
  `Vec<CompiledExpr>`, destroying the association to a `ConstraintNodeId`.
- **A complete per-iteration telemetry pipeline exists.** `SolverProgressSink` /
  `SolverProgressUpdate` / the thread-local `SOLVE_DISPATCH_CONTEXT` / the
  `solver-progress` IPC channel / `bridge.ts`'s listener / `engineStore`'s ring
  buffer / `SolverProgressOverlay` are all wired — **for FEA CG only**. The
  constraint solver emits nothing on it; only a producer is missing.
- **`TermContribution` `{ sense, weight, realized_value, contribution }`** is
  already computed per scope and `Arc`-shared, and `reify explain` drops it.
- **`PropertyEditor` already renders per-cell determinacy, freshness and an
  undef-reason chip** — the natural home for provenance, already plumbed.

### §2.4 — Vocabulary already taken

`solver-progress` / `SolverProgressOverlay` means the **FEA CG** solver (#3543).
"Auto-Resolve" means the #2967 FEA loop panel. This PRD therefore reuses **`explain`**
as its single noun across all three surfaces — `reify explain`, the GUI's
solve-summary chip and drill-ins, and `solve_report` — rather than minting a third
sense of "solver".

---

## §2.5 — Verified substrate (G3)

Every capability this PRD's mechanisms and signals assume, and the evidence it
exists. Verified at `2128c3692c` by execution or exhaustive grep.

| Capability | Where | Verified |
|---|---|---|
| `ObjectiveProvenance` (5 fields) populated on the cold `eval()` path | `reify-ir` `constraint.rs`; `engine_eval.rs` per-template + merged-cluster arms | ✅ read; both construction sites |
| Provenance **absent** from `CheckResult`/`BuildResult` (the narrowing) | `Engine::check`, `engine_constraints.rs`; `BuildResult`, `reify-eval` `lib.rs` | ✅ read — five-field construction, no provenance field |
| Provenance **empty** on warm/edit | `eval_cached` (`engine_eval.rs`); `edit_param`, `edit_source` (`engine_edit.rs`) | ✅ grep — three `HashMap::new()` sites |
| Signed per-inequality slack already computed | `collect_slack_terms`, `reify-constraints` `solver.rs` | ✅ read — `Ge/Gt/Le/Lt/And`; `Eq/Ne/Or` skipped |
| Slack **not** keyed by constraint | same — pushes into a flat `Vec<CompiledExpr>` | ✅ read |
| `ConstraintCheckEntry` = `{id, label, satisfaction}` only | `reify-eval` `lib.rs` | ✅ read — no residual/slack/active flag |
| `W_SOLVER_OPTIMALITY_UNPROVEN` emitted, coded, **cold-only** | `engine_eval.rs` — two byte-identical sites; the cold/warm divergence comment | ✅ read both + the #5118 rationale |
| `CheckResult.diagnostics` never read by the GUI | `gui/src-tauri` — `build_gui_state` sources compile/tessellation only | ✅ grep — no reader outside tests |
| Verdict casing mismatch | `build_constraints` (2 sites) vs `StatusBar`/`ConstraintPanel`; no normaliser | ✅ grep — 0 lower-case Rust literals, 0 `toLowerCase` |
| `SolverProgressSink` pipeline wired end-to-end for FEA CG only | `reify-eval` `solver_progress.rs`; `elastic_static.rs`; `TauriSolverProgressEmitter`; `engineStore`; `SolverProgressOverlay` | ✅ read — only `solver_kind` in tree is `"cg"` |
| `PropertyEditor` already renders determinacy/freshness/undef chips | `gui/src/panels/PropertyEditor.tsx`; `ValueData.reason` | ✅ read |
| `DiagnosticInfo` carries `code` on the wire; panel filters by source | `reify-core` `DiagnosticInfo`; `diagnosticsView.ts` | ✅ read |
| Debug-MCP tool registration = `ToolDef` + `dispatch_tool` + 2 allowlists | `debug_server.rs`; `debugParity.test.ts`; `gui/test/visual/assertions.ts` | ✅ read — 0 solver tools among those advertised |
| `EngineSession` has **no** production accessor for `last_check()` | `gui/src-tauri` `engine.rs` — `#[cfg(test)]` only | ✅ read — new `pub(crate)` accessor needed (λ) |
| `ValueScenario` / `VALUE_SCENARIOS` e2e harness | `gui/test/visual/assertions.ts`, `run.ts` | ✅ read — its doc names downstream tool-leaf tasks |
| Debug-MCP e2e is **not** CI-gated | `scripts/verify.sh` | ✅ grep — 0 hits for `test:e2e`/`test:visual`/`REIFY_DEBUG`/`3939` |
| `cost_robustness_tradeoff` marker-field precedent (grammar-free opt-in) | `ObjectiveSet.cost_robustness_lambda`, `reify-ir` `constraint.rs`; typing in `entity.rs` | ✅ read |
| `#robust` / `#robust(k=v)` parse cleanly (the alternative in D7) | `tree-sitter-reify/grammar.js` — `pragma` is a `purpose_member` | ✅ executed — `tree-sitter parse --quiet`, 0 ERROR nodes, exit 0 |
| `apply_robustness_floor` exists as a parameter but is hard-wired | `solve_core_with_sd_tolerance` / `solve_core`, `solver.rs`; `ResolutionProblem` has no field | ✅ read |

**No novel grammar is required.** D7's default route reuses the shipped special-form
precedent; the probed pragma alternative parses today. G3 raises no unresolved
substrate assumption — every gap this PRD closes is a *retain/key/render* gap over
data the engine already computes, except ε's constraint key and λ's observational
accessor, both of which are leaves here.

---

## §3 — Consumers (G1)

| Consumer | What it consumes | Status |
|---|---|---|
| **printer_v01 dogfood** | "Did my solve converge? which basin? why did this station park here? how much clearance slack remains?" — today answerable only by reading stderr and re-deriving by hand. | live, named |
| **LLM design iteration via `reify-debug` MCP** | `solve_report` — machine-readable per-auto provenance, verdicts, slack, optimality, refusals — so an agent editing a `.ri` closes its loop on data. Today it has 61 tools and none reports a solve outcome; the closest reach is scraping `store_state`. | live, named |
| **`reify explain` (CLI humans)** | The dropped `term_contributions` and `scope` fields, plus slack. | live, shipped surface |
| **`constraint-solver-completion.md`** | Its §10 defers "GUI / LSP rendering of `ObjectiveProvenance` … a future surface PRD (declared consumer, §1)". **This PRD is that PRD** — it discharges a declared-but-unwired consumer, rather than declaring a new one. | live, named |
| **`solver-driver-parity.md` (P1)** | P1 §3 and §8 both name P4 as the consumer of what its drivers report; owner column `P4`. Its `ResolutionProfile` two axes (iteration budget, staleness) are rendered here. | committed |
| **`gui-on-demand-measurement.md`** | Depends on this PRD's verdict-casing fix — its measured verdicts route through the same `build_constraints` payload and would render `?` for the same reason. | active, edge declared (§8) |

Every mechanism and its consumer:

| Mechanism | Consumer |
|---|---|
| `objective_provenance` on `CheckResult` / `BuildResult` (the narrowing widened) | `PropertyEditor` provenance chip (η); `solve_report` (λ); build-path `reify explain` parity |
| `margin: Option<f64>` on `ConstraintCheckEntry` + `ConstraintNodeId`-keyed slack | `ConstraintPanel` slack column (ζ); `reify explain` slack lines (ε); `solve_report` (λ) |
| `build_check_diagnostics()` + the `solve` diagnostic source tag | `DiagnosticsPanel` filter chip (γ); `StatusBar` badge counts |
| Lower-case verdict tokens + a two-way boundary test | `ConstraintPanel` badges, `StatusBar` counts (β); `gui-on-demand-measurement.md`'s measured verdicts |
| Constraint-solver producer on `SolverProgressSink` (`solver_kind: "nelder-mead"`) | `SolverProgressOverlay` (κ); `AutoResolvePanel`'s real iteration stream (δ) |
| Solve-summary chip in `StatusBar` | The human's single front door; drills into the owning panel (θ) |
| DOF badge (transferred from #4388) | `StatusBar`, rendering #4388's ledger (ι) |
| `solve_report` debug-MCP tool | LLM design iteration; the committed `ValueScenario` (λ) |
| Author opt-in to the robustness floor | `.ri` authors who want boundary-parking avoided on non-Money objectives (μ) |

**Engine-integration sub-check (G1).** Every engine-side mechanism plugs into the
catalogued **§3.5 ConstraintSolver** seam of `docs/prds/v0_3/engine-integration-norm.md`,
or is a pure widening of an existing result type. **No new engine seam is introduced.**
The GUI-side mechanisms plug into the existing `post_engine_call_telemetry`
choke-point and the `docs/gui-event-channels.md` convention (§4.3).

---

## §4 — Contract: the solve record and its three renderers (B + H)

**G5 fires** on every axis: blast radius spans `reify-ir`, `reify-constraints`,
`reify-eval`, `reify-cli`, `gui/src-tauri` and `gui/src` (6); mechanism count is ~9;
it touches the load-bearing ConstraintSolver seam and the GUI state-sync seam; and
it has ≥2 cross-PRD consumers. So: a contract section plus a two-way boundary-test
sketch, and the integration-gate leaf (φ) names that sketch as its signal.

### §4.1 — C1: one record, three renderers

The engine produces **one** serializable solve record. The CLI, the GUI and the MCP
tool are three *renderers* over it. No renderer may compute a field the record does
not carry, and no renderer may re-derive a value by parsing another renderer's
output.

This is the invariant that makes parity **testable** rather than asserted, and it is
the direct lesson of §2.2's casing defect: two sides agreed in prose and diverged in
fact because nothing drove real producer output through the real consumer.

The record's fields are the union of what is already computed, plus exactly one
explicitly reserved slot — `completeness`, which has no source today and is called
out under the table:

| Group | Fields | Source today |
|---|---|---|
| Per-auto | `cell_id`, resolved value, `scope`, `objective` (term count + terms), `combination`, `term_contributions`, `synthetic_centrality`, `inherited_from` | `ObjectiveProvenance` (5 fields, of which `reify explain` renders 3) |
| Per-constraint | `id`, `label`, `satisfaction`, `margin: Option<f64>` | `ConstraintCheckEntry` + ε's keyed slack |
| Per-solve | `OptimalityStatus`, uniqueness, the `ResolutionProfile` that ran (budget axis, staleness axis), stale-served flag, `completeness` (**RESERVED** — see §8.2) | `ranked.rs` + P1 α; the `completeness` slot has **no source today** — its vocabulary is P3 **#6706**'s `Completeness` carrier (`Exhaustive \| Partial{reason} \| Refuted{narrowing}`), first populated by P3 **#6711** |
| Diagnostics | the `DiagnosticCode`-carrying eval diagnostics for this solve | `CheckResult.diagnostics` (γ) |

**The `completeness` slot is named and reserved, and no P4 leaf populates it.** It is
listed here so that §11's "a named slot in the record" has a referent a reader can
point at, and so that a renderer author meets the slot in the contract rather than
inventing one. The vocabulary is **#6706**'s (the `Completeness` carrier); the first
producer that fills it is **#6711**, which dedups the candidate set by basin box,
attaches the verdict, and renders it through `ObjectiveProvenance` / `reify explain`
(§8.2 rules the ownership split). Reserving the slot changes **no** P4 leaf's scope,
`metadata.files` or signal: nothing in §13 gains an obligation, and no leaf acquires
a dependency on either id.

### §4.2 — C2: honest absence, never a fabricated value

Four field-level rules, each with a verified motivating defect — the first three on a
surface this PRD repairs, the fourth on a producer P3 retires (so it binds P4's
renderers without obligating any P4 leaf):

- **Slack is inequality-only.** `Eq`, `Ne` and `Or` are explicitly skipped by
  `collect_slack_terms` — there is no well-defined signed interior slack for an
  equality. The record carries `margin: None` and every renderer shows `n/a`,
  **never** `0`, which would read as "no margin left".
- **Never collapse a tri-state.** `AutoResolveConstraintProgress` today folds
  `Indeterminate` into `satisfied: false`. The record carries the three-valued
  `Satisfaction` and its typed cause; renderers must not re-collapse it.
- **Never emit a placeholder scalar.** `build_constraints_payload` hard-codes
  `value`/`unit`/`target_lower`/`target_upper` to `None` with an in-source rationale
  that emitting `0.0` "would be a wire-level lie". That judgement is correct and is
  hereby promoted to a contract rule. Populating those fields honestly requires
  widening `ConstraintCheckEntry` first (ε), which is why ζ depends on ε.
- **Never substitute a weaker claim for an absent one.** While the `completeness` slot
  (§4.1) is unpopulated, every renderer shows **nothing** for it — and in particular must
  not render a bare `unique: true` in its place. #6706's invariant C1 makes `unique` a
  *derived* value — `unique == (completeness == Exhaustive && solutions.len() == 1)` — so
  a standalone uniqueness claim, asserted without the completeness verdict it is derived
  from, is exactly the fabricated value this rule bans. The motivating defect is already
  in the tree, on the **producer** side: `SolveSpaceSolver::solve` returns
  `unique: true` unconditionally on the libslvs `Ok` arm
  (`crates/reify-constraints/src/solvespace.rs`:1700), and `relate_solve::solve_frame`
  computes `unique = fully_pinned && !unknown.free` from **local** Jacobian rank
  (`relate_solve.rs`:317) — local isolation reported as global uniqueness. P3 §0.2 item 4
  measured both; C1 retires them. This rule is P4's half: a fabricated `unique` must not
  be laundered through a renderer just because the completeness slot beside it is empty.
  No **P4** leaf is obligated by it — nothing renders uniqueness today (§13 Q1's θ chip
  states and λ's `solve_report` field list both omit it) — so the rule forbids a future
  lie rather than adding work to any leaf in §13.

### §4.3 — C3: the GUI seam obeys the existing conventions

- **Event channels.** Any new or extended channel adds its row to
  `docs/gui-event-channels.md` §1/§2 **and** to
  `docs/prds/v0_3/gui-event-channel-inventory.md` §2 in the *same commit*, plus a
  per-channel spec under `docs/gui-event-channels/` following `_template.md`'s seven
  sections. Enforced by `scripts/check_event_inventory.sh` (warning mode),
  `tests/infra/test_check_event_inventory.sh`, and — for the Consumer column —
  `gui/src/__tests__/eventChannelConsumerCoverage.test.ts`.
- **`GuiState` classification (INV-GUI-1).** `gui-state-sync.md` §5 carves event
  *streams* (naming `solver-progress` explicitly) out of INV-GUI-1's field-coverage
  scope. A telemetry **stream** therefore needs no `GuiState` field. Any telemetry
  **snapshot** field added here (the shape of the existing
  `fea_convergence: Option<FeaConvergenceInfo>`, classified
  `full_reload_only("fea-convergence-changed emitter")`) **must** carry a
  classification token in the `gui_state!` invocation — a field with no token
  matches no muncher arm and fails to compile — and must update the
  `fully_populated_gui_state()` fixture whose key-order test pins the wire format.
- **INV-GUI-2.** `solve_report` is **read-only**. It mutates no engine state and so
  does not trip the delta choke-point requirement. This PRD states that explicitly
  because #5100 (pending) is the architecture test that will enumerate write tools.

### §4.4 — C4: the debug-MCP tool obeys the four-edit rule

Adding `solve_report` requires **four** coordinated edits, not the three the
contract doc lists:

1. a `ToolDef { name, description, input_schema }` in `tool_defs()`
   (`gui/src-tauri/src/debug_server.rs`);
2. a named arm in `dispatch_tool()` — `solve_report` is engine-side, so it does
   **not** fall through to the frontend;
3. *(n/a for an engine-side tool)* a `buildHandlers()` entry in
   `gui/src/debug/bridge.ts`;
4. **both** cross-language allowlists: `PURE_ENGINE_SIDE` in
   `gui/src/__tests__/debugParity.test.ts` and `KNOWN_DEBUG_TOOL_NAMES` in
   `gui/test/visual/assertions.ts`.

Step 4 is **undocumented** in `docs/debug-mcp-contract.md` §1 and is the one that
silently bites; λ's diff must carry it, and λ additionally fixes the contract doc.

The handler follows the house split — a `solve_report_on_engine(engine: &Arc<Mutex<EngineSession>>)`
core plus a thin `handle_solve_report(state)` wrapper, so routing is unit-testable
without a `DebugServerState` — and reaches the engine **only** through
`run_on_engine` (OCCT's `blocking_send` panics inside a tokio runtime, and deep
compiles need the 256 MiB stack). Reaching `CheckResult` requires a new `pub(crate)`
observational accessor on `EngineSession`, precedented by `EngineSession::engine()`
whose rustdoc scopes it to "OBSERVATIONAL reads only … to the debug-MCP projections".

### §4.5 — C5: the record keys on codes, never on message text

Every renderer dispatches on `DiagnosticCode`, never on message substrings. Two
existing violations are in this PRD's direct blast radius and are fixed by γ:
`"Constraint solver made no progress: {reason}"` and `"Parameter `{}` resolved via
auto(free) -- result is not uniquely determined."` both carry **no**
`DiagnosticCode` today, and they are the two most legibility-relevant messages the
solver emits. Related hazard, not owned here: #4871's optimality gate keys on
`reason.contains("iteration limit")`.

---

## §5 — Boundary-test sketch (B + H, two-way)

The φ integration-gate leaf names this table as its observable signal. Every row
drives **real producer output through the real consumer** — the discipline whose
absence produced §2.2's casing defect.

| # | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|
| B1 | Backend→frontend verdict fidelity | A fixture with one satisfied, one violated and one indeterminate constraint | A real `build_constraints` payload driven through the `engineStore` reducer yields badge glyphs ✓/✗/? in that order, and `StatusBar` counts `1/1/1`. Fails today on all six assertions. |
| B2 | Verdict parity CLI↔GUI | Same fixture | The GUI's per-constraint verdicts equal `reify check`'s status lines, constraint-for-constraint. |
| B3 | Eval diagnostics reach the panel | A model whose objective solve hits the iteration limit | `W_SOLVER_OPTIMALITY_UNPROVEN` appears in `DiagnosticsPanel` with its `DiagnosticCode`, is filterable by the `solve` source chip, and is counted by `StatusBar`. |
| B4 | Slack honesty | A model with an active inequality bound and a pinned equality | The inequality row shows a finite slack in display units; the equality row shows `n/a`, never `0`. |
| B5 | Provenance parity CLI↔GUI↔MCP | A multi-objective fixture with an inherited objective | `reify explain`, the `PropertyEditor` chip and `solve_report` report the same governing objective, combination and `source` token for every auto. |
| B6 | Cold/warm provenance parity | An objective-bearing ≥2-auto model, loaded then edited | Provenance is present and equal after a cold load and after a warm `set_parameter`. **Depends on P1 κ**; before κ the warm half is structurally empty. |
| B7 | Optimality honesty | Any production objective solve | The record reports `BestFound` with its reason. It never reports `ProvenOptimal`, which is unreachable in production (§12). |
| B8 | Refusal is visible | A model with a constraint the registry declines at recognition | The refusal is visible in the GUI with its typed cause, not only on stderr, and `solve_report` carries the same cause. Vocabulary from #6659; not re-implemented here. |
| B9 | Stale-served is marked | A warm-cached solve served without recompute | The GUI marks the result stale and `solve_report` carries the staleness axis. **Depends on P1 α.** |
| B10 | MCP↔GUI agreement | Any solver-bearing fixture | `solve_report`'s per-auto and per-constraint arrays equal what the panels render, asserted by a committed `ValueScenario` against a live `reify-gui`. |

---

## §6 — Resolved design decisions

**D1 — Route to the panels that already own each shape; add one summary chip; no
new docked panel.** (Leo, 2026-08-26.) The GUI already has five solver-adjacent
surfaces and three of them are dead. Building a sixth panel would leave the dead
ones dead and duplicate what `ConstraintPanel` and `PropertyEditor` already display.
Verdicts and slack go to `ConstraintPanel`; per-auto provenance joins the existing
chips in `PropertyEditor`; eval diagnostics go to `DiagnosticsPanel` under a new
`solve` source chip; iteration/residual goes to `SolverProgressOverlay`; and a
single solve-summary chip in `StatusBar` answers "how did the solve go?" at a glance
and drills into the owning panel. *Rejected:* a docked "Explain" panel (duplicates
two existing panels, leaves three dead surfaces dead); pure distribution with no
chip (nothing answers the dogfood question in one place); editor inlays (largest new
machinery; density limits on real models; competes with lint gutters — revisit as a
follow-on once the record exists).

**D2 — `explain` is the single noun across all three surfaces.** `solver-progress`
and "auto-resolve" already denote the FEA CG solver and the FEA auto-resolve loop
respectively (§2.4). Minting a third sense of "solver" would be a legibility defect
in a legibility PRD.

**D3 — #4388 splits: it keeps the ledger, this PRD takes every renderer.** (Leo,
2026-08-26.) #4388 retains the engine-side DOF ledger, the minimal conflict set and
the `reify explain` rendering; this PRD owns the GUI DOF badge and its MCP
projection. This matches the producer/renderer architecture the rest of the PRD
follows, and avoids a fourth contested-ownership pair. A companion correction leaf
(ν) rewrites #4388's description to drop the GUI badge and point here — Leo
authorised that edit from the authoring session.

**D4 — Fix the wire contract on the Rust side, and pin it with a two-way test.**
The casing defect can be closed by lower-casing the Rust tokens or by normalising in
`bridge.ts`. Lower-casing Rust is chosen: it matches all six frontend consumers and
the entire existing fixture corpus, so no frontend test changes meaning. But the
*defect* was never the casing — it was that nothing drove real producer output
through the real consumer. B1 is therefore the load-bearing half of β, and the
casing change alone would not discharge it.

**D5 — `solve_report` is a new tool, not an extension of `engine_state`.**
`engine_state` is already a large projection and is consumed by unrelated harnesses;
a named twin of `reify explain` is the clearer contract and keeps the parity story
legible. It is read-only (§4.3).

**D6 — The record is produced by widening existing types, not by a parallel
channel.** Widening `CheckResult`/`BuildResult` to carry `objective_provenance` is a
single six-line site that un-gates provenance for the entire `check()`/`build()`
family, including the GUI's load path. A sidecar telemetry channel would duplicate
the narrowing rather than fix it.

**D7 — The robustness-floor opt-in uses the special-form precedent, not new
grammar, and lands last.** `solver.rs`'s own rejected-alternatives block records
*"Rejected: an opt-in `robust` keyword on `minimize` — needs grammar changes"*, and
`cost_robustness_tradeoff(expr, λ)` with its `ObjectiveSet.cost_robustness_lambda`
marker field is the shipped, grammar-free precedent. A `#robust` pragma was probed
and **does** parse (`tree-sitter parse --quiet`, 0 ERROR nodes, exit 0, both bare
and with `key=value` args) and remains a viable alternative if a declarative
scope-level posture is preferred at implementation time — but the special-form route
is the house pattern and is the default. See §7 for the #5711 pre-condition that
makes this the last phase.

**D8 — Staleness UX is pattern-shared, not owned.** `gui-on-demand-measurement.md`
§7 rules that the stale-render + re-apply affordance is implemented per-PRD, with
convergence on a shared frontend component "tactical for whichever lands second".
This PRD's staleness marker (P1's I5 axis) is a third instance and adopts that
posture verbatim rather than claiming the component.

---

## §7 — Pre-conditions for activating

| Pre-condition | Why | Status |
|---|---|---|
| **P1 `solver-driver-parity.md` committed** | Determines what `check`/`build` compute and report — the *content* this PRD surfaces. | ✅ committed 2026-08-26 |
| **P1 κ** (warm and edit paths choose the same solve entry point as cold) | Without it the GUI's *edit* path has structurally empty provenance and never surfaces `W_SOLVER_OPTIMALITY_UNPROVEN`. B6 and the edit half of η are gated on it. | hard prereq, real edge |
| **P1 α** (`ResolutionProfile`, the typed budget/staleness axes) | The solve-summary chip renders these axes; B9 asserts the stale marker. | hard prereq, real edge |
| **#6653 toleranced verdicts** | Without it, correctly-solved models report false-VIOLATED and this PRD would render a *wrong* verdict more prominently than before. Already a P1 hard prereq. | hard prereq, real edge |
| **#4388** (DOF ledger + conflict sets) | ι renders its ledger; ι is gated on it. Its own 7 deps remain its own. | hard prereq for ι only |
| **#5711 uniqueness contract** | Un-gating the robustness floor also un-gates the clamp box, which is blocked on #5711's ruling (in progress, contested, three review rounds). μ is phased last and is the only leaf affected. | hard prereq for μ only |
| **#6659 typed per-constraint refusal** | B8 renders its vocabulary. In progress; wording may still move. This PRD consumes, never re-implements. | soft — align vocabulary at implementation |

---

## §8 — Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `solver-driver-parity.md` (P1) | **consumes** | `ResolutionProfile`; warm/edit solve-entry convergence (κ); the one-sided-underdetermined coded diagnostic (ε) | P1 owns the drivers; **this PRD owns every renderer** | committed; edges wired at decompose |
| `constraint-solver-completion.md` | **consumes** | `ObjectiveProvenance`; its §10 names "a future surface PRD" as the declared consumer | **this PRD** discharges it | shipped producer |
| `gui-on-demand-measurement.md` | **produces for** | its measured verdicts route through the same `build_constraints` payload and break on the same casing defect; it also ships sibling debug-MCP tools; and its β independently widens `ConstraintCheckEntry` alongside this PRD's ε | **this PRD owns the casing fix + the verdict wire contract**; it owns measurement semantics | active — **edge wired**: its β #6741 depends on β #6723. See §8.1 |
| #4388 geometric-relations θ | **consumes** | DOF ledger + conflict sets | **split (D3)**: #4388 keeps ledger + `reify explain`; this PRD takes the GUI badge; ν rewrites #4388 | pending, degenerate branch |
| `ai-native-editing.md` / #5097 | **shares files** | `tool_defs()`, `dispatch_tool`, `debug-mcp-contract.md`, the parity tests | co-owned hub files; the PRD serialises under file locks by design | pending — declare, don't contest |
| `debug-server-gating-boundary.md` (#6351/#6352) | **adjacent** | relocation of `tool_defs()` into an ungated `debug_protocol.rs`; post-relocation clause C2 forbids inline `#[test]` in `debug_server.rs` | that PRD | pending; λ states which layout it targets (§13 Q2) |
| `reify-debug-mcp-expansion.md` | **adjacent** | the inspection/interaction tool families | that PRD (discharged; 14/15 leaves done despite a stale `Draft` status) | solver telemetry is outside its §0 scope boundary |
| #6654 budget hygiene | **consumes** | loud iteration exhaustion, seed-fallback loudness | #6654 | pending — render, don't own |
| #6649 cross-sub refusal | **consumes** | INV-SF-4 verdict pinning, `W_UNDERDETERMINED` wording | #6649 | pending — align vocabulary |
| #6659 typed per-constraint refusal | **consumes** | decline-at-recognition vocabulary | #6659 | in progress |
| #6646 GUI viewport at-auto pose | **sibling** | it explicitly defers "surface the solve failure" to #4388/#5415/#5420 | #6646 owns the pose solve; **this PRD claims the surfacing sliver** for solve outcomes | pending |
| `constrained-2d-sketch.md` #5509 | **adjacent** | a **second** DOF ledger (sketch), distinct from #4388's relate ledger | that PRD | pending — §11 names which ledger the badge renders |
| P3 `solution-set-completeness.md` | **consumes** | found-basins verdicts — a **named, reserved slot only** (§4.1), populated by no P4 leaf. Its vocabulary is α **#6706**'s `Completeness` carrier (`Exhaustive \| Partial{reason} \| Refuted{narrowing}`); the first leaf that populates it and makes a solution set reach any surface is ζ **#6711**. Cited by id in §4.1, §11 and §12.5 — not as "a future PRD" (INV-SF-5) | P3 | decomposed `d8bf51af15` (#6706–#6719); slot stamped by **#6751** |
| P3 ζ **#6711** ↔ P4 ξ **#6733** | **co-tenants on `reify explain`** | both extend `ObjectiveProvenance` and both edit `cmd_explain` in `crates/reify-cli/src/main.rs`. #6711 adds and renders the completeness field; ξ renders the P4-owned fields only. No dependency edge in either direction | jointly; the field is **#6711**'s | resolved here — §8.2 |
| #5975 "no optimum exists" | **consumes** | open-interval / infimum-not-attained diagnostic | #5975 | pending — render when it lands |

### §8.1 — The `gui-on-demand-measurement.md` seam, resolved

That PRD decomposed on 2026-08-26 (tasks #6740–#6748). Two concrete couplings, resolved
here rather than left to discovery at dispatch:

1. **The verdict wire contract — a real dependency, edge wired.** Its β (#6741) delivers
   "measured verdicts … riding the existing `build_gui_state`-shaped constraint payload",
   and its signal is that "the constraint-panel state show **measured** verdicts matching
   `reify check`". That signal is unachievable while every badge renders `?`, so **#6741
   now depends on β #6723**. Its own A8 amendment is devoted to `build_constraints`' two
   construction paths — the same function — but did not notice the casing defect, which is
   precisely why the edge is worth stating rather than assuming. *(Corrected 2026-08-27:
   "did not notice" is wrong — that PRD's A11, same manifest, is entirely about the casing
   defect and homes the fix; ownership since ruled to this PRD's β #6723 by `ffe203f338`.)*

2. **`ConstraintCheckEntry` — a collision, deliberately NOT an edge.** Its A5 amendment
   (which it calls "the largest unplanned-work risk in the batch") needs a structured
   measured-value channel on `ConstraintCheckEntry`; this PRD's ε (#6722) adds `margin`
   to the same struct. Neither needs the other's field, so **no dependency is wired in
   either direction** — inventing one would be a spurious edge, and the orchestrator's
   file locks already serialise the edits. The risk is not ordering, it is *divergence*:
   two independent per-constraint value channels on one struct. A coordination note is
   recorded on #6722's `details` asking whichever lands second to extend the first's shape.
   Both are bound by the same constraint: a sibling field on `ConstraintCheckEntry`, never
   a payload on `Satisfaction`, whose `content_hash` feeds the incremental cache key.

**No new contested-ownership pair is introduced.** The three known pairs
(`persistent-naming-v2 ↔ multi-kernel`, `imported-field-source ↔ multi-kernel`,
`topology-selectors ↔ persistent-naming-v2`) are untouched.

**Collision noted, not resolved here.** P1 §8 records that #6608 and #5403 hold
opposing designs for `reify check`'s `Severity::Error` exit gate with no edge
between them. This PRD depends on neither and does not pick a winner. *(Resolved 2026-08-26: #5403 delivers the gate; #6608 depends on it (edge wired) and contributes only UndefCause + new codes.)*

### §8.2 — The `reify explain` co-tenancy with P3 ζ, resolved

P3 §7.1 left this seam open in terms: *"Which leaf adds the field and which renders it
is left to **#6751** to decide and write down — neither DAG holds the seam today."* It is
ruled here, and the ruling is not a coin-flip — it writes down what both sides already
imply.

1. **Which leaf adds the field: P3 ζ #6711.** Its own DELIVER item 4 is "Extend
   `ObjectiveProvenance` and render the set + verdict in `reify explain`", and P3 §7's
   `constraint-solver-completion.md` row already assigns P3 "the added field and its
   rendering". **P4 does not add it**, and §4.1's slot is reserved precisely so that this
   PRD can name the field without claiming it.

2. **Which leaf renders it: #6711, on the CLI.** P4 ξ **#6733** renders the P4-owned
   fields only — `scope`, `term_contributions`, the slack lines, the infeasible-case
   message, the `source` token, and the stale `engine_eval.rs:3884` anchor fix. That set
   is **disjoint** from the completeness verdict, and ξ must
   **neither add nor re-render the completeness field**, which is #6711's.
   If #6711 has landed at ξ's dispatch, ξ leaves its rendering alone.

3. **No dependency edge in either direction** — the same ruling §8.1 item 2 made for
   `ConstraintCheckEntry`. Neither leaf needs the other's field, so inventing an edge
   would be spurious, and the orchestrator's file locks already serialise two edits to one
   function. **The risk is not ordering, it is divergence:** two independent renderings of
   provenance in one `cmd_explain`. Whichever lands second extends the first's shape rather
   than adding a parallel channel.

4. **The ruling is echoed on #6733's `details`** so the leaf's own dispatch context
   carries it — exactly as §8.1 item 2 records a coordination note on #6722's `details`.
   A ruling that lives only in a PRD is one the dispatching agent never reads.

---

## §9 — Sketch of approach

Four movements, in dependency order:

1. **Un-drop the record.** Widen the `EvalResult → CheckResult → BuildResult`
   narrowing to carry `objective_provenance` (α). Key per-inequality slack by
   `ConstraintNodeId` and carry it on `ConstraintCheckEntry` (ε).
2. **Repair the surfaces that already exist.** Fix the verdict wire contract and pin
   it two-way (β). Route eval diagnostics into the Diagnostics panel under a new
   `solve` source tag (γ). Repair `AutoResolvePanel`'s lifecycle and stop it
   pretending to draw charts it has no data for (δ).
3. **Render.** Slack + verdicts in `ConstraintPanel` (ζ); provenance chips in
   `PropertyEditor` (η); the solve-summary chip in `StatusBar` (θ); the DOF badge
   (ι); a constraint-solver producer on the existing `SolverProgressSink` pipeline
   plus the overlay's visibility fix (κ); `solve_report` on the debug MCP (λ); the
   dropped fields and slack in `reify explain` (ξ).
4. **Close the loop.** The two-way integration gate (φ), the author opt-in to the
   robustness floor with its docs-truth leaves (μ, ο, π, ς), the #4388 correction
   (ν), and the PRD-close stamp (ω).

---

## §10 — Decomposition plan (task IDs assigned 2026-08-26)

**Phase 1 — un-drop the record.**

- **α (#6721) — Carry `objective_provenance` through `CheckResult` and `BuildResult`.**
  Modules: `reify-eval` (`lib.rs`, `engine_constraints.rs`, `engine_build.rs`).
  Widen the two result structs and thread the field through the six-line narrowing.
  *Intermediate* — unlocks η, λ, ξ. Signal: unlocks η's user-visible chip; pinned by
  a Rust test asserting a cold `check()` on a multi-objective fixture returns
  non-empty provenance (today: structurally impossible — no field exists).

- **ε (#6722) — Key per-inequality slack by `ConstraintNodeId`; carry it on
  `ConstraintCheckEntry`.** Modules: `reify-constraints` (`solver.rs`), `reify-ir`
  (`constraint.rs`), `reify-eval` (`engine_constraints.rs`). Change
  `collect_slack_terms`'s output to carry the id; **land the same op-rule change in
  `collect_floor_terms` and `derive_from_expr` in lockstep** (the in-crate
  three-member pact) and in the cross-crate mirror
  `engine_eval.rs::has_inequality_slack`. Add `margin: Option<f64>` to
  `ConstraintCheckEntry` (a sibling field, **not** a payload on `Satisfaction`,
  whose `content_hash` feeds the incremental cache key). Evaluate at the post-solve
  value map inside `check_constraints_against_templates`. *Intermediate* — unlocks
  ζ, ξ, λ. Signal (**restated 2026-09-03, see Amendment A1**): a Rust test drives a
  model with one active `>= 2mm` inequality bound and one pinned equality through the
  **eval/solve path** and asserts the escaping `ConstraintCheckEntry` carries a
  populated `margin` for the inequality and `None` for the equality — structurally
  impossible today, the field does not exist. The renderers are downstream and already
  owned: ζ (`ConstraintPanel` slack column), ξ (`reify explain` slack lines), λ
  (`solve_report`). Printing slack in `reify check` **stdout is deliberately NOT this
  leaf's signal** (A1). Depends on #6653.

**Phase 2 — repair the dead surfaces (integration-gate cluster).**

- **β (#6723) — Verdict wire contract: fix the casing and pin it two-way.** Modules:
  `gui/src-tauri` (`engine.rs`), `gui/src-tauri/src/tests`, `gui/src/__tests__`.
  Emit lower-case tokens from both `build_constraints` sites (D4); add a Rust serde
  roundtrip test pinning the exact tokens; add the **B1 two-way boundary test**
  driving a real backend payload through the `engineStore` reducer and asserting
  badge glyphs and `StatusBar` counts. *Leaf.* Signal: loading a fixture with one
  satisfied, one violated and one indeterminate constraint shows ✓/✗/? and
  `StatusBar` reads `1 / 1 / 1` — today all three render `?` and the counts read
  `0 / 0 / 3`. Asserted via `scripts/gui-test.sh` plus a live-GUI debug-MCP check.

- **γ (#6724) — Eval diagnostics reach the GUI under a `solve` source tag.** Modules:
  `gui/src-tauri` (`engine.rs`), `gui/src` (`App.tsx`, `panels/diagnosticsView.ts`,
  `panels/DiagnosticsPanel.tsx`). Add `build_check_diagnostics()` reading
  `last_check().diagnostics`; add the third `source` tag and its filter chip. Also
  add `DiagnosticCode`s to the two uncoded solver warnings (§4.5). *Leaf.* Signal:
  loading a model whose objective solve hits the iteration limit shows
  `W_SOLVER_OPTIMALITY_UNPROVEN` in the Diagnostics panel, filterable by the `solve`
  chip — today no eval-time diagnostic reaches the GUI at all.

- **δ (#6725) — `AutoResolvePanel` lifecycle: mount on data, render honestly.** Modules:
  `gui/src` (`stores/engineStore.ts`, `panels/AutoResolvePanel.tsx`, `App.tsx`).
  Mount on `iterations.length > 0` rather than the synchronously-closed `active`
  flag; stop clearing `iterations` on complete; render a single-sample state
  honestly instead of an empty chart frame. *Leaf.* Signal: an FEA auto-resolve model
  leaves a readable panel after the loop completes — today it flashes for at most one
  frame and its two charts are structurally undrawable. Its real multi-sample stream
  arrives with κ.

**Phase 3 — render the record.**

- **ζ (#6726) — `ConstraintPanel`: verdicts + slack column.** Modules: `gui/src-tauri`
  (`engine.rs`), `gui/src` (`panels/ConstraintPanel.tsx`). Carry `margin` on the
  constraint payload; render a slack column showing `n/a` for equalities (C2).
  *Leaf.* Signal: a model whose `minimize` parks on an active clearance bound shows
  that constraint at slack `0.00mm` while a slack-bearing sibling shows its real
  margin. Depends on β, ε.

- **η (#6727) — `PropertyEditor`: per-auto provenance chip.** Modules: `gui/src-tauri`
  (`engine.rs`, `types.rs`), `gui/src` (`panels/PropertyEditor.tsx`, `types.ts`).
  Beside the existing determinacy/freshness/undef chips. *Leaf.* Signal: a
  multi-objective fixture shows, per resolved auto, the governing objective, the
  combination and the `explicit` / `synthetic-centrality` / `inherited` source token,
  agreeing with `reify explain`. Depends on α; the **edit-path** half depends on P1 κ.

- **θ (#6728) — `StatusBar` solve-summary chip.** Modules: `gui/src`
  (`panels/StatusBar.tsx`, `App.tsx`). One chip: optimality status, budget-exhaustion
  and stale-served markers, colour-coded; click drills into the owning panel. *Leaf.*
  Signal: a budget-exhausted solve shows a distinct chip state and clicking it
  reveals `W_SOLVER_OPTIMALITY_UNPROVEN` in the Diagnostics panel. Depends on β, γ;
  the staleness axis depends on P1 α.

- **ι (#6730) — DOF badge in `StatusBar`.** Modules: `gui/src`
  (`panels/DofBadge.tsx`, `DofBadge.module.css`, `panels/StatusBar.tsx`,
  `panels/index.ts`, `__tests__/DofBadge.test.tsx`). Renders #4388's ledger. *Leaf.*
  Signal: an under-constrained `at auto` sub shows `spent 5 · free 1` in the badge,
  agreeing with `reify explain`'s ledger line. Depends on #4388 (ν already executed).
  **Honesty constraint:** `SystemBuilder::solve`'s empty-constraint early return of
  `dof: 0` is a known lie for sketches (libslvs reports an honest `dof: 4` for two
  free 2D points); the badge must not render that zero as a fact.

- **κ (#6731) — Constraint-solver producer on `SolverProgressSink`; fix the overlay's
  visibility.** Modules: `reify-eval` (`solver_progress.rs` seam), `reify-constraints`
  (`solver.rs`), `gui/src` (`stores/engineStore.ts`, `panels/SolverProgressOverlay.tsx`).
  Emit `solver_kind: "nelder-mead"` per iteration on the existing pipeline; resolve
  the 1 s debounce racing `evaluation-status: idle`; either populate `eta_ms` or
  remove the dead render. Note `reify-constraints` does not depend on `reify-eval`,
  so the producer needs the seam threaded rather than a direct call. *Leaf.* Signal:
  a multi-auto objective solve draws a live residual trace in the overlay — today the
  constraint solver emits nothing on this channel and the overlay is almost never
  visible. Adds/extends a channel → carries its `docs/gui-event-channels/` spec and
  inventory rows in the same commit (C3).

- **λ (#6732) — `solve_report` debug-MCP tool.** Modules: `gui/src-tauri`
  (`debug_server.rs`, `engine.rs`), `gui/src/__tests__/debugParity.test.ts`,
  `gui/test/visual/assertions.ts`, `gui/test/fixtures/`, `docs/debug-mcp-contract.md`.
  Engine-side tool per C4's four edits; new `pub(crate)` observational accessor for
  `last_check()`. *Leaf.* Signal: a committed `ValueScenario` in `VALUE_SCENARIOS`
  opens a solver-bearing fixture, calls `solve_report`, and asserts per-auto
  provenance and per-constraint slack by dotted path — **green via
  `npm --prefix gui run test:e2e` against a live `reify-gui`, not in CI** (§12).
  Depends on α, ε.

- **ξ (#6733) — `reify explain`: the dropped fields, slack, and a failure vocabulary.**
  Modules: `reify-cli` (`main.rs`), `crates/reify-cli/tests/harness_cli/cli_explain.rs`,
  `crates/reify-cli/tests/fixtures/`. Render `scope` and `term_contributions`; add
  slack lines; give the infeasible case its own message instead of reusing
  `No objective provenance recorded` (which today is indistinguishable from "this
  model has no autos"); add a `source` token for the feasibility-only case, which
  today is mislabelled `explicit`. Fix the stale `engine_eval.rs:3884` anchor in
  `cmd_explain`'s own rustdoc. *Leaf.* Signal: `reify explain` on an infeasible model
  prints a distinct diagnosis, and on a multi-term objective prints each term's
  realised contribution. Depends on α, ε.

**Phase 4 — docs-truth for the legibility surface, then the integration gate.**

- **ο (#6735) — Docs-truth for the observable legibility surface.** Modules:
  `crates/reify-mcp/src/tools/chunks/`, `.claude/skills/reify-design/SKILL.md`,
  `docs/reify-language-spec.md`. The docs-truth gate fires on **diagnostics** and on
  **GUI behaviour a design session relies on**, so it is not μ's alone: this leaf
  documents the new `DiagnosticCode`s (γ), `reify explain`'s changed output and new
  failure vocabulary (ξ), and where a designer reads slack, provenance and
  optimality in the GUI. Every documented signature verified against the compiler
  arms/registries. **Deliberately not downstream of μ** — μ is gated on the
  contested #5711 and must not block the main documentation. Coordinate with P1 ψ,
  which sweeps the same chunk directory for driver claims; this leaf owns the
  legibility rows only. *Leaf.* Signal: **discoverability acceptance** — an author
  who knows the goal ("why did my part stop here, and how much room is left?") but
  not the feature name finds slack and provenance from the chunks or the corpus
  index. Depends on γ, ζ, ξ.

**Phase 4b — the integration gate.**

- **φ (#6736) — Two-way solve-record parity gate.** Modules: a test target plus
  `gui/test/fixtures/` and `tests/prd-gate/fixtures/`. **Leaf — names the whole §5
  table as its signal.** Depends on β, γ, ζ, η, θ, κ, λ, ξ. Rows B6 and B9
  additionally depend on P1 κ and P1 α.

**Phase 5 — the robustness-floor opt-in (language surface; lands last).**

- **μ (#6737) — Author opt-in to the robustness floor for non-Money objectives.** Modules:
  `reify-ir` (`constraint.rs`), `reify-compiler` (`entity.rs`), `reify-constraints`
  (`solver.rs`), `reify-eval` (`engine_eval.rs`). Special-form route per D7: a marker
  field mirroring `cost_robustness_lambda`, compile-time typing with coded
  diagnostics. Thread `apply_robustness_floor` from the DSL — it exists as a
  parameter of `solve_core_with_sd_tolerance` but is hard-wired `true` by
  `solve_core`, and `ResolutionProblem` carries no field for it. **Edit the eval-side
  twin `engine_eval.rs::objective_is_money` and `scope_qualifies_for_robustness_floor`
  in lockstep** — their doc comments demand the two-sided edit. *Leaf.* Signal: a
  non-Money `minimize` with the opt-in resolves off the constraint boundary and shows
  its remaining slack in `ConstraintPanel`; without the opt-in it parks on the bound
  as today. Depends on ζ and on **#5711** (§7).
- **π (#6738) — docs-truth for the opt-in.** Modules: `crates/reify-mcp/src/tools/chunks/`,
  `examples/best_practices/` + `INDEX.md`, `.claude/skills/reify-design/SKILL.md`.
  Doc-chunk text for the opt-in with every signature verified against the compiler
  registries; a worked example that compiles under
  `crates/reify-compiler/tests/examples_smoke.rs`; the one-line cheatsheet index
  entry. *Leaf.* Signal: **discoverability acceptance** — an author who knows the
  goal ("stop my optimum parking on the clearance bound") but not the feature name
  reaches it from the chunks or the corpus index. Depends on μ.

**Phase 6 — corrections and close.**

- **ν (no task — executed at authoring time).** #4388's description was rewritten in
  the authoring session (Leo's explicit authorisation, 2026-08-26) to drop the GUI
  badge arm, name this PRD's ι as the badge's owner, and remove the five
  `DofBadge`/`StatusBar` entries from its `metadata.files` (18 → 13). Readback
  confirmed. It also carries forward the libslvs `dof: 0` honesty note. **No leaf is
  filed for this** — doing it at authoring time removes a coordination hop and lets ι
  depend directly on #4388. Recorded here so the decomposition is not read as
  incomplete.
- **ω (#6739) — PRD close.** Set the terminal `Status` marker, name the landed leaf IDs, add
  the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and apply the
  matching header to the capability manifest. *Leaf.* Signal: the committed header.
  Depends on every other leaf.

---

## §11 — Out of scope

- **The drivers themselves** — which surfaces run a solver, the `ResolutionProfile`
  type, cold/warm/edit convergence. P1 owns all of it; this PRD renders its output.
- **Solver numerics** — multistart quality, basin search, the classifier trap,
  GN/LM. P2 and the solver-internals batch.
- **Multimodality verdicts / found-basins honesty.** P3 owns the semantics. This PRD
  leaves a **named slot** in the record and nothing more — the reserved
  `completeness` field in §4.1, guarded by §4.2's fourth C2 bullet, populated by no
  P4 leaf. Per INV-SF-5 the slot names live owners rather than "a future PRD", and it
  names them one role at a time, in `solution-set-completeness.md`:

  - Vocabulary carrier for the slot: α **#6706** — the `Completeness` type
    (`Exhaustive | Partial{reason} | Refuted{narrowing}`) whose words the slot holds.
  - First leaf that populates the slot: ζ **#6711** — it dedups the candidate set by
    basin box, attaches the verdict, and renders it through `ObjectiveProvenance` /
    `reify explain`.

  §8.2 rules which of the two co-tenants on `reify explain` adds the field (#6711) and
  which does not (ξ #6733).
- **The DOF ledger, minimal conflict sets and `W_UNDERDETERMINED` extension** —
  #4388 (D3). This PRD renders the ledger; it does not compute it.
- **The sketch DOF ledger** (`constrained-2d-sketch.md` #5509) — a *second*,
  distinct ledger. The ι badge renders **#4388's relate/pose ledger only**; a sketch
  badge is that PRD's.
- **Typed per-constraint refusal semantics** — #6659. Rendered, not owned.
- **Toleranced verdicts** — #6653. A hard prerequisite, not absorbed.
- **Budget hygiene** (dim-scaled budgets, stagnation stop/restart) — #6654. This PRD
  renders exhaustion; #6654 fixes the budgets.
- **`reify check`'s Error-severity exit gate** — #5403 delivers it; #6608 layers its new codes on it (ruled 2026-08-26, §8).
- **Kernel-measured verdicts** (ReprWithin, GD&T `Conforms`, DFM) —
  `gui-on-demand-measurement.md`. This PRD owns the verdict *wire contract* they
  ride on; it does not measure anything.
- **`reify mcp-server`** — its deletion is ratified and owned by #6665. Solver
  telemetry goes on `reify-debug` only.
- **LSP hover/inlay rendering of provenance** — a fourth surface; deliberately not
  opened here. D1 rejected editor inlays for this pass.
- **Retiring `SolverProgressOverlay` in favour of a docked panel** — D1 keeps it.
- **Cross-machine determinism** — P1 §11 (T3).

---

## §12 — G6 premise-validity notes

Every claim below was verified at HEAD `2128c3692c` by execution or exhaustive grep.

- **No leaf may assert `ProvenOptimal`.** `SolverRegistry::production()` leaves the
  Logical slot `None` and CP-SAT — the only solver that emits `ProvenOptimal` — is
  unregistered (#5469 pending). **Every production objective solve returns
  `BestFound`.** B7 asserts this negatively rather than asserting an unreachable
  positive. A surface rendering "proven optimal" would be lying today.
- **No leaf may assert an iteration count.** No iteration count, wall-clock or
  exit-residual escapes `reify-constraints`: `SolveMeta` is private and one bit wide
  (`iter_limited: bool`). κ's signal is a *residual trace* from the existing
  per-iteration sink, not a count read off a result type.
- **No leaf may present `candidates[1..]` as "alternative designs".**
  `solver.rs` warns explicitly that they are **not deduplicated** — for a
  single-basin objective they may be K near-identical repeats. Alternatives are P3's
  subject; if any renderer ever shows them it must dedupe by resolved-value
  fingerprint first.
- **Slack is inequality-only and this is structural**, not an omission —
  `Eq`/`Ne`/`Or` have no well-defined signed interior slack. ε's signal asserts
  `n/a` for equalities, which is a *positive* assertion about honest absence.
- **The three §2.2 defects are asserted as observed failures, not as guesses.** The
  casing mismatch, the diagnostics dead-end and the panel lifecycle were each
  confirmed by reading the production feed and both consumers, and by confirming the
  absence of any normaliser. β/γ/δ's signals are therefore RED-test-safe: they will
  fail today for the stated reason.
- **No numeric accuracy threshold is asserted anywhere in this PRD.** Slack values
  are reported, never bounded; the only numeric postcondition is B4's `n/a`-vs-`0`
  distinction, which is categorical. G6 branches 1 and 2 do not fire.
- **`reify explain` is kernel-less** (`Engine::new(…, None)`), and `relate_solve`
  requires a kernel — so `at auto` **pose** autos produce zero provenance on the CLI
  today. ξ does not change this; pose legibility rides #6646 and #4388.

---

## §12.5 — G7 design-invariant walk

Walked against `docs/legibility/design-invariants.md` (reify's own families:
silent-failure INV-SF-1..7, angle-crossing INV-AD-1..4) plus the GUI rows in
`docs/invariants.md`. **No waiver is required**; each hit is resolved by design.

| Invariant | Hit? | Resolution |
|---|---|---|
| INV-SF-1 `undef-has-provenance` | yes | Every renderer shows the recorded `UndefCause` for an undef auto; `PropertyEditor`'s undef chip already does. This PRD adds no new path that can leave a cell undef. Note `reify check` still never enables `capture_undef_causes` — that is #5400's, not absorbed. |
| INV-SF-2 `error-severity-exits-nonzero` | yes (corollary) | A routine solve status is **not** Error-severity. `BestFound`, budget-exhausted and stale-served are Info/Warning; the θ chip's colour, not a severity, carries urgency. The GUI has no exit code; the corollary binds the *severity choice*, which C5 fixes. |
| INV-SF-3 `declared-intent-consumed-or-diagnosed` | yes | The record carries a "why this did not run" slot for a skipped or declined solve (P1's I5 marks it; this PRD renders it). No renderer may show a blank where a stage was skipped. |
| INV-SF-4 `indeterminate-attributable-transient` | yes | C2 forbids collapsing the tri-state; the record carries `Satisfaction` **plus** its typed cause, and B8 asserts the cause is visible. Vocabulary comes from #6659/#6649, not re-invented. |
| INV-SF-5 `placeholders-owned-and-loud` | yes | The P3 multimodality slot cites two **live tasks** by id — α **#6706** (the `Completeness` vocabulary carrier) and ζ **#6711** (the first leaf that populates it) — not "a future PRD", the blanket-escape pattern this invariant bans. Stamped in §4.1 (the reserved slot), §8 (the seam row + §8.2) and §11, and re-checked mechanically before the terminal stamp by ω #6739's two `p3-multimodality-slot-cites-*` manifest bindings — one per id, both `expect: present`, each anchored to §11's own wording for that id so dropping either cite reds the gate (a single `#6706\|#6711` alternation would not: it is satisfied by either id, anywhere in the file, including by this row). The invariant's normative rule (`docs/legibility/design-invariants.md`:145-155) is scoped to placeholders in **tracked source**, so this row applies its *posture* to a PRD-record slot by analogy — the same move `spec-conformance-suite.capability-manifest.yaml`:643 makes for test scaffolding — and is not a literal tracked-source hit. **Corrected #6751 (2026-08-27):** as first landed this row read "the citation is stamped at decompose" while no P3 id appeared anywhere in the document. |
| INV-SF-6 `diagnostics-carry-codes` | yes | C5: every renderer keys on `DiagnosticCode`, never message text. γ additionally **adds codes** to the two uncoded solver warnings, so the PRD reduces the violation count rather than building on it. |
| INV-SF-7 `parse-is-value-faithful` | no | No new grammar. D7's default is a special-form call; the probed pragma composes with no adjacency-sensitive rule. |
| INV-AD-1..4 (angle crossings) | no | Nothing here types a quotient as Angle; slack carries the constraint's own dimension. |
| INV-GUI-1 (every `GuiState` field has a declared sync mechanism) | conditional | Event *streams* are carved out by `gui-state-sync.md` §5. Any telemetry **snapshot** field added must carry a `gui_state!` classification token (C3) — enforced at compile time — and update the `fully_populated_gui_state()` fixture. |
| INV-GUI-2 (engine mutations flow through the delta choke-point) | no | `solve_report` is **read-only** and mutates nothing (C3). Stated explicitly because #5100 will enumerate write tools. |
| INV-GUI-3 (`.ri` is canonical truth for mutations) | no | This PRD adds no mutation path. |
| INV-META-1 (tasks cite the INV they serve) | n/a → obeyed | Each leaf citing an invariant carries it in its description at decompose. |

**One fragility this PRD touches but does not own.** ε extends a decomposition rule
that already exists in four places — `collect_slack_terms`, `collect_floor_terms`,
`derive_from_expr` (the in-crate "op-rule pact") and the cross-crate mirror
`engine_eval.rs::has_inequality_slack` — held together by prose cross-reference
comments. The cross-crate copy is deliberate: sharing would invert the
`reify-eval → reify-constraints` dependency. ε must join the pact explicitly rather
than adding a fifth silent copy, and its diff must touch all members in lockstep.
This is the `no-lockstep-duplication` shape from dark-factory's family; reify's own
family does not name it, so it is recorded here as an observation, not a waiver.

**One lesson this walk learned about itself.** The INV-SF-5 row above was, as first
landed, not a certification but a *promise*: its truth depended on a decompose-time
action ("the citation is stamped at decompose") that decompose never performed, and no
P3 id appeared anywhere in this document for it to point at. A promise in a landed
document reads as a fact — and worse, it reads as a fact that has already been checked,
because it sits in the column headed *Resolution*. The general rule: **where a G7 row
defers to a decompose-time action, the decompose-close step must verify the action
happened**, or the invariant walk certifies its own intention rather than the state of
the world. The concrete remedy is shipped with this correction — ω **#6739**'s
capability manifest now re-greps the citation immediately before it applies the terminal
status, so this specific row cannot rot back into a promise without ω's own gate failing.
The remedy carries a second-order version of the same lesson: the first draft of that
gate greped the bare alternation `#6706|#6711` over the whole file, which both ids
already satisfy from several sections — including this paragraph. A check that the
correction narrative alone keeps green is the promise defect one level up, so the gate is
now two checks, one per id, each anchored to §11's own wording rather than to a bare id.

---

## §13 — Open questions (tactical, deferred to implementation)

1. **Solve-summary chip states.** How many distinct visual states the θ chip carries
   (converged / best-found / budget-exhausted / underdetermined / refused / stale) vs
   collapsing some into one "needs attention" state. *Suggested:* three colours,
   with the precise cause on hover and on click-through. Decide during θ.
2. **Which `debug_server` layout λ targets.** #6351 relocates `tool_defs()` into an
   ungated `debug_protocol.rs`, after which clause C2 forbids inline `#[test]` in
   `debug_server.rs`. #6351 is pending and blocked on #6338. *Suggested:* target
   today's gated layout and put λ's schema test where C2 will want it, so the
   relocation is a move rather than a rewrite. Decide during λ.
3. **`eta_ms`.** Populate it from the residual history or delete the dead render.
   *Suggested:* delete in κ, file the estimator separately — it is unowned today and
   an ETA is a different design question from legibility. Decide during κ.
4. **`EvaluationStatus.phase = 'resolving'`.** The variant exists in both type
   mirrors and is never emitted, and `progress` is always `None` — a free,
   already-plumbed coarse-status channel. *Suggested:* use it for the θ chip's
   in-flight state rather than adding a channel. Decide during θ.
5. **Slack display units.** Per-constraint display-unit conversion vs raw SI in the
   record with conversion at the renderer. *Suggested:* SI in the record, conversion
   at each renderer, matching `build_parameters_payload`'s existing
   `format_display_triple` discipline. Decide during ζ.
6. **Whether ξ adds `--format json` to `reify explain`.** There is no
   machine-readable mode on any subcommand today, and `solve_report` covers the
   agent case. *Suggested:* no — keep the CLI human-facing; revisit if a
   non-GUI agent consumer appears. Decide during ξ. *(Superseded 2026-08-27: the
   premise is stale — driver-contract σ #6800 delivers a `--json` driver-result
   envelope, and driver-contract §8.4 names that PRD as the non-GUI agent consumer;
   the binding correction lives in #6733's details.)*
