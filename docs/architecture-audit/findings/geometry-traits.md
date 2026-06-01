# Audit: Geometry Trait Declarations + Conformance Machinery

**PRD path:** `docs/prds/geometry-traits.md`
**Auditor:** audit-geometry-traits
**Date:** 2026-05-12
**Mechanism count:** 17
**Gap count:** 5

## Top concerns

- **The Unbounded primitives `half_space` and `extrude_infinite` named throughout the PRD (Background, Architectural decisions §3, Worked examples) do not exist in the codebase** (no `PrimitiveKind::HalfSpace`, no stdlib helper, no parser hook). Their absence has been knowingly documented as `TODO(geometry-traits-followup)` in `geometry_traits_inference.rs`, but it makes the headline acceptance criterion — "compile diagnostic `E_GEOMETRY_UNBOUNDED` when a value statically known to lack `Bounded` flows into a `Bounded`-requiring call site" — **trivially un-exercisable end-to-end** today. The diagnostic plumbing is fully wired (`emit_geometry_unbounded`, conformance walker hook, inference table), but every primitive currently in the table returns `Bounded=true`, so no source program in v0.1 can actually trigger it. The negative-path *test* is unit-level (`InferredTraits { bounded: false, ... }` synthesized by hand); the PRD's "Tests" §6 explicitly names `volume(half_space(...))` errors as the e2e shape and that test does not exist.
- **DRIFT from the PRD's `inferred_traits : TraitSet` field on `CompiledGeometryOp`** (§Scope.2 — "extend `CompiledGeometryOp` with an `inferred_traits : TraitSet` field; populate it during the compile pass"). The shipped design is a **pure function over `CompiledExpr`** (`infer_traits_for_expr_in_env`), with a parallel op-array walker (`infer_traits_for_op`). The module's doc-comment explicitly flags this as a "deliberate departure from the PRD's wording" (rationale: avoids 7-variant constructor refactor; consumer recomputes cheaply). Behaviour is equivalent for the current consumer, but anyone reading the PRD will look for a field that isn't there.
- **DRIFT: PRD §3 Trait flow rules say "`Tube` only when shell-thickness < radius" determines `Convex`** (i.e. solid tubes are convex, hollow ones are not). Code returns `InferredTraits::all()` (Convex=true) unconditionally for `PrimitiveKind::Tube`. There is no test pinning the hollow-tube non-convex case. Low impact (Convex is the least-consumed of the three compile-inferred traits) but a measurable correctness gap if any consumer ever specializes on it.
- **Test names DRIFT from PRD**. PRD §Acceptance and §Task 6 cite `is_watertight_user_assertion_short_circuits_to_true` and `is_watertight_closed_bound_does_not_short_circuit` as the pinning tests in `crates/reify-eval/tests/conformance_runtime.rs`. The actually-shipped names are `watertight_user_assertion_short_circuits_kernel_query` (in the integration file) and `try_eval_conformance_query_user_assertion_closed_does_not_short_circuit_is_watertight` (in `geometry_ops.rs` unit tests, **not** in the integration file). Functionality is fully pinned; documentation has rotted.

## Mechanisms

