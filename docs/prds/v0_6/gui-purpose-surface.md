# GUI purpose surface — activation, purpose-scoped verdicts, ruled staleness UX

**Status: ACTIVE** — chartered by Leo 2026-08-26 (driver-contract matrix ruling 4, spec-conformance program); authored 2026-08-26. Milestone v0_6.

**Code anchors** verified against main `da8091cbe8` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Approach: B + H** (contract + two-way boundary tests). Blast radius 4 crates/packages (`reify-eval`, `reify-cli`, `gui/src-tauri`, `gui/src`), mechanism count ≥ 8, touches the constraint-verdict seam, and has ≥ 2 cross-PRD consumers (`gui-on-demand-measurement`, the future driver-contract implementation PRD).

---

## 1. Goal

A designer working in the Reify GUI picks a declared purpose from a selector, binds its entity parameters, and sees the purpose's constraints appear in the constraint panel as purpose-scoped verdicts alongside the base ones. The purpose is **sticky**: it survives edits and recompiles until deselected. After a warm edit, every GUI element tied to that purpose renders **visibly stale** (dimmer/greyer) with a prominent **"Reapply purpose"** affordance, until reapplied.

Underneath that surface, and load-bearing beyond it: the CLI's hand-assembled purpose activation sequence becomes a shared `activate_purpose_session()` seam in `reify-eval`, called by both `cmd_check` and the GUI `EngineSession`. Extracting it **removes a fork in `cmd_check` that today makes `reify check --purpose` silently return a false green.**

## 2. Background — what exists today, measured

### 2.1 Purposes are invisible in the GUI

`grep -i purpose` over `gui/src-tauri/src/` returns only doc-comment prose. `EngineSession` has no `activate_purpose` / `is_purpose_active` / `active_purposes` method. The `invoke_handler` list (`gui/src-tauri/src/main.rs:983`) registers 33 commands, none purpose-related. `set_purpose` appears nowhere in the repo except the ruling text — **the name is free**.

This is cross-driver survey row **D4** (`docs/notes/cross-driver-divergence-survey-draft.md`): *"`--purpose` exists only on `reify check`, and since `engine.check()` skips purpose-injected constraints, every other driver (GUI, LSP, all other CLI, MCP) silently never checks them."*

### 2.2 …but the GUI already has a purpose consumer, fed a constant

`generatePurposeViews(tree, activePurposes)` (`gui/src/stores/autoViewGenerator.ts`) mints one `auto:purpose:<name>` view per active purpose, with a dedicated `manufacturingReadyVisibilityFor` heuristic. It is reached through `viewStateStore.regenerateAutoViews(tree, activePurposes = [], displaySubjects)`. The **sole production call site passes a hardcoded empty array**:

```ts
// gui/src/App.tsx:702
viewStateStore.regenerateAutoViews(entityTree(), [], displaySubjects);
```

`generatePurposeViews` early-returns on `length === 0`, so `auto:purpose:*` views are unreachable in the running GUI and the `manufacturing_ready` heuristic has never once executed. It is exercised only in vitest. This is a producer-orphan of the C-10 shape, live since task #1745 closed — and a one-line, already-tested consumer waiting for an active-purpose list.

`docs/prds/v0_6/purposes-completion.md` §3 recorded this and §10 deferred it: *"GUI purpose-activation command … is a separate GUI PRD … **Follow-up:** file a `gui-purpose-activation` task post-batch."* That follow-up was never filed. **This PRD is that successor.**

### 2.3 The `cmd_check` fork — and the false green it produces

`cmd_check` (`crates/reify-cli/src/main.rs`) branches on whether `--purpose` was passed, and the two arms build **different engines**:

| | no `--purpose` | with `--purpose` |
|---|---|---|
| engine | `Engine::with_registered_kernel(checker)` | `Engine::new(checker, **None**)` — no kernel |
| ReprWithin | `set_capture_repr_tol(true)` + `tessellate_realizations` | absent |
| Conforms / DFM | `build(&compiled, ExportFormat::Step)` populates `realization_handles` | absent |
| thickness DFM | `ensure_openvdb_kernel()` | absent |
| verdicts | `engine.check()` (template-walk) | `check_constraints_with_values()` (graph-walk) |
| GD&T conformance | inside `check()` via `measure_gdt_conformance` | absent |
| GD&T legality | `run_gdt_check_passes` | `run_gdt_check_passes` (same) |
| DFM error escalation | `dfm_has_error_diagnostic` → `FAILURE` | absent |

**Executed evidence** (debug binary built 2026-08-26, fixture `tests/prd-gate/fixtures/gui_purpose_surface.ri`):

```
$ reify check tests/prd-gate/fixtures/gui_purpose_surface.ri
  VIOLATED BallCheck#constraint[0]
  error: RepresentationWithin: sampled facet deviation 6.006e-2 m exceeds bound 1.000e-6 m for BallCheck
  → exit 1

$ reify check --purpose design_review=BallCheck tests/prd-gate/fixtures/gui_purpose_surface.ri
  INDETERMINATE BallCheck#constraint[0]
  VIOLATED purpose:design_review@BallCheck#constraint[0]
  warning: constraint BallCheck#constraint[0] indeterminate: undefined inputs: BallCheck.subject
  → exit 1

$ reify check --purpose simulation_ready=Ball tests/prd-gate/fixtures/gui_purpose_surface.ri
  OK purpose:simulation_ready@Ball#constraint[0]
  OK purpose:simulation_ready@Ball#constraint[1]
  INDETERMINATE BallCheck#constraint[0]
  No constraints violated (1 indeterminate).
  → exit 0
```

Three findings, all new (not in survey row D4, which records only the *outward* absence of purposes from other drivers):

