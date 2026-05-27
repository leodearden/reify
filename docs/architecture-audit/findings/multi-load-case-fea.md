# Audit: Multi-Load-Case Workflow for FEA

**PRD path:** `docs/prds/v0_3/multi-load-case-fea.md`
**Auditor:** audit-multi-load-case-fea
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 12 (mechanisms that are NOT WIRED)

## Top concerns

- The PRD-named entry point `solve_load_cases(body, material, cases, options) -> MultiCaseResult` has **no implementation anywhere in the codebase** (zero hits for `solve_load_cases` in `crates/*/src/`). Task 3005 is `pending`, gated on FEA #2924 which is also still pending. Six other mechanisms (validation suite, end-to-end example, GUI dropdown, docs page, diagnostic mapping) inherit this blocker.
- Same shape as GR-001: the PRD assumes a runtime entry point that does not exist. Worse, this is the *upstream* producer of `MultiCaseResult`, so every downstream stdlib helper (envelope_*, linear_combine, worst_case, case_names, result_for) has been built against a value shape that is currently only constructible synthetically in tests.
- `LoadCase.loads : List<Real>` and `LoadCase.supports : List<Real>` are deliberate **PARTIAL/DRIFT placeholders** (the stdlib `.ri` file documents this explicitly): the PRD called for `List<Load>` and `List<Support>` with nominal trait bounds, but the code uses `List<Real>` pending `trait def Load` / `trait def Support`. Combined with Reify's nominal-only trait system (per audit-brief givens), this is a wider type-system gap, not just a TODO.
- The PRD's design-loop demo (`param thickness : Length = auto` and `minimize mass(...) subject to max(envelope_von_mises(results)) < material.yield_stress`) mixes grammar-supported and fictional surfaces: `param thickness : Length = auto` is grammar-supported at the param-default position (via `auto_keyword`); `subject to` is fictional — `minimize` accepts a `where_clause` per `crates/reify-syntax/src/ts_parser.rs:1624-1636`. The example file `examples/m6/multi_load_bracket.ri` does not exist. *(2026-05-27 update: broader `auto` binding-site coverage beyond param-default — sub-overrides, named-args, let, connect-param — is being addressed by `docs/prds/auto-binding-site-positions.md`, α task 3802 landed.)*
- The cache-reuse mechanism the PRD promises ("volume-mesh ComputeNode cache hits once and is reused for every case's assembly") cannot be verified until `solve_load_cases` lands, AND inherits the entire ComputeNode-infrastructure dependency stack (3377-3385) plus GR-001.

## Mechanisms

### M-001: `LoadCase` stdlib structure_def

- **State:** PARTIAL
- **Failure mode:** F1/F4 (compile-time contract exists but with drift from PRD type spec)
- **Evidence:** `crates/reify-compiler/stdlib/fea_multi_case.ri:75-105` (declared); task 3004 done (commit 9e901fe84f, found_on_main); but PRD §"Sketch" says `loads : List<Load>` / `supports : List<Support>` and the code uses `List<Real>` placeholders (file:90, 98) with explicit `TODO(load-trait)` notes. Header note 1 documents the drift.
- **Blocks:** runtime usage of the field; transitively blocks any solver consumer that wants typed dispatch on load/support kinds (M-007, M-013)
- **Note:** Struct exists and parses; deviates from PRD type signature in two field slots. The drift is documented in-file as a precedent, but the PRD itself is silent on the relaxation.

