//! End-to-end engine-level integration tests for the production-wired
//! tolerance subsystem: dispatcher emission of import-promise and
//! zero-promise diagnostics on `build()`, `RealizationCache` population
//! and short-circuit keyed on demanded tolerance, and
//! `per_stage_tolerance_for_plan` consumption from the realization loop.
//! Originally filed under task 2874; the file has since accreted pins from
//! several follow-up tasks — each test's doc names its own.
//!
//! Imports use the established test fixture surface
//! (`reify_test_support::{make_engine, step_input_template, step_output_template,
//! my_design_template, manufacturing_purpose}` + `CompiledModuleBuilder`).

#[allow(unused_imports)]
use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
#[allow(unused_imports)]
use reify_core::{ContentHash, DiagnosticCode, ModulePath, Severity, Type, ValueCellId};
#[allow(unused_imports)]
use reify_eval::{
    DispatchPlan, dispatch, per_stage_tolerance_for_plan, tolerance_budget::per_stage_tolerance,
};
use reify_ir::{
    CapabilityDescriptor, CompiledExpr, ExportFormat, KernelId, Operation, ReprKind, Value,
};
#[allow(unused_imports)]
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
#[allow(unused_imports)]
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, make_engine, manufacturing_purpose, mm,
    my_design_template, step_input_template, step_output_template,
};
#[allow(unused_imports)]
use std::collections::{BTreeMap, HashSet};

/// Pins the landed contract: `build()` routes every (input template,
/// subject, output template) triple through
/// `Engine::check_imported_tolerance_promise` and forwards any `Some(diag)`
/// into `BuildResult.diagnostics`.
///
/// The fixture is the canonical "promise loose, demand tight" pairing: a
/// `STEPInput` template carries a 50µm imported-geometry tolerance promise,
/// the `STEPOutput` template's body constraint is `RepresentationWithin(…, 1µm)`,
/// and a manufacturing purpose at 1µm is activated against `MyDesign`. Per
/// the truth table in `Engine::check_imported_tolerance_promise`
/// (`src/engine_tolerance.rs`), `min(1µm, 1µm) = 1µm` is strictly tighter
/// than the 50µm promise, so the runtime must surface a single
/// `Severity::Warning` carrying
/// `DiagnosticCode::ImportedTolerancePromiseInsufficient` whose message
/// names the input template (`"STEPInput"`) so authors can locate the
/// import site.
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

/// Pins the second branch of `Engine::check_imported_tolerance_promise`'s
/// dispatch — the zero-promise lint introduced by task 2833 — in the
/// production emission path.
///
/// Setup mirrors
/// `build_emits_imported_tolerance_promise_insufficient_warning_when_demand_strictly_tighter_than_promise`
/// above but with `step_input_template(0.0)`: the `STEPInput` template's
/// `param tolerance : Length = 0m` is a placeholder-default footgun where
/// authors leave the promise at zero and silently disable the strict-`<`
/// insufficient-promise warning. With `promise == 0.0` and a positive
/// demanded (1µm via STEPOutput body + manufacturing purpose), the
/// `Engine::check_imported_tolerance_promise` dispatcher takes its
/// zero-promise branch and emits a `Severity::Warning` carrying
/// `DiagnosticCode::InputTolerancePromiseIsZero` (NOT
/// `ImportedTolerancePromiseInsufficient` — the two codes are mutually
/// exclusive per the dispatch order in
/// `Engine::check_imported_tolerance_promise`, `src/engine_tolerance.rs`).
///
/// The test asserts the emitted code is `InputTolerancePromiseIsZero`. The
/// dispatcher forwards any `Some(diag)` through to `BuildResult.diagnostics`
/// code-agnostically, which this test guards: a future refactor that
/// filters `code == ImportedTolerancePromiseInsufficient` only would
/// silently drop the zero-promise branch.
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
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![box_op])
        .build()
}

/// Pins the landed contract: `Engine::execute_realization_ops`
/// (`src/engine_build.rs`) receives the threaded `demanded_tol` and, on
/// post-realization success for a NAMED realization, inserts the terminal
/// handle into `Engine::realization_cache` keyed on
/// `(entity_id, ReprKind::BRep, demanded_tol)`.
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
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() completes against a \
         manufacturing purpose at 1µm; got cache len={} (entries dump: {:?})",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Pins the landed cache-hit short-circuit at the top of
/// `Engine::execute_realization_ops`: on a cache hit it pushes the cached
/// handle, writes `named_steps`, and returns early without dispatching the
/// realization's ops to the kernel.
///
/// Setup mirrors `build_populates_realization_cache_keyed_on_demanded_tolerance`
/// above — `STEPOutput(1µm)` + `MyDesign` realization (one `Box` primitive
/// op) + manufacturing purpose at 1µm. The cache key
/// `("MyDesign", ReprKind::BRep, 1e-6)` is populated on the first `build()`
/// (see `build_populates_realization_cache_keyed_on_demanded_tolerance`),
/// so a second `build()` with the same module and the same demand should
/// see the cache lookup succeed at the top of `execute_realization_ops` and
/// return the cached terminal handle without dispatching the realization's
/// ops to the kernel.
///
/// The test pins this contract by:
/// 1. Constructing a `MockGeometryKernel` and grabbing its
///    `operations_ref()` (an `Arc<Mutex<Vec<GeometryOpRecord>>>`) BEFORE
///    transferring ownership into the engine — that gives us a stable
///    shared-handle on the kernel's recorded-operations vector across the
///    two `build()` calls.
/// 2. Running the first `build()` and asserting the recorded-ops vector
///    grew by ≥1 entry (kernel was invoked: cache miss, op dispatched,
///    cache populated by the post-realization insert in
///    `execute_realization_ops`).
/// 3. Re-activating the purpose because `build()` calls `check()` which
///    calls `eval()` which clears `active_purpose_bindings` (`Engine::eval`
///    in `src/engine_eval.rs`). Without re-activation the second build's
///    pre-`check()` precompute would observe an empty tolerance scope, the
///    threaded `demanded_tol` would be `None`, and the cache lookup at the
///    top of `execute_realization_ops` would not even fire — defeating the
///    test's premise. (This mirrors the pattern
///    `cache_lookup_misses_when_purpose_changes_demanded_tolerance`
///    documents for the cache-miss-on-tighter-demand case.)
/// 4. Running the second `build()` and asserting the recorded-ops vector
///    DID NOT grow — the realization was served entirely from cache.
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
    // which cleared `active_purpose_bindings` (`Engine::eval`,
    // `src/engine_eval.rs`). The pre-`check()` precompute on the second
    // build would otherwise observe an empty scope and yield
    // `demanded_tol = None`, suppressing the cache lookup. Re-activation
    // puts the same `(manufacturing → MyDesign)` binding back so the second
    // build observes `demanded_tol = Some(1e-6)`, matching the cache key
    // populated by the first build.
    engine.activate_purpose("manufacturing", "MyDesign");

    let _build2 = engine.build(&module, ExportFormat::Step);
    let ops_after_second = ops_handle.lock().unwrap().len();
    assert_eq!(
        ops_after_second,
        ops_after_first,
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

/// Pins that `Engine::tessellate_realizations(&module)` forwards the
/// per-output demanded tolerance — routed through
/// `compute_realization_tolerance_budget` against
/// `kernel_registry::collect_registry()` — to `GeometryKernel::tessellate`
/// instead of the module-level `effective_tessellation_tolerance` default
/// (`0.0001` SI metres = 0.1 mm).
///
/// Setup mirrors `build_populates_realization_cache_keyed_on_demanded_tolerance`
/// / `second_build_with_unchanged_purpose_and_module_short_circuits_kernel_via_cache_hit`:
/// an STEPOutput template carries a 1 µm `RepresentationWithin` body bound,
/// a `MyDesign` template carries a single named realization producing one
/// `Box` primitive op, and `manufacturing_purpose("manufacturing", 1e-6)` is
/// activated against `"MyDesign"`. `MockGeometryKernel` carries a
/// `tessellate_tolerances: Arc<Mutex<Vec<f64>>>` recorder, exposed via
/// `tessellate_tolerances_ref()`; the test grabs the recorder before
/// transferring kernel ownership into the engine.
///
/// The test calls `engine.tessellate_realizations(&module)` once, then asserts
/// the recorder contains exactly one entry equal to `1e-6` — the demanded
/// tolerance — NOT `0.0001` (the module pragma default that
/// `effective_tessellation_tolerance` returns when `default_tolerance` is
/// `None`). With the helper's hard-coded `(BooleanUnion, BRep, {BRep})`
/// triple and the occt-only single-kernel registry, dispatch returns a
/// 0-conversion plan and `per_stage_tolerance_for_plan` passes the demand
/// through unchanged — so `budget == 1e-6` exactly.
#[test]
fn tessellate_realizations_uses_demanded_tolerance_through_per_stage_budget() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_tessellate_uses_demanded_tolerance_via_per_stage_budget".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let tess_tols_handle = kernel.tessellate_tolerances_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let _tess = engine.tessellate_realizations(&module);

    let recorded = tess_tols_handle.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "expected exactly one tessellate(handle, tol) call (one realization \
         with one terminal handle); got {} recorded tolerance(s): {:?}",
        recorded.len(),
        recorded,
    );
    assert_eq!(
        recorded[0], 1e-6,
        "expected the kernel to receive the demanded tolerance (1µm from \
         STEPOutput body + manufacturing(1e-6)) routed through \
         compute_realization_tolerance_budget; got {} (the module-pragma \
         default 0.0001 indicates the per-stage budget pipeline is bypassed \
         and effective_tessellation_tolerance is forwarded instead). Full \
         recorded tolerances: {:?}",
        recorded[0], recorded,
    );
}