1. **A violated constraint becomes indeterminate** because `--purpose` was passed. The kernel measurement never ran.
2. **The attributed cause is false.** `"undefined inputs: BallCheck.subject"` — the input is defined; the measurement pass simply did not run. A misattributed `Indeterminate` is exactly what the 2026-08-26 INV-SF-4 doctrine forbids (`docs/legibility/design-invariants.md`, INV-SF-4 Doctrine).
3. **`--purpose` can flip the exit code to a false green.** `simulation_ready` is **prelude-merged into every module** (`crates/reify-compiler/stdlib/determinacy_purposes.ri`, via `merge_prelude_purposes`), so *any* module can activate it with no declaration — and doing so turns a genuinely violated design into `exit 0`. Same class as survey rows D13/D14 (CB/HIGH).

### 2.4 The GUI's own two paths already disagree about purposes

`EngineSession`'s full-recompile path (`load_file` / `load_from_source` / `update_source` → `check_with_solve_slot` → `Engine::check`) resolves constraints by walking `module.templates[*].constraints` (`check_constraints_against_templates`). Its warm-edit path (`set_parameter` → `Engine::edit_check`) calls `check_constraints_with_values`, which walks `snapshot.graph.constraints`.

Purpose constraints are injected into the **graph**, under entity prefix `purpose:<name>@<token>` (`engine_purposes.rs`). The two enumerations never intersect. So the moment a purpose becomes activatable in the GUI, warm edits would **show** its constraints and the next source edit would **hide** them — unless the PRD closes this. It is not enough to add a toggle.

Note the nuance: `Engine::eval` preserves and re-injects `active_purpose_bindings` across a fresh eval, so activation *state* already survives a recompile. Only the *reporting* pass misses it.

### 2.5 Substrate that already exists

- `CompiledModule.compiled_purposes: Vec<CompiledPurpose>` is a **public field**, walked in production by `reify-doc-build`. Declaration-side enumeration (name, `is_pub`, param names, param `entity_kind`, `declaration_span`) needs **no new API**.
- `pub purpose simulation_ready` and `design_review` ship in stdlib and are prelude-merged into every module — so every `.ri` file already has two activatable purposes.
- `geometric_params` / `material_params` reflective queries **do** resolve (task-4137). (`docs/notes/purpose-reflective-aggregation.md` still says otherwise; that note is stale — corrected by the corrections leaf.)
- The only existing "dim for stale" DOM style in the codebase is `gui/src/panels/DesignTree.module.css` `.stale { opacity: 0.5; font-style: italic; }`.
- `set_fea_case` is a complete, tested precedent for an engine-mutating debug tool.

## 3. Consumer (G1)

**Primary — the GUI designer workflow.** Purposes are the language's mechanism for saying "check this design *for this job*". Today that is reachable only by dropping to a CLI flag, so the GUI — the primary design surface — cannot express it at all.

**Concrete consumers wired by this PRD:**

| Mechanism introduced | Named consumer |
|---|---|
| purpose activation folded into the shared seam | `cmd_check` (leaf α, same batch); GUI `EngineSession` (leaf β, same batch); future driver-contract PRD's `--purpose` spread (declared, not depended on) |
| typed `PurposeActivationError` | `cmd_check`'s error strings (leaf α); the `set_purpose` tool's honest failure reporting (leaf δ) |
| `ConstraintData.purpose` + `purpose_applied_epoch` | `ConstraintPanel` (leaf ζ); the selector's stale state (leaf ε) |
| purpose intent / stickiness on `EngineSession` | the selector's round-trip state (leaf ε); BT-6/BT-7 (leaf η) |
| `set_purpose` debug-MCP tool | the η integration gate; the future cross-driver parity gate |
| Active-purpose list on `GuiState` | `regenerateAutoViews` at `App.tsx:702` — **an existing, tested consumer** (§2.2) |

No mechanism in this PRD lacks a same-batch consumer. Integration seam (overlay §3.5, ConstraintSolver / constraint-injection path): activation uses the existing injection path; **no new in-engine seam is introduced.**

## 4. Ruled shape (implement; do not relitigate)

From `docs/notes/driver-contract-matrix-draft.md` RULINGS item 4 (Leo, 2026-08-26), verbatim:

> **GUI purpose surface: CHARTERED.** Shape: extract the CLI's hand-assembled activation sequence (main.rs:693-824) into a shared `activate_purpose_session()` seam used by CLI and EngineSession; v1 = purpose selector + bindings form + purpose-scoped verdicts in panels; active purpose sticky across edits/recompiles; `set_purpose` debug-MCP tool; purposes activatable in GUI + check, other CLI drivers gain `--purpose` in the flag-unification wave. **Additional ruled UX requirement**: after activation, GUI elements tied to a purpose whose application is STALE (the warm-edit incremental path has fired at least once without the purpose being incrementally reapplied) render visibly stale (dimmer/greyer), and a prominent "reapply / recheck purpose" affordance appears.

Two ownership questions the ruling did not reach were put to Leo on 2026-08-26 and ruled twice, because the facts moved underneath the first ruling. `gui-on-demand-measurement` was **decomposed into #6740–#6748 while this PRD was being authored** (commit `901f8f5b25`), turning a paper contract into nine pending tasks. Leo's final ruling, on the corrected facts:

- **Consume the sibling's leaves; do not duplicate them.** Every substrate this PRD shares with that batch is taken as a real `add_dependency` edge rather than rebuilt.
- **The `cmd_check` fork is still this PRD's to kill.** #6740 extracts the measurement arms *behavior-identically* — which preserves today's two-arm fork rather than repairing it, so the false green (§2.3) survives it. This PRD's leaf α folds purpose activation into the seam #6740 extracts, turning the branch into a parameter. One body, reached in two steps by two batches.
- **The session generation counter is #6742's.** It specifies C-STALE verbatim and is already filed. This PRD **stamps a purpose-application epoch onto that counter** rather than building a second one. (This reverses the earlier ruling, which was made when no such task existed.)

