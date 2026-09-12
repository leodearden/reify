# Capability manifest — GUI purpose surface

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/gui-purpose-surface.md`. Each binding maps a
leaf's asserted capability to **executed** evidence with a PASS verdict; any FAIL blocks the
batch. Authored at decompose time, 2026-08-26/27.

**Leaf task IDs:** stamped into the YAML sidecar
(`gui-purpose-surface.capability-manifest.yaml`) by `commit_planning`.

**Probe environment:** `target/debug/reify` (debug binary built 2026-08-26); `tree-sitter` from
`~/.cargo/bin`; committed fixture `tests/prd-gate/fixtures/gui_purpose_surface.ri`. Source
anchors read against main `da8091cbe8` (2026-08-26).

---

## Scope note — substrate vs deliverable, and one territory this PRD does NOT enter

Decompose-time verification asserts only the **assumed substrate** and the **baseline defects**
each RED signal repairs — never the leaves' own deliverables.

One correction is load-bearing and was found by adversarial review before filing: the
`cmd_check` measurement defect this PRD's investigation uncovered (§2.3) is **reserved to
`check-diagnostic-truthfulness.md`** by a binding G4 ruling, and **#5748 is `in-progress` and
claimed** on exactly that function. Leaf α was accordingly narrowed to a **behaviour-preserving**
seam extraction, wired downstream of #5748, and the residual defect is **filed to PRD 2** by
leaf ι rather than fixed here. *(Superseded 2026-08-27: the residue was adopted by
driver-contract α #6773 as an acceptance clause — `driver-contract-implementation.md` §8.3,
commit `1d487a7d1f`; ι files only if the defect escapes #5748/#6803/#6773.)* No binding in this
manifest asserts a change to `cmd_check`
routing, exit codes, `finish_check` or `--strict`.

## Numeric note (G6 branches 1 / 2)

**No accuracy-bound or exactness premise exists in this PRD.** Two numbers appear, neither a
tolerance this PRD asserts: the fixture's sampled facet deviation `6.006e-2 m` against a `1um`
bound (a **five-orders-of-magnitude** margin, chosen so the VIOLATED verdict cannot flip on
kernel or platform variation), and the `wall_margin` purpose's `500mm` threshold (a discrete
verdict flip at `wall=100mm` → `OK` vs `wall=700mm` → `VIOLATED`, verified by execution). Both
assertions are **discrete verdicts**, not float equalities. No floor exposure.

## Grammar evidence (anti-mismatch)

No novel syntax anywhere — this PRD exposes an existing language feature on a new surface. The
single committed fixture parses with **0 ERROR nodes** (`tree-sitter parse --quiet`, exit 0,
2026-08-27). `grammar_confirmed: true` for all ten leaves.

## Executed probes

All run against `target/debug/reify`, 2026-08-26/27.

| # | Probe | Observed |
|---|---|---|
| P1 | bare `reify check <fixture>` | `VIOLATED BallCheck#constraint[0]`, deviation `6.006e-2 m`, **exit 1** |
| P2 | `--purpose simulation_ready=Ball <fixture>` | `BallCheck#constraint[0]` → `INDETERMINATE`, **exit 0** |
| P3 | `--strict --purpose simulation_ready=Ball <fixture>` | **exit 1** — the exit-0 needs the non-strict policy too, so P2 is a *composition*, not a single cause |
| P4 | injected-verdict discriminator | `purpose:design_review@BallCheck#constraint[0]` |
| P5 | `--purpose wall_margin=Ball` (module-declared, let-bearing) | `OK` at `wall=100mm`; `VIOLATED` at `wall=700mm` |
| P6 | `RepresentationWithin` inside a purpose body — three binding shapes | **all INDETERMINATE** (`undefined inputs: <purpose>.subject`) — REFUTED |
| P7 | `--purpose nonexistent_purpose=BallCheck` | error + **exit 1** (rejection fires, not a silent accept) |
| P8 | `--purpose design_review` (no `=`) | **exit 1** |