/// Pins the partial-order semantics on the realization-cache integration: a
/// tighter demand cannot be served by a looser cached entry.
///
/// The `cached_tol ≤ requested_tol` rule enforced by `RealizationCache::lookup`
/// (`src/realization_cache.rs`) implements the "tighter satisfies looser"
/// contract: a cache populated at 1e-6 satisfies a later request at any
/// `tol ≥ 1e-6` (looser-or-equal), but a request at `tol < 1e-6` (tighter)
/// MUST miss because the cached representation is at 1e-6 precision —
/// insufficient for the tighter consumer. This test pins that the cache
/// integration honours that rule end-to-end through the cache-hit
/// short-circuit in `Engine::execute_realization_ops`.
///
/// Setup mirrors `second_build_with_unchanged_purpose_and_module_short_circuits_kernel_via_cache_hit`
/// except a SECOND `manufacturing_tighter` purpose at 1e-9 m is compiled
/// into the same module. After the first `build()` (with `manufacturing` at
/// 1e-6 active) the cache is populated at `("MyDesign", BRep, 1e-6)`. We
/// then deactivate `manufacturing`, activate `manufacturing_tighter` (which
/// substitutes a fresh 1e-9 m `RepresentationWithin` constraint at the same
/// subject), and run `build()` again. The second build's pre-`check()`
/// precompute computes `demanded_tol = Some(1e-9)` (the tightest
/// contributor across the active scope), threads that into
/// `execute_realization_ops`, and the cache lookup at
/// `("MyDesign", BRep, 1e-9)` MISSES the cached `1e-6` entry — kernel
/// re-executes the realization ops, growing `kernel.operations()`.
///
/// The post-second-build `kernel.operations()` count must therefore strictly
/// EXCEED the post-first-build count (cache-miss path: kernel was invoked
/// again to satisfy the tighter demand). If the assertion fails — i.e. the
/// counts are equal — the cache is incorrectly serving a tighter request
/// from a looser cached entry, breaking the partial-order contract.
///
/// The partial-order rule is enforced by the bucket lookup primitive
/// itself; the engine threads the requested tolerance to it unchanged. A
/// failure here therefore means the cache-key value plumbing has broken
/// (stale `demanded_tol` captured across builds) — investigate at the
/// precompute site (`tessellate_realizations` / `build`) and at the
/// cache-lookup site at the top of `execute_realization_ops`.
#[test]
fn cache_lookup_misses_when_purpose_changes_demanded_tolerance() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cache_miss_when_purpose_changes_demand".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .compiled_purpose(manufacturing_purpose("manufacturing_tighter", 1e-9))
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
         (cache miss → realization ops dispatched, cache populated at \
         (MyDesign, BRep, 1e-6)); got ops_after_first={}",
        ops_after_first,
    );
    // Confirm the cache was populated at the looser tolerance — proves the
    // setup of the partial-order test is correct (without this pin, a bug
    // that fails to populate the cache at all would cause the second build
    // to also see ops_after_second > ops_after_first via the same "no cache"
    // path, falsely satisfying the test's headline assertion below).
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "expected first build to populate the cache at (MyDesign, BRep, 1e-6); \
         partial-order test premise requires this entry to exist before the \
         tighter-demand request",
    );

    // Switch to a strictly-tighter purpose: deactivate the 1µm manufacturing
    // and activate the 1nm one. `activate_purpose` is a no-op if the named
    // purpose is already active, so we MUST deactivate first to swap demand.
    engine.deactivate_purpose("manufacturing");
    engine.activate_purpose("manufacturing_tighter", "MyDesign");

    let _build2 = engine.build(&module, ExportFormat::Step);
    let ops_after_second = ops_handle.lock().unwrap().len();
    assert!(
        ops_after_second > ops_after_first,
        "expected second build() at the strictly-tighter demanded tolerance \
         (1e-9) to MISS the cached entry at (MyDesign, BRep, 1e-6) and \
         re-invoke the kernel — the partial-order rule (cached_tol ≤ \
         requested_tol) blocks a 1e-6 cached entry from satisfying a 1e-9 \
         request because 1e-6 > 1e-9 (\"tighter\" is not \"looser-or-equal\"). \
         Got ops_after_first={}, ops_after_second={} — equal counts indicate \
         the cache served a tighter request from a looser cached entry, \
         violating the partial-order contract pinned by \
         `RealizationCache::lookup` (`src/realization_cache.rs`) and \
         `ToleranceBucket::lookup`.",
        ops_after_first,
        ops_after_second,
    );
}