### M-002: `MultiCaseResult` stdlib structure_def

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/stdlib/fea_multi_case.ri:247-254`; task 3004 done; tests at `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs`
- **Note:** Type-resolves cleanly with `cases : Map<String, ElasticResult>`; structurally guaranteed key-uniqueness via `BTreeMap`.

### M-003: `LoadCase` / `MultiCaseResult` runtime constructor evaluation (e.g. `LoadCase{name: ..., loads: [...], supports: [...]}`)

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes user code calls these constructors; runtime path absent)
- **Evidence:** GR-001 (struct-constructor runtime eval is broken). The PRD's "User pattern" block (lines 52-57) and decomposition task #7 both rely on this. Task 3018 (end-to-end example) is `deferred` and explicitly references this dependency. PRD task #1 planning context in task 3004 acknowledges Map keys uniqueness only when the producer (#2 / 3005) eventually builds the struct value.
- **Blocks:** 3005, 3018, 3015 (every consumer that needs to instantiate the types from user code)
- **Note:** Even though the type *resolves*, `LoadCase(...)` and `MultiCaseResult(...)` evaluate to `Value::Undef` per GR-001. Test smoke fixtures synthesize the shape directly as `Value::Map` to side-step this.

### M-004: `solve_load_cases(body, material, cases : List<LoadCase>, options : ElasticOptions = .default) -> MultiCaseResult` stdlib helper

- **State:** FICTION
- **Failure mode:** F1 (PRD-named entry point absent; the central mechanism of this PRD)
- **Evidence:** Zero implementation hits for `solve_load_cases` in `crates/*/src/` (only PRD-text references and tests' string mentions). Task 3005 status `pending` with dependencies 2924 (also pending), 3004; metadata claims expected files `crates/reify-stdlib/src/fea.rs`, `crates/reify-compiler/stdlib/fea.ri`, `crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs`.
- **Blocks:** 2924 (FEA #16 engine integration, also pending), 3015 (validation suite), 3018 (end-to-end example, deferred), 3022 (docs), 3026 (GUI dropdown), 3029 (diagnostic mapping)
- **Note:** This is the *only* documented producer of `MultiCaseResult`. Until 3005 lands, every downstream consumer in stdlib has been built against a value shape constructible only via test synthesis. Inherits GR-001 transitively (LoadCase struct value cannot be constructed at runtime).

### M-005: `solve_elastic_static(...)` stdlib entry (single-case solver call)

- **State:** FICTION
- **Failure mode:** F6 (ComputeNode infrastructure leaned on but absent; pre-condition for this PRD)
- **Evidence:** Zero implementation hits for `solve_elastic_static` as a callable function (only doc-strings in `warm_state.rs:82` and `mesh_morph/src/options.rs:109` and a PRD-text mention). Task 2924 pending. Cross-references the FEA PRD's task #16; this PRD calls it a pre-condition (line 64).
- **Blocks:** 3005 (transitively — solve_load_cases loops this), 3015 (validation suite uses both)
- **Note:** Out-of-scope-for-this-PRD architecturally but a hard pre-condition. Audit-brief worked example for FEA PRD covers it as the canonical FICTION/F6 case.

### M-006: ComputeNode dispatch + `@optimized("solver::elastic_static")` registration for FEA

- **State:** PARTIAL
- **Failure mode:** F6 (infrastructure partly built; stdlib `fn` integration absent)
- **Evidence:** `@optimized` mechanism exists (`crates/reify-eval/src/graph.rs:33`, `engine_admin.rs:398`, `optimized_registry_tests.rs`). ComputeNode-infrastructure PRD tasks 3379-3385 mostly done per audit-brief givens; 3377 done, 3379/3383/3384 cancelled-as-superseded by compute-node-contract §8 DAG, 3426 (pending) supersedes 3378 (deferred). Task 2924 (FEA #16) pending with full dependency chain.
- **Blocks:** 3005 (cache-reuse claim in PRD §"Cache reuse is the natural common case" relies on this for volume-mesh reuse), 3015
- **Note:** Cross-PRD breadcrumb (see Phase 2 of compute-node-infrastructure PRD). The cache-key composition machinery `crates/reify-eval/src/compute_cache_key.rs` is partially in place.

### M-007: Per-case effective-options resolution (case.options Some → use; None → inherit shared)

- **State:** FICTION
- **Failure mode:** F1 (PRD specifies behavior; producer absent)
- **Evidence:** Spec'd in PRD §"Per-case options are optional" and in task 3005 details; no callsite extracts `LoadCase.options` and threads it to a solver call because no callsite exists. `crates/reify-stdlib/src/fea.rs` has no `extract_options` or `effective_options` symbol.
- **Blocks:** 3005 (the producer side); 3011 (linear_combine compatibility pre-check claims to inspect per-case options — see M-010)
- **Note:** Compatibility-matrix knob handling (cg_tolerance OK per-case vs mesh_size disables superposition) is the policy this mechanism would implement.

### M-008: `envelope_max` / `envelope_min` over `Map<String, Field<Point3, T : Ordered>>`

- **State:** PARTIAL
- **Failure mode:** F4 (drift from PRD type signature; runtime behavior present)
- **Evidence:** `crates/reify-stdlib/src/fea.rs:41-42` (dispatch), `envelope_reduce` body. Task 3006 done (commit 61c069353f). However, the PRD signature requires `T : Ordered` constraint, and there is no `Ordered` trait declared anywhere (`grep -rn "trait def Ordered\|\"Ordered\""` returns zero hits). Runtime acceptance is shallow-kind-check, not trait-bounded.
- **Blocks:** none directly (callers can still work); but type-level guard claimed by PRD is fiction
- **Note:** Empty-map / single-case / non-Map handling collapses to `Value::Undef` per PRD task #10 deferral.

### M-009: Convenience envelope helpers (`envelope_von_mises`, `envelope_max_principal`, `envelope_displacement_magnitude`)

- **State:** PARTIAL
- **Failure mode:** F4 (returned type signature in PRD differs from current runtime shape)
- **Evidence:** `crates/reify-stdlib/src/fea.rs:46-48, 368-422`. Task 3007 done (commit 3559ad7e70). PRD signature returns `Field<Point3, Pressure>` / `Field<Point3, Length>`; runtime returns `Value::Map` with sampled-grid metadata because `Field<X,Y>` in param position is still gated by task 3117 (deferred). Stress codomain is `Real`-placeholder pending field-in-param (TODO(field-in-param) at solver_elastic.ri:229).
- **Blocks:** type-checked composition with downstream `max(envelope_von_mises(...))` is shallow; numeric round-trip works.
- **Note:** Functional but not strongly-typed per PRD letter.

### M-010: `linear_combine(base_results, weights) -> ElasticResult` linear superposition

- **State:** PARTIAL
- **Failure mode:** F4 (compatibility pre-check semantics deviates from PRD)
- **Evidence:** `crates/reify-stdlib/src/fea.rs:45, 108-330`. Task 3011 done (commit 9b386c0a02). PRD requires checking "all referenced base cases have compatible mesh / element-order layout (option-derived; LoadCase.options either none or matching across the referenced subset)" — but per the stdlib doc comment (file:154-160), the implemented check is grid-metadata equality (a proxy because per-case options aren't captured yet — M-007 is fiction). Output `frame` is `Undef`, `max_von_mises` is scalar-only (Tensor recomputation deferred to task 3117); `iterations` is `Undef`; `converged=true` synthesized.
- **Blocks:** 3015 (validation suite asserts behavior + 3029 diagnostics depend on real per-case option capture)
- **Note:** Silent-Undef on shape failures per PRD task #10 deferral. Diagnostic emission absent. Mesh-incompatibility diagnostic claimed by PRD task #10 (3029) but not landed.

### M-011: Superposition validation suite (`1.4·A + 0.7·B` direct vs `linear_combine` round-trip)

- **State:** FICTION
- **Failure mode:** F1 (PRD describes a regression suite; nothing yet)
- **Evidence:** Task 3015 pending, gated on 3011 (done) and 2928 (pending — FEA analytical validation). Expected file `crates/reify-eval/tests/multi_load_case_superposition_validation.rs` does not exist.
- **Blocks:** documentation (3022) credibility; absorbs M-004, M-005 blockers transitively (no `solve_elastic_static` to compare against)
- **Note:** Test fixtures (cantilever, pressurised cylinder) are themselves contingent on FEA validation suite #2928 which is pending.

### M-012: `worst_case(mcr, scalar_fn) -> String` lambda-dispatched accessor

- **State:** PARTIAL
- **Failure mode:** F4 (lambda parameter-type syntax limitation forces user workaround)
- **Evidence:** `crates/reify-expr/src/lib.rs:435, 925-1027` (`eval_worst_case_dispatch`); `crates/reify-stdlib/src/fea.rs:49-56` (permanent Undef stub for the eval_fea arm). Task 3007 done. Critical caveat documented at `crates/reify-compiler/stdlib/fea_multi_case.ri:226-238`: current Reify lambda parameter-type syntax only accepts bare named types resolvable by `resolve_type_name`; `|e| e["displacement"]` is rejected because untyped lambda params default to `Type::Real`. Users must pre-bind per-case values and pass identity `|f| f`.
- **Blocks:** ergonomic call patterns implied by PRD (task #3007 follow-up); not blocking any task per se
- **Note:** This is documented DRIFT — implementation works under a workaround that the PRD never anticipated. The function works for the identity case; richer lambda parameter-type syntax is orthogonal future work.

### M-013: `case_names(mcr)` / `result_for(mcr, name)` accessor free-functions

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/fea.rs:43-44, 741, 778`. Task 3004 done (includes accessor smoke tests in `crates/reify-eval/tests/multi_load_case_stdlib_smoke.rs`).
- **Note:** Silent-Undef on miss, deterministic lex order via BTreeMap. The only true full-WIRED gap-free mechanism on the consumer side.

### M-014: Volume-mesh ComputeNode cache reuse across cases when `body, material, options.element_order, options.mesh_size` match

- **State:** FICTION
- **Failure mode:** F6 (mechanism claimed will "just work"; verifier and producer both absent)
- **Evidence:** PRD §"Cache reuse is the natural common case" (lines 96-97); task 3005 detail "Verify volume-mesh ComputeNode cache hits exactly once across the two solves" with TBD instrumentation. `compute_cache_key.rs` machinery exists but no producer wires it for FEA per M-006.
- **Blocks:** 3005 success criterion includes the regression test for this
- **Note:** Cache-hit telemetry path the test will use is unclear from code; `crates/reify-eval/src/cache.rs:20` mentions ComputeNode caching context P3.3 only.

### M-015: End-to-end example `examples/m6/multi_load_bracket.ri` with `param thickness = auto` + `minimize mass subject to max(envelope_von_mises(results)) < yield_stress`

- **State:** FICTION (multiple sub-mechanisms; PRD treats as one demo)
- **Failure mode:** F1 (entire example absent) + F4 (grammar drift on `subject to`) + F2 (stdlib type absence for `BodyForce`/`Gravity`)
- **Evidence:** File `examples/m6/multi_load_bracket.ri` does not exist. Directory `examples/m6/` does not exist. Task 3018 status `deferred`. The `subject to` clause does not exist in the grammar — `minimize` accepts a `where_clause` per `crates/reify-syntax/src/ts_parser.rs:1624-1636`. PRD also references `BodyForce`/`Gravity` load types (5g acceleration case in task 3018 detail) which have no stdlib presence. *(2026-05-27 update: `param thickness : Length = auto` is grammar-supported at the param-default position via `auto_keyword`; this sub-mechanism is no longer a grammar fiction. The fictional sub-mechanisms here are `subject to` and `BodyForce`/`Gravity`. Broader `auto` binding-site coverage is in `docs/prds/auto-binding-site-positions.md`, α task 3802 landed.)*
- **Blocks:** the design-loop story of the entire PRD (Goal section claim "should be a one-liner")
- **Note:** Three distinct gaps bundled: (a) the example file itself awaits 3005 + 3007 + 2929; (b) `subject to` is fictional (use `where`); (c) `BodyForce`/`Gravity` load types have no stdlib presence. The `= auto` portion is NOT a grammar fiction and is not tracked as a blocker.

### M-016: GUI case-picker dropdown for `MultiCaseResult` (FEA-mode toggle, contour swap, visual regression)

- **State:** FICTION
- **Failure mode:** F1 (PRD-spec'd UI absent)
- **Evidence:** Task 3026 pending. No matches for `FeaCasePicker`, `active_case`, `case_picker`, `case-picker` in `gui/src/`, `gui/src-tauri/src/`. Gates 2962 (stress contour rendering) and 2961 (FEA-mode toggle, done) — 2962 still pending. Visual regression harness 2954-2958 status unverified by this audit but the PRD lists it as a gate.
- **Blocks:** PRD §"GUI integration: minimum-viable case picker" deliverable
- **Note:** Engine-side multi-case → ElasticResult discrimination at the Tauri boundary is not yet present; expected files in metadata (`gui/src/panels/FeaCasePickerDropdown.tsx`, `gui/src/stores/engine.ts`, `gui/src/viewport/FeaModeToolbar.tsx`).

### M-017: Diagnostic surface for multi-case-specific failure modes (empty cases, dup names, mesh-incompat, unknown-case-in-weights, empty weights)

- **State:** FICTION
- **Failure mode:** F1 (PRD-spec'd diagnostics; current path is silent-Undef)
- **Evidence:** Task 3029 pending. All current envelope/linear_combine/accessor implementations explicitly choose `Value::Undef` on shape failure per task #10 deferral (multiple comments in `crates/reify-stdlib/src/fea.rs`). Expected impl files `crates/reify-eval/src/engine_eval.rs`, `engine_build.rs`. No `multi-load-case::` error codes anywhere in the source.
- **Blocks:** UX of every consumer; integrates with #2929 (FEA diagnostic infra, pending)
- **Note:** Silent-Undef discipline is intentional and centrally documented; the PRD always intended it as a v0.3.x deferral, but a v0.3.x release without diagnostics is degraded.

### M-018: Stdlib docs page (`docs/stdlib/multi-load-fea.md` or equivalent)

- **State:** FICTION
- **Failure mode:** F1 (artifact absent; non-runtime)
- **Evidence:** Task 3022 pending. `docs/stdlib/` directory does not exist. Cross-link target from the v0.3 FEA single-case page is also absent (FEA PRD's #2929 deliverable not landed).
- **Blocks:** discoverability only; not a runtime mechanism but a deliverable the PRD names.
- **Note:** Listed for completeness; lowest urgency mechanism.

## Cross-PRD breadcrumbs

- **GR-001 (struct-constructor runtime evaluation)** — transitively blocks M-003, M-004, M-007. Producer-side fiction is the single largest source of latent failures.
- **`structural-analysis-fea.md`** — owns `solve_elastic_static` (M-005), `@optimized` registration (M-006 / task 2924), `ElasticResult` / `ElasticOptions` (done as M-001/M-002's precondition), Field-max/min reductions (#2913 done), FEA analytical validation suite (#2928 pending — blocks M-011), bracket fixture (#2929 pending — blocks M-015), diagnostic infra (#2929 — blocks M-017). This PRD is a strict consumer of structural-analysis-fea.
- **`compute-node-infrastructure.md`** — owns the ComputeNode dispatch surface (M-006), cache-key composition + cache reuse (M-014). Phase 3 should treat the volume-mesh cache reuse claim here as inherited from that PRD.
- **`fea-gui-rendering.md`** — owns the FEA-mode toggle (done #2961), stress contour (pending #2962), visual regression infra (#2954-2958), which gate M-016. Multi-load case GUI is a strict extension over fea-gui-rendering.
- **`per-purpose-tolerance.md`** — referenced indirectly via `cg_tolerance` and mesh-size derivation in `ElasticOptions`. Not exercised by this PRD beyond ElasticOptions defaults.
- **`structural-analysis-shells.md`**, **`hex-wedge-meshing.md`**, **`mesh-morphing.md`**, **`a-posteriori-error-estimation.md`** — listed in PRD §"Relationship to other PRDs" as compositional but not consumed at runtime.
- **`Load` / `Support` nominal-trait system (M-001 drift)** — Reify's nominal trait system (audit-brief givens) cannot easily provide structural conformance for kind-tagged Maps. Cross-cuts whatever solution emerges for GR-001.
- **`subject to` clause / `BodyForce` / `Gravity` (M-015 sub-gaps)** — language-surface fictions referenced by this PRD that are not tracked in any decomposition task here. Phase 3 may want to surface these as separate gaps with their own provenance. *(2026-05-27 update: `= auto` at the param-default position is grammar-supported and has been removed from this list. Broader `auto` binding-site coverage is being formalized by `docs/prds/auto-binding-site-positions.md`.)*
