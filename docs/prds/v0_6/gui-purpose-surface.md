# GUI purpose surface — activation, purpose-scoped verdicts, ruled staleness UX

**Status: ACTIVE** — chartered by Leo 2026-08-26 (driver-contract matrix ruling 4, spec-conformance program); authored 2026-08-26/27. Milestone v0_6.

**Code anchors** verified against main `da8091cbe8` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Approach: B + H** (contract + two-way boundary tests). Blast radius 4 crates/packages (`reify-eval`, `reify-cli`, `gui/src-tauri`, `gui/src`), mechanism count ≥ 8, and ≥ 2 cross-PRD consumers.

---

## 1. Goal

A designer working in the Reify GUI picks a declared purpose from a selector, binds its entity parameters, and sees the purpose's constraints appear in the constraint panel as purpose-scoped verdicts alongside the base ones. The purpose is **sticky**: it survives edits and recompiles until deselected. After a warm edit, every GUI element tied to that purpose renders **visibly stale** (dimmer/greyer) with a prominent **"Reapply purpose"** affordance, until reapplied.

The enabling mechanism, load-bearing beyond this PRD: the CLI's hand-assembled purpose activation sequence becomes a shared `activate_purpose_session()` seam in `reify-eval`, called by both `cmd_check` and the GUI `EngineSession`. `driver-contract-implementation.md` §8.1 declares a **hard** inbound edge on it for its leaf φ (the `--purpose` spread to the other CLI drivers).

**Scope discipline, ruled 2026-08-26/27:** the seam extraction is **behaviour-preserving**. This PRD changes no `cmd_check` routing, no exit code, no `finish_check`, no `--strict` semantics — all four are reserved to `check-diagnostic-truthfulness.md` by a standing binding G4 ruling (§7). A real defect this PRD's investigation uncovered in that reserved territory is **handed over as a finding**, not fixed here (§2.3).

## 2. Background — what exists today, measured

### 2.1 Purposes are invisible in the GUI

`EngineSession` has no `activate_purpose` / `is_purpose_active` / `active_purposes` method. The `invoke_handler` list (`gui/src-tauri/src/main.rs`) registers exactly 33 commands, none purpose-related. `set_purpose` appears in no source tree — **the name is free**.

One nuance worth carrying into leaf γ: `gui/src-tauri/src/engine.rs` is **not** purpose-blank. Its `format_expr` already has a live match arm for `CompiledExprKind::PurposeReflectiveAggregation { param_name, query_kind }`, with a comment noting that once `activate_purpose` runs the variant is replaced by a populated `ListLiteral`, so the GUI normally meets it only in pre-activation debug views. That is a pre-existing purpose-aware rendering path to reconcile with, not to rediscover.

This is cross-driver survey row **D4**: *"`--purpose` exists only on `reify check`, and since `engine.check()` skips purpose-injected constraints, every other driver (GUI, LSP, all other CLI, MCP) silently never checks them."*

### 2.2 …but the GUI already has a purpose consumer, fed a constant

`generatePurposeViews(tree, activePurposes)` (`gui/src/stores/autoViewGenerator.ts`) mints one `auto:purpose:<name>` view per active purpose. It is reached through `viewStateStore.regenerateAutoViews(tree, activePurposes = [], displaySubjects)`, whose **sole production call site passes a hardcoded empty array**:

```ts
// gui/src/App.tsx:702
viewStateStore.regenerateAutoViews(entityTree(), [], displaySubjects);
```

`generatePurposeViews` early-returns on `length === 0`, so `auto:purpose:*` views are unreachable in the running GUI; only vitest exercises them. A producer-orphan of the C-10 shape, live since #1745 closed — and a one-line, already-tested consumer waiting for an active-purpose list.

`purposes-completion.md` §3 recorded this and §10 deferred it: *"…is a separate GUI PRD … **Follow-up:** file a `gui-purpose-activation` task post-batch."* That follow-up was never filed. **This PRD is that successor.**

### 2.3 A defect found in reserved territory — reported, not fixed here

`cmd_check` forks on `--purpose`, and the two arms build different engines: the `--purpose` arm constructs a kernel-free `Engine::new(checker, None)` and runs none of the measurement arms (`set_capture_repr_tol`, the handle-populating `build()`, `tessellate_realizations`, `ensure_openvdb_kernel`, `measure_gdt_conformance`, the DFM error escalation).

**Executed evidence** (`target/debug/reify`, 2026-08-26/27, fixture `tests/prd-gate/fixtures/gui_purpose_surface.ri`; every run also emits `warning: constraint expression has type Ball, expected Bool`, elided here for width but present in the real output):

```
$ reify check <fixture>
  VIOLATED BallCheck#constraint[0]
  error: RepresentationWithin: sampled facet deviation 6.006e-2 m exceeds bound 1.000e-6 m for BallCheck
  → exit 1

$ reify check --purpose simulation_ready=Ball <fixture>
  OK purpose:simulation_ready@Ball#constraint[0..1]
  INDETERMINATE BallCheck#constraint[0]
  No constraints violated (1 indeterminate).
  → exit 0
```

