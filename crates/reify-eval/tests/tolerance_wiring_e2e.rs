//! End-to-end engine-level integration tests for task 2874 — exercises the
//! production-wired tolerance subsystem: dispatcher emission of import-promise
//! + zero-promise diagnostics on `build()`, `RealizationCache` population /
//! short-circuit keyed on demanded tolerance, and `per_stage_tolerance_for_plan`
//! consumption from the realization loop.
//!
//! Imports use the established test fixture surface
//! (`reify_test_support::{make_engine, step_input_template, step_output_template,
//! my_design_template, manufacturing_purpose}` + `CompiledModuleBuilder`).
//! Per-step tests are added by the subsequent TDD steps.

#[allow(unused_imports)]
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
#[allow(unused_imports)]
use reify_test_support::{
    make_engine, manufacturing_purpose, mm, my_design_template, step_input_template,
    step_output_template, MockConstraintChecker, MockGeometryKernel,
};
#[allow(unused_imports)]
use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
#[allow(unused_imports)]
use reify_types::{
    CapabilityDescriptor, CompiledExpr, DiagnosticCode, ExportFormat, ModulePath, Operation,
    ReprKind, Severity, Type,
};
#[allow(unused_imports)]
use reify_eval::{
    dispatch, per_stage_tolerance_for_plan, tolerance_budget::per_stage_tolerance, DispatchPlan,
};
#[allow(unused_imports)]
use std::collections::{BTreeMap, HashSet};

/// Step-1 (failing initially; passes once step-2's
/// `emit_imported_tolerance_promise_diagnostics_for_module` helper is wired
/// into the production `build()` path).
///
/// The fixture is the canonical "promise loose, demand tight" pairing: a
/// `STEPInput` template carries a 50µm imported-geometry tolerance promise,
/// the `STEPOutput` template's body constraint is `RepresentationWithin(…, 1µm)`,
/// and a manufacturing purpose at 1µm is activated against `MyDesign`. Per the
/// `Engine::check_imported_tolerance_promise` truth table (engine_tolerance.rs:
/// 36-67), `min(1µm, 1µm) = 1µm` is strictly tighter than the 50µm promise, so
/// the runtime must surface a single `Severity::Warning` carrying
/// `DiagnosticCode::ImportedTolerancePromiseInsufficient` whose message names
/// the input template (`"STEPInput"`) so authors can locate the import site.
///
/// Today (pre step-2) the production `build()` path never invokes
/// `Engine::check_imported_tolerance_promise`, so this assertion FAILS — no
/// matching diagnostic is present in `BuildResult.diagnostics`. After step-2
/// adds the dispatcher helper and wires it from `build` /
/// `build_snapshot` / `tessellate_realizations`, the assertion passes.
#[test]
fn build_emits_imported_tolerance_promise_insufficient_warning_when_demand_strictly_tighter_than_promise()
 {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_build_emits_imported_tolerance_promise_warning".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Step);

    let matched: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)
        })
        .collect();

    assert_eq!(
        matched.len(),
        1,
        "expected exactly one ImportedTolerancePromiseInsufficient warning in \
         BuildResult.diagnostics; got {} matching diagnostics. Full diagnostic \
         set: {:?}",
        matched.len(),
        build.diagnostics,
    );
    assert!(
        matched[0].message.contains("STEPInput"),
        "warning message must name the input template so authors can locate \
         the import site (got: {:?})",
        matched[0].message,
    );
}

