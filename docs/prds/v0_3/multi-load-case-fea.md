# PRD: Multi-Load-Case Workflow for FEA

Status: design resolved + decomposed (2026-05-05) — deferred, candidate v0.3.x. Stdlib pattern over the v0.3 FEA kernel; no language-feature changes. Filed 2026-05-02 from FEA PRD spillover.

> **2026-05-12 grammar-fiction sweep** (docs/architecture-audit/phase-3-grammar-fiction-triage-log.md):
> Design-loop demo previously used `subject to` clause which is not in the
> Reify grammar. Replaced with the shipped `where`-clause spelling of
> `minimize` (per `crates/reify-syntax/src/ts_parser.rs`). `param thickness : Length = auto` retained — `auto` as a value-default keyword
> is supported via `auto_keyword` in `tree-sitter-reify/grammar.js:430`. The
> distinct gap of `auto:` in **type-arg position** is owned by the
> auto-type-param-resolution PRD's grammar-chain follow-up. Also retired the
> assumption of `sum(... for ... in ...)` comprehension; see money-dimension
> PRD's parallel rewrite.

## Goal

Make multi-load-case structural analysis ergonomic. Real designs are evaluated against multiple load conditions — operating, transport, accident — and the design constraint is typically "max von Mises across all cases < yield_stress." Currently this requires manual orchestration; should be a one-liner.

## Background

Real engineering FEA workflows almost never use a single load case. Examples:
- A bracket: operating load (5 kN normal), overload (10 kN, 2× safety factor), transport drop (5g acceleration).
- A pressure vessel: working pressure, proof-pressure test, burst test.
- A wing spar: cruise, manoeuvre limit load, gust loads at multiple frequencies.

Designs are typically validated against the *envelope* across all cases — max stress at any point under any load. The auto-resolve loop's design driver is "minimize mass subject to envelope < yield_stress."

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships single-case `solve_elastic_static`. Composing multi-case workflows is technically possible — `forall load in load_cases: solve_elastic_static(...)` then take per-point max — but verbose and easy to get subtly wrong (forgetting a case, mismatching envelope semantics, etc.).

Designers think in load cases. Reify's stdlib should too.

## Why deferred (and separate from FEA PRD)

- v0.3 FEA PRD focused on the fundamental kernel; multi-load is an ergonomic layer on top.
- Design surface decisions (helper function vs. dedicated language form vs. forall pattern) benefit from seeing how the v0.3 single-case API actually feels in use.
- Small enough to land as a stdlib addition once chosen direction is clear.

## Sketch of approach

A pure-stdlib layer over the v0.3 FEA kernel. New types `LoadCase` and `MultiCaseResult`; a helper `solve_load_cases` that loops `solve_elastic_static` per case while reusing the volume-mesh ComputeNode across cases; field-reduction primitives for envelope construction; a `linear_combine` superposition primitive valid only for linear-elastic results; a single GUI dropdown for per-case inspection.

```
struct LoadCase {
    name     : String
    loads    : List<Load>
    supports : List<Support>
    options  : Optional<ElasticOptions> = none   // none = inherit shared options
}

struct MultiCaseResult {
    cases : Map<String, ElasticResult>
}

solve_load_cases(
    body, material,
    cases   : List<LoadCase>,
    options : ElasticOptions = .default,
) -> MultiCaseResult
```

User pattern:
```
results = solve_load_cases(bracket, Steel_AISI_1045, [
    LoadCase{name: "operating", loads: [...], supports: [...]},
    LoadCase{name: "overload",  loads: [...], supports: [...]},
    LoadCase{name: "transport", loads: [...], supports: [...]},
])

minimize mass(bracket) where max(envelope_von_mises(results)) < material.yield_stress
```

> **Grammar note.** The `where`-clause spelling is what the current
> `minimize` form (`crates/reify-syntax/src/ts_parser.rs::lower_minimize_decl`)
> parses. An earlier draft of this PRD used `subject to` which is **not** in
> the grammar; the rewrite preserves the design intent.