### M-001: Stdlib trait file `geometry_traits.ri` with seven marker traits + inheritance edge

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/stdlib/geometry_traits.ri:15-44` (all seven `trait` decls + `trait Watertight : Closed + Manifold`); `crates/reify-compiler/src/stdlib_loader.rs:95-98` (`("std.geometry.traits", include_str!("../stdlib/geometry_traits.ri"))`); `crates/reify-compiler/tests/geometry_traits_tests.rs` (~386 lines of smoke tests).
- **Blocks:** none
- **Note:** PRD §Scope.1 fully satisfied — loader picks it up via the static manifest in `stdlib_loader.rs`, all seven names resolve.

### M-002: Per-op compile-time trait inference table for Bounded/Connected/Convex

- **State:** PARTIAL
- **Failure mode:** DRIFT-shaped — PRD specifies an `inferred_traits` field on `CompiledGeometryOp`; shipped impl is a pure function (see M-014).
- **Evidence:** `crates/reify-compiler/src/geometry_traits_inference.rs` (676 lines); covers primitives (`Box`/`Cylinder`/`Sphere`/`Tube`), Booleans (`union`/`difference`/`intersection`/`union_all`/`intersection_all`), transforms, modifies, patterns, sweeps, curves; `crates/reify-compiler/tests/geometry_traits_inference_tests.rs` (1255 lines of unit tests).
- **Blocks:** none observed
- **Note:** Functionally complete for the **subset of primitives that exist**; Unbounded sources (M-006) are absent so half the table is theoretical.

### M-003: Per-op `combine_*` rules — union/difference/intersection propagation

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `geometry_traits_inference.rs:192-226` (`combine_union` AND-Bounded, drop Connected/Convex; `combine_difference` left-Bounded, drop Connected/Convex; `combine_intersection` OR-Bounded, AND-Convex, drop Connected); each rule is `pub const fn`, unit-tested in `geometry_traits_inference_tests.rs`.
- **Blocks:** none
- **Note:** Rules match the PRD §3 Boolean propagation table exactly.

### M-004: Per-op `combine_*` rules — transform/modify/pattern/sweep

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `geometry_traits_inference.rs:234-278` (`combine_transform` identity; `combine_modify` keeps Bounded+Connected, drops Convex; `combine_pattern` keeps Bounded only; `combine_sweep` inherits Bounded+Connected from profile, drops Convex).
- **Blocks:** none
- **Note:** Matches PRD §3 transform/sweep/fillet/chamfer/shell/draft/thicken rules.

### M-005: Primitive lookup table — Box/Cylinder/Sphere/Tube

- **State:** PARTIAL
- **Failure mode:** DRIFT for `Tube` — PRD says Convex iff shell-thickness < radius; impl is unconditional `all()` (Convex=true).
- **Evidence:** `geometry_traits_inference.rs:176-183` (single arm `Box | Cylinder | Sphere | Tube => InferredTraits::all()`); no shell-thickness branch.
- **Blocks:** none observed (no consumer specializes on Tube convexity today)
- **Note:** PRD §3 architectural decision text is `"Tube only when shell-thickness < radius"`; the impl silently treats every Tube as convex. No test fixture pins the hollow case.

### M-006: Unbounded primitives `half_space` and `extrude_infinite`

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes mechanism; code provides nothing)
- **Evidence:** No `PrimitiveKind::HalfSpace` or `PrimitiveKind::ExtrudeInfinite` in `crates/reify-compiler/src/types.rs:917` (`enum PrimitiveKind { Box, Cylinder, Sphere, Tube }`); no `"half_space"` or `"extrude_infinite"` arm in `geometry_traits_inference.rs` dispatch; no stdlib helper, no parser-side name registration. `geometry_traits_inference.rs:48-72` documents the absence as `TODO(geometry-traits-followup)`. `tests/geometry_traits_inference_tests.rs:107,422,468-471,1207-1212` repeatedly notes "deferred until `half_space` / `extrude_infinite` lands".
- **Blocks:** PRD §4 `E_GEOMETRY_UNBOUNDED` end-to-end test (`volume(half_space(...))` errors); PRD §Worked examples cannot be written today; the whole "Unbounded propagation" semantics is unexercisable in v0.1.
- **Note:** This is the single biggest live gap. Diagnostic infrastructure (M-009), inference fallback (M-007), and warning machinery (M-013) are all wired and waiting; only the producers of `Bounded=false` are absent. No follow-up task ID surfaced for this work in fused-memory. **Sequencing:** Consumer surface for unbounded-geometry `Bounded` checks now lands via `docs/prds/v0_3/geometry-handle-runtime.md` (Type::Geometry flows through value cells via `Value::GeometryHandle`); the `Bounded` negative-path becomes exercisable once GR-018 ships the producers `half_space()` / `extrude_infinite()`. Production owned by GR-018.

### M-007: `try_infer_traits_for_function_call` dispatch on function name with `_ => None` fallback

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `geometry_traits_inference.rs:525-615` (explicit arms for box/cylinder/sphere/tube/union/difference/intersection/union_all/intersection_all/translate/rotate/scale/rotate_around/fillet/chamfer/shell/draft/thicken/linear_pattern/circular_pattern/mirror/linear_pattern_2d/arbitrary_pattern/extrude/extrude_symmetric/revolve/revolve_full/sweep/sweep_guided/loft/loft_guided/pipe/line_segment/arc/helix/interp/bezier/nurbs). Coverage test `every_geometry_function_name_has_explicit_dispatch_arm` in `tests/geometry_traits_inference_tests.rs` pins that every name in `GEOMETRY_FUNCTION_NAMES` has an arm.
- **Blocks:** none
- **Note:** Audited-place-for-unknown-name-fallback is well-flagged; coverage test would fire loudly if a new constructor name slipped in without an explicit arm.

### M-008: `LetBindingEnv` trait + `RealizationLetEnv` impl — resolve `ValueRef(id)` through realization op-array

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `geometry_traits_inference.rs:286-305` (trait + `EmptyLetEnv` default); `crates/reify-compiler/src/conformance/mod.rs:536-595` (`RealizationLetEnv` impl walks `template.realizations` + `value_cells`, with `RefCell<Vec<ValueCellId>>` cycle guard); task 2549 (per fused-memory search result `e03a5dbb`) added a cycle/depth guard for `infer_op`/`infer_geom_ref` `Step` recursion.
- **Blocks:** none
- **Note:** Plumbing for `let g = box(...)` indirection is real — `infer_traits_for_expr_in_env(g_ref, &env)` does the right thing. Without this, a `let g : Solid = half_space(...)` plus `volume(g)` would have safe-defaulted to Bounded.

### M-009: Compile-time diagnostic `E_GEOMETRY_UNBOUNDED` at call sites binding `Bounded`

- **State:** PARTIAL
- **Failure mode:** F1-adjacent — diagnostic emission is wired, but unreachable in v0.1 source because M-006 absent. No e2e source-level test exists.
- **Evidence:** `crates/reify-compiler/src/conformance/mod.rs:289-318` (`emit_geometry_unbounded` pushes `Diagnostic::error` with `DiagnosticCode::GeometryUnbounded`); `:660-680` (conformance walker route: `is_geometry_arg` test → match `required_trait` to `GeometryTrait::Bounded` → `infer_traits_for_expr_in_env(compiled_arg, &env)` → if `!inferred.has(Bounded)` emit); `crates/reify-types/src/diagnostics.rs` (`DiagnosticCode::GeometryUnbounded` variant). Unit test `:3855-3873` constructs a synthetic `Diagnostic` to pin code+severity+message but no Reify-source-level e2e test calls it.
- **Blocks:** PRD §4 e2e: `volume(half_space(...))` would emit, but `half_space` doesn't exist (see M-006).
- **Note:** Routing is correct; the diagnostic will fire the day an Unbounded source lands. Until then this is a "loaded gun, no target" — passes unit tests but never executes in user code.

### M-010: Compile-time diagnostic `TypeNotConformingToTrait` for missing Connected/Convex at Bounded-shaped slot

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `conformance/mod.rs:331-348` (`emit_geometry_trait_violation` reuses `DiagnosticCode::TypeNotConformingToTrait`); `:660-685` walker hooks into both Connected and Convex; design decision §2 reuses the existing trait-bound code rather than allocating new ones for Connected/Convex.
- **Blocks:** none
- **Note:** PRD-author-locked design decision — only Bounded gets the dedicated error code; Connected/Convex piggyback. Inferred-flag drops for Connected/Convex can fire from current primitives (e.g. `union(box, box)` drops Connected) so this code path *is* reachable in v0.1, unlike M-009.

### M-011: OCCT FFI `is_watertight`/`is_manifold`/`is_orientable`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:3050-3097` (real `BRepCheck_Analyzer.IsValid()` + edge_face_map walk + `ShapeAnalysis_Shell::CheckOrientedShells` impls); `crates/reify-kernel-occt/src/ffi.rs:769-778` (cxx bridge); `src/lib.rs:2325-2341` (`GeometryQuery::IsWatertight`/`IsManifold`/`IsOrientable` arms in `query()` dispatch); `tests/conformance_integration.rs` (12 tests covering box/closed shell/compsolid/sphere/cylinder/face/wire/edge/vertex/non-manifold compound/open shell/non-orientable shell — both positive AND negative fixture branches; OCCT-version-stable assertions; gated by `#[cfg(all(has_occt, feature = "test-fixtures"))]`).
- **Blocks:** none
- **Note:** Implementation is real and follows the BRepCheck-style FFI pattern from #319 as PRD specifies. SHAPE_TYPE guard on `is_watertight` (SOLID/COMPSOLID/SHELL only) is deliberate (compound exclusion documented).