**Mechanism, confirmed in source (not inferred):** `dispatch_constraints` fast-paths straight to the language-level checker when `achieved_repr_tol.is_empty()`, skipping the `RepresentationWithin` interception; `achieved_repr_tol` is populated only by `tessellate_realizations` under `set_capture_repr_tol(true)`, neither of which the `--purpose` arm calls. Both check bodies share `dispatch_constraints`, so the template-walk-vs-graph-walk difference is **not** the cause.

**Honest attribution** — three corrections to the first reading of this evidence:

- The exit-0 is a **composition**: dropped measurement **∘** the non-strict policy that treats `Indeterminate` as pass. `reify check --strict --purpose simulation_ready=Ball <fixture>` already **exits 1**. That second factor is explicitly owned, and explicitly left unchanged, by `check-diagnostic-truthfulness.md`.
- Only a **measurement-backed** violation flips. A plain violated constraint keeps its verdict and exit 1 under `--purpose`.
- The `"indeterminate: undefined inputs: …"` wording is a **pre-existing** `Indeterminate`-reporting shape, not one `--purpose` introduces — it appears on the bare path too for a struct-ref-param constraint.

**Ownership:** this is `check-diagnostic-truthfulness.md` territory under the binding G4 ruling (§7), and **#5748 is in flight on exactly it** — its own charter names *"(c) `--purpose` (:693-824): unconditionally `Engine::new(checker, None).eval(&compiled)` … Same defect as (a)"*. #5748 routes that arm through `with_registered_kernel` + build, but does **not** add `set_capture_repr_tol(true)` + `tessellate_realizations` — so the `RepresentationWithin` residue survives it. **Leaf ι files that residue as a finding against PRD 2.** This PRD fixes none of it.

### 2.4 The GUI's own two paths already disagree about purposes

`EngineSession`'s full-recompile path (`load_file` / `load_from_source` / `update_source` → `check_with_solve_slot` → `Engine::check`) resolves constraints by walking `module.templates[*].constraints` (`check_constraints_against_templates`). Its warm-edit path (`set_parameter` → `Engine::edit_check`) calls `check_constraints_with_values`, which walks `snapshot.graph.constraints`. Purpose constraints are injected into the **graph**. The two enumerations never intersect and nothing reconciles them, so the moment a purpose becomes activatable in the GUI, a warm edit would **show** its constraints and the next source edit would **hide** them.

Two asymmetries a unifying leaf must not trip over, running in **opposite** directions:

- The template-walk runs `structural_query::expand_constraint_expr` for `self.members` / `self.descendants`; the graph-walk does **not**. Routing everything through the graph-walk would silently drop structural-query expansion.
- The graph-walk overlays `active_purpose_let_cells`; the template-walk has no equivalent.

So the fix is not "pick the graph-walk" — it is a union preserving both obligations. Objectives are unaffected: `active_objectives()` is consumed by the solver, not by either check body.

`Engine::eval` preserves and re-injects `active_purpose_bindings` across a fresh eval, so activation *state* already survives a recompile; only the *reporting* pass misses it.

### 2.5 Substrate that already exists

- `CompiledModule.compiled_purposes` is a **public field**, walked in production by `reify-doc-build`. Declaration-side enumeration needs **no new API**.
- `pub purpose simulation_ready` and `design_review` ship in stdlib and are prelude-merged into every module, so every `.ri` file already has two activatable purposes.
- `geometric_params` / `material_params` reflective queries **do** resolve (task-4137); `docs/notes/purpose-reflective-aggregation.md` is stale on this and is corrected by leaf ι.
- The only existing "dim for stale" DOM style is `gui/src/panels/DesignTree.module.css` `.stale { opacity: 0.5; font-style: italic; }`.
- `set_fea_case` is a complete, tested precedent for an engine-mutating debug tool, including the mandatory delta-baseline refresh.

### 2.6 What a purpose can and cannot carry (probed 2026-08-27)

Load-bearing for the staleness fixture, so probed rather than assumed:

| Purpose body shape | Result |
|---|---|
| `constraint RepresentationWithin(subject, 1um)`, param typed `: Ball` | **INDETERMINATE** — `undefined inputs: <purpose>.subject` |
| same, param typed `: Structure` (wildcard), bound to `Ball` | **INDETERMINATE** |
| same, wildcard bound to the structure that owns the check | **INDETERMINATE** |
| `let margin = subject.radius - subject.wall` + `constraint margin > 500mm` | **works, verdict-sensitive**: `wall=100mm` → `OK`; `wall=700mm` → `VIOLATED` |

**A purpose body cannot host a working kernel-measured constraint today** — an entity-ref param never resolves to a measurable subject. The achievable warm-edit-sensitive shape is a `let` over subject params, which routes through the injected `active_purpose_let_cells` machinery. The fixture uses that shape, and BT-5 is written as a differential oracle accordingly (§9).

## 3. Consumer (G1)

**Primary — the GUI designer workflow.** Purposes are the language's mechanism for saying "check this design *for this job*"; today that is reachable only from a CLI flag.

