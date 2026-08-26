# GUI on-demand kernel measurement (ReprWithin · GD&T Conforms · DFM)

**Status: ACTIVE** — chartered by Leo 2026-08-26 (driver-contract matrix ruling 6c, spec-conformance
program); authored 2026-08-26. Milestone v0_6.

**Code anchors** verified against main `e3ddf6406c` (2026-08-26). Main moves fast — cite-by-symbol;
re-locate lines at implementation time.

## 1. Goal

A designer working in the GUI viewport gets **measured** verdicts for the three kernel-measured
constraint kinds — `RepresentationWithin`, geometric GD&T `Conforms`, and DFM rules (including
OpenVDB thickness checks) — without leaving the GUI. Measurement runs **off the keystroke path**:
on idle after an edit, or on an explicit "measure now". After a warm edit, a previously measured
verdict renders **visibly stale** with a prominent re-measure affordance until re-measured.

This is the chartered follow-up to the v1 attributable-Indeterminate posture (precision PRD §4.6
"Revisit if a use case contradicts" — the use case is here): the GUI is the primary use case, live
manufacturing feedback is the product promise, and the "not measured here — run check" posture must
not become permanent by default.

## 2. Background

- `reify check`'s kernel arms are the **measurement reference implementation**: `cmd_check`'s
  targeted sequence in `crates/reify-cli/src/main.rs` — kind detection
  (`module_has_geometric_conforms` / `module_has_representation_within` / `module_has_dfm_rule` /
  `module_has_thickness_dfm_rule`), `Engine::with_registered_kernel`, `set_capture_repr_tol(true)`
  for ReprWithin, a handle-populating `build()` for Conforms/DFM, `tessellate_realizations()` for
  ReprWithin, `ensure_openvdb_kernel()` for thickness-DFM, then `check()` (which runs
  `measure_gdt_conformance`, the `dispatch_constraints` RepresentationWithin interception, and
  `measure_dfm_rules`).
- The GUI runs `engine.check()` at every mutating entry point and `tessellate_snapshot` in
  `build_gui_state`, but **never invokes any measurement arm**: zero calls to
  `set_capture_repr_tol`, `tessellate_realizations`, `run_gdt_check_passes`, or
  `ensure_openvdb_kernel` in `gui/src-tauri` (verified 2026-08-26; cross-driver survey row D3).
  All three kinds render Indeterminate in the constraint panel.