### M-012: `GeometryQuery::IsWatertight/IsManifold/IsOrientable` enum variants + `query_name()` arm

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/geometry.rs:776-789` (three variants carrying `GeometryHandleId`); `:986-988` exhaustive `query_name` match.
- **Blocks:** none
- **Note:** Exhaustive match means adding a 4th conformance query would require source updates here.

### M-013: Per-trait stdlib helpers `is_watertight(g)/is_manifold(g)/is_orientable(g)`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/units.rs:75-82` (`GEOMETRY_QUERY_HELPER_NAMES` const + `is_geometry_query_helper` classifier); `src/expr.rs:1072-1079` (NoUserFunctions arm forces `Type::Bool` for the three names regardless of arg shape — sidesteps the no-`Type::Geometry`-in-`Value` invariant); `crates/reify-compiler/stdlib/geometry_traits.ri:46-80` (doc-only block; no `fn` decls — by design, see file's `// Why no fn declarations` block).
- **Blocks:** none
- **Note:** The "first query-style user-callable stdlib functions in Reify" per PRD §Task 5 — shipped as recognized-by-name builtins, not via `fn` declarations (would have routed through `eval_user_function_call` and bypassed kernel access).

### M-014: Eval-time dispatch `try_eval_conformance_query` (lookup-by-cell-id sideband)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1219-1317` (full function: 6 ordered guards [FunctionCall, helper name, user-assertion escape hatch, single-arg ValueRef shape, named_steps cell-member lookup, kernel.query]); `crates/reify-eval/src/engine_build.rs:2062-2103` (`post_process_conformance_queries` invoked from `build()`, `build_snapshot()`, AND `tessellate_realizations` — three call sites — after `execute_realization_ops` populates `named_steps`); pinned by `try_eval_conformance_query_*` unit tests (`geometry_ops.rs:5189-5470`, ~10 tests) + integration tests in `conformance_runtime.rs` (kernel-reply true/false, user-assertion short-circuit, literal-arg defensive fall-through, OCCT-backed e2e).
- **Blocks:** none
- **Note:** Design Decision §1 (Type::Geometry has no Value variant; round-trip via cell-id sideband) is the load-bearing architectural choice — preserves snapshot/journal/content-hash semantics. The mechanism is the single most-tested piece of the PRD.

### M-015: `W_TRAIT_USER_ASSERTED` compile-time warning (specialization escape hatch)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/entity.rs:907-938` (per-`trait_bound` iteration in `trait_bounds` loop; name-based detection via `is_geometry_marker_trait`); `crates/reify-compiler/src/geometry_traits.rs:23-42` (`GEOMETRY_MARKER_TRAITS` const + predicate); `DiagnosticCode::TraitUserAsserted` in `reify-types`; `tests/geometry_traits_user_asserted_tests.rs` (309 lines: single-bound emission, multi-bound distinct spans, non-geometry-trait negative, parametric over all seven, occurrence_def case).
- **Blocks:** none
- **Note:** Emits one warning per `(structure_def, geometry_marker_bound)` pair as PRD requires. "Fires exactly once" is enforced by the bound-loop structure (each bound visited once), not by an explicit dedup set — re-declaring `: Watertight + Watertight` would produce two warnings, but Reify's parser likely rejects duplicate bounds upstream (not verified in this audit).

### M-016: Runtime user-assertion short-circuit (asymmetric per-marker)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `geometry_ops.rs:1255-1262` (step-3 check: `template_trait_bounds.iter().any(|t| t == marker_trait)` short-circuits to `Bool(true)` before kernel query); pinned by `try_eval_conformance_query_user_assertion_watertight_short_circuits` (`:5330`), `_manifold_short_circuits` (`:5366`), `_orientable_short_circuits` (`:5391`), AND `_closed_does_not_short_circuit_is_watertight` (`:5416`); integration tests `watertight_user_assertion_short_circuits_kernel_query` and `tessellate_realizations_honours_user_assertion_escape_hatch` use `CountingMockKernel` to assert zero kernel calls.
- **Blocks:** none
- **Note:** PRD §Design notes Decision 3 fully shipped — asymmetric per-marker semantics ("`: Closed` does NOT short-circuit `is_watertight`") is explicitly pinned by both unit and integration tests. **PRD test names DRIFT**: PRD §Acceptance and §Task 6 cite `is_watertight_user_assertion_short_circuits_to_true` and `is_watertight_closed_bound_does_not_short_circuit` — these names don't exist; see M-017.

### M-017: PRD-cited test names that do not exist verbatim

- **State:** DRIFT
- **Failure mode:** documentation rot
- **Evidence:** PRD §Acceptance, §Task 6 and §Design notes all cite `is_watertight_user_assertion_short_circuits_to_true` and `is_watertight_closed_bound_does_not_short_circuit` in `crates/reify-eval/tests/conformance_runtime.rs`. Actual integration-file test is `watertight_user_assertion_short_circuits_kernel_query`; the asymmetry test (`try_eval_conformance_query_user_assertion_closed_does_not_short_circuit_is_watertight`) is in `geometry_ops.rs` unit tests, **not** in the integration file. Coverage is equivalent or stronger than PRD described, but a reader checking acceptance criteria by `grep`ing test names will get false negatives.
- **Blocks:** none (functional)
- **Note:** PRD's "Implementation as shipped" block (lines 222-246) was meant to keep PRD↔code names in sync; that block is internally consistent (lists `conformance_runtime.rs` with "6 integration tests"), but §Acceptance and §Task 6 (added/edited later?) reference yet a third naming scheme. Cosmetic but the PRD claims explicit pinning that doesn't exist by name.

## Cross-PRD breadcrumbs

- **GR-001 (struct-ctor runtime eval):** Does NOT affect this PRD — the seven traits are marker traits with no fields, and the specialization escape hatch uses `: TraitName` (a trait-bound on a `structure def`, not a struct constructor invocation). Path is clean.
- **PRD `field-source-kinds.md`** — explicitly noted Out-of-scope (`imported` field source kind, multi-kernel). Phase 2/3 should check whether `imported` interacts with conformance.
- **PRD `topology-selectors.md`** — Solvespace-style attribute-persistent conformance attestations cited as "ties to feature-tag work" in §Out of scope.
- **PRD `stdlib-trait-breadth.md`** — task 2347 (per fused-memory) audited stdlib trait inheritance and confirmed `Watertight : Closed + Manifold` was correctly added by task 2297. Sibling work; doesn't blocker this PRD.
- **v0.2 PRD candidates**: generic `conforms<T : Geometry, R : Trait>(g, Type<R>) → Bool`, field-aware trait inference, attribute-persistent attestations, `imported` field source kind, multi-kernel — all four explicitly deferred to v0.2 by this PRD.
- **`Value::TraitTag` / type-as-value plumbing** — referenced as the prerequisite for generic `conforms<T,R>` in §Design notes Decision 2 and §Out of scope. Not present in `crates/reify-types/` today (grep confirms). If any other PRD assumes "trait names as values", it transitively depends on this absent mechanism.