| Mechanism introduced | Named consumer |
|---|---|
| `activate_purpose_session()` seam | `cmd_check` (leaf α, same batch); GUI `EngineSession` (leaf β, same batch); `driver-contract-implementation.md` leaf φ, which declares a **hard** edge on it |
| typed `PurposeActivationError` | `cmd_check`'s error strings (α); the `set_purpose` tool's honest failure reporting (δ) |
| `ConstraintData.purpose` + `purpose_applied_epoch` | `ConstraintPanel` (ζ); the selector's stale state (ε) |
| purpose intent / stickiness on `EngineSession` | the selector's round-trip state (ε); BT-6/BT-7 (η) |
| `set_purpose` debug-MCP tool | the η integration gate; the future cross-driver parity gate |
| Active-purpose list on `GuiState` | `regenerateAutoViews` at `App.tsx:702` — **an existing, tested consumer** (§2.2) |

No mechanism lacks a same-batch consumer. Integration seam (overlay §3.5): activation uses the existing constraint-injection path; **no new in-engine seam is introduced.**

## 4. Ruled shape (implement; do not relitigate)

`docs/notes/driver-contract-matrix-draft.md` RULINGS item 4 (Leo, 2026-08-26), verbatim:

> **GUI purpose surface: CHARTERED.** Shape: extract the CLI's hand-assembled activation sequence (main.rs:693-824) into a shared `activate_purpose_session()` seam used by CLI and EngineSession; v1 = purpose selector + bindings form + purpose-scoped verdicts in panels; active purpose sticky across edits/recompiles; `set_purpose` debug-MCP tool; purposes activatable in GUI + check, other CLI drivers gain `--purpose` in the flag-unification wave. **Additional ruled UX requirement**: after activation, GUI elements tied to a purpose whose application is STALE (the warm-edit incremental path has fired at least once without the purpose being incrementally reapplied) render visibly stale (dimmer/greyer), and a prominent "reapply / recheck purpose" affordance appears.

Ownership questions the ruling did not reach were put to Leo and ruled across 2026-08-26/27, twice revised as facts moved underneath:

- **Consume, don't duplicate.** `gui-on-demand-measurement` decomposed into #6740–#6748 (`901f8f5b25`) *during* authoring. Every substrate shared with that batch is a real `add_dependency` edge, not a rebuild.
- **The generation counter is #6742's.** This PRD stamps a purpose-application epoch onto it rather than building a second one.
- **α is a behaviour-preserving seam extraction only.** `cmd_check` / `finish_check` / exit codes / `--strict` are reserved to `check-diagnostic-truthfulness.md` by a binding G4 ruling, and #5748 is actively rewriting that arm. α introduces the seam and routes the purpose branch through it **without changing routing, exit codes or diagnostics**, downstream of #5748. The measurement defect §2.3 found is handed to PRD 2 as a filed finding (leaf ι).

Scope-boundary placement (`conformance-scope-boundary-draft.md`, RATIFIED): purpose *semantics* are Ring 1; *which surfaces can activate* and the flag/panel surface are **Ring 2**; the visual staleness styling is **Ring 3**, explicitly delegated — *"GUI presentation/UX … | GUI product work (purpose charter carries its own UX rulings)"*.

## 5. Resolved design decisions

**D1 — The seam lives in `reify-eval`.** `reify-cli` is bin-only (no `[lib]`) and `gui/src-tauri` does not depend on it, so nothing can `use reify_cli::…`. Every constituent call is already an `Engine` method in `reify-eval`. *Caveat for dispatch:* #5748 introduces `realize_for_check`, which does **not** exist on main — re-read the landed set before building the seam.

**D2 — The seam takes `&mut Engine`; it never constructs one.** A seam that built its own engine would hand the GUI the kernel-free engine behind §2.3. Engine construction stays the caller's; unifying it across drivers is `driver-contract-implementation.md` §1's charter (17 construction sites, 12 capability fingerprints), not this PRD's.

**D3 — Purpose activation is a parameter of the seam, not a branch** — within the seam's own body. Whether `cmd_check`'s *outer* routing keeps a branch is #5748/#5403's call, untouched here.

**D4 — Activation errors are typed and carry `DiagnosticCode`.** `Engine::activate_purpose` returns `()` and is silent on all four failure modes; `activate_purpose_with_bindings` returns free-form `Result<(), String>`. A GUI selector can render neither. The seam returns a typed `PurposeActivationError`; **`cmd_check` formats each variant back to today's exact string**, so CLI output stays byte-identical. INV-SF-6.

**D5 — Purpose-scoped verdicts are a partition, not a new carrier.** Injected constraints already carry entity prefix `purpose:<name>@<token>`; `ConstraintData` gains `purpose: Option<String>` derived from it.

**D6 — The generation counter is consumed, not rebuilt.** #6742 builds `EngineSession.generation` to its C-STALE spec. This PRD adds `purpose_applied_epoch` on top and joins the purpose commands to #6742's set of generation-advancing entry points.