- Cost context (dated, measured in the precision PRD §2.2, 2026-08): capture+measure for a bounded
  1 m sphere at `#precision(0.3mm)` costs ~10.6 s under `reify check`; the B-rep-only `build()` is
  ~0.37 s; precision-δ (#6168) narrows measurement to bounded subjects. Seconds-class, idle-time
  work — not keystroke-path work.
- Provenance: cross-driver divergence survey + driver-contract matrix
  (`docs/notes/cross-driver-divergence-survey-draft.md`,
  `docs/notes/driver-contract-matrix-draft.md`, both 2026-08-26 — this PRD implements the GUI
  cells of matrix rows 11/12 under ruling 6c). Baseline tasks filed from the same ruling: #6666
  (GUI OpenVDB registration), #6667 (attributable Indeterminate for GD&T/DFM).

## 3. Verified substrate (G3)

| Capability | Where | Verified |
|---|---|---|
| check's measurement-arm sequence (the reference) | `cmd_check`, `crates/reify-cli/src/main.rs` (kernel-arm block at :598-651) | ✅ read 2026-08-26 |
| GUI demand dispatch: `EngineSession::sync_demand` → `set_demand_selective`; delta contract on `Engine::demand_scoped_unified_pass` | `gui/src-tauri/src/engine.rs`; `crates/reify-eval/src/engine_build.rs` | ✅ read |
| `geometry_derived_cache` retention + prune (#5338) — the "never paint Final for an undemanded body" discipline | `EngineSession::sync_demand`, `gui/src-tauri/src/engine.rs` | ✅ read |
| Solve-slot cancellation lifecycle (task γ/4086): `SolveCancellationSink`, `check_with_solve_slot`, `CancellationHandle`, `AppState.pending_solve_cancel`; cancel locks only the slot, never the session mutex | `gui/src-tauri/src/engine.rs` | ✅ read |
| `build_gui_state` — the single shared rebuild path (load_file / update_source / set_parameter); `tessellate_snapshot` + post-geometry constraint overlay; `build_constraints` payload; "constraint-update" events | `gui/src-tauri/src/engine.rs` | ✅ read |
| GUI debug server JSON-RPC method table (`engine_state`, `demand_dispatch`, `wait_for_idle`, …) | `gui/src-tauri/src/debug_server.rs` | ✅ read |
| `achieved_repr_tol` capture gated on `capture_repr_tol`; measurement site in the tessellate callback | `crates/reify-eval/src/geometry_ops.rs`, precision PRD §3 | ✅ read (via precision PRD, spot-checked) |
| `measure_gdt_conformance` / `run_gdt_check_passes` / `measure_dfm_rules` inside `check()` | `crates/reify-eval/src/engine_constraints.rs` | ✅ grep |
| `ensure_openvdb_kernel` — registry-driven, idempotent, no-op without the openvdb feature | `crates/reify-eval` (called from `cmd_check` thickness arm) | ✅ read |
| Passive observed-demand registry: constraint-panel constraints already registered as demand sources (task 4532, zero-behavior-change) | `crates/reify-eval/src/observed_demand.rs` | ✅ memory + task record |
| Baseline tasks #6666 (GUI OpenVDB) and #6667 (attributable Indeterminate GD&T/DFM) | fused-memory | ✅ both `pending`, scoped as briefed; #6667 defers measurement to this PRD by name |
| Precision PRD tasks: γ1 refine loop **#6171 pending**, δ narrowing **#6168 pending**, ζ attribution **#6169 pending** | fused-memory | ✅ read — see §6/§7 for the collision this PRD resolves |

No novel `.ri` syntax anywhere in this PRD (`grammar_confirmed: true` for every leaf; no grammar
fixtures needed).

## 4. Sketch of approach

One new engine seam, one new GUI pass, no new scheduler:

1. **Extract the reference into a shared seam.** `cmd_check`'s measurement-arm sequence becomes a
   single engine-level API (working name `Engine::run_measurement_pass(&compiled, MeasureOptions)
   -> CheckResult`; final name tactical). `cmd_check` rewires to call it (behavior-identical);
   the GUI calls the same seam. The future cross-driver conformance suite gets one symbol to pin
   GUI≡check measurement parity against.
2. **Run it as a session pass.** `EngineSession` gains a measurement pass that invokes the seam on
   the **live session engine** (reusing warm state, realization cache, and registered kernels),
   under the existing solve-slot cancellation lifecycle (γ/4086 pattern). No parallel scheduler,
   no second engine, no worker thread: mutating entry points cancel any in-flight measurement
   first, so keystroke latency is bounded by cancellation responsiveness, exactly as for FEA
   solves today.
3. **Trigger on idle or on request.** The frontend triggers measurement after a debounced idle
   delay following the last mutating operation (default on, gated on the module carrying at least
   one of the three kinds — mirroring check's own kind gating), and immediately on an explicit
   "Measure now" affordance. Both funnel into one Tauri command.
4. **Surface through the standard state flow.** Measured verdicts, measured values (achieved
   deviation, DFM measurements), and a measurement **epoch** ride the existing
   `build_gui_state`-shaped constraint payload + diff/event flow into the constraint panel. The
   #6667 attribution channel is extended into a four-state surface: *unmeasured (attributed)* →
   *measuring* → *measured (fresh)* → *measured (stale)*.
5. **Staleness by epoch.** Every mutating entry point advances the session's edit generation;
   measurement results are stamped with the generation they measured. Panel rendering dims any
   measured verdict whose epoch predates the current generation and shows the re-measure
   affordance (same UX pattern the GUI purpose charter ruled for stale purpose application).
6. **Observable to machines.** The GUI debug server gains `measure_constraints` (trigger,
   optionally wait) and `measurement_status` (results, epochs, staleness, in-flight state) so the
   future cross-driver parity gate — and this PRD's own integration tests — can drive and observe
   GUI measurement mechanically.

### Mechanisms and consumers (G1)

| Mechanism | Consumer |
|---|---|
| `Engine::run_measurement_pass` seam (reify-eval) | `cmd_check` (rewired), the GUI session pass, the driver-contract conformance suite (future) |
| capture/refine flag split (§5 D2) | precision γ1 #6171 (refine loop gates on refine, check-only); GUI capture-only path |
| `EngineSession` measurement pass + Tauri command | frontend idle trigger + "Measure now"; debug-MCP tools |
| Measurement epoch + stale flag in constraint payload | constraint panel rendering; precedent for the purpose charter's stale-purpose UX |
| debug-MCP `measure_constraints` / `measurement_status` | cross-driver parity gate (driver-contract program); leaf ζ's tests |
| Frontend affordances (idle, measure-now, stale render, progress) | the designer in the viewport — the G1 primary consumer |

## 5. Resolved design decisions

**D1 — one shared seam, extracted from check, not a GUI re-implementation.** The GUI must not
grow a second copy of the arm sequence: the seam is the reference implementation for both callers,
and the warm-edit ≡ full-recompile contract plus the future GUI≡check parity gate are only
testable against a shared symbol. `cmd_check` keeps its CLI-side reporting/exit logic; only the
arm sequence moves.

**D2 — capture and refine are separate engine toggles; the GUI never refines.** Today
`capture_repr_tol` means "measure". Precision-γ1 (#6171, pending) as written gates its
measure-and-refine loop on bare `capture_repr_tol` + a declared bound — under the precision PRD's
assumption (its §4.6 "F's scoping is structural": only `cmd_check` ever sets capture) that was
safe. This PRD breaks that assumption: the GUI starts capturing. So the seam introduces the split
now: `MeasureOptions { refine: bool }` (or an equivalent second engine flag) — check passes
refine-on (its ruled §4.6 posture, live once γ1 lands), the GUI passes refine-off always
(brief-ruled: precision PRD refine-cost rationale, "a 10–20 s measure+refine pass per edit is not
a design session"). Whichever of α/#6171 lands second reconciles against the flag split; leaf θ
amends #6171's text so its implementer gates the loop on the refine flag, never bare capture.

**D3 — live session engine, no parallel scheduler.** Measurement runs on the session's engine —
reusing the warm-state pool, realization cache, and registered kernels — as a session-level pass
through the existing command/dispatch plumbing, under the solve-slot cancellation lifecycle
(γ/4086). Rejected alternative: a scratch engine per measurement (isolation for free, but it
abandons warm state, makes the warm-edit differential oracle vacuous, and duplicates kernel
memory). The price of the live engine is a **state-discipline contract** (§8 C-STATE) pinned by a
no-perturbation boundary test.

**D4 — trigger = debounced idle (default on) + explicit measure-now.** Idle auto-measure is gated
on the module carrying ≥1 of the three kinds (mirrors check's kind gating; a module with none
never enters any of this — the same negative signal the precision PRD demands). Cancellation
protects the keystroke path, so default-on is safe; cost is seconds-class (§2). An in-flight
measurement is cancelled and rescheduled by a newer request (latest-wins; at most one in flight).

**D5 — measurement scope is the whole module (check parity), not the viewport demand cone.**
Verdict parity with `reify check` is the product promise; a DFM violation on a currently-hidden
body must still surface in the panel. Precision-δ's narrowing to bounded subjects (#6168) is
inherited automatically through the shared seam. The viewport demand cone does not scope
measurement; a later optimization may narrow via the observed-demand registry (task 4532
substrate) — out of scope here.

**D6 — measurement writes the constraint-verdict channel only, never the value-cell overlay.**
The full-scope handle-populating build inside the measurement pass must not feed
`geometry_derived_cache` / the delta-values overlay: painting a `determined`/`final` value cell
for a body the demand pass never demanded is exactly the arch-§8 violation the #5338 prune
discharges. Measurement results are constraint verdicts + measurement metadata, full stop.

**D7 — staleness by monotone edit-generation epoch; stale verdicts are retained, flagged, and
dimmed.** A measured verdict is never silently discarded on edit and never silently served as
fresh: it keeps rendering with a stale style (dimmer/greyer) + re-measure affordance until
re-measured (the ruled staleness-UX pattern, shared with the purpose charter). A measurement that
completes after a newer edit publishes **as already stale** rather than being dropped. Whether the
epoch reuses existing freshness machinery or a new counter is tactical.

**D8 — degradation is attributed, never silent (INV-SF-1/-3/-4).** Kernel absent, OpenVDB feature
absent, measurement cancelled, seam error — each yields the #6667 attributable-Indeterminate state
with a cause naming *why* it is unmeasured, plus a diagnostic where the reference (check C1)
emits one. No false Violated, no false Satisfied, no bare Indeterminate.

**D9 — thickness-DFM rides the seam's own `ensure_openvdb_kernel` arm; #6666 covers viewport
realization.** The seam calls `ensure_openvdb_kernel` for thickness modules exactly as check does
(idempotent, registry-driven, no-op without the feature). #6666 (GUI OpenVDB registration for
isosurface realization) is the substrate that makes the *design itself* realize in the viewport;
the thickness-measurement leg (leaf ε) depends on it.

## 6. Pre-conditions

- **#6667** (attributable Indeterminate for GD&T/DFM) — the attribution channel this PRD's
  four-state surface extends. *Filing gap noted:* #6667 tells its implementer to "locate the zeta
  mechanism", but precision-ζ (#6169) is still pending and #6667 carries no dependency edge —
  the decompose session for this PRD records the #6667 → #6169 edge (leaf θ).
- **#6666** (GUI OpenVDB kernel registration) — required by leaf ε only.
- **Precision PRD tasks** #6171 (γ1 refine loop) and #6168 (δ narrowing): not dependencies, but
  live seams — D2 resolves the γ1 collision; δ's narrowing is inherited through the seam
  whenever it lands, in either order.

## 7. Cross-PRD relationships (G4)

| PRD / program | Direction | Mechanism at the seam | Owner of the integration |
|---|---|---|---|
| `precision-nominal-representation-guarantee.md` | consumes | ReprWithin semantics, `capture_repr_tol`/`achieved_repr_tol`, refine-off rationale; §4.6 GUI-viewport row is superseded by this PRD (its own "revisit" clause) | **this PRD**: leaf α implements the capture/refine split; leaf θ lands the §4.6 correction note + amends #6171. The precision PRD keeps owning refine itself (γ1/γ2). |
| Driver-contract program (matrix + survey drafts, `docs/notes/`; PRDs to be authored) | produces | this PRD implements the GUI cells of matrix rows 11/12; debug-MCP `measure_constraints`/`measurement_status` are the surface its parity gate will drive | parity gate: **driver-contract PRD(s)** (future). Tool names + semantics: **this PRD** (§8 C-MCP). |
| GUI purpose charter (matrix ruling 4; not yet filed) | pattern-shares | the stale-render + re-apply affordance UX pattern | each PRD implements its own instance; converging on a shared frontend stale-overlay component is tactical for whichever lands second |
| RepresentationWithin export-refusal η / GUI export gap (#6190) | adjacent only | export-path refusal is a different surface | #6190 / the #6170 program — explicitly **not** absorbed here |
| `selective-demand.md` (stub) + observed-demand substrate (4532) | adjacent only | future narrowing of measurement scope via observed demand | selective-demand PRD, if/when activated |

No new contested-ownership pair is introduced (the three known pairs are untouched).

## 8. Contract (H)

**C-SEAM.** `Engine::run_measurement_pass(&compiled, MeasureOptions) -> CheckResult` (final
name/signature tactical) reproduces `cmd_check`'s arm sequence exactly: kind detection → capture
flag for ReprWithin → handle-populating build for Conforms/DFM → `tessellate_realizations` for
ReprWithin → `ensure_openvdb_kernel` for thickness → `check()`. A module with none of the three
kinds is a cheap no-op (kind gating lives inside the seam). `cmd_check` after the rewire produces
verdicts, diagnostics, and exit codes identical to before it — pinned by the existing CLI harness
suites, which stay green unmodified.

**C-FLAGS.** `MeasureOptions.refine` (or an equivalent engine flag) is the *only* gate for the
γ1 refine loop; `capture_repr_tol` alone never triggers refinement. check → refine-on; GUI →
refine-off, unconditionally, v1. Capture is observation-only: capture-on tessellation yields mesh
bytes identical to capture-off (pinned, BT-4).

**C-STATE.** The measurement pass on the live session engine may mutate only measurement
side-channels (`achieved_repr_tol`, `realization_handles`, DFM/GD&T measurement maps, its own
result/epoch state). It restores the capture/refine flags and the selective-demand roots to their
pre-pass values on every exit path (Ok, Err, cancelled), and it never writes the value-cell
overlay (D6). Observable form: a forced `build_gui_state` after a measurement pass yields meshes
and values identical to a control session that never measured (BT-3).

**C-CANCEL.** The measurement pass runs under the solve-slot lifecycle: a fresh
`CancellationHandle` is published before the pass and cleared after; cancellation locks only the
slot. Every mutating session entry point (`set_parameter`, `update_source`, `load_file`, and the
purpose/FEA mutating commands) cancels an in-flight measurement before acquiring the session. A
cancelled pass publishes no fresh verdicts (previously measured verdicts keep their epoch and go
stale as usual) and records `cancelled` as the attribution cause if nothing was ever measured.

**C-STALE.** The session keeps a monotone edit generation, advanced by every mutating entry
point. Measurement results carry the generation of the compiled module they measured.
`stale := result.epoch < session.generation`. Fresh results overwrite stale ones atomically per
constraint; a pass that completes against a superseded generation publishes as stale (D7).

**C-STATUS.** Each kernel-measured constraint's panel state is exactly one of:
`unmeasured(cause)` (#6667 attribution — including kernel-absent, feature-absent, cancelled,
never-ran) · `measuring` · `measured(verdict, values, epoch)` fresh · `measured(...)` + `stale`.
Transitions only via measurement passes and epoch advancement. No state renders as bare
Indeterminate.

**C-MCP.** Debug-server methods: `measure_constraints` (params: `wait: bool`, `timeout_ms`;
triggers a pass, optionally awaits completion, returns the terminal status) and
`measurement_status` (returns per-constraint C-STATUS state, epochs, session generation, and
whether a pass is in flight). Both are additive to the existing JSON-RPC method table and follow
its conventions. These names are the parity-gate contract; changing them after the
driver-contract suite consumes them is a cross-PRD break.

## 9. Boundary-test sketch (H)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| BT-1 | GUI measure ≡ check (the parity oracle) | fixture corpus: ReprWithin (satisfied + violated), geometric Conforms, DFM overhang/draft/min-wall, thickness-DFM | for each fixture, the GUI-configuration engine's `run_measurement_pass(refine=off)` verdicts + measured values equal `reify check`'s on the same file (modulo check-side refine once γ1 lands: assert against a refine-off check invocation of the seam) |
| BT-2 | Warm edit ≡ full recompile (the differential self-oracle) | fixture loaded, measured; then a warm `edit_param` that changes geometry; re-measure | verdicts + measured values equal a fresh session that loads the post-edit source and measures — byte-equal where the underlying values are deterministic |
| BT-3 | Measurement does not perturb the viewport | any fixture; interleave: rebuild → measure → forced rebuild | meshes + values of the post-measurement rebuild identical to a never-measured control; demand roots and capture/refine flags restored |
| BT-4 | Capture is observation-only | bounded fixture | mesh bytes with `capture_repr_tol=true` identical to `capture_repr_tol=false` |
| BT-5 | Staleness lifecycle | measure → warm edit → re-measure, via debug-MCP | after edit: verdict retained + `stale=true`; after re-measure: fresh verdict, `stale=false`; epochs monotone |
| BT-6 | Cancellation | slow fixture (thickness or tight-precision bound); edit issued mid-measurement | edit completes without waiting for the pass; no fresh verdicts published; attribution `cancelled` or prior state retained; a later idle pass completes normally |
| BT-7 | Degradation attribution | GUI engine with no kernel / no openvdb feature | C-STATUS `unmeasured(cause)` naming the missing capability; no false verdict; mirrors check's C1 exit-clean behavior |
| BT-8 | Kind gating (the required negative) | module with none of the three kinds | no measurement pass is scheduled on idle; no capture; behavior byte-identical to today |

## 10. Decomposition plan

Greek labels; real task ids assigned at decompose time and backfilled here. **Leaf** = names a
user-observable signal. All leaves: no new `.ri` syntax (`grammar_confirmed: true`).

**α — Extract the measurement seam; split capture from refine.** *(Leaf. deps: none)*
C-SEAM + C-FLAGS. Rewire `cmd_check`; introduce `MeasureOptions`/the refine flag (inert until
γ1 lands the loop).
*Signal:* existing `reify check` CLI harness suites pass unmodified (they pin today's output);
a seam-level test drives the GUI-shape invocation (`refine=off`) on a measurement fixture and
gets check-equal verdicts.
*Modules:* `crates/reify-eval` (engine surface), `crates/reify-cli/src/main.rs`.

**β — GUI session measurement pass, end-to-end.** *(Leaf. deps: α, #6667)*
C-STATE + C-CANCEL + C-MCP + the C-STATUS backend half: `EngineSession` measurement pass, Tauri
command, debug-server `measure_constraints`/`measurement_status`, results into the constraint
payload + events.
*Signal:* against a debug-build GUI via debug-MCP: load a fixture carrying a violated DFM rule, a
ReprWithin bound, and a geometric Conforms; `measure_constraints(wait=true)`; `measurement_status`
and the constraint-panel state show **measured** verdicts matching `reify check` on the same file
— without leaving the GUI.
*Modules:* `gui/src-tauri` (engine.rs, invoke handlers, debug_server.rs).

**γ — Staleness epochs.** *(Leaf. deps: β)*
C-STALE: generation counter, epoch stamping, stale flag through payload + events, re-measure
retrigger.
*Signal:* debug-MCP: measure → warm `edit_param` → payload shows the measured verdict retained
with `stale=true`; re-measure clears it. (BT-5.)
*Modules:* `gui/src-tauri/src/engine.rs`, payload types.

**δ — Frontend: idle trigger + measure-now + stale rendering + progress.** *(Leaf. deps: β, γ)*
Debounced idle auto-measure (default on, kind-gated), "Measure now" affordance, dimmed stale
style + re-measure affordance, in-progress state; composes #6667's attribution wording for the
unmeasured state.
*Signal:* in the running GUI: edit a param, wait the idle delay, the constraint panel shows a
measured verdict with no further user action; after another edit the verdict dims and the
re-measure affordance appears. Pinned by vitest (`scripts/gui-test.sh`) + a debug-MCP runtime
test.
*Modules:* `gui/src` (constraint panel, idle scheduling), minimal `gui/src-tauri` glue.

**ε — Thickness-DFM / OpenVDB leg.** *(Leaf. deps: β, #6666)*
The seam's `ensure_openvdb_kernel` arm exercised under the GUI configuration, on an isosurface
fixture that #6666 makes realizable in the viewport.
*Signal:* an isosurface design carrying a `min_feature_size` DFM rule shows the same measured
thickness verdict in the GUI panel (via debug-MCP) that `reify check` reports.
*Modules:* `gui/src-tauri`, test fixtures.

**ζ — Differential-oracle + no-perturbation gates.** *(Leaf. deps: β, γ)*
BT-1..BT-4 + BT-8 as gate-resident tests (BT-5..BT-7 land with their owning leaves).
**Same-diff drift-guard registrations per the overlay rule**: nextest heavy/smoke partition
entries for any new heavy integration test; `tests/infra/run-all-classification.manifest` bucket
row if any new `tests/infra/test_*.sh`; no new wall-clock upper bounds (assert structurally, as
precision-δ does).
*Signal:* the suite is green on the leaf's own branch and red under seeded violations (e.g. a
deliberate value-cell overlay write from the pass turns BT-3 red).
*Modules:* `gui/src-tauri` tests, `crates/reify-eval` tests, `.config/nextest.toml` /
`tests/infra` as applicable.

**η — Docs-truth surface.** *(Leaf. deps: δ, ε)*
Doc-chunk update(s) (`crates/reify-mcp/src/tools/chunks/`): the DFM / GD&T / precision topic
chunks gain the GUI measurement workflow in intent terms ("check manufacturability while
designing": idle measurement, measure-now, staleness, when `reify check` is still the tool);
`.claude/skills/reify-design/SKILL.md` index line. No exemplar-corpus leaf: no new `.ri`
authoring idiom is introduced (the constraint kinds already exist; only where they get measured
changes).
*Signal:* discoverability acceptance — an author who knows the goal but not the feature name
finds the mechanism from the chunks/index.

**θ — Companion corrections (docs + task records; no code).** *(Leaf. deps: α, β)*
(1) Precision PRD §4.6: dated correction note on the GUI-viewport row (superseded by this PRD,
per its own revisit clause) and on the "F's scoping is structural" claim (capture is no longer
cmd_check-only; byte-identity now rests on C-FLAGS/BT-4) — a note added beside the original text,
never a rewrite of the as-authored rationale. (2) Amend #6171 (precision γ1): gate the refine
loop on the refine flag, never bare `capture_repr_tol` (D2). (3) Record the #6667 → #6169
dependency edge. 
*Signal:* committed docs edit + updated task records readable via `get_task`.

**ι — PRD-close stamp.** *(Leaf. deps: every other leaf)*
Terminal-status obligations per the overlay: backfilled leaf IDs, terminal token, AS-AUTHORED
freeze + LIVE map, matching capability-manifest header.
*Signal:* the committed header.

Dependency DAG: α → β → {γ, ε}; {β, γ} → {δ, ζ}; {δ, ε} → η; {α, β} → θ; all → ι.
Out-of-batch: β ← #6667; ε ← #6666 (real `add_dependency` edges at decompose time).

## 11. Out of scope

- **Refine in the GUI** — ruled off (D2). Revisiting is a future ruling against measured idle-time
  budgets, not a tactical decision.
- **GUI export refusal / export parity** (#6190) and anything on the export surface.
- **`reify eval`/`report`/`explain` measurement arms** — driver-contract program (all-get-all
  implementation) territory.
- **The cross-driver parity gate itself** — future driver-contract PRD; this PRD only ships the
  debug-MCP surface it will drive.
- **Purpose-scoped measurement / the GUI purpose surface** (matrix ruling 4) — its own charter;
  only the staleness UX pattern is shared.
- **Measurement-scope narrowing via observed demand** (4532 / selective-demand) — later
  optimization.
- **FEA measurement/caching** — different constraint family; the D12 FEA-cache item stays with
  its own task.

## 12. Open questions (tactical)

1. **Idle-delay constant and a user-facing auto-measure toggle.** Default-on with a fixed
   debounce ships v1; whether the delay/toggle surfaces in settings is a UX call. Decide during δ.
2. **Epoch mechanism.** Reuse the existing freshness/hash tracking vs a plain counter on
   `EngineSession`. Decide during γ.
3. **Seam name + exact `MeasureOptions` shape** (e.g. whether kind-gating flags are caller-visible).
   Decide during α.
4. **Whether the pass's handle-populating build can reuse live realization handles** already
   populated by the last viewport pass instead of re-building. Optimization; correctness is owned
   by BT-2/BT-3 either way. Decide during β or defer.
5. **Where the `measuring` progress state renders** (per-constraint spinner vs panel-level bar).
   Decide during δ.