/// Step-3 (locks the second branch of `Engine::check_imported_tolerance_promise`'s
/// dispatch — the zero-promise lint introduced by task 2833 — into the production
/// emission path).
///
/// Setup mirrors step-1 but with `step_input_template(0.0)`: the `STEPInput`
/// template's `param tolerance : Length = 0m` is a placeholder-default
/// footgun where authors leave the promise at zero and silently disable the
/// strict-`<` insufficient-promise warning. With `promise == 0.0` and a
/// positive demanded (1µm via STEPOutput body + manufacturing purpose), the
/// `Engine::check_imported_tolerance_promise` dispatcher takes its
/// zero-promise branch and emits a `Severity::Warning` carrying
/// `DiagnosticCode::InputTolerancePromiseIsZero` (NOT
/// `ImportedTolerancePromiseInsufficient` — the two codes are mutually
/// exclusive per the dispatch order pinned at engine_tolerance.rs:31-67).
///
/// The test asserts the emitted code is `InputTolerancePromiseIsZero`. Pre-
/// step-2 wiring this assertion failed because nothing in `build()` invoked
/// the dispatcher. After step-2's helper threads any `Some(diag)` from the
/// dispatcher through to `BuildResult.diagnostics` (code-agnostic
/// forwarding), this assertion passes — guarding against a future refactor
/// that filters `code == ImportedTolerancePromiseInsufficient` only and
/// drops the zero-promise branch.
#[test]
fn build_emits_input_tolerance_promise_is_zero_warning_when_promise_zero_and_demand_positive() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_build_emits_input_tolerance_promise_is_zero_warning".to_string(),
    ]))
    .template(step_input_template(0.0))
    .template(step_output_template(1e-6))
    .template(my_design_template())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let mut engine = make_engine();
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Step);

    let zero_matched: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::InputTolerancePromiseIsZero)
        })
        .collect();

    assert_eq!(
        zero_matched.len(),
        1,
        "expected exactly one InputTolerancePromiseIsZero warning in \
         BuildResult.diagnostics; got {} matching diagnostics. Full \
         diagnostic set: {:?}",
        zero_matched.len(),
        build.diagnostics,
    );

    // Mutual exclusivity: when promise == 0.0, the strict-`<` insufficient
    // branch never fires (per `is_promise_insufficient(demanded, 0.0)` →
    // `demanded < 0.0` → false for non-negative demands). Pin that the
    // helper does NOT also emit the insufficient warning here.
    let insufficient_matched: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)
        })
        .collect();
    assert_eq!(
        insufficient_matched.len(),
        0,
        "ImportedTolerancePromiseInsufficient must NOT fire when promise \
         is zero (mutually-exclusive with the zero-promise branch); got \
         {} matching diagnostics. Full diagnostic set: {:?}",
        insufficient_matched.len(),
        build.diagnostics,
    );
}

/// Build a `MyDesign`-shaped [`reify_compiler::TopologyTemplate`] that carries
/// a single named realization producing one `Box` primitive op. The realization
/// id is `(entity = "MyDesign", index = 0)` and the realization's `name` is
/// `"body"` so the post-realization `named_steps` map is populated.
///
/// Mirrors the realization shape pinned by `tessellate_single_box_realization`
/// in `tests/tessellation.rs`. The thickness param fixed by
/// `my_design_template` is omitted here because the test focuses on the
/// realization → cache wiring; the param is irrelevant to the cache key
/// `(entity_id, repr_kind, demanded_tol)`.
fn my_design_template_with_box_realization() -> reify_compiler::TopologyTemplate {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(10.0)),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    };
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .realization_named("MyDesign", 0, "body", vec![box_op])
        .build()
}