Scope-boundary placement (`docs/notes/conformance-scope-boundary-draft.md`, RATIFIED): purpose *semantics* are Ring 1; *which surfaces can activate* and the flag/panel surface are **Ring 2**; the visual staleness styling is **Ring 3**, explicitly delegated — *"GUI presentation/UX (viewport rendering, panel layout, staleness styling) | GUI product work (purpose charter carries its own UX rulings)"*. This PRD therefore owns its UX rulings outright; the conformance suite does not test them.

## 5. Resolved design decisions

**D1 — The seam lives in `reify-eval`, not a new crate and not `reify-cli`.**
`reify-cli` is bin-only (`[[bin]]`, no `[lib]`) and the GUI does not depend on it, so nothing can `use reify_cli::…`. Every constituent call — `with_registered_kernel`, `set_capture_repr_tol`, `build`, `tessellate_realizations`, `ensure_openvdb_kernel`, `eval`, `activate_purpose*`, `check_constraints_with_values`, `run_gdt_check_passes` — is already an `Engine` method in `reify-eval`. The four `module_has_*` kind detectors move out of `reify-cli/src/main.rs` into `reify-eval` beside the seam. A new workspace crate would earn nothing and cost a build edge.

**D2 — The seam takes `&mut Engine`; it never constructs one.**
This is the whole point. A seam that built its own engine would hand the GUI the kernel-free `Engine::new(checker, None)` that causes §2.3's false green. Engine construction stays the caller's business — the GUI's session engine already carries kernels and `SolverRegistry::production()`. (Unifying engine *construction* across drivers is matrix ruling 2's cell, not this PRD's; see #6696.)

**D3 — Purposes are a parameter of one body, never a second branch.**
Measurement arms run under their existing kind gating regardless of whether a purpose is active. This is what makes `reify check --purpose` a superset of `reify check` instead of a lossy sibling.

**D4 — Activation errors are typed, and carry `DiagnosticCode`.**
Today `Engine::activate_purpose` returns `()` and is **silent** on all four of its failure modes; `is_purpose_active` is the only oracle, and the CLI collapses four causes into one `eprintln!` string. `activate_purpose_with_bindings` returns `Result<(), String>` — free-form prose. A GUI selector cannot render either. The seam returns a typed `PurposeActivationError` with a `DiagnosticCode` per variant (unknown purpose · no eval state · unnamed binding in a multi-binding value · unknown param · duplicate binding · missing binding). **The CLI formats them back to today's exact strings**, so existing CLI harness output stays byte-identical. INV-SF-6 `diagnostics-carry-codes`.

**D5 — Purpose-scoped verdicts are a partition, not a new carrier.**
Injected constraints already carry entity prefix `purpose:<name>@<token>`. `ConstraintData` gains `purpose: Option<String>` (None = base constraint), derived from that prefix. No second result vector, no second event channel.

**D6 — The generation counter is consumed, not rebuilt; this PRD adds a purpose-application epoch on top of it.**
`gui-on-demand-measurement` leaf **#6742** builds the monotone `EngineSession.generation` to its C-STALE spec, advanced by every mutating entry point. This PRD does **not** build a second counter. It adds one field — the generation at which the active purpose was last fully applied — and derives `purpose_stale` from the shared counter. The new purpose commands must join #6742's set of generation-advancing entry points; that is a one-line obligation on this PRD's side of the seam, recorded in §7 and wired as a real edge.

**D7 — Stale means *degraded*, not *absent* — and the predicate is honest about it.**
Because `edit_check` already walks the graph (§2.4), a warm edit **does** recompute purpose-injected verdicts. What it does *not* do is re-run the measurement arms or rebuild `active_tolerance_scope` (refreshed only inside `rebuild_purpose_infrastructure` at activation). So:

> `purpose_stale := purpose_applied_epoch < session.generation`

means *"these verdicts were produced by the incremental path since the last full activation pass"* — not *"there are no verdicts"*. Verdicts are **retained and dimmed**, never blanked. This also makes the ruled state model self-consistent: a full recompile re-activates, which bumps `purpose_applied_epoch` to the current generation, so recompiles land **fresh**; only warm edits go stale. Same predicate shape as measurement staleness, one counter.

**D8 — Never a bare `stale` at session level.**
`EngineSession::is_stale()` already exists and means *the last edit failed to compile* (`last_reload_error.is_some()`), surfaced as top-level `"stale"` in `engine_state_json`. That is a **failure flag, not a freshness flag**. This PRD does not touch it. Freshness rides `ConstraintData.stale` / `.epoch` (per C-STALE) and, in the `set_purpose` / `engine_state` responses, the explicitly-named `purpose_stale`. No consumer may read the two as the same fact.

**D9 — Sticky is *desired intent*, reconciled and diagnosed.**
`EngineSession` holds the user's requested activations as intent, reconciled after each compile. `Engine::eval` silently drops a preserved binding whose purpose no longer exists — acceptable inside the engine, unacceptable as a user surface. On reconcile, a desired purpose that the recompiled module no longer declares emits a **coded diagnostic naming it** and clears from the selector. INV-SF-3 `declared-intent-consumed-or-diagnosed`: a declaration that cannot be consumed is never a silent no-op.