/// Pins the per-stage tolerance-budget pipeline at the engine surface.
/// `Engine::compute_realization_tolerance_budget` synthesises a
/// `DispatchPlan` via
/// `dispatch(registry, Operation::BooleanUnion, ReprKind::BRep, available)`
/// and forwards through `per_stage_tolerance_for_plan(&plan, demanded_tol)`,
/// taking the borrowed-value registry map and a caller-supplied
/// `available: &HashSet<ReprKind>` (production callers hoist both once per
/// build in `Engine::compute_tessellation_budgets`).
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
///   The 2-stage chain in `tests/harness_tolerance/tolerance_dispatch_budget.rs`
///   (alpha: BRep→Sdf, beta: Sdf→Mesh, manifold: BooleanUnion on Mesh) yields
///   a 2-conversion plan only when dispatched for `demanded = ReprKind::Mesh`.
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
///   mirrors the dispatcher's own multi-stage unit test
///   `per_stage_tolerance_for_plan_multi_stage_chain_uses_geometric_split`
///   (`src/dispatcher.rs`) and the lib re-export integration smoke
///   `lib_re_exports_per_stage_tolerance_for_plan_and_dispatch_end_to_end`
///   (`tests/harness_tolerance/tolerance_dispatch_budget.rs`); replicating
///   the assertion at the engine-test layer locks the integration of the
///   budget primitive into the same test file as the helper, so a future
///   refactor cannot drop the wiring without breaking this pin.
///
/// The call below is the compile-time pin on the helper's public
/// visibility: `compute_realization_tolerance_budget` is an un-gated `pub
/// fn` on `Engine` (`src/engine_build.rs`), called directly as
/// `engine.compute_realization_tolerance_budget(&single_borrow, &available,
/// demand)`.
#[test]
fn per_stage_tolerance_for_plan_governs_tolerance_budget_for_two_stage_dispatch_chain() {
    let engine = make_engine();

    // ── Part (i): single-kernel registry, 0-conversion plan, pass-through ──

    let occt = CapabilityDescriptor {
        supports: vec![(Operation::BooleanUnion, ReprKind::BRep)],
    };
    let mut single: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    single.insert("occt".to_string(), occt);
    // `compute_realization_tolerance_budget` takes the borrowed-value
    // variant of the registry that `dispatch` requires, plus a
    // caller-supplied `available: &HashSet<ReprKind>` (task 3227).
    // Production callers (inside `compute_tessellation_budgets`) hoist both
    // once per build; this direct test-seam call builds them at the call
    // site instead, mirroring the borrowed-registry pattern.
    let single_borrow: BTreeMap<String, &CapabilityDescriptor> =
        single.iter().map(|(k, v)| (k.clone(), v)).collect();
    // Use `Engine::budget_available_set()` — the public helper that wraps
    // `BUDGET_QUERY_TRIPLE_V02.2` — so a future change to the underlying
    // slice is caught here automatically without requiring cross-crate access
    // to the `pub(crate)` const.
    let available: HashSet<ReprKind> = reify_eval::Engine::budget_available_set();

    let demand = 1e-6_f64;
    assert_eq!(
        engine.compute_realization_tolerance_budget(&single_borrow, &available, demand),
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
    // and tests/harness_tolerance/tolerance_dispatch_budget.rs::lib_re_exports_per_stage_tolerance_for_plan_and_dispatch_end_to_end.
    let plan_two = DispatchPlan {
        kernel: "manifold".to_string(),
        conversions: vec![
            (KernelId::Fidget, ReprKind::BRep, ReprKind::Sdf),
            (KernelId::Gmsh, ReprKind::Sdf, ReprKind::Mesh),
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

/// Final integration smoke: pins all four wiring axes simultaneously
/// against `Engine::tessellate_realizations`.
///
/// Single test that builds the canonical fixture (`step_input_template(50µm)`,
/// `step_output_template(1µm)`, `MyDesign` realization with one Box primitive
/// op, and `manufacturing_purpose("manufacturing", 1µm)`), runs
/// `engine.tessellate_realizations(&module)`, and asserts ALL FOUR
/// production-wiring contracts hold simultaneously:
///
/// 1. **Imported-tolerance-promise diagnostic emission**: `TessellateResult.diagnostics`
///    contains exactly one `Severity::Warning` carrying
///    `DiagnosticCode::ImportedTolerancePromiseInsufficient` whose message
///    names `"STEPInput"`. Pinned independently by
///    `build_emits_imported_tolerance_promise_insufficient_warning_when_demand_strictly_tighter_than_promise`
///    against `build()`; this test pins the same emission contract on the
///    `tessellate_realizations()` surface so a future refactor that splits
///    the diagnostic emission helper between `build` and
///    `tessellate_realizations` cannot disconnect one without the other.
/// 2. **Demanded-tolerance routing through per-stage budget to kernel.tessellate**:
///    the recording mock kernel's `tessellate_tolerances` records exactly one
///    entry equal to `1e-6` (the demanded tolerance, routed through
///    `compute_realization_tolerance_budget` against the default registry's
///    empty-conversion plan, which passes the demand through unchanged).
///    Pinned independently by
///    `tessellate_realizations_uses_demanded_tolerance_through_per_stage_budget`;
///    this test locks it as part of the integration-axis bundle.
/// 3. **RealizationCache populated at the demanded tolerance**:
///    `engine.realization_cache().lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))`
///    returns `Some(_)` after `tessellate_realizations()` completes. Pinned
///    independently by `build_populates_realization_cache_keyed_on_demanded_tolerance`
///    against `build()`; this test pins the same cache-population contract
///    on the `tessellate_realizations()` surface.
/// 4. **Per-realization budget consumption (implicitly pinned by axis 2)**:
///    the budget pipeline runs through `compute_realization_tolerance_budget`
///    with the inventory-collected registry — under the v0.2 occt-only
///    inventory the dispatch returns a 0-conversion plan and the demand
///    passes through bit-exactly; multi-kernel adapters will produce a
///    real chain when they land.
///    `per_stage_tolerance_for_plan_governs_tolerance_budget_for_two_stage_dispatch_chain`
///    pins the multi-stage primitive in isolation; this test's axis 2 pin
///    asserts the integration carries the demand value through to the
///    kernel correctly.
///
/// **Why a single test for all four axes**: each axis is already
/// independently pinned by its own regression test above, but the
/// integration shape — running them simultaneously through ONE invocation
/// of `tessellate_realizations` — guards against a future refactor that
/// re-orders the build pipeline and disconnects one of the axes. A
/// regression here flags an ordering bug in the wiring even when each
/// individual unit test still passes.
///
/// **Reuses the recording-extension on `MockGeometryKernel`**:
/// `MockGeometryKernel` exposes the recorded `tessellate(handle, tol)`
/// calls via `tessellate_tolerances_ref()`, giving shared access so the
/// kernel can be transferred into the engine via `Box::new` and we can
/// still observe the recorded tolerances after `tessellate_realizations()`
/// returns.
#[test]
fn end_to_end_tolerance_wiring_threads_promise_diagnostic_cache_and_per_stage_budget() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_end_to_end_tolerance_wiring_smoke".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let tess_tols_handle = kernel.tessellate_tolerances_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let tess = engine.tessellate_realizations(&module);

    // ── Axis 1: ImportedTolerancePromiseInsufficient diagnostic on tessellate ──
    let promise_warnings: Vec<_> = tess
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::ImportedTolerancePromiseInsufficient)
        })
        .collect();
    assert_eq!(
        promise_warnings.len(),
        1,
        "axis 1: TessellateResult.diagnostics must contain exactly one \
         ImportedTolerancePromiseInsufficient warning (50µm promise vs 1µm \
         demand → strict-< insufficient); got {} matching diagnostics. Full \
         diagnostic set: {:?}",
        promise_warnings.len(),
        tess.diagnostics,
    );
    assert!(
        promise_warnings[0].message.contains("STEPInput"),
        "axis 1: warning message must name the input template so authors \
         can locate the import site; got: {:?}",
        promise_warnings[0].message,
    );

    // ── Axis 2: kernel.tessellate received the demanded tolerance ──
    let recorded_tols = tess_tols_handle.lock().unwrap().clone();
    assert_eq!(
        recorded_tols.len(),
        1,
        "axis 2: expected exactly one tessellate(handle, tol) call (one \
         realization with one terminal handle); got {} recorded tolerance(s): \
         {:?}",
        recorded_tols.len(),
        recorded_tols,
    );
    assert_eq!(
        recorded_tols[0], 1e-6,
        "axis 2: kernel.tessellate must receive the demanded tolerance \
         (1µm from STEPOutput body + manufacturing(1e-6)) routed through \
         compute_realization_tolerance_budget with the default registry's \
         empty-conversion plan (pass-through). Got {} (the module-pragma \
         default 0.0001 indicates the per-stage budget pipeline is bypassed \
         and effective_tessellation_tolerance is forwarded instead). Full \
         recorded tolerances: {:?}",
        recorded_tols[0], recorded_tols,
    );

    // ── Axis 3: RealizationCache populated at the demanded tolerance ──
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "axis 3: tessellate_realizations() must populate the RealizationCache \
         at (\"MyDesign\", ReprKind::BRep, 1e-6) after a successful realization \
         (mirrors the build() population contract pinned by \
         build_populates_realization_cache_keyed_on_demanded_tolerance). Cache \
         len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Builds a `MyDesign` module (`STEPOutput(1e-6)` + shared
/// `my_design_template_with_box_realization()` + `manufacturing_purpose`),
/// constructs an `Engine` with mock checker/kernel, and drives the canonical
/// `eval → activate_purpose → build` flow to populate the
/// `RealizationCache`. Asserts the cache-populated premise before returning
/// so a caller's post-op assertion is never vacuous.
///
/// `module_name` becomes the built module's single `ModulePath` segment —
/// purely a debug label, since every assertion keys on the fixed entity id
/// `"MyDesign"`, not the module path.
fn engine_with_populated_realization_cache(module_name: &str) -> reify_eval::Engine {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![module_name.to_string()]))
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
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "test premise: expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() (per the \
         build-time realization-cache population contract). Without this \
         premise the caller's post-op assertion is vacuous. Cache len={}, \
         dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    engine
}