/// Step-5 (failing initially; passes once step-6 plumbs `demanded_tol` through
/// `Engine::execute_realization_ops` and writes the resulting handle into
/// `Engine::realization_cache` keyed on `(entity_id, ReprKind::BRep, demanded_tol)`).
///
/// Build a module that pairs an `STEPOutput` template (1µm
/// `RepresentationWithin` body bound) with a `MyDesign` template carrying a
/// single named realization (one `Box` primitive op). Activate
/// `manufacturing_purpose("manufacturing", 1e-6)` against `"MyDesign"` so the
/// engine's `active_purpose_bindings` and `active_tolerance_scope` populate
/// the demand-side contributors at 1µm. Run `build(&module, ExportFormat::Step)`.
///
/// After `build()` returns, the `RealizationCache` must contain an entry at
/// `("MyDesign", ReprKind::BRep, 1e-6)`. The lookup uses the partial-order
/// "tighter satisfies looser" rule (`cached_tol ≤ requested_tol`); a cache
/// populated at exactly the requested tolerance must therefore return
/// `Some(&handle)` for an exact-tolerance lookup.
///
/// Today (pre step-6) `execute_realization_ops` does not consult the cache and
/// does not insert into it after a successful realization, so the lookup
/// returns `None` and this test FAILS. Once step-6 wires the demanded
/// tolerance through the helper and inserts the terminal handle on
/// post-realization success, the assertion passes.
#[test]
fn build_populates_realization_cache_keyed_on_demanded_tolerance() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_build_populates_realization_cache".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let _build = engine.build(&module, ExportFormat::Step);

    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6)
            .is_some(),
        "expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() completes against a \
         manufacturing purpose at 1µm; got cache len={} (entries dump: {:?})",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Step-7 (failing initially; passes once step-8 adds the cache-hit
/// short-circuit at the top of `Engine::execute_realization_ops`).
///
/// Setup mirrors step-5 — `STEPOutput(1µm)` + `MyDesign` realization (one
/// `Box` primitive op) + manufacturing purpose at 1µm. The cache key
/// `("MyDesign", ReprKind::BRep, 1e-6)` is populated on the first `build()`
/// (verified by step-5's test), so a second `build()` with the same module
/// and the same demand should see the cache lookup succeed at the top of
/// `execute_realization_ops` and return the cached terminal handle without
/// dispatching the realization's ops to the kernel.
///
/// The test pins this contract by:
/// 1. Constructing a `MockGeometryKernel` and grabbing its
///    `operations_ref()` (an `Arc<Mutex<Vec<GeometryOpRecord>>>`) BEFORE
///    transferring ownership into the engine — that gives us a stable
///    shared-handle on the kernel's recorded-operations vector across the
///    two `build()` calls.
/// 2. Running the first `build()` and asserting the recorded-ops vector
///    grew by ≥1 entry (kernel was invoked: cache miss, op dispatched,
///    cache populated by step-6's post-realization insert).
/// 3. Re-activating the purpose because `build()` calls `check()` which
///    calls `eval()` which clears `active_purpose_bindings` (engine_eval.rs
///    around lines 1149-1150). Without re-activation the second build's
///    pre-`check()` precompute would observe an empty tolerance scope, the
///    threaded `demanded_tol` would be `None`, and the cache lookup at the
///    top of `execute_realization_ops` would not even fire — defeating the
///    test's premise. (This mirrors the pattern step-13 documents for the
///    cache-miss-on-tighter-demand case.)
/// 4. Running the second `build()` and asserting the recorded-ops vector
///    DID NOT grow — the realization was served entirely from cache.
///
/// Today (pre step-8) the cache short-circuit does not exist, so even
/// though `realization_cache.lookup(…)` returns `Some(_)` at the top of
/// `execute_realization_ops`, nothing consults that lookup before the op
/// loop runs. The kernel re-executes the realization's ops on every
/// build, so the second-build assertion FAILS. Once step-8 wires the
/// realization-level short-circuit (push cached handle, write
/// `named_steps`, return early), the assertion passes.
#[test]
fn second_build_with_unchanged_purpose_and_module_short_circuits_kernel_via_cache_hit() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_second_build_short_circuits_via_cache_hit".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_handle = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let _build1 = engine.build(&module, ExportFormat::Step);
    let ops_after_first = ops_handle.lock().unwrap().len();
    assert!(
        ops_after_first >= 1,
        "expected first build() to invoke the kernel at least once \
         (cache miss → realization ops dispatched, cache populated); got \
         ops_after_first={}",
        ops_after_first,
    );

    // Re-activate purpose: build() above called check() which called eval()
    // which cleared `active_purpose_bindings` (engine_eval.rs:1149-1150). The
    // pre-`check()` precompute on the second build would otherwise observe
    // an empty scope and yield `demanded_tol = None`, suppressing the cache
    // lookup. Re-activation puts the same `(manufacturing → MyDesign)`
    // binding back so the second build observes `demanded_tol = Some(1e-6)`,
    // matching the cache key populated by the first build.
    engine.activate_purpose("manufacturing", "MyDesign");

    let _build2 = engine.build(&module, ExportFormat::Step);
    let ops_after_second = ops_handle.lock().unwrap().len();
    assert_eq!(
        ops_after_second, ops_after_first,
        "expected second build() to be served entirely from RealizationCache \
         (cache hit at (MyDesign, BRep, 1e-6) populated by the first build); \
         got ops_after_first={}, ops_after_second={} — kernel was invoked \
         {} additional time(s) on the second build, indicating the \
         cache-hit short-circuit at the top of execute_realization_ops is \
         absent or mis-keyed.",
        ops_after_first,
        ops_after_second,
        ops_after_second - ops_after_first,
    );
}