**D10 — Activation generates a view; it never switches the viewport.**
Feeding `activePurposes` into `regenerateAutoViews` makes an `auto:purpose:<name>` view *appear in the view selector*. v1 never changes the active view. This honours the ruled "no viewport change in v1" without fighting machinery that already exists (§2.2). Per-purpose *tolerance* — the change that would genuinely re-realize geometry — is `docs/prds/v0_2/per-purpose-tolerance.md`, deferred.

**D11 — The "bindings form" is an entity picker, not a value form.**
`CompiledPurposeParam` is `{ name: String, entity_kind: String }`. Purpose params bind **entity references**, not values: there are no types, units, or defaults to render. v1 offers, per param, a picker over candidate entities filtered by `entity_kind` (a structure-template name, or the `Structure` wildcard), sourced from the existing `get_entity_tree()` / `get_entity_identity_map()`. A general "which entities may legally bind to param p" API does not exist and is **out of scope** (§11) — v1's filter is the template-name match, which is what `Type::StructureRef(entity_kind)` already means.

**D12 — The selector lists prelude purposes alongside module-declared ones.**
`simulation_ready` / `design_review` are prelude-merged and universal by design (`purposes-completion` §3: activation against a user structure with *no per-structure opt-in*). Hiding them would hide the only purposes most modules have. They are grouped visually as standard vs module-declared, using `declaration_span.is_prelude()` — the same discriminator `reify-doc-build` uses.

**D13 — One affordance: "Reapply purpose".**
Two operations technically exist (`activate_purpose` is `&mut self`; `check_constraints_with_values` is `&self`). Exposing both would ask the designer to understand the difference. v1 ships a single button that re-runs the unified body — re-activate *and* re-check — which is the only combination that clears every staleness cause (measurement arms + tolerance scope). Ruling 4's "reapply / recheck purpose" is satisfied by the reapply half; the recheck-only variant is not offered.

**D14 — The stale visual token converges on the one that exists.**
`DesignTree.module.css` `.stale { opacity: 0.5; font-style: italic; }` is the only DOM stale style in the tree. This PRD lifts it to a shared token rather than inventing a second dimming vocabulary. The status-badge hook is `ConstraintPanel.module.css`'s existing `[data-status="…"]` attribute-selector pattern. `gui-on-demand-measurement` §7 ruled that converging on a shared stale-overlay component is *"tactical for whichever lands second"* — this PRD lands first, so it authors the token and that PRD inherits it. Affordance wording comes from **ruling 4's own words** ("reapply / recheck purpose"), never the measurement PRD's "Measure now".

## 6. Substrate verification (G3) — executed, not asserted

Probe environment: `target/debug/reify` (built 2026-08-26); `tree-sitter` from `~/.cargo/bin`; fixture `tests/prd-gate/fixtures/gui_purpose_surface.ri`.

| Assumed capability | Probe | Result |
|---|---|---|
| `--purpose` drops kernel measurement | §2.3 three-way run | **CONFIRMED** — VIOLATED → INDETERMINATE, exit 1 → exit 0 |
| Injected-constraint discriminator | same run | **CONFIRMED** — `purpose:design_review@BallCheck#constraint[0]` |
| Prelude purposes universally activatable | `--purpose simulation_ready=Ball` on a file declaring no purpose | **CONFIRMED** — two injected constraints, both OK |
| Unknown-purpose rejection fires | `--purpose nonexistent_purpose=BallCheck` | **CONFIRMED** — error + **exit 1** (not a silent accept) |
| Malformed flag value rejection fires | `--purpose design_review` (no `=`) | **CONFIRMED** — **exit 1** |
| Fixture grammar | `tree-sitter parse --quiet` | **exit 0**, 0 ERROR nodes |
| `CompiledModule.compiled_purposes` enumerable | public field; production walk in `reify-doc-build` | **CONFIRMED** — no new API needed |
| `set_purpose` name free | `grep -rn "set_purpose"` repo-wide | **CONFIRMED** — one hit, the ruling text |

**No novel `.ri` syntax.** `grammar_confirmed: true` for every leaf.

**Substrate gaps queued as this PRD's own work, not assumed:** no `Engine` accessor for *active* purposes (leaf β); no candidate-entity enumeration (leaf ε uses the `entity_kind` filter; a general API is §11); `build_constraints` cannot render a purpose constraint's expression (leaf γ — it cross-references `compiled.templates[*].constraints` by id and `.unwrap_or_default()`s to `expression: ""`, `parameter_ids: []`).

**Premise corrections to the charter brief**, recorded so no implementer re-derives them: there is **no clap** in `reify check` (hand-rolled arg walk + `parse_purpose_flag`); purpose bindings have **no types/units/defaults** (entity refs only); the GD&T *legality* pass already runs on both arms (task 4589) — what the purpose arm loses is GD&T *conformance measurement* inside `check()`.

## 7. Cross-PRD relationship (G4)