/// One body of the γ (#4730) two-body fixture: a template named `name`
/// whose single named realization is a `Box` whose width READS the Length
/// param `<name>.w` (default `width_mm`), with a literal `depth_mm`. A
/// second param, `<name>.label`, is read by nothing — editing it moves no
/// realization's input cone.
///
/// The realization entity equals the template name because `build()`'s
/// schedule filter matches `rid.entity == template.name`.
fn body_template_reading_width(
    name: &str,
    width_mm: f64,
    depth_mm: f64,
) -> reify_compiler::TopologyTemplate {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            (
                "width".into(),
                CompiledExpr::value_ref(ValueCellId::new(name, "w"), Type::length()),
            ),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(depth_mm)),
        ],
    };
    TopologyTemplateBuilder::new(name)
        .param(name, "w", Type::length(), Some(mm_lit(width_mm)))
        .param(name, "label", Type::dimensionless_scalar(), None)
        .realization_named(name, 0, "body", vec![box_op])
        .build()
}

/// The two-body module: `PartA` (w = 10mm, depth `part_a_depth_mm`) and
/// `PartB` (w = 20mm, depth 5mm), plus one manufacturing purpose PER
/// entity. `activate_purpose` is keyed by purpose name, so one shared purpose
/// would silently bind only the first entity and leave the other uncached.
fn two_body_module(module_name: &str, part_a_depth_mm: f64) -> reify_compiler::CompiledModule {
    CompiledModuleBuilder::new(ModulePath::new(vec![module_name.to_string()]))
        .template(step_output_template(1e-6))
        .template(body_template_reading_width("PartA", 10.0, part_a_depth_mm))
        .template(body_template_reading_width("PartB", 20.0, 5.0))
        .compiled_purpose(manufacturing_purpose("mfg_a", 1e-6))
        .compiled_purpose(manufacturing_purpose("mfg_b", 1e-6))
        .build()
}

/// An engine over [`two_body_module`] after `eval → activate both purposes →
/// build`, with the mock kernel's op recorder captured before boxing.
struct TwoCachedBodies {
    engine: reify_eval::Engine,
    module: reify_compiler::CompiledModule,
    ops: std::sync::Arc<std::sync::Mutex<Vec<reify_test_support::mocks::GeometryOpRecord>>>,
}

impl TwoCachedBodies {
    fn served(&self, entity: &str) -> Option<reify_ir::KernelHandle> {
        self.engine
            .test_terminal_handle(entity, ReprKind::BRep, 1e-6)
    }

    fn op_count(&self) -> usize {
        self.ops.lock().unwrap().len()
    }

    /// The kernel op that produced `handle`.
    fn producing_op(&self, handle: reify_ir::KernelHandle) -> reify_ir::GeometryOp {
        self.ops
            .lock()
            .unwrap()
            .iter()
            .find(|r| r.result_handle == handle.id)
            .unwrap_or_else(|| panic!("no recorded op produced {handle:?}"))
            .op
            .clone()
    }
}

/// Builds [`TwoCachedBodies`] and PREMISE-LOCKS that both bodies are cache
/// resident at `(entity, BRep, 1e-6, NO_OPTIONS)` — without that, every
/// survival assertion a caller makes is vacuous.
fn engine_with_two_cached_bodies(module_name: &str) -> TwoCachedBodies {
    let module = two_body_module(module_name, 5.0);
    let kernel = MockGeometryKernel::new();
    let ops = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let _eval = engine.eval(&module);
    engine.activate_purpose("mfg_a", "PartA");
    engine.activate_purpose("mfg_b", "PartB");
    let _build = engine.build(&module, ExportFormat::Step);

    let fixture = TwoCachedBodies {
        engine,
        module,
        ops,
    };
    for entity in ["PartA", "PartB"] {
        assert!(
            fixture.served(entity).is_some(),
            "test premise: {entity} must be cache-resident at (BRep, 1e-6) after the \
             cold build; cache dump: {:?}",
            fixture.engine.realization_cache(),
        );
    }
    fixture
}

/// γ (#4730, PRD `selective-realization-eviction` D5) supersedes the task
/// #2874 expression "`edit_param` flushes the whole realization cache" while
/// keeping its invariant — no stale handle is ever served after an edit. An
/// edit of `PartA.w` evicts `PartA`'s family and nothing else: `PartB`'s
/// input cone did not move, so its cached handle stays valid and stays
/// servable.
#[test]
fn edit_param_evicts_only_the_edited_bodys_family_and_the_unaffected_body_still_hits() {
    let mut fixture = engine_with_two_cached_bodies("test_edit_param_keyed_eviction");
    let part_b_before = fixture.served("PartB");

    fixture
        .engine
        .edit_param(ValueCellId::new("PartA", "w"), mm(30.0))
        .expect("edit_param must succeed against the PartA.w Length param");

    assert_eq!(
        fixture.served("PartA"),
        None,
        "the edited body's family must be evicted, or the next build serves its \
         stale handle; cache dump: {:?}",
        fixture.engine.realization_cache(),
    );
    assert_eq!(
        fixture.served("PartB"),
        part_b_before,
        "the unaffected body's entry must survive the edit unchanged"
    );
}