P1–P3 are evidence for the finding leaf ι hands to PRD 2 *(superseded 2026-08-27: the residue is
adopted by driver-contract α #6773 per `driver-contract-implementation.md` §8.3; ι files only if
it escapes #5748/#6803/#6773)*. P5 is the staleness fixture's basis.
P6 is why BT-5 is a **differential** oracle rather than a measurement assertion. P7/P8 are the
G6 branch-4 rejection bindings, **observed to fire**.

---

## α — `activate_purpose_session()`, behaviour-preserving

*Out-of-batch producers: **#5748** (`in-progress`, claimed — rewriting the same function), **#6740** (`pending`). Both upstream.*

| Capability | Binding | Verdict |
|---|---|---|
| the seam has a home both callers can reach | `reify-cli` is bin-only (no `[lib]`) and `gui/src-tauri/Cargo.toml` does not list it; `reify-eval` **is** listed | PASS |
| every constituent call is an `Engine` method | `eval`, `activate_purpose*`, `check_constraints_with_values`, `run_gdt_check_passes` — all `reify-eval` | PASS |
| #5748 is upstream, not downstream | `status: in-progress`, `claimant_run_id` live, `heartbeat_at` within the hour; wired as a real edge | PASS — DAG-direction correct |
| the reserved territory is respected | `check-diagnostic-truthfulness.md`: *"`cmd_check`/`finish_check`/…/exit codes/`--strict` are **PRD 2 ONLY**"*; α asserts **no** change to any of them | PASS |
| both enumerations' obligations are known | template-walk runs `structural_query::expand_constraint_expr`; graph-walk overlays `active_purpose_let_cells` — the union C-SEAM must preserve | PASS |
| activation APIs are production-wired | `activate_purpose` / `activate_purpose_with_bindings` / `is_purpose_active`, consumed by `cmd_check` (#4000/#4006/#4009, `done`) | PASS |
| typed activation errors are needed and absent | `activate_purpose` returns `()` and is silent on four failure modes; `activate_purpose_with_bindings` returns free-form `Result<(), String>` | PASS (gap confirmed) |
| the equivalence lock has a corpus | 59 existing `--purpose` assertions across `cli_purpose.rs`, `cli_purpose_stdlib.rs`, `cli_gdt_legality.rs`, `cli_determinacy_intrinsics.rs`, `cli_check.rs` | PASS |
| `realize_for_check` caveat recorded | introduced by #5748, **absent on main** — D1 flags re-reading the landed set | PASS |

## β — GUI routes both paths through the seam; commands; intent

| Capability | Binding | Verdict |
|---|---|---|
| single GUI check choke point | `EngineSession::check_with_solve_slot` wraps `Engine::check`, called by all three full-recompile entry points | PASS |
| warm path already graph-walks | `Engine::edit_check` → `check_constraints_with_values` — the §2.4 disagreement | PASS |
| mutate→rebuild→delta→emit precedent | `set_active_fea_case` → `set_active_fea_case_impl` → `compute_delta` + `emit_delta` | PASS |
| command registration site | `tauri::generate_handler!` — 33 commands, none purpose-related | PASS |
| engine preserves bindings across recompile | `Engine::eval` snapshots `active_purpose_bindings` and re-applies them | PASS |
| a vanished purpose is silently dropped today | the re-apply path skips a binding whose purpose no longer exists, with no diagnostic — the INV-SF-3 defect β repairs | PASS (defect confirmed) |
| commit choke point exists for reconcile | `CoreState::commit_state`, the documented five-field atomic commit | PASS |
| the commands can reach their own read surface | β's modules include `commands.rs`, where `engine_state_json` lives | PASS |

## γ — Verdict partition + `purpose_applied_epoch`; legible rendering

*Out-of-batch producers: **#6742** (generation counter), **#6723** (verdict casing) — both `pending`, both upstream.*

| Capability | Binding | Verdict |
|---|---|---|
| partition key exists | probe P4 — `purpose:design_review@BallCheck#constraint[0]` | PASS |
| added fields need no new event channel | `GuiState.constraints` carries `diffed keyed(key=node_id, …)` | PASS |
| blank-expression defect is real | `build_constraints` matches `entry.id` against `compiled.templates[*].constraints` then `.unwrap_or_default()`s; a purpose-injected id belongs to no template | PASS (defect confirmed) |
| purpose rows do reach the panel (so γ's signal is not inverted on δ) | `build_constraints` iterates `check.constraint_results`, not templates — purpose rows appear (blank) as soon as verdicts flow | PASS |
| a pre-existing purpose-aware render path exists to reconcile with | `format_expr` has a live `CompiledExprKind::PurposeReflectiveAggregation` arm in `gui/src-tauri/src/engine.rs` | PASS |
| the counter is consumed, not rebuilt | **#6742** (`pending`) owns `EngineSession.generation`; upstream edge | PASS — DAG-direction correct |
| verdicts are readable at all | **#6723** (`pending`) fixes the casing mismatch rendering every badge `?`; upstream edge | PASS |
| no second per-constraint channel is opened | #6722 widens `ConstraintCheckEntry` with `margin`; this leaf widens `ConstraintData` with `purpose` — different structs, different fields; per #6722's own note, no edge | PASS (collision named) |

## δ — `set_purpose` debug-MCP tool

| Capability | Binding | Verdict |
|---|---|---|
| tool name is free in every source tree | repo-wide `set_purpose` hits only docs (the ruling, this PRD's own artifacts, and `driver-contract-implementation.md`) — zero source hits | PASS |
| name satisfies the extraction regex | `[a-z0-9_]+` per `toolDefNames.ts` | PASS |
| end-to-end tool pattern exists | `set_fea_case`: `ToolDef` → dispatch arm → handler → `run_on_engine` → frontend push | PASS |
| delta-baseline refresh available and mandatory | `set_fea_case_on_engine_and_refresh_baseline` → `compute_delta(last_state, &gs)` | PASS |
| engine work must leave tokio | `run_on_engine` → `spawn_on_large_stack` (OCCT panics under tokio) | PASS |
| four registration guards identified | `PURE_ENGINE_SIDE` · `KNOWN_DEBUG_TOOL_NAMES` · `toolDefNames` · `debugContract` | PASS |
| honest failure reporting is possible | α's typed errors, **upstream in-batch** | PASS |
| no tool-name collision with the sibling batch | #6741 lands `measure_constraints` / `measurement_status` — disjoint | PASS |

## ε — Purpose selector + entity-binding picker

| Capability | Binding | Verdict |
|---|---|---|
| declared purposes enumerable with no new API | `CompiledModule.compiled_purposes` public field; production walk in `reify-doc-build` | PASS |
| param shape is entity-ref-only | `CompiledPurposeParam { name, entity_kind }` — no value type, unit or default; corrects the charter brief's "bindings form" premise | PASS |
| prelude/user discriminator exists | `declaration_span.is_prelude()`, the same filter `reify-doc-build` uses | PASS |
| prelude purposes universally activatable | probe — `--purpose simulation_ready=Ball` against a file declaring no purpose | PASS |
| entity candidates sourceable | `get_entity_tree` / `get_entity_identity_map` | PASS |
| the frontend consumer is fed a constant today | `App.tsx:702` passes a hardcoded `[]`, making `auto:purpose:*` unreachable — ε's `expect: absent` check | PASS (orphan confirmed) |

## ζ — Ruled staleness UX (the purpose instance of the shared token)

*Out-of-batch producer: **#6743** (`pending`).*

| Capability | Binding | Verdict |
|---|---|---|
| the shared token has a filed owner | **#6743**, `pending`, `files_to_modify` includes `ConstraintPanel.tsx` + `.module.css` — upstream edge | PASS — DAG-direction correct |
| per-PRD-instance shape is ruled | `gui-on-demand-measurement` §7: *"each PRD implements its own instance…"* | PASS |
| an in-tree visual token exists | `DesignTree.module.css` `.stale { opacity: 0.5; font-style: italic }` — the only DOM stale style in the tree | PASS |
| badge styling hook exists | `ConstraintPanel.module.css` `.statusBadge[data-status="…"]` | PASS |
| retain-and-dim precedent carries no DOM CSS | probe staleness ships `stale: boolean` + a `Re-pin` button and a grey 3D marker; `.probe-stale` has **no** stylesheet rule | PASS (gap confirmed) |
| **the fixture can actually exercise staleness** | probe P5 — `wall_margin` is module-declared, let-bearing and verdict-sensitive to a warm-editable param (`OK` at 100mm, `VIOLATED` at 700mm) | PASS |
| **the vacuity that a measurement-bearing purpose would have introduced is refuted, not assumed** | probe P6 — `RepresentationWithin` in a purpose body is INDETERMINATE in all three binding shapes, so D7 claims only *not re-applied*, and BT-5 is a **differential** oracle (reapply ≡ fresh full pass) rather than a degradation assertion | PASS |
| the `stale` name collision is real and avoided | `EngineSession::is_stale()` == `last_reload_error.is_some()` (compile FAILURE), surfaced as top-level `"stale"` in `engine_state_json`; D8 forbids conflating it | PASS |
| affordance wording is this PRD's | ruling 4 says "reapply / recheck purpose"; #6743 says "Measure now" — distinct surfaces | PASS |

## η — Integration gate

| Capability | Binding | Verdict |
|---|---|---|
| fixture committed and parses | `tests/prd-gate/fixtures/gui_purpose_surface.ri`, `tree-sitter parse --quiet` exit 0 | PASS |
| fixture carries every scenario the BTs need | P1 (measurement control), P5 (declared warm-edit-sensitive purpose), prelude activation — all in one module | PASS |
| GUI-driving harness exists | `gui/test/visual/` over the `:3939/mcp` transport | PASS |
| Rust debug-tool test home exists | inline `#[cfg(test)] mod tests` in `debug_server.rs` | PASS |
| every BT capability is upstream | BT-1/BT-2←α, BT-3/BT-4/BT-6/BT-7/BT-11←β, BT-9←γ, BT-8←α+δ, BT-10←ε, BT-5←ζ — all in η's transitive closure | PASS (no inversion) |
| drift-guard registration carried same-diff | overlay gate-test rule; this leaf's OWN diff carries the bucket row and nextest entry | PASS |
| no overlap with the sibling gate | #6745 covers its own PRD's measurement-parity BTs; these are purpose-specific and share only the harness | PASS |

## θ — Docs-truth: the purposes chunk

*Out-of-batch producer: **#6746** (`pending`).*

| Capability | Binding | Verdict |
|---|---|---|
| scope is disjoint from #6746 | #6746's `files_to_modify` is `chunks/constraints.md`, `chunks/stdlib.md`, `SKILL.md`; this leaf owns `chunks/purposes.md`, which #6746 does not touch | PASS |
| the shared file is ordered, not raced | both touch `SKILL.md`; the edge θ ← #6746 makes this leaf **extend** the landed index line | PASS — DAG-direction correct |
| the false stdlib claim is real | the chunk lists `manufacturing_ready` under "Standard Library Purposes"; `determinacy_purposes.ri` defines only `simulation_ready` and `design_review`; `manufacturing_ready` occurs only under `examples/` | PASS (defect confirmed) |
| the chunk's reflective example is **not** vacuous | `geometric_params` / `material_params` resolved queries **are** populated (`traits.rs`, task-4137) — hazard checked and cleared | PASS |
| there is a real authoring trap to document | probe P6 — a purpose body cannot carry a working kernel-measured constraint; undocumented today | PASS |
| no exemplar-corpus leaf owed | purposes already have corpus presence; no new authoring idiom | PASS (documented exemption) |

## ι — Companion corrections and the handed-over finding

*(Corrected 2026-08-27: the first two rows' filing instruction is superseded — the residue was
adopted by driver-contract α #6773 as an acceptance clause, `driver-contract-implementation.md`
§8.3 / commit `1d487a7d1f`; ι re-verifies and files against driver-contract only if the defect
escapes all of #5748/#6803/#6773. See task #6838 for the revised instruction.)*

| Capability | Binding | Verdict |
|---|---|---|
| the residue this leaf files is real and survives #5748 | #5748 routes the purpose arm through `with_registered_kernel` + build but adds **neither** `set_capture_repr_tol(true)` **nor** `tessellate_realizations`; `dispatch_constraints` fast-paths past the ReprWithin interception while `achieved_repr_tol` is empty | PASS |
| filing, not rewriting, is the correct instrument | #5748 is `in-progress` and claimed; a *new* task in PRD 2's territory wired to #5748/#5403 touches no other session's record | PASS |
| `purposes-completion` follow-up is un-filed | its §10 names a `gui-purpose-activation` follow-up; no such task exists | PASS |
| survey D4 is a dated snapshot | appended as a **dated addendum**, never an edit to the snapshot | PASS |
| the reflective-aggregation note is stale | its "Remaining gap" section vs task-4137's landed resolution | PASS |
| the sibling PRD needs the consumer row | `gui-on-demand-measurement.md` still calls the purpose charter *"not yet filed"* | PASS |

## κ — PRD-close stamp

| Capability | Binding | Verdict |
|---|---|---|
| terminal vocabulary is closed | `SHIPPED` / `SUPERSEDED` / `WITHDRAWN`, first token after `Status`, case-insensitive | PASS |
| freeze-header shape has ratified exemplars | headers of `v0_6/data-carrying-enums.md` and `kernel-seam-contracts.md` | PASS |
| cancelled-sibling disposition is defined | a `cancelled` sibling counts as satisfied for κ's edge | PASS |

---

## Cross-cutting FAIL check

No binding resolves to `declared-only`, `test-only`, `producer-absent`, `producer-extent-short`,
`producer-downstream`, `fixture-ERROR`, `bound≤floor` or `rejection-absent`. The two rejection
assertions were **observed to fire** (P7/P8: exit 1, not a silent accept).

**Six out-of-batch producers — all non-terminal, all wired upstream:** #5748 (α, `in-progress`),
#6740 (α), #6742 (γ), #6723 (γ), #6743 (ζ), #6746 (θ). None is `done` or `cancelled`; none is
inverted.

**One deliberate non-edge:** `solver-legibility-telemetry` #6722/#6726 widen a *different* struct
with *different* fields — recorded as a collision on both sides rather than a spurious dependency.

**One premise refuted before filing, not after:** P6 killed the measurement-bearing-purpose
fixture design. D7 and BT-5 were rewritten to claim only what the engine supports, rather than
shipping a staleness test that would have asserted `stale` over provably fresh verdicts.