/// Step-9 (failing initially; passes once step-10 adds the
/// `Engine::compute_realization_tolerance_budget(&self, registry, demanded_tol)`
/// helper that synthesises a `DispatchPlan` via
/// `dispatch(registry, Operation::BooleanUnion, ReprKind::BRep, &{ReprKind::BRep})`
/// and forwards through `per_stage_tolerance_for_plan(&plan, demanded_tol)`).
///
/// Pins the per-stage tolerance-budget pipeline at the engine surface:
///
/// - **Part (i): single-kernel registry → 0-conversion plan, helper passes
///   `demanded_tol` through unchanged.** The fixture registers a single
///   `occt`-shaped descriptor that supports `(BooleanUnion, BRep)`. Under the
///   helper's hard-coded `(op, demanded, available) =
///   (BooleanUnion, BRep, {BRep})` triple, the BFS in `dispatch` finds a
///   final-stage match at depth 0 and returns `DispatchPlan { kernel: "occt",
///   conversions: vec![] }`. `per_stage_tolerance_for_plan` on an empty chain
///   pass-throughs the input by contract (dispatcher.rs §truth-table), so the
///   helper returns `demanded_tol` bit-exactly.
///
/// - **Part (ii): two-stage chain primitive → `per_stage_tolerance(_, 2)`.**
///   The 2-stage chain in `tests/tolerance_dispatch_budget.rs` (alpha:
///   BRep→Sdf, beta: Sdf→Mesh, manifold: BooleanUnion on Mesh) yields a
///   2-conversion plan only when dispatched for `demanded = ReprKind::Mesh`.
///   The engine helper hard-codes `demanded = ReprKind::BRep` (per the design
///   decision: `RealizationDecl` carries no Operation/ReprKind metadata, and
///   the v0.2 occt-only baseline is BRep-on-BRep), so a 2-stage chain ending
///   in Mesh is unreachable through the helper's BFS — the helper's `None`
///   branch returns `demanded_tol` unchanged (no plan ⇒ no budget allocation).
///   To pin the 2-stage budget primitive that the helper consumes when a
///   non-trivial plan IS available (multi-kernel adapter tasks land it), we
///   construct a `DispatchPlan` literal with two conversions and assert that
///   `per_stage_tolerance_for_plan(&plan, demanded_tol)` equals
///   `per_stage_tolerance(demanded_tol, 2)`. The literal-construction route
///   mirrors the dispatcher's own multi-stage unit tests (dispatcher.rs:1349)
///   and the lib re-export integration smoke
///   (`tolerance_dispatch_budget.rs:46`); replicating the assertion at the
///   engine-test layer locks the integration of the budget primitive into
///   the same test file as the helper, so a future refactor cannot drop the
///   wiring without breaking this pin.
///
/// Today (pre step-10) the helper does not exist, so the call to
/// `engine.compute_realization_tolerance_budget(&single, demand)` is a
/// compile error and this test FAILS. Once step-10 lands the helper as a
/// cfg-gated `pub` accessor (mirroring `realization_cache()` /
/// `feature_tag_table()` precedent), the call resolves and both parts of the
/// assertion pass.
#[test]
fn per_stage_tolerance_for_plan_governs_tolerance_budget_for_two_stage_dispatch_chain() {
    let engine = make_engine();

    // ── Part (i): single-kernel registry, 0-conversion plan, pass-through ──

    let occt = CapabilityDescriptor {
        supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
    };
    let mut single: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
    single.insert("occt".to_string(), &occt);

    let demand = 1e-6_f64;
    assert_eq!(
        engine.compute_realization_tolerance_budget(&single, demand),
        demand,
        "single-kernel registry yields a 0-conversion DispatchPlan under \
         dispatch(_, BooleanUnion, BRep, {{BRep}}); per_stage_tolerance_for_plan \
         on an empty chain must pass demanded_tol through unchanged \
         (bit-exact). Helper deviation here would indicate either (a) the \
         empty-chain pass-through contract is broken in \
         per_stage_tolerance_for_plan, or (b) the helper applied the \
         safety-factor fold at len()=0 instead of bypassing it (an off-by-one \
         in the n_stages resolution). Demand: {demand}",
    );

    // ── Part (ii): two-stage chain primitive, geometric per-stage split ───

    // 2-conversion plan literal; matches the chain-shape pinned by
    // dispatcher.rs::per_stage_tolerance_for_plan_multi_stage_chain_uses_geometric_split
    // and tests/tolerance_dispatch_budget.rs::lib_re_exports_per_stage_tolerance_for_plan_and_dispatch_end_to_end.
    let plan_two = DispatchPlan {
        kernel: "manifold".to_string(),
        conversions: vec![
            ("alpha".to_string(), ReprKind::BRep, ReprKind::Sdf),
            ("beta".to_string(), ReprKind::Sdf, ReprKind::Mesh),
        ],
    };
    assert_eq!(
        per_stage_tolerance_for_plan(&plan_two, demand),
        per_stage_tolerance(demand, 2),
        "two-stage dispatch chain (BRep→Sdf→Mesh, BooleanUnion on Mesh) must \
         yield per_stage_tolerance(demanded_tol, 2). This is the budget \
         primitive that compute_realization_tolerance_budget consumes when \
         the underlying dispatch returns a multi-stage plan — for the \
         helper's hard-coded (BooleanUnion, BRep, {{BRep}}) triple a 2-stage \
         plan is unreachable (BFS visited set blocks BRep re-entry), so this \
         pin is the integration-layer guarantee that the budget primitive \
         remains correctly wired against the dispatcher API surface even \
         though the helper itself routes through the None-branch pass-through \
         on this registry shape.",
    );
}