/// γ (#4730, PRD D5) supersedes task #2874's "a REJECTED `edit_param` still
/// flushes". A rejected edit moves nothing, so evicting nothing is correct:
/// both entries are unchanged and the next `build_snapshot` is served
/// entirely from the cache — not stale, because no input moved.
#[test]
fn rejected_edit_param_evicts_nothing_and_the_next_build_serves_only_cache_hits() {
    let mut fixture = engine_with_two_cached_bodies("test_rejected_edit_param_evicts_nothing");
    let before = (fixture.served("PartA"), fixture.served("PartB"));

    let result = fixture
        .engine
        .edit_param(ValueCellId::new("PartA", "no_such_param"), Value::Real(1.0));
    assert!(
        matches!(result, Err(reify_eval::EngineError::CellNotFound { .. })),
        "expected CellNotFound for an absent cell, got {result:?}"
    );
    assert_eq!(
        (fixture.served("PartA"), fixture.served("PartB")),
        before,
        "a rejected edit must leave every cached entry unchanged"
    );

    let ops_before = fixture.op_count();
    let module = fixture.module.clone();
    fixture.engine.build_snapshot(&module, ExportFormat::Step);
    assert_eq!(
        fixture.op_count(),
        ops_before,
        "nothing moved, so the build after a rejected edit must dispatch no kernel op"
    );
}

/// γ (#4730, PRD D5/D7) supersedes task #2874's "`edit_source` flushes the
/// whole realization cache". The v2 module changes ONLY `PartA`'s box (its
/// depth literal); `PartB`'s template is byte-identical. The recompiled body
/// is evicted, the identical one survives with the same handle, and the
/// next build re-executes exactly `PartA`'s one op — producing the v2 depth.
#[test]
fn edit_source_evicts_the_recompiled_body_and_keeps_the_byte_identical_one() {
    let mut fixture = engine_with_two_cached_bodies("test_edit_source_keyed_eviction");
    let part_b_before = fixture.served("PartB");

    let module_v2 = two_body_module("test_edit_source_keyed_eviction_v2", 7.5);
    fixture
        .engine
        .edit_source(&module_v2)
        .expect("edit_source must succeed against the structurally-valid v2 module");

    assert_eq!(
        fixture.served("PartA"),
        None,
        "the recompiled body's family must be evicted"
    );
    assert_eq!(
        fixture.served("PartB"),
        part_b_before,
        "the byte-identical body must survive the recompile with the same handle"
    );

    let ops_before = fixture.op_count();
    fixture
        .engine
        .build_snapshot(&module_v2, ExportFormat::Step);
    let new_ops: Vec<_> = fixture.ops.lock().unwrap()[ops_before..]
        .iter()
        .map(|r| r.op.clone())
        .collect();
    assert_eq!(
        new_ops.len(),
        1,
        "exactly PartA's one op must re-execute; new ops: {new_ops:?}"
    );
    assert!(
        matches!(&new_ops[0], reify_ir::GeometryOp::Box { depth, .. } if *depth == mm(7.5)),
        "the re-executed box must carry the v2 depth; got {:?}",
        new_ops[0]
    );
}

/// `edit_source`'s only rejection path is `NotInitialized`
/// (`self.eval_state.is_none()`, checked first in the function body). This
/// pins that the guard fires — and returns cleanly rather than panicking —
/// before anything else in the body, including the
/// `self.eval_state.as_ref().unwrap()` further down that would panic were
/// the guard ever removed or reordered past it. A never-eval'd engine's
/// cache is empty either way, so the cache assertion only confirms the
/// rejected call left no unexpected state behind.
#[test]
fn edit_source_rejects_with_not_initialized_before_any_eval() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_edit_source_not_initialized_guard".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.edit_source(&module);
    assert!(
        matches!(result, Err(reify_eval::EngineError::NotInitialized)),
        "expected edit_source on a never-eval'd Engine to be rejected with \
         EngineError::NotInitialized rather than panicking or succeeding, \
         got {:?}",
        result,
    );
    assert!(
        engine.realization_cache().is_empty(),
        "expected realization_cache to remain empty after a rejected \
         edit_source call on a never-eval'd Engine; len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Landed contract: `Engine::clear_realization_cache(&mut self)` is a
/// public, un-gated mutator on `Engine` (engine_admin.rs), giving production
/// callers a non-destructive realization-cache flush primitive. The
/// read-side `realization_cache(&self)` accessor stays
/// `#[cfg(any(test, feature = "test-instrumentation"))]`-gated.
///
/// Pins the public escape hatch the docstring critique demands: production
/// callers MUST be able to flush the realization cache without enabling
/// test instrumentation. The READ-side `realization_cache(&self)` accessor
/// is cfg-gated and READ-ONLY (the cache stores kernel-internal
/// `GeometryHandleId` values that should not leak into the production
/// surface), so a production caller that needs to invalidate cached
/// `GeometryHandleId`s outside the auto-invalidation hook points
/// (`edit_param` / `edit_source`) needs a WRITE-side primitive that does not
/// require constructing a fresh `Engine` (which would discard every other
/// piece of engine state: snapshots, param overrides, registered
/// solvers/kernels, `feature_tag_table`, `topology_attribute_table`, etc.).
///
/// `pub fn clear_realization_cache(&mut self)` carries no cfg gate (mirrors
/// the `Engine::clear_param_overrides` precedent in `engine_admin.rs`), so
/// it is that WRITE-side primitive: the READ-side accessor
/// `realization_cache(&self)` keeps its cfg gate, but the WRITE-side
/// mutator is public so the docstring's promised mitigation is actually
/// reachable.
///
/// Setup uses the shared `engine_with_populated_realization_cache` helper
/// (STEPOutput(1e-6) + MyDesign with one Box-primitive realization +
/// manufacturing(1e-6)). Sequence:
///   (a) `engine_with_populated_realization_cache(...)` — eval →
///       activate_purpose → build → assert cache populated.
///   (b) Call `engine.clear_realization_cache()` directly (no cfg-gated
///       accessor; this is a production-surface mutator).
///   (c) Assert the cache is empty at `(MyDesign, BRep, 1e-6)`.
#[test]
fn clear_realization_cache_public_api_resets_cache_for_production_callers() {
    // (a) Cold-start eval, activate purpose, build → cache populated by the
    // build-time wiring; the helper asserts the cache-populated premise
    // before returning so the post-clear assertion below is never vacuous.
    let mut engine =
        engine_with_populated_realization_cache("test_clear_realization_cache_public_api");

    // (b) Call the public escape hatch `Engine::clear_realization_cache`.
    // This is the critical line — it compiles iff
    // `Engine::clear_realization_cache` is a public method on the un-gated
    // production surface. A `pub(crate)` or cfg-gated method would still
    // pass type-checking inside this test crate (since `cfg(test)` is on
    // for integration tests too), so the docstring above reinforces that
    // the gate-LESS shape is intentional and that test-only callers should
    // NOT be the only consumers.
    engine.clear_realization_cache();

    // (c) Assert the cache was cleared by the public mutator — the cache is
    // keyed on `(entity_id, repr_kind, demanded_tol)` and a cleared cache
    // returns `None` for every lookup, including exact-key ones.
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_none(),
        "expected Engine::clear_realization_cache() to flush the cache so \
         every (entity_id, repr_kind, demanded_tol) lookup returns None. \
         Lookup at (\"MyDesign\", ReprKind::BRep, 1e-6) returned Some(_) \
         after clear_realization_cache() — the entry survived the clear, \
         breaking the Engine::clear_realization_cache public-mutator \
         contract. Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Task 3103: pins that the active tolerance scope survives
/// `build()`'s internal eval cycle so callers need no re-activation.
///
/// The canonical user flow is `engine.eval → activate_purpose → engine.build`.
/// Before task 3103, `Engine::eval` (called internally by `build()`) cleared
/// `active_purpose_bindings` and `active_tolerance_scope`, so after `build()`
/// returned the scope was empty even though the user had activated a purpose.
/// Task 3103 fixes this by preserving bindings across eval() and re-injecting
/// them against the fresh snapshot; the tolerance scope therefore survives the
/// internal eval round-trip.
///
/// Precondition: `active_tolerance_for("MyDesign")` returns `Some(1e-6)`
/// immediately after `activate_purpose`.
/// Post-build assertion: `active_tolerance_for("MyDesign")` still returns
/// `Some(1e-6)` WITHOUT any re-activation between the user's
/// `activate_purpose` call and `build()`.
#[test]
fn eval_then_activate_purpose_then_build_preserves_tolerance_scope_across_internal_eval() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_tol_scope_survives_build_internal_eval".to_string(),
    ]))
    .template(step_input_template(50e-6))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // Canonical user flow: eval → activate_purpose (no re-activation after this)
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    // Precondition: scope is populated before build()
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(1e-6),
        "precondition: active_tolerance_for must return Some(1e-6) immediately \
         after activate_purpose"
    );

    // build() calls check() → eval() internally; task 3103 ensures the scope
    // is preserved across that internal eval round-trip.
    let _build = engine.build(&module, ExportFormat::Step);

    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(1e-6),
        "expected the active tolerance scope to survive build()'s internal eval — \
         task 3103 closes the gap where eval() cleared active_purpose_bindings / \
         active_tolerance_scope and forced production callers to re-activate \
         purposes between every build"
    );
}