## Pre-conditions for activating

- v0.3 FEA kernel shipped (`structural-analysis-fea.md` tasks #2911 ElasticResult, #2924 engine integration, #2929 end-to-end demo). Multi-case is a thin layer over that kernel; no kernel changes.
- Field reductions in stdlib (task #2913 of v0.3 FEA PRD).
- Topology selectors expressive enough to attach loads/supports per case (already true after v0.2).

## Resolved design decisions (2026-05-05)

**Direction A (stdlib helper) over Direction B (dedicated grammar).** Same precedent as v0.3 FEA boundary conditions: kinematic-constraints earned its dedicated form through usage volume; multi-load hasn't. Plain stdlib composes with existing field reductions and ComputeNode caching without grammar work. Revisit Direction B if v0.4+ usage volume demands it.

**Named struct types over loose maps.** `LoadCase` (input) and `MultiCaseResult` (output) are stdlib structs, not inline record literals or `Map<String, ElasticResult>`. Matches the rest of v0.3 FEA (`ElasticResult`, `ElasticOptions`, `ElasticMaterial`); gives a place to hang per-case option overrides and provenance metadata; lets the type system catch envelope-vs-case-pick mistakes.

**Envelope primitives are compositional.** Two stdlib primitives over per-case fields:
```
envelope_max(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T> where T : Ordered
envelope_min(fields : Map<String, Field<Point3, T>>) -> Field<Point3, T> where T : Ordered
```
Plus three convenience helpers for common combinations: `envelope_von_mises(results)`, `envelope_max_principal(results)`, `envelope_displacement_magnitude(results)`. Generalises naturally — any future scalar field can be enveloped by composition; no hardcoded list of "supported envelope functions." Plus `worst_case(results, scalar_fn) -> String` returning the case name with the global maximum of the chosen scalar.

**Linear-elastic superposition shipped as `linear_combine`.** For linear-elastic FEA, `result(αA + βB) = α·result(A) + β·result(B)` — combining pre-solved base results is just field arithmetic, no solver work. Big speedup for design-code combination sweeps (LRFD-style `1.4D + 1.7L`). API:
```
linear_combine(
    base_results : MultiCaseResult,
    weights      : Map<String, Number>,
) -> ElasticResult
```
Shipped because cheap (single primitive over field arithmetic), big speedup for users running combination sweeps, and naturally signals linearity in its name. **Strictly linear-elastic only**; documented in API doc and enforced at the type level once non-linear solvers ship in v0.4+ (each non-linear `solve_*_static` returns its own result type with no `linear_combine` overload). LRFD/ASD code-specific factor tables remain out of scope per parent FEA PRD.

**Per-case options are optional and per-knob differentiated.** `LoadCase.options : Optional<ElasticOptions> = none` defaults to the shared options passed to `solve_load_cases`. Compatibility matrix:

| Option override | Per-case OK? | Effect |
|---|---|---|
| `cg_tolerance`, `max_iter`, `threads`, `#deterministic` | yes | per-case independent; envelope and superposition unaffected |
| `mesh_size`, `element_order` | allowed but disables superposition | different DOF layout per case; `linear_combine` rejects with diagnostic if base meshes differ |

**Cache reuse is the natural common case.** When `body`, `material`, `options.element_order`, `options.mesh_size` match across cases, the volume-mesh ComputeNode cache hits once and is reused for every case's assembly (the v0.3 FEA cache key already excludes loads/supports from the mesh hash). Most multi-case workflows share BCs across cases and only vary loads, so the warm-start chain is naturally efficient — first case pays full cost, subsequent cases reuse mesh + (when supports unchanged) factorisation.

**GUI integration: minimum-viable case picker, full envelope visualisation deferred.** Add a "Case" dropdown to the FEA-mode toggle in the existing FEA viewer; pick which case's stress field to render. Cheap (one dropdown, swap which `ElasticResult` sources the contour) and answers the PRD's own question about per-case stress-field inspection. Full envelope view, worst-case-region highlight, and side-by-side case comparison stay v0.4 (already noted as out-of-scope in `fea-gui-rendering.md`). The FEA-GUI PRD's existing right-sidebar tab and FEA-mode toggle host this without architectural change.

**Out of scope clarified — sequenced static cases.** Independent multi-load cases (this PRD) differ from sequenced static cases where state carries from one case to the next (plastic work hardening, residual stress accumulation). Sequenced cases need a non-linear solver; they belong with the future plasticity PRD, not here.

**Time-history transient loading remains out of scope.** Sibling PRD `structural-analysis-transient.md` if it materialises.

## Decomposition plan

Ten tasks. All depend on v0.3 FEA kernel landing first; the GUI task additionally depends on FEA-GUI infrastructure. Internal dependencies form a small chain (types → solver helper → reductions → superposition + example).

**Stdlib types and solver helper:**

1. `LoadCase` and `MultiCaseResult` stdlib structs. `LoadCase` carries `name : String`, `loads : List<Load>`, `supports : List<Support>`, `options : Optional<ElasticOptions> = none`. `MultiCaseResult` carries `cases : Map<String, ElasticResult>` plus accessor methods. Constructor validation (non-empty case list, unique names per call). **Gate:** v0.3 FEA `ElasticResult` (#2911) and `ElasticOptions` (#2911 / #2914) shipped.
2. `solve_load_cases` stdlib helper: iterates `solve_elastic_static` per case, threading shared options with per-case overrides. Returns `MultiCaseResult`. Verifies that volume-mesh cache reuse occurs when cases share `body, material, options.element_order, options.mesh_size`. **Gate:** task 1 + v0.3 FEA engine integration (#2924).

**Envelope and reduction primitives:**

3. `envelope_max` and `envelope_min` stdlib reductions over `Map<String, Field<Point3, T : Ordered>>`. Per-point reduction across the case axis. Compose with per-case scalar projections (e.g. `von_mises_field`, `displacement_magnitude_field`). **Gate:** field reductions (#2913) shipped.
4. Convenience helpers: `envelope_von_mises(results) -> Field<Point3, Pressure>`, `envelope_max_principal(results) -> Field<Point3, Pressure>`, `envelope_displacement_magnitude(results) -> Field<Point3, Length>`, `worst_case(results, scalar_fn) -> String`. Each is a one-liner over task 3's primitives + existing v0.3 FEA scalar reductions; package gives users zero-friction common cases without precluding compositional use. **Gate:** task 3.

**Linear-elastic superposition:**

5. `linear_combine(base_results : MultiCaseResult, weights : Map<String, Number>) -> ElasticResult` linear-elastic superposition primitive. Field arithmetic over displacement and stress fields, weighted-sum reduction. **Validation pre-check:** rejects with a diagnostic if any pair of referenced base results has incompatible mesh / element-order layouts (task 1 records these on `LoadCase.options`). **Documentation:** linear-elastic-only constraint, with the v0.4+ migration path (non-linear solver result types will not provide `linear_combine` overloads). **Gate:** task 1.
6. Superposition validation suite: solve a manually-constructed `1.4·A + 0.7·B` load combination directly via `solve_elastic_static`, then compute the same combination via `linear_combine` over independent A and B results; assert displacement and stress fields match within solver-tolerance × weight-sum bound. Run on at least the cantilever and pressurised-cylinder reference scenes (already in #2928's analytic-validation suite). **Gate:** task 5 + #2928.

**End-to-end and documentation:**

7. End-to-end example file: `examples/m6/multi_load_bracket.ri`. Bracket with three load cases (operating, overload, transport), `param thickness : Length = auto` (the `auto` value-default keyword is in grammar.js:`auto_keyword`), `minimize mass(bracket) where max(envelope_von_mises(results)) < material.yield_stress`. Closes the design-loop demo from this PRD's Goal section. **Gate:** tasks 2 + 4 + #2929 (single-case bracket demo).
8. PRD-aligned documentation: stdlib doc page on multi-load cases covering (a) the basic `solve_load_cases` pattern, (b) envelope construction with the convenience helpers and the compositional primitives, (c) `linear_combine` with the linear-elastic constraint and combination-sweep example, (d) per-case options compatibility matrix. **Gate:** tasks 1-7.

**GUI integration (gates on FEA-GUI infrastructure):**

9. GUI case-picker dropdown for `MultiCaseResult`. When the engine output is a `MultiCaseResult` rather than a single `ElasticResult`, the FEA-mode toggle in the existing FEA viewer grows a "Case" dropdown listing case names; selecting a case sources the contour / deformed-shape view from that case's `ElasticResult`. No architectural change — extends FEA-mode state to hold an optional active-case selector. Visual regression baseline: bracket multi-load scene with operating-case selected, screenshot diff stable. **Gate:** task 1 + FEA-GUI #2961 (FEA-mode toggle), #2962 (FEA contour), #2954-2958 (visual-regression infra).

**Polish and follow-ons:**

10. Diagnostic mapping for multi-case-specific failure modes: empty case list, duplicate case names, mesh-incompatible cases attempted in `linear_combine`, weight map references unknown case name. Each maps to an actionable Reify diagnostic. **Gate:** tasks 1-5.

## Out of scope for this PRD

- Time-history / transient loading — separate PRD (`structural-analysis-transient.md` if it materialises).
- Sequenced static cases with state carry-over (plastic work hardening, residual stress) — belongs with non-linear plasticity PRD; needs a non-linear solver.
- Load combination factors per LRFD / ASD design codes — domain-specific add-on, post-v0.3.
- Probabilistic / reliability-based load combinations — research-grade, not in scope.
- Auto-detection of load cases from usage context — speculative; users specify their cases explicitly.
- Full envelope visualisation, worst-case-region highlight, and side-by-side case comparison in the GUI — v0.4 feature per `fea-gui-rendering.md`. v0.3.x ships the case-picker only.

## Relationship to other PRDs and tasks

- **Direct extension of `structural-analysis-fea.md`** — uses kernel (#2911, #2913, #2924, #2929) and field reductions as-is; only adds an ergonomic stdlib layer.
- **Composes with `fea-gui-rendering.md`** — the case-picker extends the FEA-mode toggle (#2961) and contour pipeline (#2962); visual regression rides on the harness from #2954-2958. Full envelope visualisation deferred to v0.4 alongside the `DualViewport` comparison work noted in that PRD.
- **Composes with `a-posteriori-error-estimation.md` (v0.4)** — error budget applied per case or shared across cases is a budget-design question; resolved when a-posteriori work activates. Not a v0.3.x blocker.
- **Touches `structural-analysis-shells.md` and `hex-wedge-meshing.md`** — multi-load works the same regardless of element type; ergonomic layer is element-agnostic. The per-case `options.element_order` mechanism extends naturally to future element kinds.
- **Composes with `mesh-morphing.md`** — when geometry morphs across an auto-resolve step, all cases' meshes morph together; warm-start chain is preserved per case. No additional integration work in this PRD.
- **Backend event channel seam-owned by `docs/prds/v0_3/gui-event-channel-inventory.md`** — the `fea-case-changed` channel (consumed by `FeaCasePickerDropdown`) is inventoried in inventory §2.2 Phase 3 task η, with this PRD's M-016 (`ValueData.case_id` discriminator wiring) as the upstream prereq. Emitter wiring is decomposed in the inventory PRD. See also `docs/gui-event-channels.md`.
- **Composes with `docs/prds/v0_3/geometry-handle-runtime.md` (GHR-ζ, GHR-θ)** — `LoadCase + Bracket : Physical` with real geometry is now unblocked. Spec-shape `Physical` with `param geometry : Solid` lands via GHR-ζ; consumer-side multi-case fixtures can build geometry-bearing brackets (`bracket : Physical` carrying a real `Solid`). The v0.1 `Physical` trait used flat scalar params as a deliberate placeholder before GHR; that workaround is retired in GHR-ζ.