**D7 — Stale means *not re-applied*, and the oracle is differential.** A warm edit **does** recompute purpose-injected verdicts (`edit_check` walks the graph); it does **not** re-run measurement arms or rebuild `active_tolerance_scope`. But §2.6 shows a purpose cannot carry a measured constraint at all, so for every purpose authorable today the warm result may well be *correct*. v1 therefore claims only what is true: `purpose_stale` means **"the cheap path ran and the purpose has not been re-applied since"** — a bookkeeping fact, honestly signalled, not an assertion that the verdict is wrong. Whether the cheap path cost anything is settled by BT-5's differential oracle (reapply ≡ fresh full pass), which is the matrix's own ruled *"warm-edit ≡ full-recompile differential self-oracle"* shape. Verdicts are **retained and dimmed**, never blanked. A full recompile re-activates, bumping the epoch, so recompiles land fresh.

**D8 — Never a bare `stale` at session level.** `EngineSession::is_stale()` already exists and means *the last edit failed to compile* (`last_reload_error.is_some()`), surfaced as top-level `"stale"` in `engine_state_json`. That is a **failure flag, not a freshness flag**; this PRD does not touch it. Freshness rides `ConstraintData.stale` / `.epoch` and the explicitly-named `purpose_stale`.

**D9 — Sticky is *desired intent*, reconciled and diagnosed.** `EngineSession` holds requested activations as intent, reconciled after each compile. `Engine::eval` silently drops a preserved binding whose purpose no longer exists — acceptable inside the engine, unacceptable as a user surface. On reconcile, a vanished purpose emits a **coded diagnostic naming it** and clears from the selector. INV-SF-3.

**D10 — Activation generates a view; it never switches the viewport.** v1 never changes the active view. Per-purpose *tolerance* — the change that would genuinely re-realize geometry — is `per-purpose-tolerance.md`, deferred.

**D11 — The "bindings form" is an entity picker.** `CompiledPurposeParam` is `{ name, entity_kind }`: entity references, no types/units/defaults to render. v1 offers a picker filtered by `entity_kind` off the existing entity tree.

**D12 — The selector lists prelude purposes alongside module-declared ones**, grouped via `declaration_span.is_prelude()`. They are universal by design and are the only purposes most modules have.

**D13 — One affordance: "Reapply purpose"**, re-running the full activation pass — the only combination that clears every staleness cause.

**D14 — The stale visual token converges on the one that exists.** `DesignTree.module.css`'s `.stale`, hooked through `ConstraintPanel.module.css`'s existing `[data-status="…"]` pattern. #6743 lands the shared token for measured verdicts; this PRD styles the **purpose instance** of it, per the ruled *"each PRD implements its own instance"*. Wording comes from ruling 4 ("reapply / recheck purpose"), never #6743's "Measure now".

## 6. Substrate verification (G3) — executed, not asserted

Probe environment: `target/debug/reify` (built 2026-08-26); `tree-sitter` from `~/.cargo/bin`; fixture `tests/prd-gate/fixtures/gui_purpose_surface.ri`.

| Assumed capability | Probe | Result |
|---|---|---|
| `--purpose` drops kernel measurement | §2.3 runs | **CONFIRMED** (mechanism traced in source; attribution corrected) |
| `--strict --purpose` already exits 1 | direct run | **CONFIRMED** — the exit-0 needs the non-strict policy too |
| Injected-constraint discriminator | same runs | **CONFIRMED** — `purpose:<name>@<entity>#constraint[i]` |
| Prelude purposes universally activatable | `--purpose simulation_ready=Ball` on a file declaring none | **CONFIRMED** |
| A purpose can carry a working `RepresentationWithin` | three binding shapes, §2.6 | **REFUTED** — all INDETERMINATE |
| A purpose `let` over subject params works, verdict-sensitive | §2.6 | **CONFIRMED** — `OK` at `wall=100mm`, `VIOLATED` at `700mm` |
| Unknown-purpose / malformed-value rejection fires | two runs | **CONFIRMED** — error + **exit 1** each, not silent accept |
| Fixture grammar | `tree-sitter parse --quiet` | **exit 0**, 0 ERROR nodes |
| `CompiledModule.compiled_purposes` enumerable | public field; `reify-doc-build` walk | **CONFIRMED** |

**No novel `.ri` syntax.** `grammar_confirmed: true` for every leaf.

**Substrate gaps queued as this PRD's own work:** no `Engine` accessor for *active* purposes (β); no candidate-entity enumeration (ε uses the `entity_kind` filter; a general API is §11); `build_constraints` renders a purpose-injected constraint blank — it cross-references `compiled.templates[*].constraints` by id and `.unwrap_or_default()`s (γ). Note `build_constraints` iterates `check.constraint_results`, so purpose rows **do** appear (blank) as soon as verdicts flow — γ's signal is not inverted on δ.

**Premise corrections carried for implementers:** there is **no clap** in `reify check` (hand-rolled arg walk + `parse_purpose_flag`); purpose bindings have **no types/units/defaults**; the GD&T *legality* pass already runs on both arms (task 4589) — what the purpose arm loses is GD&T *conformance measurement*.

## 7. Cross-PRD relationship (G4)