/// Task 3176: pins that an anonymous realization (one whose
/// `RealizationDecl.name == None`, constructed via
/// `TopologyTemplateBuilder::realization(...)` rather than
/// `realization_named(...)`) does NOT populate the `RealizationCache` even
/// when a demanded tolerance is active.
///
/// **Why anonymous realizations exist in this test only**: the production
/// compiler always emits `Some(name)` for every `RealizationDecl` it produces
/// (see `RealizationDecl::name` in `crates/reify-compiler/src/types.rs`). `None` only arises
/// from the `TopologyTemplateBuilder::realization(...)` test-support helper,
/// which is what this test uses to exercise the anonymous-realization code
/// path.
///
/// **The regression this test guards against**: the post-success
/// cache-insert gate in `execute_realization_ops` matches the cache-hit
/// short-circuit's lookup gate exactly — both gates are
/// `is_terminal_realization && let (Some(tol), Some(_name)) =
/// (demanded_tol, realization_name)`,
/// so an anonymous realization never populates the cache. Were the two
/// gates to drift apart again, an anonymous realization would populate the
/// cache on the first build but could never be served from it: the lookup
/// gate requires a name, so subsequent builds would skip the
/// short-circuit, the kernel would re-run, and the post-success insert
/// would hit `ToleranceBucket::insert`'s partial-order rejection (the
/// prior entry already satisfies) — wasting the cached slot and re-running
/// the op chain every build.
///
/// Sequence:
///   (a) `engine.eval(&module)` → `engine.activate_purpose("manufacturing",
///       "MyDesign")` → `engine.build(&module, ExportFormat::Step)`.
///   (b) Assert kernel was invoked (premise check: build path reached
///       `execute_realization_ops`).
///   (c) Assert `engine.realization_cache().len() == 0` — the anonymous
///       realization must not populate the cache.
///   (d) Second `engine.build(...)` → assert `len() == 0` again (no slot
///       wastage across repeated builds).
///
/// Complements `build_populates_realization_cache_keyed_on_demanded_tolerance`
/// (which pins that NAMED realizations DO populate the cache).
#[test]
fn anonymous_realization_does_not_populate_realization_cache_when_lookup_gate_requires_name() {
    // Build a module with an ANONYMOUS realization — `realization(...)` not
    // `realization_named(...)` — so `RealizationDecl.name == None`.
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(10.0)),
            ("height".into(), mm_lit(20.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    };
    let anonymous_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        // `realization(...)` → `RealizationDecl.name == None`
        .realization("MyDesign", 0, vec![box_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_anonymous_realization_does_not_populate_cache".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(anonymous_template)
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_handle = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // (a) Canonical user flow: eval → activate_purpose → build.
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");
    engine.build(&module, ExportFormat::Step);

    // (b) Premise check: the kernel was invoked (build actually reached
    // `execute_realization_ops` and dispatched at least one op).
    let ops_after_first = ops_handle.lock().unwrap().len();
    assert!(
        ops_after_first >= 1,
        "test premise: expected build() to invoke the kernel at least once \
         (execute_realization_ops dispatched at least one op); got \
         ops_after_first={}. If this fails the test is vacuous.",
        ops_after_first,
    );

    // (c) Core assertion: the anonymous realization must NOT populate the cache.
    // The insert gate requires is_terminal_realization and
    // realization_name.is_some() in addition to demanded_tol.is_some(), so
    // it is skipped here (realization_name.is_none()).
    assert_eq!(
        engine.realization_cache().len(),
        0,
        "expected RealizationCache to be empty after building an anonymous \
         realization (RealizationDecl.name == None): the post-success insert \
         gate must require realization_name.is_some() to match the lookup gate. \
         Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    // (d) Belt-and-braces: a second build must also leave the cache empty —
    // no slot wastage across repeated builds.
    // Note: task 3103 made eval() preserve active_purpose_bindings across
    // its internal round-trip, so no re-activation is needed here.
    engine.build(&module, ExportFormat::Step);
    assert_eq!(
        engine.realization_cache().len(),
        0,
        "expected RealizationCache to remain empty after a second build with \
         an anonymous realization; cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Task #3176's end-to-end `edit → build_snapshot` freshness contract,
/// re-expressed for γ (#4730, PRD `selective-realization-eviction` D5).
///
/// #3176 edited a param that fed NO body and asserted the kernel re-ran —
/// a pin of the wholesale flush, under which keyed eviction would correctly
/// HIT. The invariant it guarded is that the handle a `build_snapshot`
/// serves after an edit is never stale. Here the edit feeds `PartA` only:
/// the kernel re-runs exactly `PartA`'s one op, the handle `PartA` now serves
/// was produced by a Box carrying the NEW width, and `PartB`'s handle is
/// untouched. A follow-up display-only edit dispatches nothing at all.
///
/// No re-activation between calls: neither `edit_param` nor
/// `build_snapshot` calls `eval()`, and task 3103 made `eval()` preserve
/// `active_purpose_bindings` anyway.
#[test]
fn edit_param_followed_by_build_snapshot_re_executes_only_the_edited_body_so_no_handle_is_stale() {
    let mut fixture = engine_with_two_cached_bodies("test_edit_param_then_build_snapshot");
    let module = fixture.module.clone();
    let part_b_before = fixture.served("PartB");

    let ops_before = fixture.op_count();
    fixture
        .engine
        .edit_param(ValueCellId::new("PartA", "w"), mm(30.0))
        .expect("edit_param must succeed against the PartA.w Length param");
    fixture.engine.build_snapshot(&module, ExportFormat::Step);

    assert_eq!(
        fixture.op_count() - ops_before,
        1,
        "exactly PartA's one op must re-execute"
    );
    let part_a = fixture
        .served("PartA")
        .expect("PartA must be re-cached by the rebuild");
    assert!(
        matches!(
            fixture.producing_op(part_a),
            reify_ir::GeometryOp::Box { width, .. } if width == mm(30.0)
        ),
        "PartA's served handle must come from a Box with the NEW width, not a stale one"
    );
    assert_eq!(
        fixture.served("PartB"),
        part_b_before,
        "PartB's handle must be untouched by an edit that did not feed it"
    );

    let ops_before_label = fixture.op_count();
    fixture
        .engine
        .edit_param(ValueCellId::new("PartB", "label"), Value::Real(0.5))
        .expect("edit_param must succeed against the display-only PartB.label param");
    fixture.engine.build_snapshot(&module, ExportFormat::Step);
    assert_eq!(
        fixture.op_count(),
        ops_before_label,
        "a display-only edit moves no realization, so the rebuild dispatches nothing"
    );
}

/// Characterization test: regression pin for the cache-hit short-circuit's
/// attribute-table behaviour.
///
/// **What this test pins:** after the second `build()` call is served from
/// `RealizationCache` (the cache-hit short-circuit at
/// `engine_build.rs::execute_realization_ops` fires), `topology_attribute_table`
/// is empty (no entry for the sphere's seeded face handle, table-wide empty).
/// The root cause is documented in the "Known limitation" docstring on
/// `execute_realization_ops`: the short-circuit returns early before the
/// per-op primitive-attribute-seeding call, and the table is reset to
/// `default()` at the start of every `build()`.
///
/// (Prior to task 4827 this test pinned the same short-circuit behaviour via
/// the now-deleted `feature_tag_table`, keyed on the parent-solid handle
/// directly. `topology_attribute_table` only ever records entries for a
/// primitive's extracted faces/edges/vertices — never for the primitive's own
/// result handle — so the re-pointed PINs below key on the sphere's seeded
/// face handle instead.)
///
/// **Why MockGeometryKernel with staged extraction fixtures:** (1) The test
/// runs unconditionally — no `OCCT_AVAILABLE` skip gate that could hide the
/// regression in OCCT-less CI environments.  (2)
/// `MockGeometryKernel::operations_ref()` exposes the ops counter needed to
/// assert the cache-hit short-circuit actually fired, which is the
/// non-vacuousness premise for the regression assertions.  (3) A bare
/// `MockGeometryKernel` (no staged fixtures) errors on every
/// `extract_faces`/`extract_edges` call, which would make the primitive
/// seeder return `Err` (silently swallowed as "auxiliary-metadata failure")
/// and leave `topology_attribute_table` permanently empty — vacuously
/// satisfying the PINs for the wrong reason. Staging
/// `with_extracted_faces`/`with_extracted_edges` for the sphere's
/// `GeometryHandleId(1)` (the kernel's one-and-only handle in this fixture)
/// makes the seeding path actually populate the table, so the SANITY check
/// below is meaningful. A `Sphere` primitive is used (rather than `Box`)
/// because its seeding arm only extracts faces/edges — no vertex extraction
/// or `GeometryQuery::BoundingBox` staging is required.
///
/// **This is a CHARACTERIZATION test** — it passes immediately on first run
/// because it pins existing behaviour.  Flipping any assertion to FAIL is the
/// regression signal: it means either the per-build reset stopped firing, or
/// population moved outside the op-loop, or the cache short-circuit stopped
/// firing.
#[test]
fn cache_hit_short_circuit_leaves_topology_attribute_table_empty_after_second_build() {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_lit(5.0))],
    };
    let sphere_realization_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![sphere_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cache_hit_leaves_topology_attribute_table_empty".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(sphere_realization_template)
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // The sphere is the sole op dispatched by this engine instance, so the
    // kernel's first (and only) handle is `GeometryHandleId(1)` — stage its
    // face/edge extraction so the primitive seeder actually populates
    // `topology_attribute_table` (see "Why MockGeometryKernel" above).
    let seeded_solid = reify_ir::GeometryHandleId(1);
    let seeded_face = reify_ir::GeometryHandleId(101);
    let seeded_edge = reify_ir::GeometryHandleId(102);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new()
        .with_extracted_faces(seeded_solid, vec![seeded_face])
        .with_extracted_edges(seeded_solid, vec![seeded_edge]);
    let ops_handle = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    // ── Build #1 ──────────────────────────────────────────────────────────────
    let _build1 = engine.build(&module, ExportFormat::Step);
    let ops_after_first = ops_handle.lock().unwrap().len();

    // SANITY: kernel was invoked on build #1 (cache miss → op dispatched →
    // cache populated). Without this precondition the regression assertions
    // below could be trivially vacuous if the test fixture is broken.
    assert!(
        ops_after_first >= 1,
        "sanity: expected first build() to invoke the kernel at least once \
         (cache miss → realization ops dispatched, cache populated); got \
         ops_after_first={}",
        ops_after_first,
    );

    // Capture the handle that the first build stored in the RealizationCache.
    let cached_handle: reify_ir::KernelHandle = *engine
        .realization_cache()
        .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
        .expect(
            "sanity: first build() must populate the RealizationCache at \
             (\"MyDesign\", ReprKind::BRep, 1e-6)",
        );

    // SANITY: the op-loop populated topology_attribute_table for the sphere's
    // seeded face handle on build #1 (via the staged extraction fixtures).
    // This makes the PIN below non-vacuous — if the table were already empty
    // after build #1, asserting it is empty after build #2 would prove
    // nothing.
    assert!(
        engine
            .topology_attribute_table()
            .lookup(reify_ir::KernelHandle {
                kernel: KernelId::Occt,
                id: seeded_face,
            })
            .is_some(),
        "sanity: expected topology_attribute_table to contain an entry for \
         seeded_face {:?} after build #1 — the primitive-attribute seeder in \
         the op-loop must record the sphere's extracted face. If this fires, \
         the seeding path has changed and the regression PIN assertions below \
         may no longer be meaningful.",
        seeded_face,
    );

    // Defensive re-activation: `eval()` PRESERVES `active_purpose_bindings`
    // by `mem::take`-ing them and re-applying via `activate_purpose()` after
    // the snapshot is rebuilt (task 3103, see engine_eval.rs around the
    // mem::take call). So this is a no-op against today's contract — the
    // second `activate_purpose("manufacturing", "MyDesign")` hits the
    // idempotent early-return in `activate_purpose_constraints` (see the
    // docstring on `activate_purpose` in engine_purposes.rs). It is kept as
    // belt-and-suspenders defense against a future regression in that
    // preservation contract that would otherwise silently defeat the test
    // premise (no demanded_tol → no cache lookup → no short-circuit).
    engine.activate_purpose("manufacturing", "MyDesign");

    // ── Build #2 ──────────────────────────────────────────────────────────────
    let _build2 = engine.build(&module, ExportFormat::Step);
    let ops_after_second = ops_handle.lock().unwrap().len();

    // SANITY: cache short-circuit fired on build #2 (kernel NOT re-invoked).
    // Without this premise the regression assertions are vacuous: a non-firing
    // short-circuit would let the op-loop run and re-populate the tables
    // normally, so the PINs below would be testing the wrong code path.
    assert_eq!(
        ops_after_second,
        ops_after_first,
        "sanity: expected second build() to be served entirely from \
         RealizationCache (cache-hit short-circuit); got \
         ops_after_first={}, ops_after_second={} — kernel was invoked \
         {} additional time(s), indicating the short-circuit did not fire.",
        ops_after_first,
        ops_after_second,
        ops_after_second - ops_after_first,
    );

    // SANITY: cache entry survived the second build untouched.
    assert_eq!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0)),
        Some(&cached_handle),
        "sanity: expected RealizationCache entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) to survive the second build; got \
         None — cache was cleared or key was invalidated unexpectedly.",
    );

    // ── Regression PINs ───────────────────────────────────────────────────────

    // PIN 1 (headline) + PIN 2 (stronger) layering rationale: PIN 2's
    // table-wide emptiness strictly implies PIN 1's per-handle absence, so
    // they overlap in truth condition.  Both are kept because PIN 1's failure
    // message names `seeded_face` directly and is the more diagnostic signal
    // for the canonical regression (population only of the seeded face),
    // while PIN 2 generalises to catch any new population path.
    //
    // PIN 1 (headline): the cache-hit short-circuit skips the per-op
    // primitive-attribute-seeding call, and the per-build reset at the top of
    // build() clears the table before the short-circuit fires. Net effect:
    // the previously-seeded face has no entry.
    assert!(
        engine
            .topology_attribute_table()
            .lookup(reify_ir::KernelHandle {
                kernel: KernelId::Occt,
                id: seeded_face,
            })
            .is_none(),
        "regression PIN: expected topology_attribute_table to have NO entry \
         for seeded_face {:?} on the second build — the cache-hit \
         short-circuit at engine_build.rs::execute_realization_ops \
         deliberately skips the per-op primitive-attribute seeder; if this \
         fires, either the per-build reset stopped firing or population moved \
         outside the op-loop.",
        seeded_face,
    );

    // PIN 2 (stronger): the entire table is empty — no spurious population from
    // any other source.
    assert!(
        engine.topology_attribute_table().is_empty(),
        "regression PIN: expected topology_attribute_table to be completely \
         empty after a cache-served build — only the primitive-attribute \
         seeder in the op-loop populates this table, and the cache-hit \
         short-circuit skips that loop entirely. If this fires, a new \
         population path outside the op-loop has been introduced.",
    );
}