`gui-on-demand-measurement` decomposed into **#6740–#6748** (commit `901f8f5b25`) *while this
PRD was being authored*. Leo ruled 2026-08-26 that this PRD **consumes** that batch rather
than duplicating it. Every shared substrate below is a real `add_dependency` edge.

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **#6740** — measurement seam extraction (`gui-on-demand-measurement` α) | consumes | `Engine::run_measurement_pass` extracted from `cmd_check` | that task | **edge: α ← #6740.** #6740 is explicitly *behavior-identical*, so it extracts the arms and **leaves the purpose fork standing**. Killing the fork is this PRD's α. |
| **#6742** — session edit generation + stale flag (its γ, C-STALE) | consumes | monotone `EngineSession.generation` | that task | **edge: γ ← #6742.** This PRD adds `purpose_applied_epoch` on top; the purpose commands join its set of generation-advancing entry points. |
| **#6743** — frontend stale rendering + re-measure affordance (its δ) | consumes | the shared dim/grey stale token + the badge-casing fix its A11 carries | that task | **edge: ζ ← #6743.** This PRD's stale styling is the *purpose instance* of that token, not a second vocabulary (ruled: *"each PRD implements its own instance"*). |
| **#6746** — docs-truth chunks + `reify-design` index (its η) | consumes | `.claude/skills/reify-design/SKILL.md` index line; `chunks/constraints.md`, `chunks/stdlib.md` | that task | **edge: docs leaf ← #6746.** This PRD's docs leaf is scoped strictly to `chunks/purposes.md`, which #6746 does **not** touch — gap coverage, not duplication. |
| **#6723** — verdict-casing fix (`solver-legibility-telemetry` β) | consumes | `ConstraintData.status` PascalCase↔lowercase mismatch that renders every badge `?` | that task | **edge: γ ← #6723.** Purpose-scoped verdicts are unreadable until it lands, for the same reason #6741 depends on it. |
| `solver-legibility-telemetry` (#6722 / #6726) | **collision, not dependency** | both widen the per-constraint wire contract — #6722 adds `margin` to `ConstraintCheckEntry`, this PRD adds `purpose`/`epoch` to `ConstraintData` | each owns its own field | Different structs, different fields, neither needs the other's. Per the precedent set on #6722's own coordination note: **no edge**; file locks serialise, and whichever lands second extends the first's shape rather than opening a second per-constraint channel. |
| `docs/prds/v0_6/purposes-completion.md` | consumes | `activate_purpose` / `activate_purpose_with_bindings` (#4000/#4006/#4009, `done`) | that PRD | **wired.** This PRD is the successor its §10 named and never filed. |
| `docs/prds/v0_6/driver-contract-implementation.md` (**landed 2026-08-26**, leaves #6773–#6808) | produces | `activate_purpose_session()` — the seam its `--purpose` spread consumes, its leaf φ **#6804** | **this PRD** | declared, **not depended on**. G1 is satisfied by the GUI + `cmd_check`, both in-batch. **Row corrected 2026-08-26**: it read "does not exist on any branch", true when authored and false a few hours later. That PRD's φ #6804 is filed **`deferred`** precisely because this PRD's seam leaf had no task id yet — when this PRD decomposes, wire `add_dependency(6804, <this PRD's α>)` and flip #6804 to `pending`. Until then its close leaf #6808 is blocked on #6804 by design. |
| `docs/prds/v0_6/solver-driver-parity.md` (P1) | adjacent | explicitly pushes purpose surfaces out of P1 | this PRD picks it up | no contest |
| `docs/prds/v0_2/per-purpose-tolerance.md` | adjacent | purpose-driven realization tolerance — what would make activation viewport-affecting | that PRD (deferred to v0.2) | D10 makes v1's viewport silence deliberate |
| `docs/prds/v0_6/reify-debug-mcp-expansion.md` | shares hub | `debug_server.rs` tool-defs + dispatch | that PRD (Draft) | hub contention documented there as expected; `set_purpose` joins `measure_constraints` / `measurement_status` |

**Ownership summary.** This PRD owns: the purpose activation seam and the fork's removal;
purpose intent + stickiness; the purpose-scoped verdict partition; `purpose_applied_epoch`;
the `set_purpose` tool; the selector and entity picker; and the purpose instance of the
staleness UX. It owns **no** general measurement, counter, casing or docs-chunk substrate —
all are consumed by edge (five edges, two sibling batches).

No new contested-ownership pair is introduced; the three known pairs are untouched.

## 8. Contract (H)

**C-SEAM.** `activate_purpose_session(engine: &mut Engine, compiled: &CompiledModule, opts: &SessionCheckOptions) -> Result<SessionCheckOutcome, EngineError>` in `reify-eval` (final name/signature tactical; the public seam name is ruled). It is the **single** check body: kind detection → measurement arms (capture / handle-populating build / tessellate / OpenVDB) → `eval` → per-activation purpose binding → **one** `check_constraints_with_values` over all graph constraints → GD&T legality. It never constructs an `Engine` (D2). `cmd_check` after the rewire produces verdicts, diagnostics and exit codes identical to today **for every input that does not use `--purpose`**, pinned by the existing CLI harness suites staying green unmodified. For `--purpose` inputs it deliberately differs — that difference is the §2.3 bug fix and is pinned by its own locks.

**C-VERDICT.** `SessionCheckOutcome` partitions results by the injected entity prefix: every entry is either base or attributed to exactly one activated purpose. Base verdicts under an active purpose are **byte-identical** to the same file's verdicts with no purpose active — activating a purpose may only *add* constraints, never weaken an existing one. This is the invariant §2.3 violates today, and BT-1 is its lock.

**C-ACTIVATE.** Activation errors are typed with a `DiagnosticCode` per variant (D4). Activation is atomic per request: a failed binding leaves the engine with no partial activation for that purpose. `cmd_check` renders each variant to its current exact string.

**C-GEN.** The monotone `EngineSession.generation` is **#6742's** (its C-STALE). This PRD binds to it in two places: the purpose commands must advance it like any other mutating entry point, and a `purpose_applied_epoch` records the generation of the last full activation pass, giving `purpose_stale := purpose_applied_epoch < session.generation`. One counter, two readers — never a second counter.

**C-INTENT.** The session holds requested activations as **intent**, independent of engine state, and reconciles after every compile. A desired purpose the recompiled module no longer declares produces a coded diagnostic naming it and is cleared. Deactivation removes injected constraints, let-cells and objectives (the existing `deactivate_purpose` invariant). Intent survives edits and recompiles until the user deselects (the ruled stickiness).

**C-STATE.** Purpose activation on the live session engine mutates the graph (injected constraints, let-cells, objective map) and rebuilds `active_tolerance_scope` — which feeds the compute-cache bucket key via `Engine::active_tolerance_for`. Activation is therefore **not purely additive** and must run the full body, never the warm path (D3/D13). Deactivating restores the pre-activation graph.

**C-PANEL.** `ConstraintData` gains `purpose: Option<String>`, `stale: bool`, `epoch: u64`. All three ride the existing `diffed keyed(...)` macro on `GuiState.constraints`, so no new event channel is added. `build_constraints` resolves a purpose-injected constraint's expression and `parameter_ids` from the purpose's own compiled constraints rather than `.unwrap_or_default()`-ing to blank.

**C-MCP.** Debug-server method `set_purpose` (params: `name: string`, `bindings: object|null`, `clear: bool`) activates or clears a purpose on the session engine and returns `{ ok, active_purposes, purpose_stale, generation }`. It **must** refresh the delta baseline via `compute_delta(last_state, &gs)` before returning — omitting it desyncs the next normal Tauri command (the documented latent-bug-#7 shape). It runs engine work off the tokio thread via `run_on_engine`. The tool name is `[a-z0-9_]+` as the extraction regex requires, and it registers in all four guards: `debugParity.test.ts`'s `PURE_ENGINE_SIDE` allowlist, `KNOWN_DEBUG_TOOL_NAMES`, `toolDefNames.ts`, `debugContract.test.ts`. `set_purpose` reports activation failure honestly — never `{ok: true}` on a silent no-op.

**C-UX.** A purpose-tied element is stale iff `purpose_applied_epoch < session.generation`. Stale elements retain their verdict and render with the shared stale token (D14); a prominent **"Reapply purpose"** affordance appears while any purpose is stale. No purpose state renders as bare `Indeterminate` without an attributed cause.

## 9. Boundary-test sketch (H) — faces both producer and consumer sides

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| BT-1 | **Purpose activation never weakens a base verdict** (the false-green lock) | `gui_purpose_surface.ri`; run with no purpose, then with `design_review=BallCheck`, then `simulation_ready=Ball` | `BallCheck#constraint[0]` is `VIOLATED` in all three; exit code 1 in all three; the measured deviation is present in all three. Today all three differ (§2.3). |
| BT-2 | CLI regression: no-purpose path byte-identical | existing CLI harness fixtures | every existing `reify check` suite green unmodified |
| BT-3 | GUI ≡ check on the same file | fixture corpus incl. ReprWithin + a purpose | `EngineSession`'s post-activation verdict set equals `reify check --purpose …`'s, verdict-for-verdict |
| BT-4 | **Full recompile ≡ warm edit for purpose visibility** (the §2.4 lock) | purpose active; edit a param warm; then edit source | purpose-injected constraints present in the panel after **both**; today the source edit drops them |
| BT-5 | Staleness lifecycle | activate → warm edit → reapply, driven via `set_purpose` + `engine_state` | after activate: `purpose_stale=false`; after warm edit: verdicts **retained**, `purpose_stale=true`, reapply affordance present; after reapply: fresh, `purpose_stale=false`; generation monotone throughout |
| BT-6 | Stickiness across recompile | purpose active; `update_source` with an unrelated edit | purpose still active, verdicts present, `purpose_stale=false` (recompile re-activates, D7) |
| BT-7 | **Vanished purpose is diagnosed, not silently dropped** (INV-SF-3) | purpose active; edit source to delete that purpose declaration | a coded diagnostic names the dropped purpose; selector clears it; no stale verdicts linger |
| BT-8 | Typed activation rejection | `set_purpose` with an unknown name; with an unnamed binding in a multi-binding value; with an unknown param | each returns its distinct typed code — not one collapsed message, not `{ok:true}` |
| BT-9 | Panel renders purpose constraints legibly | purpose with a non-trivial constraint body | expression text is non-empty and `parameter_ids` cross-highlight, tagged with the purpose name (today: blank) |
| BT-10 | Viewport unchanged by activation (D10) | any fixture; activate a purpose | `auto:purpose:<name>` appears in the view list; active view id, mesh set and camera unchanged |
| BT-11 | Deactivate restores | activate then clear | graph value-cells, constraints and objective map identical to pre-activation; panel shows base verdicts only |

BT-1, BT-4 and BT-7 are the three that fail today. They are the integration gate's core.

## 10. Decomposition plan

Greek labels; real task ids assigned at decompose time and backfilled here. **Leaf** = names a
user-observable signal. All leaves: no new `.ri` syntax (`grammar_confirmed: true`).
Five substrates are **consumed by edge** from two sibling batches rather than rebuilt (§7).

**α — Fold purpose activation into the seam; kill the `cmd_check` fork.** *(Leaf. deps: none in-batch; **out-of-batch #6740**)*
C-SEAM + C-VERDICT + C-ACTIVATE. #6740 extracts the measurement arms behavior-identically, leaving the two-arm fork intact; α turns the `--purpose` branch into a **parameter** of that one body, so measurement arms run whether or not a purpose is active. Adds the typed `PurposeActivationError` (`DiagnosticCode` per variant), replacing the silent `()` return and the free-form `Result<(), String>`; `cmd_check` renders each variant to its current exact string.
*Signal:* `reify check --purpose simulation_ready=Ball tests/prd-gate/fixtures/gui_purpose_surface.ri` reports `VIOLATED BallCheck#constraint[0]` with the measured deviation and **exits 1** — where it prints "No constraints violated" and exits 0 today (BT-1). Every existing CLI harness suite stays green unmodified (BT-2).
*Modules:* `crates/reify-eval`, `crates/reify-cli/src/main.rs`.

**β — GUI routes both paths through the seam; `set_purpose` / `clear_purpose` commands; purpose intent.** *(Leaf. deps: α)*
C-INTENT. Replace `check_with_solve_slot`'s `Engine::check` with the seam so full-recompile and warm-edit stop disagreeing (§2.4); add the two Tauri commands on the `set_active_fea_case` mutate→rebuild→delta→emit shape; hold requested activations as **intent**, reconciled after each compile, with a coded diagnostic when a recompiled module no longer declares a desired purpose (INV-SF-3). The purpose commands join #6742's set of generation-advancing entry points.
*Signal:* with a purpose activated, its constraints appear in the GUI panel and **survive a source edit** — where a full recompile drops them today (BT-4); a source edit deleting the active purpose raises a diagnostic naming it instead of silently dropping it (BT-7).
*Modules:* `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/main.rs`, `gui/src-tauri/src/commands.rs`.

**γ — Purpose-scoped verdict partition + `purpose_applied_epoch`; purpose constraints render legibly.** *(Leaf. deps: β; **out-of-batch #6742, #6723**)*
C-PANEL. `ConstraintData` gains `purpose: Option<String>` derived from the `purpose:<name>@<token>` prefix (rides the existing diff macro — no new event channel). Stamp `purpose_applied_epoch` against **#6742's** counter. Fix `build_constraints` so a purpose-injected id resolves its expression and `parameter_ids` from `CompiledPurpose.constraints` instead of `.unwrap_or_default()`-ing to blank. Extends whatever wire shape #6723 has landed rather than opening a parallel channel.
*Signal:* a purpose constraint in the panel shows its real expression text, cross-highlights its parameters, and is attributed to its purpose — today it renders blank (BT-9).
*Modules:* `gui/src-tauri/src/types.rs`, `gui/src-tauri/src/engine.rs`, `gui/src/types.ts`, `gui/src/stores/engineStore.ts`.

**δ — `set_purpose` debug-MCP tool.** *(Leaf. deps: β)*
C-MCP. `ToolDef` + dispatch arm + handler + `run_on_engine` + **delta-baseline refresh**; honest failure reporting (never `{ok:true}` on a silent no-op); registration in all four guards (`PURE_ENGINE_SIDE`, `KNOWN_DEBUG_TOOL_NAMES`, `toolDefNames`, `debugContract`).
*Signal:* a scripted `reify-debug` session activates a purpose, reads back `active_purposes` / `purpose_stale` / `generation`, and a subsequent normal GUI command diffs against the correct baseline (no stale-baseline desync).
*Modules:* `gui/src-tauri/src/debug_server.rs`, `gui/src/__tests__/`, `gui/test/visual/assertions.ts`.

**ε — Purpose selector + entity-binding picker; feed `activePurposes`.** *(Leaf. deps: γ)*
D11 + D12. SolidJS selector listing module-declared and prelude purposes (grouped via `declaration_span.is_prelude()`); per-param entity picker filtered by `entity_kind` off the existing entity tree; replace the hardcoded `[]` at `App.tsx:702` with the live active-purpose list.
*Signal:* opening any `.ri` file offers `simulation_ready` and `design_review` with no per-file declaration; picking one makes an `auto:purpose:<name>` view appear in the view selector while the active view, meshes and camera are unchanged (BT-10).
*Modules:* `gui/src/panels/`, `gui/src/App.tsx`, `gui/src/stores/viewStateStore.ts`.

**ζ — Ruled staleness UX: the purpose instance of the shared stale token.** *(Leaf. deps: ε; **out-of-batch #6743**)*
C-UX + D13 + D14. #6743 lands the dim/grey stale rendering and the badge-casing fix for measured verdicts; ζ applies that **same token** to purpose-tied rows and the selector entry when `purpose_stale`, and adds a prominent **"Reapply purpose"** affordance that re-runs the unified body. Not a second dimming vocabulary — the ruled *"each PRD implements its own instance"*.
*Signal:* with a purpose active, dragging a parameter dims its purpose rows and raises the reapply affordance while **retaining** the verdicts; clicking reapply restores full styling with fresh verdicts and clears the flag (BT-5).
*Modules:* `gui/src/panels/ConstraintPanel.tsx` + `.module.css`, `gui/src/App.tsx`.

**η — B+H integration gate: the §9 boundary-test suite.** *(Leaf. deps: α, β, γ, δ, ε, ζ)*
Drive BT-1..BT-11 in one CI-able run, the GUI half via `reify-debug` MCP against `tests/prd-gate/fixtures/gui_purpose_surface.ri`. **Carries its own drift-guard registrations in the same diff**: a bucket row in `tests/infra/run-all-classification.manifest` for any new `tests/infra/test_*.sh`, nextest heavy/smoke partition entries in `.config/nextest.toml`, and no new wall-clock upper bounds.
*Signal:* one scripted run shows the three today-failing scenarios green — BT-1 (no false green), BT-4 (warm ≡ recompile), BT-7 (vanished purpose diagnosed) — alongside the rest of the table.
*Modules:* `crates/reify-eval/tests`, `gui/src-tauri/src/tests`, `gui/test/visual`, `tests/infra`.

**θ — Docs-truth: the purposes chunk.** *(Leaf. deps: ζ; **out-of-batch #6746**)*
Scoped strictly to what #6746 does not cover. Update `crates/reify-mcp/src/tools/chunks/purposes.md`: document GUI activation and the `--purpose` flag, and **remove the false claim that `manufacturing_ready` is a standard-library purpose** (stdlib ships only `simulation_ready` and `design_review`). Extend #6746's `reify-design` index line rather than adding a competing one. Discoverability acceptance: an author who knows the goal ("check this design is ready to simulate") but not the feature name finds purpose activation from the chunk.
*No exemplar-corpus leaf:* purposes already have corpus presence (`examples/m10_purpose_activation.ri`, `m5_purpose.ri`, `determinacy_intrinsics.ri`) and this PRD introduces **no new authoring idiom** — it exposes an existing one on a new surface.
*Signal:* `reify_language_reference` returns a purposes chunk whose stdlib list matches `determinacy_purposes.ri` and that names GUI activation; each documented signature compiles as written in a smoke `.ri`.
*Modules:* `crates/reify-mcp/src/tools/chunks/purposes.md`.

**ι — Companion cross-PRD corrections.** *(Leaf. deps: α)*
Docs + task records, no product code. (a) `purposes-completion.md` §10: mark the `gui-purpose-activation` follow-up **executed by this PRD**. (b) `docs/notes/cross-driver-divergence-survey-draft.md`: append the inward twin of D4 — `check --purpose` is not `check` — as a **dated addendum**, never an edit to the dated snapshot. (c) `docs/notes/purpose-reflective-aggregation.md`: record that `geometric_params` / `material_params` now resolve (task-4137), superseding its "remaining gap" section. (d) `gui-on-demand-measurement.md`: add the one row naming this PRD as the consumer of #6740/#6742/#6743/#6746 and noting that #6740's behavior-identical extraction deliberately leaves the fork for α — so a reader of either PRD sees the same seam story.
*Signal:* the four documents no longer contradict the ruled ownership or the measured behaviour; a reader of #6740 learns that the fork it preserves is closed by this PRD's α rather than being an oversight.
*Modules:* `docs/`.

**κ — PRD-close stamp.** *(Leaf. deps: every other leaf)*
Set the Status marker to the terminal token, name the landed leaf ids, add the AS-AUTHORED freeze paragraph and the LIVE vs AS-AUTHORED map, and apply the matching header to `gui-purpose-surface.capability-manifest.md`.
*Signal:* the committed header.
*Modules:* `docs/prds/v0_6/`.

**Dependency DAG:** `α → β → {γ, δ}; γ → ε → ζ → {η, θ}; {α…ζ} → η; α → ι; all → κ`.
**Out-of-batch edges** (real `add_dependency` at decompose time): `α ← #6740`, `γ ← #6742`, `γ ← #6723`, `ζ ← #6743`, `θ ← #6746`.
**Deliberate non-edges:** `solver-legibility-telemetry` #6722/#6726 — a wire-contract *collision*, not a dependency; per the precedent recorded on #6722, no edge is wired and whichever lands second extends the first's shape.

## 11. Out of scope

- **`--purpose` on the other CLI drivers** (`eval`, `build`, `report`, `explain`, `test`). Ruled to the flag-unification wave of the driver-contract implementation PRD, which consumes this PRD's seam. This PRD makes it *possible*, not *done*.
- **Multiple simultaneously-active purposes.** v1 is one active purpose (ruled). The seam accepts a `Vec` because `cmd_check`'s repeatable flag already does; the GUI surface exposes one.
- **Incremental purpose reapplication.** Ruled explicitly to a later leaf: v1 activation runs the full body. Only the staleness styling and the reapply affordance are v1.
- **Viewport response to purpose activation** — visibility rules beyond the existing auto-view generation, and purpose-driven realization tolerance (`per-purpose-tolerance.md`, v0.2).
- **A general "legal binding candidates for param p" API.** v1 filters by `entity_kind` template-name match (D11). A schema-level candidate query is a follow-up.
- **Purpose *authoring* in the GUI** (writing or editing a `purpose` block from the UI). Selection and binding only.
- **Unifying engine construction across drivers** (matrix ruling 2's "one shared engine constructor"). This PRD takes the engine as a parameter precisely so it does not have to own that; see #6696.
- **The measurement seam extraction (#6740), the session generation counter (#6742), the shared stale token and badge-casing fix (#6743, #6723), and the measurement docs chunks (#6746).** All four are consumed by real dependency edges, not rebuilt here (§7). This PRD adds only the purpose-specific layer on each.
- **Purpose-scoped kernel measurement semantics** — whether a purpose may *narrow* which measurements run. v1 runs the module's measurement arms unconditionally by kind gating.

## 12. Open (tactical) questions

1. **Seam function name and exact signature.** Ruling 4 fixes the public name `activate_purpose_session()`; whether the unified body carries that name or wraps a `run_session_check` is tactical. **Suggested resolution:** keep the ruled name for the seam callers use, documented as "the shared session check body". Decide during α.
2. **How purpose activation composes with #6740's `MeasureOptions`.** #6740 owns the options struct and the capture/refine split; α adds purpose activation beside it. **Suggested resolution:** extend #6740's landed struct with the activation list rather than threading a second parameter — read its shape at dispatch time, since #6740's own final naming is its Open Question 3. Decide during α.
3. **Whether `purpose_stale` also gates the `auto:purpose:<name>` view entry.** D14 dims panel rows and the selector entry; whether the generated view is also marked stale is a judgement call. **Suggested resolution:** yes, same token, for consistency. Decide during η.
4. **Progress rendering during reapply.** Reuse the existing `evaluation-status` phase channel and `SolverProgressOverlay`, or a per-purpose spinner. **Suggested resolution:** reuse the existing channel; a full-body reapply is the same class of work as a recompile. Decide during η.
5. **Selector grouping label for prelude purposes.** "Standard" vs "Library" vs "Built-in". **Suggested resolution:** "Standard", matching the chunk's "Standard Library Purposes" heading. Decide during ζ.