Two sibling batches decomposed *during* this PRD's authoring, and one reserved territory is under active edit. All are wired, or explicitly non-wired, below.

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **`check-diagnostic-truthfulness.md` — #5748 (`in-progress`, claimed), #5403 (`pending`)** | **reserved territory** | `cmd_check` routing, `finish_check`, build-diagnostic collection, exit codes, `--strict` | **that PRD, by a binding G4 ruling** (*"PRD 2 ONLY — no contested ownership"*) | **edge: α ← #5748.** α is behaviour-preserving and runs *after* #5748's rewrite of the same function. The §2.3 measurement residue is **filed to** PRD 2 by leaf ι — not fixed here. |
| **#6740** — measurement seam extraction (`gui-on-demand-measurement` α) | consumes | `Engine::run_measurement_pass` | that task | **edge: α ← #6740.** Read its landed API shape at dispatch (final name is that PRD's Open Question 3). |
| **#6742** — session edit generation (its γ, C-STALE) | consumes | monotone `EngineSession.generation` | that task | **edge: γ ← #6742.** This PRD adds `purpose_applied_epoch`; the purpose commands join its advancing set. |
| **#6743** — frontend stale rendering + re-measure affordance (its δ) | consumes | the shared dim/grey token + the badge-casing fix its A11 carries | that task | **edge: ζ ← #6743.** The purpose styling is an *instance*, not a second vocabulary. |
| **#6746** — docs-truth chunks + `reify-design` index (its η) | consumes | `SKILL.md` index line; `chunks/constraints.md`, `chunks/stdlib.md` | that task | **edge: θ ← #6746.** This PRD's docs leaf is scoped to `chunks/purposes.md`, which #6746 does not touch. |
| **#6723** — verdict-casing fix (`solver-legibility-telemetry` β) | consumes | the PascalCase↔lowercase mismatch rendering every badge `?` | that task | **edge: γ ← #6723**, the same reason #6741 depends on it. |
| **`driver-contract-implementation.md`** (landed `b5b298d842` 2026-08-26; decomposed, leaves #6773–#6808) | **produces** | `activate_purpose_session()` — its §8.1 marks the edge **hard for leaf φ**; §8.3 gives the protocol: wire a real edge if this PRD's seam leaf is filed, else file φ `deferred` | **this PRD** owns the seam | **Both edges wired 2026-08-27** by that PRD's session: `φ #6804 ← α #6803` (φ flipped `deferred`→`pending`), and `α #6773 ← α #6803` because its capability profile must adopt the unified check body rather than re-derive it. Several of its leaves also edit `crates/reify-cli/src/main.rs` — a third writer on that hotfile. *(Row corrected 2026-08-27: it read "untracked in project_root", true while this PRD was being authored and false by the time it landed.)* |
| `solver-legibility-telemetry` #6722 / #6726 | **collision, not dependency** | #6722 widens `ConstraintCheckEntry` with `margin`; this PRD widens `ConstraintData` with `purpose` | each owns its field | Different structs, different fields. Per #6722's own recorded note: **no edge**; whichever lands second extends the first's shape. |
| `purposes-completion.md` | consumes | `activate_purpose*` (#4000/#4006/#4009, `done`) | that PRD | **wired.** This PRD is the successor its §10 named and never filed. |
| `solver-driver-parity.md` (P1) | adjacent | pushes purpose surfaces out of P1 | this PRD picks it up | no contest |
| `per-purpose-tolerance.md` | adjacent | purpose-driven realization tolerance | that PRD (v0.2) | D10 makes v1's viewport silence deliberate |
| `reify-debug-mcp-expansion.md` | shares hub | `debug_server.rs` tool-defs + dispatch | that PRD (Draft) | hub contention documented there as expected |

**Ownership summary.** This PRD owns: the `activate_purpose_session()` seam; typed activation errors; purpose intent + stickiness; the purpose-scoped verdict partition; `purpose_applied_epoch`; the `set_purpose` tool; the selector and entity picker; and the purpose instance of the staleness UX. It owns **no** `cmd_check` semantics, no measurement pass, no generation counter, no verdict casing and no measurement docs — six edges across three sibling batches.

**Contested-ownership note.** `cmd_check` is genuinely contested territory with a standing ruling in another PRD's favour. This PRD does **not** add a fourth contested pair: it defers to that ruling outright.

## 8. Contract (H)

**C-SEAM.** `activate_purpose_session(engine: &mut Engine, compiled: &CompiledModule, activations: &[PurposeActivation]) -> Result<SessionCheckOutcome, EngineError>` in `reify-eval` (final name/signature tactical; the public seam name is ruled). It performs `eval` → per-activation binding → **one** constraint pass preserving *both* enumerations' obligations (§2.4: structural-query expansion *and* purpose let-cell overlay) → GD&T legality. It never constructs an `Engine` (D2), and it makes **no routing or exit-code decision** — the caller keeps those. `cmd_check` after the rewire produces verdicts, diagnostics and exit codes **identical to whatever #5748 leaves**, for every input including `--purpose`.

**C-VERDICT.** `SessionCheckOutcome` partitions results by the injected entity prefix: every entry is either base or attributed to exactly one activated purpose. Activating a purpose may only *add* constraints, never alter an existing verdict.

**C-ACTIVATE.** Activation errors are typed with a `DiagnosticCode` per variant. Activation is atomic per request: a failed binding leaves no partial activation. `cmd_check` renders each variant to its current exact string.

**C-GEN.** The monotone `EngineSession.generation` is **#6742's**. This PRD binds to it twice: the purpose commands advance it like any other mutating entry point, and `purpose_applied_epoch` records the generation of the last full activation, giving `purpose_stale := purpose_applied_epoch < session.generation`. One counter, two readers.

**C-INTENT.** The session holds requested activations as intent, independent of engine state, reconciled after every compile. A desired purpose the recompiled module no longer declares produces a coded diagnostic naming it and is cleared. Deactivation removes injected constraints, let-cells and objectives. Intent survives edits and recompiles until the user deselects.

**C-STATE.** Activation mutates the graph (injected constraints, let-cells, objective map) and rebuilds `active_tolerance_scope`, which feeds the compute-cache bucket key via `Engine::active_tolerance_for`. It is therefore **not purely additive** and must run the full body, never the warm path.

**C-PANEL.** `ConstraintData` gains `purpose: Option<String>`, `stale: bool`, `epoch: u64`, all riding the existing `diffed keyed(...)` macro — no new event channel. `build_constraints` resolves a purpose-injected constraint's expression and `parameter_ids` from the purpose's own compiled constraints rather than `.unwrap_or_default()`-ing to blank, reconciling with the existing `PurposeReflectiveAggregation` arm in `format_expr` (§2.1).

**C-MCP.** Debug-server method `set_purpose` (`name`, `bindings`, `clear`) activates or clears a purpose and returns `{ ok, active_purposes, purpose_stale, generation }`. It **must** refresh the delta baseline via `compute_delta(last_state, &gs)` and run engine work off tokio via `run_on_engine`. The name is `[a-z0-9_]+` and registers in all four guards. It reports activation failure honestly — never `{ok: true}` on a silent no-op.

**C-UX.** A purpose-tied element is stale iff `purpose_applied_epoch < session.generation`. Stale elements **retain** their verdict and render with the shared stale token; a prominent "Reapply purpose" affordance appears while any purpose is stale.

## 9. Boundary-test sketch (H)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| BT-1 | **Seam equivalence** (α's real lock) | the CLI harness corpus, as #5748 leaves it | `cmd_check` routed through the seam produces verdicts, diagnostics and exit codes **identical** to the pre-seam build, across every fixture including all 59 existing `--purpose` assertions |
| BT-2 | Non-purpose regression | existing `reify check` suites | green **unmodified** (the no-`--purpose` path is untouched by construction) |
| BT-3 | GUI ≡ check on the same file | fixture corpus incl. a declared purpose | `EngineSession`'s post-activation verdict set equals `reify check --purpose …`'s, verdict-for-verdict |
| BT-4 | **Full recompile ≡ warm edit for purpose visibility** (§2.4 lock) | purpose active; warm-edit a param; then edit source | purpose constraints present in the panel after **both**; today the source edit drops them |
| BT-5 | **Staleness differential oracle** (the ruled matrix shape) | `wall_margin` active; warm-edit `Ball.wall` across the 500mm threshold; then reapply | after the edit: verdicts **retained**, `purpose_stale=true`, affordance present. After reapply: verdicts **equal a fresh session** that loaded the post-edit source and activated the same purpose — so the flag is proven honest whether or not the cheap path degraded anything. Epochs monotone. |
| BT-6 | Stickiness across recompile | purpose active; `update_source` with an unrelated edit | still active, verdicts present, `purpose_stale=false` |
| BT-7 | **Vanished purpose is diagnosed, not dropped** (INV-SF-3) | purpose active; delete its declaration | coded diagnostic names it; selector clears it; no stale verdicts linger |
| BT-8 | Typed activation rejection | `set_purpose` with unknown name / unnamed binding in a multi-binding value / unknown param | each returns its **distinct** typed code — not one collapsed message, not `{ok:true}` |
| BT-9 | Panel renders purpose constraints legibly | `wall_margin` active | expression text non-empty, `parameter_ids` cross-highlight, tagged with the purpose (today: blank) |
| BT-10 | Viewport unchanged by activation (D10) | any fixture; activate | `auto:purpose:<name>` appears in the view list; active view id, mesh set and camera unchanged |
| BT-11 | Deactivate restores | activate then clear | graph value-cells, constraints and objective map identical to pre-activation |

BT-4, BT-7 and BT-9 fail today. BT-1 is α's equivalence lock, not a behaviour change.

## 10. Decomposition plan

Greek labels with their **real task ids**, backfilled at decompose time (2026-08-27). **Leaf** = names a user-observable signal. All leaves: no new `.ri` syntax (`grammar_confirmed: true`). Six substrates are **consumed by edge** from three sibling batches rather than rebuilt (§7).

**α (#6803) — `activate_purpose_session()`: extract the activation sequence into `reify-eval`, behaviour-preserving.** *(Leaf. deps: none in-batch; **out-of-batch #5748, #6740**)*
C-SEAM + C-VERDICT + C-ACTIVATE. Introduce the seam; route `cmd_check`'s purpose branch through it **without changing routing, exit codes, diagnostics or `--strict`** (reserved to PRD 2, §7). Preserve both enumerations' obligations (§2.4). Add typed `PurposeActivationError` with a `DiagnosticCode` per variant, rendered by `cmd_check` to today's exact strings.
*Signal:* the full `reify check` CLI harness corpus — including all 59 existing `--purpose` assertions — is byte-identical before and after the rewire (BT-1/BT-2), and `reify-eval` exposes a seam the GUI can call without depending on `reify-cli`.
*Modules:* `crates/reify-eval`, `crates/reify-cli/src/main.rs`.

**β (#6831) — GUI routes both paths through the seam; `set_purpose`/`clear_purpose` commands; purpose intent.** *(Leaf. deps: α)*
C-INTENT. Replace `check_with_solve_slot`'s `Engine::check` with the seam so the two GUI paths stop disagreeing (§2.4); add the two Tauri commands on the `set_active_fea_case` shape; hold activations as intent, reconciled after each compile with a coded diagnostic for a vanished purpose (INV-SF-3); join the purpose commands to #6742's generation-advancing set.
*Signal:* with a purpose activated, its constraints appear in the panel and **survive a source edit** (BT-4); deleting the active purpose's declaration raises a diagnostic naming it (BT-7).
*Modules:* `gui/src-tauri/src/engine.rs`, `main.rs`, `commands.rs`.

**γ (#6832) — Verdict partition + `purpose_applied_epoch`; legible rendering.** *(Leaf. deps: β; **out-of-batch #6742, #6723**)*
C-PANEL. `ConstraintData` gains `purpose` (derived from the prefix), `stale`, `epoch`. Stamp `purpose_applied_epoch` against #6742's counter. Fix `build_constraints` to resolve purpose-injected expressions from `CompiledPurpose.constraints`, reconciling with the existing `PurposeReflectiveAggregation` arm. Extend whatever wire shape #6723 landed.
*Signal:* a purpose constraint shows real expression text, cross-highlights its parameters, and is attributed to its purpose — today blank (BT-9).
*Modules:* `gui/src-tauri/src/types.rs`, `engine.rs`, `gui/src/types.ts`, `gui/src/stores/engineStore.ts`.

**δ (#6833) — `set_purpose` debug-MCP tool.** *(Leaf. deps: β)*
C-MCP. `ToolDef` + dispatch + handler + `run_on_engine` + delta-baseline refresh; honest failure reporting; all four guard registrations.
*Signal:* a scripted `reify-debug` session activates a purpose, reads back `active_purposes`/`purpose_stale`/`generation`, and a subsequent normal GUI command diffs against the correct baseline.
*Modules:* `gui/src-tauri/src/debug_server.rs`, `gui/src/__tests__/`, `gui/test/visual/assertions.ts`.

**ε (#6834) — Purpose selector + entity-binding picker; feed `activePurposes`.** *(Leaf. deps: γ)*
D11 + D12. Selector listing module-declared and prelude purposes; per-param entity picker filtered by `entity_kind`; replace the hardcoded `[]` at `App.tsx:702`.
*Signal:* any `.ri` file offers `simulation_ready`/`design_review` with no per-file declaration; picking one makes an `auto:purpose:<name>` view appear while the active view, meshes and camera are unchanged (BT-10).
*Modules:* `gui/src/panels/`, `gui/src/App.tsx`, `gui/src/stores/viewStateStore.ts`.

**ζ (#6835) — Ruled staleness UX: the purpose instance of the shared stale token.** *(Leaf. deps: ε; **out-of-batch #6743**)*
C-UX + D13 + D14. Apply #6743's token to purpose-tied rows and the selector entry when `purpose_stale`; add the "Reapply purpose" affordance re-running the full activation pass.
*Signal:* with `wall_margin` active, dragging `Ball.wall` dims its purpose rows and raises the affordance while **retaining** verdicts; reapply restores full styling with fresh verdicts and clears the flag (BT-5).
*Modules:* `gui/src/panels/ConstraintPanel.tsx` + `.module.css`, `gui/src/App.tsx`.

**η (#6836) — B+H integration gate: the §9 boundary suite.** *(Leaf. deps: α, β, γ, δ, ε, ζ)*
Drive BT-1..BT-11 in one CI-able run, the GUI half via `reify-debug` MCP against the fixture. BT-5 is the **differential** oracle (reapply ≡ fresh full pass). **Carries its own drift-guard registrations in the same diff.**
*Signal:* one scripted run shows the three today-failing scenarios green — BT-4, BT-7, BT-9 — alongside the rest, with BT-1 pinning CLI equivalence.
*Modules:* `crates/reify-eval/tests`, `gui/src-tauri/src/tests`, `gui/test/visual`, `tests/infra`.

**θ (#6837) — Docs-truth: the purposes chunk.** *(Leaf. deps: ζ; **out-of-batch #6746**)*
Scoped to what #6746 does not cover. Update `chunks/purposes.md`: document GUI activation and `--purpose`; **remove the false claim that `manufacturing_ready` is a standard-library purpose**; record that a purpose body cannot carry a kernel-measured constraint (§2.6) so authors do not rediscover it. Extend #6746's `reify-design` index line rather than adding a competing one.
*No exemplar-corpus leaf:* purposes already have corpus presence and this PRD adds no new authoring idiom.
*Signal:* the purposes chunk's stdlib list matches `determinacy_purposes.ri` and names GUI activation; each documented signature compiles as written in a smoke `.ri`.
*Modules:* `crates/reify-mcp/src/tools/chunks/purposes.md`.

**ι (#6838) — Companion corrections and the handed-over finding.** *(Leaf. deps: α)*
Docs + one filed task, no product code. (a) **File the §2.3 measurement residue against PRD 2** — after #5748, the `--purpose` arm still lacks `set_capture_repr_tol(true)` + `tessellate_realizations`, so `RepresentationWithin` degrades; file it in `check-diagnostic-truthfulness` territory with this PRD's executed evidence, wired to #5748/#5403. (b) `purposes-completion.md` §10: mark the follow-up executed. (c) survey D4: append the inward twin as a **dated addendum**, never an edit to the snapshot. (d) `purpose-reflective-aggregation.md`: record task-4137's landed filter-kind resolution. (e) `gui-on-demand-measurement.md`: name this PRD as consumer of #6740/#6742/#6743/#6746. **No sibling task record is edited** — the residue is a *new* task, not a rewrite of #5748.
*Signal:* PRD 2 owns a filed, evidenced task for the residue; the four documents no longer contradict the ruled ownership.
*Modules:* `docs/`.

**κ (#6839) — PRD-close stamp.** *(Leaf. deps: every other leaf)*
Terminal Status token, landed leaf ids, AS-AUTHORED freeze paragraph, LIVE vs AS-AUTHORED map, matching header on the manifest.
*Signal:* the committed header.
*Modules:* `docs/prds/v0_6/`.

**Dependency DAG:** `α → β → {γ, δ}; γ → ε → ζ; {α…ζ} → η; ζ → θ; α → ι; all → κ`
— by id: `6803 → 6831 → {6832, 6833}; 6832 → 6834 → 6835; {6803…6835} → 6836; 6835 → 6837; 6803 → 6838; all → 6839`.
**Out-of-batch edges** (real `add_dependency`): `α ← #5748`, `α ← #6740`, `γ ← #6742`, `γ ← #6723`, `ζ ← #6743`, `θ ← #6746`.
**Deliberate non-edges:** `solver-legibility-telemetry` #6722/#6726 — a wire-contract *collision*, not a dependency.

## 11. Out of scope

- **All `cmd_check` semantics** — routing, exit codes, `finish_check`, `--strict`, diagnostic collection. Reserved to `check-diagnostic-truthfulness.md` by binding G4 ruling; #5748/#5403 in flight. The §2.3 measurement residue is **filed to** that PRD by leaf ι, not fixed here.
- **`--purpose` on the other CLI drivers.** `driver-contract-implementation.md` leaf φ, which consumes this PRD's seam.
- **Multiple simultaneously-active purposes.** v1 is one (ruled); the seam accepts a `Vec` because the CLI flag is repeatable.
- **Incremental purpose reapplication.** v1 activation runs the full pass; only the staleness styling and affordance are v1.
- **Making a purpose able to carry a kernel-measured constraint.** §2.6 shows it cannot today. Documented by θ; fixing it is a separate charter.
- **Viewport response to activation** beyond existing auto-view generation, and purpose-driven realization tolerance.
- **A general "legal binding candidates for param p" API.** v1 filters by `entity_kind`.
- **Purpose *authoring* in the GUI.** Selection and binding only.
- **Unifying engine construction across drivers** — `driver-contract-implementation.md` §1.
- **The `manufacturingReadyVisibilityFor` orphan.** `autoViewGenerator.ts` selects it by `purpose === 'manufacturing_ready'`, and no purpose of that name exists anywhere — leaf θ removes the doc claim that one does. Feeding `activePurposes` unblocks `generatePurposeViews` generally but leaves that specific heuristic dead. Named here so it is not mistaken for something this PRD revives; retiring or re-homing it is a follow-up.
- **The six consumed substrates** (#5748, #6740, #6742, #6743, #6723, #6746) — all by edge, none rebuilt.

## 12. Open (tactical) questions

1. **Seam name and exact signature.** Ruling 4 fixes the public name; whether the body wraps a `run_session_check` is tactical. **Suggested:** keep the ruled name. Decide during α.
2. **How activation composes with #6740's `MeasureOptions` and #5748's `realize_for_check`.** Both land in the same region before α runs. **Suggested:** extend the landed structs rather than threading parallel parameters; re-read both at dispatch. Decide during α.
3. **Whether `purpose_stale` also marks the `auto:purpose:<name>` view entry.** **Suggested:** yes, same token. Decide during ζ.
4. **Progress rendering during reapply** — reuse the `evaluation-status` channel and `SolverProgressOverlay`, or a per-purpose spinner. **Suggested:** reuse the existing channel. Decide during ζ.
5. **Selector grouping label for prelude purposes** — "Standard" vs "Library" vs "Built-in". **Suggested:** "Standard". Decide during ε.