/// Task 4152: `CacheStats::realization_entries` counts a REAL terminal
/// realization-cache entry exactly once, and a repeat build served from the
/// cache does not re-count it.
///
/// This is the counter's engine-level wiring test. It reuses the harness that
/// `build_populates_realization_cache_keyed_on_demanded_tolerance` established
/// — an `STEPOutput(1µm)` bound plus a `manufacturing` purpose activated
/// against `MyDesign` — because that combination is what makes `demanded_tol`
/// `Some`, and the terminal cache insert is gated on it. Without a tolerance
/// contract the engine deliberately caches nothing at all (`engine_build.rs`:
/// "no tolerance contract → no caching"), so a fixture lacking one is not a
/// valid vehicle for this assertion.
///
/// Three signals, in order:
///
/// 1. **Before `build()`**: a fresh engine has realized nothing → 0.
/// 2. **After `build()`**: `MyDesign`'s single named realization inserts one
///    terminal entry → 1. Conversion intermediates, when a build has any, go
///    through the uncounted plain `insert` and must not inflate this.
/// 3. **After a second `build()` of the same module**: still 1 — the cache-hit
///    short-circuit serves the realization, and the dominated re-insert that
///    would follow returns `false`, so nothing new is counted.
///
/// `realization_cache().len()` is asserted alongside as an independent second
/// opinion: it counts entries CURRENTLY resident, the counter counts entries
/// EVER created. They agree here only because nothing was evicted — which is
/// exactly why the counter, not `len()`, is the durable signal.
#[test]
fn realization_entries_counts_terminal_cache_entry_once_and_not_on_cache_hit() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_realization_entries_counts_terminal_cache_entry".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    assert_eq!(
        engine.cache_stats().realization_entries,
        0,
        "a fresh engine must have realized and cached no geometry yet"
    );

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let _build = engine.build(&module, ExportFormat::Step);

    assert_eq!(
        engine.realization_cache().len(),
        1,
        "sanity: the demanded-tolerance contract must have populated exactly one \
         cache entry (cache dump: {:?})",
        engine.realization_cache(),
    );
    assert_eq!(
        engine.cache_stats().realization_entries,
        1,
        "one terminal realization must create exactly one COUNTED cache entry"
    );

    // Second build of the SAME module: served from the cache, so no new entry.
    let _build2 = engine.build(&module, ExportFormat::Step);
    assert_eq!(
        engine.cache_stats().realization_entries,
        1,
        "a repeat build served from the realization cache must NOT increment the \
         counter — a cache hit realized nothing"
    );
}

/// Task 4152: `realization_entries` SURVIVES `clear_realization_cache()`.
///
/// Counter survival is structural — see `RealizationCache::clear` in
/// `src/realization_cache.rs` for the mechanism and rationale, which this
/// test pins end-to-end rather than restates.
///
/// Asserts the flush genuinely emptied the cache (so this is not vacuously
/// true), that the counter is unmoved, and that a subsequent build re-realizes
/// — taking the counter to 2, because a flushed cache means a genuinely new
/// entry rather than a hit.
#[test]
fn realization_entries_survives_clear_realization_cache() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_realization_entries_survives_clear".to_string(),
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

    assert_eq!(
        engine.cache_stats().realization_entries,
        1,
        "precondition: the first build must have counted one terminal entry"
    );

    engine.clear_realization_cache();

    assert!(
        engine.realization_cache().is_empty(),
        "clear_realization_cache() must genuinely empty the cache — otherwise \
         the counter assertion below would be vacuous"
    );
    assert_eq!(
        engine.cache_stats().realization_entries,
        1,
        "the LIFETIME counter must survive a cache flush; edit_param/edit_source \
         both flush, so a reset here would zero the metric on every edit"
    );

    // Flushed cache → the next build cannot hit, so it genuinely re-realizes.
    let _build2 = engine.build(&module, ExportFormat::Step);
    assert_eq!(
        engine.cache_stats().realization_entries,
        2,
        "after a flush the next build must create a genuinely new entry, so the \
         counter advances to 2 (contrast the cache-hit case, which stays put)"
    );
}
