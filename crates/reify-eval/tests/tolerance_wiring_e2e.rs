//! End-to-end engine-level integration tests for task 2874 — exercises the
//! production-wired tolerance subsystem: dispatcher emission of import-promise
//! and zero-promise diagnostics on `build()`, `RealizationCache` population
//! and short-circuit keyed on demanded tolerance, and
//! `per_stage_tolerance_for_plan` consumption from the realization loop.
//!
//! Imports use the established test fixture surface
//! (`reify_test_support::{make_engine, step_input_template, step_output_template,
//! my_design_template, manufacturing_purpose}` + `CompiledModuleBuilder`).
//! Per-step tests are added by the subsequent TDD steps.

#[allow(unused_imports)]
use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
#[allow(unused_imports)]
use reify_eval::{
    DispatchPlan, dispatch, per_stage_tolerance_for_plan, tolerance_budget::per_stage_tolerance,
};
#[allow(unused_imports)]
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
#[allow(unused_imports)]
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, make_engine, manufacturing_purpose, mm,
    my_design_template, step_input_template, step_output_template,
};
#[allow(unused_imports)]
use reify_core::{ContentHash, DiagnosticCode, ModulePath, Severity, Type, ValueCellId};
use reify_ir::{CapabilityDescriptor, CompiledExpr, ExportFormat, Operation, ReprKind, Value};
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
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
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

/// Step-11 (failing initially; passes once step-12 wires
/// `Engine::compute_realization_tolerance_budget(...)` into the
/// `kernel.tessellate(...)` call site inside `tessellate_from_values`).
///
/// Pins that `Engine::tessellate_realizations(&module)` forwards the
/// per-output demanded tolerance — routed through
/// `compute_realization_tolerance_budget` against
/// `kernel_registry::collect_registry()` — to `GeometryKernel::tessellate`
/// instead of the module-level `effective_tessellation_tolerance` default
/// (`0.0001` SI metres = 0.1 mm).
///
/// Setup mirrors step-5/step-7: an STEPOutput template carries a 1 µm
/// `RepresentationWithin` body bound, a `MyDesign` template carries a single
/// named realization producing one `Box` primitive op, and
/// `manufacturing_purpose("manufacturing", 1e-6)` is activated against
/// `"MyDesign"`. The engine is constructed with a `MockGeometryKernel`
/// extended (step-11) with a `tessellate_tolerances: Arc<Mutex<Vec<f64>>>`
/// recorder; the test grabs the recorder via `tessellate_tolerances_ref()`
/// before transferring kernel ownership into the engine.
///
/// The test calls `engine.tessellate_realizations(&module)` once, then asserts
/// the recorder contains exactly one entry equal to `1e-6` — the demanded
/// tolerance — NOT `0.0001` (the module pragma default that
/// `effective_tessellation_tolerance` returns when `default_tolerance` is
/// `None`). With the helper's hard-coded `(BooleanUnion, BRep, {BRep})`
/// triple and the occt-only single-kernel registry, dispatch returns a
/// 0-conversion plan and `per_stage_tolerance_for_plan` passes the demand
/// through unchanged — so `budget == 1e-6` exactly.
///
/// Today (pre step-12) the tessellate path forwards
/// `Self::effective_tessellation_tolerance(module)` to `kernel.tessellate`,
/// so the recorder captures `0.0001` and the assertion FAILS. Once step-12
/// replaces that argument with the per-realization budget computed via
/// `compute_realization_tolerance_budget`, the assertion passes.
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

/// Step-13 (locks the partial-order semantics on the realization-cache
/// integration: a tighter demand cannot be served by a looser cached entry).
///
/// The `RealizationCache::lookup` rule (`cached_tol ≤ requested_tol`)
/// implements the "tighter satisfies looser" contract pinned at
/// `realization_cache.rs:101-116`: a cache populated at 1e-6 satisfies a
/// later request at any `tol ≥ 1e-6` (looser-or-equal), but a request at
/// `tol < 1e-6` (tighter) MUST miss because the cached representation is
/// at 1e-6 precision — insufficient for the tighter consumer. This test
/// pins that the cache integration honours that rule end-to-end through
/// `Engine::execute_realization_ops`'s cache-hit short-circuit (step-8).
///
/// Setup mirrors step-7 except a SECOND `manufacturing_tighter` purpose at
/// 1e-9 m is compiled into the same module. After the first `build()` (with
/// `manufacturing` at 1e-6 active) the cache is populated at
/// `("MyDesign", BRep, 1e-6)`. We then deactivate `manufacturing`, activate
/// `manufacturing_tighter` (which substitutes a fresh 1e-9 m
/// `RepresentationWithin` constraint at the same subject), and run `build()`
/// again. The second build's pre-`check()` precompute computes
/// `demanded_tol = Some(1e-9)` (the tightest contributor across the active
/// scope), threads that into `execute_realization_ops`, and the cache lookup
/// at `("MyDesign", BRep, 1e-9)` MISSES the cached `1e-6` entry — kernel
/// re-executes the realization ops, growing `kernel.operations()`.
///
/// The post-second-build `kernel.operations()` count must therefore strictly
/// EXCEED the post-first-build count (cache-miss path: kernel was invoked
/// again to satisfy the tighter demand). If the assertion fails — i.e. the
/// counts are equal — the cache is incorrectly serving a tighter request
/// from a looser cached entry, breaking the partial-order contract.
///
/// Step-14 (verification-only impl) confirms that no new wiring is needed:
/// the bucket lookup primitive already enforces `cached_tol ≤ requested_tol`,
/// and the engine wiring threads the requested tolerance to the bucket's
/// lookup unchanged. If this test FAILS today, the bug is in the
/// step-6 / step-8 wiring's cache-key value plumbing (stale `demanded_tol`
/// captured across builds) — investigate at the precompute site
/// (`tessellate_realizations` / `build`) and at the cache-lookup site at the
/// top of `execute_realization_ops`.
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
         `RealizationCache::lookup` (realization_cache.rs:101-116) and \
         `ToleranceBucket::lookup`.",
        ops_after_first,
        ops_after_second,
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
    let mut single: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    single.insert("occt".to_string(), occt);
    // Amendment 2: `compute_realization_tolerance_budget` now takes the
    // borrowed-value variant of the registry that `dispatch` requires —
    // production callers build it once per build inside
    // `compute_tessellation_budgets`. Direct test-seam callers build it at
    // the call site.
    let single_borrow: BTreeMap<String, &CapabilityDescriptor> =
        single.iter().map(|(k, v)| (k.clone(), v)).collect();
    // Amendment 3 (task 3227): `compute_realization_tolerance_budget` now
    // takes the `available: &HashSet<ReprKind>` as a caller-supplied arg.
    // Production callers hoist one HashSet per build in
    // `compute_tessellation_budgets`. Direct test-seam callers build it at
    // the call site, mirroring the borrowed-registry pattern.
    //
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

/// Step-15 (final integration smoke; pins all four wiring axes
/// simultaneously against `Engine::tessellate_realizations`).
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
///    names `"STEPInput"`. Pinned independently by step-1 against `build()`;
///    this step pins the same emission contract on the `tessellate_realizations()`
///    surface so a future refactor that splits the diagnostic emission helper
///    between `build` and `tessellate_realizations` cannot disconnect one
///    without the other.
/// 2. **Demanded-tolerance routing through per-stage budget to kernel.tessellate**:
///    the recording mock kernel's `tessellate_tolerances` records exactly one
///    entry equal to `1e-6` (the demanded tolerance, routed through
///    `compute_realization_tolerance_budget` against the default registry's
///    empty-conversion plan, which passes the demand through unchanged).
///    Pinned independently by step-11; this step locks it as part of the
///    integration-axis bundle.
/// 3. **RealizationCache populated at the demanded tolerance**:
///    `engine.realization_cache().lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))`
///    returns `Some(_)` after `tessellate_realizations()` completes. Pinned
///    independently by step-5 against `build()`; this step pins the same
///    cache-population contract on the `tessellate_realizations()` surface.
/// 4. **Per-realization budget consumption (implicitly pinned by axis 2)**:
///    the budget pipeline runs through `compute_realization_tolerance_budget`
///    with the inventory-collected registry — under the v0.2 occt-only
///    inventory the dispatch returns a 0-conversion plan and the demand
///    passes through bit-exactly; multi-kernel adapters will produce a
///    real chain when they land. Step-9 pins the multi-stage primitive
///    in isolation; this step's axis 2 pin asserts the integration carries
///    the demand value through to the kernel correctly.
///
/// **Why a single test for all four axes**: each axis is already
/// independently pinned by a step-N regression test, but the integration
/// shape — running them simultaneously through ONE invocation of
/// `tessellate_realizations` — guards against a future refactor that
/// re-orders the build pipeline and disconnects one of the axes. A
/// regression here flags an ordering bug in the wiring even when each
/// individual unit test still passes.
///
/// **Reuses the recording-extension on `MockGeometryKernel`**:
/// `tessellate_tolerances_ref()` (added in step-11) gives shared access to
/// the recorded `tessellate(handle, tol)` calls so the kernel can be
/// transferred into the engine via `Box::new` and we can still observe
/// the recorded tolerances after `tessellate_realizations()` returns.
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
         (mirrors the build() population contract pinned by step-5). Cache \
         len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Step-17 (failing initially; passes once step-18 wires the auto-invalidation
/// hook into `Engine::edit_param`).
///
/// Pins the production-correctness fix for the reviewer's blocking issue
/// (engine_build.rs:511-516 + engine_admin.rs:218-230 + the field docstring on
/// `Engine::realization_cache` at lib.rs:490-535): the cache MUST be reset on
/// `edit_param` so a subsequent `build_snapshot()` cannot silently return a
/// stale `GeometryHandleId` pointing at the OLD geometry. The current field
/// docstring promises "Production callers must therefore either (a) avoid
/// `build_snapshot` after `edit_param`, or (b) clear `realization_cache`
/// themselves between the edit and the snapshot rebuild" — but the public
/// surface offers no clear-cache primitive (the only mutator is internal),
/// so clause (b) is unreachable and clause (a) defeats `build_snapshot`'s
/// incremental-rebuild contract. The fix is to auto-flush the cache at the
/// `edit_param` hook point — mirroring the `feature_tag_table` /
/// `topology_attribute_table` reset-at-hook-point pattern.
///
/// Setup mirrors step-5 / step-7 / step-15: `step_output_template(1µm)` plus
/// `MyDesign` template with one Box realization plus
/// `manufacturing_purpose("manufacturing", 1e-6)`. `MyDesign.thickness : Real`
/// is the param cell we mutate — it does not need to drive the Box's args for
/// this test, since the assertion is on cache-state immediately after
/// `edit_param` returns (NOT on whether a subsequent build produces fresh
/// geometry). Any `edit_param` invocation against any param cell in the
/// graph must clear the cache, because the engine cannot know which cells
/// participate in the realization's input cone without a cross-reference
/// the current architecture does not maintain.
///
/// Sequence:
///   (a) `engine.eval(&module)` → `engine.activate_purpose("manufacturing",
///       "MyDesign")` → `engine.build(&module, ExportFormat::Step)` →
///       assert `engine.realization_cache().lookup("MyDesign", ReprKind::BRep,
///       1e-6, ContentHash(0)).is_some()` (cache populated by step-6 wiring; pins
///       the test premise).
///   (b) `engine.edit_param(ValueCellId::new("MyDesign", "thickness"),
///       Value::Real(<new>)).unwrap()`.
///   (c) Without calling `build_snapshot` yet, assert
///       `engine.realization_cache().lookup("MyDesign", ReprKind::BRep,
///       1e-6, ContentHash(0)).is_none()` — the entry was cleared on edit.
///
/// Today (pre step-18) `edit_param` does NOT touch `realization_cache`, so
/// the cache entry persists across the edit and the next `build_snapshot()`
/// would silently return the stale handle. Step-18 adds
/// `self.realization_cache = RealizationCache::new();` near the top of
/// `edit_param` (placed after the function-entry guards but before any state
/// mutation that could fail), which makes this assertion pass.
#[test]
fn edit_param_clears_realization_cache_to_prevent_stale_handle_on_subsequent_build_snapshot() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_edit_param_clears_realization_cache".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // (a) Cold-start eval, activate purpose, build → cache populated by step-6.
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");
    let _build = engine.build(&module, ExportFormat::Step);
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "test premise: expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() (per step-5/step-6 \
         wiring). Without this premise the post-edit assertion is vacuous. \
         Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    // (b) Edit any param cell — `MyDesign.thickness` is the param the
    // template carries. We don't need the param to drive the Box's args for
    // this test; the assertion below is on cache state immediately after
    // `edit_param` returns. Auto-invalidation must fire regardless of
    // whether the edited cell participates in the realization's input cone,
    // because the engine cannot prove non-participation without per-cell
    // dependency analysis we do not currently maintain.
    let thickness_id = ValueCellId::new("MyDesign", "thickness");
    let _result = engine
        .edit_param(thickness_id, Value::Real(0.005))
        .expect("edit_param must succeed against the MyDesign.thickness Real param");

    // (c) Assert the cache was cleared by edit_param. This pins the
    // auto-invalidation contract step-18 establishes — without it, the
    // entry persists and a subsequent build_snapshot() would silently
    // return a stale GeometryHandleId from the (entity, repr, tol) bucket.
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_none(),
        "expected edit_param to auto-invalidate the RealizationCache so a \
         subsequent build_snapshot() cannot return a stale GeometryHandleId. \
         Lookup at (\"MyDesign\", ReprKind::BRep, 1e-6) returned Some(_) \
         after edit_param — the entry survived the edit, breaking the \
         auto-invalidation contract step-18 establishes. Cache len={}, dump: \
         {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Build a `MyDesign`-shaped template carrying a single named realization
/// producing one `Box` primitive with caller-specified dimensions (in mm).
///
/// Mirrors `my_design_template_with_box_realization()` but parametrises the
/// box dimensions so step-19 can build two structurally-identical modules
/// that differ only in geometry literals (the "different parameter defaults,
/// structurally identical realization graph" shape the plan asks for to
/// pin edit_source's auto-invalidation behaviour against a non-trivial
/// content-diff).
fn my_design_template_with_box_realization_dims(
    width_mm: f64,
    height_mm: f64,
    depth_mm: f64,
) -> reify_compiler::TopologyTemplate {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(width_mm)),
            ("height".into(), mm_lit(height_mm)),
            ("depth".into(), mm_lit(depth_mm)),
        ],
    };
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .realization_named("MyDesign", 0, "body", vec![box_op])
        .build()
}

/// Step-19 (failing initially; passes against the wiring landed by step-18,
/// which adds the analogous `self.realization_cache = RealizationCache::new()`
/// reset to `Engine::edit_source` at the same near-entry placement as
/// `Engine::edit_param`).
///
/// Pins the parallel auto-invalidation contract for the source-edit hot
/// path, mirroring step-17's contract for the parameter-edit hot path. The
/// plan calls for both resets (in `edit_param` and `edit_source`) to land
/// in step-18, but a separate test pin guards against a future refactor
/// that resets in one of the two functions and silently regresses the
/// other.
///
/// Setup mirrors step-17 but exercises `engine.edit_source(&new_module)`
/// instead of `engine.edit_param(...)`. The "second module" is built with
/// the same template shape as the first but with different box-primitive
/// dimensions — a "different parameter defaults, structurally identical
/// realization graph" content diff that is realistic for a source-edit
/// hot path (the user changes geometry literals, not just one param value).
///
/// Sequence:
///   (a) `engine.eval(&module1)` → `engine.activate_purpose("manufacturing",
///       "MyDesign")` → `engine.build(&module1, ExportFormat::Step)` →
///       assert `engine.realization_cache().lookup("MyDesign", ReprKind::BRep,
///       1e-6, ContentHash(0)).is_some()` (cache populated; pins the test premise).
///   (b) Build a second `CompiledModule` with the same templates and
///       purposes, but a `MyDesign` realization carrying different Box
///       dimensions. Call `engine.edit_source(&module2).unwrap()`.
///   (c) Without calling `build_snapshot` yet, assert
///       `engine.realization_cache().lookup("MyDesign", ReprKind::BRep,
///       1e-6, ContentHash(0)).is_none()` — the entry was cleared on edit_source.
///
/// Today (pre step-18) `edit_source` does NOT touch `realization_cache`, so
/// the cache entry persists across the source edit and a subsequent
/// `build()` / `build_snapshot()` would silently return a stale
/// `GeometryHandleId` pointing at the OLD geometry. After step-18's wiring,
/// `edit_source` resets the cache near function entry — symmetric with
/// `edit_param` — and this assertion passes.
#[test]
fn edit_source_clears_realization_cache_to_prevent_stale_handle_on_subsequent_build() {
    let module1 = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_edit_source_clears_realization_cache_v1".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization_dims(
        10.0, 20.0, 5.0,
    ))
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // (a) Cold-start eval, activate purpose, build → cache populated by step-6.
    let _eval = engine.eval(&module1);
    engine.activate_purpose("manufacturing", "MyDesign");
    let _build = engine.build(&module1, ExportFormat::Step);
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "test premise: expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() (per step-5/step-6 \
         wiring). Without this premise the post-edit assertion is vacuous. \
         Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    // (b) Build a structurally-similar second module with different Box
    // dimensions. The geometry literals differ from module1, so the
    // realization output would change — but the test does NOT depend on
    // the content diff: edit_source's auto-invalidation is unconditional
    // (mirrors edit_param). The diff just makes the test scenario realistic.
    let module2 = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_edit_source_clears_realization_cache_v2".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization_dims(
        15.0, 25.0, 7.5,
    ))
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();
    let _diff = engine
        .edit_source(&module2)
        .expect("edit_source must succeed against a structurally-valid second module");

    // (c) Assert the cache was cleared by edit_source. This pins the
    // auto-invalidation contract step-18 establishes for edit_source —
    // symmetric with the edit_param contract pinned by step-17. Without
    // it, the entry persists and a subsequent build()/build_snapshot()
    // would silently return a stale GeometryHandleId from the
    // (entity, repr, tol) bucket pointing at the OLD geometry.
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_none(),
        "expected edit_source to auto-invalidate the RealizationCache so a \
         subsequent build()/build_snapshot() cannot return a stale \
         GeometryHandleId. Lookup at (\"MyDesign\", ReprKind::BRep, 1e-6) \
         returned Some(_) after edit_source — the entry survived the edit, \
         breaking the auto-invalidation contract step-18 establishes (the \
         reset must fire in BOTH edit_param and edit_source; this test \
         guards against a future refactor that resets in only one of the \
         two functions). Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Step-21 (failing initially; passes once step-22 adds the public
/// `Engine::clear_realization_cache(&mut self)` mutator on the un-gated
/// public surface).
///
/// Pins the public escape hatch the docstring critique demands: production
/// callers MUST be able to flush the realization cache without enabling
/// test instrumentation. Today the only `realization_cache(&self)` accessor
/// is `#[cfg(any(test, feature = "test-instrumentation"))]`-gated and
/// READ-ONLY — there is no mutator on the public surface, so a production
/// caller that needs to invalidate cached `GeometryHandleId`s outside the
/// auto-invalidation hook points (`edit_param` / `edit_source`) physically
/// cannot satisfy that contract without constructing a fresh `Engine`
/// (which would discard every other piece of engine state: snapshots,
/// param overrides, registered solvers/kernels, `feature_tag_table`,
/// `topology_attribute_table`, etc.).
///
/// Step-22 adds `pub fn clear_realization_cache(&mut self)` with no cfg
/// gate (mirrors `Engine::clear_param_overrides` precedent in
/// `engine_admin.rs`), giving production callers a non-destructive flush
/// primitive. The READ-side accessor `realization_cache(&self)` keeps its
/// cfg gate (the cache stores kernel-internal `GeometryHandleId` values
/// that should not leak into the production surface), but the WRITE-side
/// mutator must be public so the docstring's promised mitigation is
/// actually reachable.
///
/// Setup mirrors step-7 / step-15: STEPOutput(1e-6) + MyDesign with one
/// Box-primitive realization + manufacturing(1e-6). Sequence:
///   (a) `eval` → `activate_purpose` → `build` → assert cache populated.
///   (b) Call `engine.clear_realization_cache()` directly (no cfg-gated
///       accessor; this is a production-surface mutator).
///   (c) Assert the cache is empty at `(MyDesign, BRep, 1e-6)`.
///
/// The test fails today because `clear_realization_cache` does not exist
/// on `Engine`. Once step-22 lands the method, the assertion passes
/// without any wiring change in `engine_build.rs`.
#[test]
fn clear_realization_cache_public_api_resets_cache_for_production_callers() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_clear_realization_cache_public_api".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // (a) Cold-start eval, activate purpose, build → cache populated by step-6.
    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");
    let _build = engine.build(&module, ExportFormat::Step);
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "test premise: expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() (per step-5/step-6 \
         wiring). Without this premise the post-clear assertion is vacuous. \
         Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    // (b) Call the public escape hatch added in step-22. This is the
    // critical line — it compiles iff `Engine::clear_realization_cache` is
    // a public method on the un-gated production surface. A `pub(crate)`
    // or cfg-gated method would still pass type-checking inside this test
    // crate (since `cfg(test)` is on for integration tests too), so the
    // step-22 docstring should reinforce that the gate-LESS shape is
    // intentional and that test-only callers should NOT be the only
    // consumers.
    engine.clear_realization_cache();

    // (c) Assert the cache was cleared by the public mutator. Mirrors the
    // post-edit assertions in step-17 / step-19 — the cache is keyed on
    // `(entity_id, repr_kind, demanded_tol)` and a cleared cache returns
    // `None` for every lookup, including exact-key ones.
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_none(),
        "expected Engine::clear_realization_cache() to flush the cache so \
         every (entity_id, repr_kind, demanded_tol) lookup returns None. \
         Lookup at (\"MyDesign\", ReprKind::BRep, 1e-6) returned Some(_) \
         after clear_realization_cache() — the entry survived the clear, \
         breaking the public-mutator contract step-22 establishes. Cache \
         len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );
}

/// Task 3103, step-3 — pins that the active tolerance scope survives
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

/// Task 3176, step-1 (RED) — pins that an anonymous realization (one whose
/// `RealizationDecl.name == None`, constructed via
/// `TopologyTemplateBuilder::realization(...)` rather than
/// `realization_named(...)`) does NOT populate the `RealizationCache` even
/// when a demanded tolerance is active.
///
/// **Why anonymous realizations exist in this test only**: the production
/// compiler always emits `Some(name)` for every `RealizationDecl` it produces
/// (see `crates/reify-compiler/src/types.rs:848-857`). `None` only arises
/// from the `TopologyTemplateBuilder::realization(...)` test-support helper,
/// which is what this test uses to exercise the anonymous-realization code
/// path.
///
/// **The asymmetry this test exposes**: before the step-2 fix, the
/// post-success cache-insert at `engine_build.rs` fires whenever
/// `demanded_tol.is_some()`, regardless of `realization_name`. But the
/// cache-hit short-circuit at the top of `execute_realization_ops` requires
/// BOTH `demanded_tol.is_some()` AND `realization_name.is_some()`. The
/// result: an anonymous realization populates the cache on the first build
/// but can never be served from it. On subsequent builds the lookup
/// short-circuit skips (no name), the kernel re-runs, and the post-success
/// insert hits `ToleranceBucket::insert`'s partial-order rejection (the
/// prior entry already satisfies). The cached slot is wasted and the op
/// chain re-runs every build.
///
/// After the step-2 fix tightens the insert gate to match the lookup gate
/// (`if let (Some(tol), Some(_name)) = (demanded_tol, realization_name)`),
/// this test passes: the anonymous realization never populates the cache.
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
/// (which pins that NAMED realizations DO populate the cache) and the existing
/// `edit_param_clears_realization_cache_...` pin (cache-clear mechanism).
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
        .param("MyDesign", "thickness", Type::Real, None)
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
    // Before the step-2 fix the insert fires (demanded_tol.is_some() is
    // sufficient); after the fix it is skipped (realization_name.is_none()).
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

/// Task 3176, step-4 (GREEN-on-arrival) — end-to-end behavioral pin for the
/// `edit_param → build_snapshot` freshness contract.
///
/// **What this pins**: the existing test
/// `edit_param_clears_realization_cache_to_prevent_stale_handle_on_subsequent_build_snapshot`
/// asserts the cache-clear *mechanism* fires (the cache is empty immediately
/// after `edit_param` returns). This test asserts the user-visible *behavior*:
/// that a subsequent `build_snapshot` actually invokes the kernel afresh
/// (cold-misses on the cleared cache), so the resulting `GeometryHandleId` is
/// NOT a stale cached one. The two tests are complementary — they guard against
/// orthogonal regressions.
///
/// **No re-activation between calls**: `edit_param` does NOT call `eval()` —
/// it does its own incremental re-evaluation via `reify_expr::eval_expr`.
/// `build_snapshot` also does NOT call `eval()` (it builds from the existing
/// snapshot). Additionally, task 3103 (commits cb5c58ff6a → c8e6fe56da) changed
/// `Engine::eval()` to preserve `active_purpose_bindings` via `mem::take` +
/// re-inject (engine_eval.rs:1162-1176), so bindings survive even when an
/// internal eval round-trip fires. The lifecycle contract is "eval →
/// activate_purpose → build requires no re-activation" (pinned by
/// `eval_then_activate_purpose_then_build_preserves_tolerance_scope_across_internal_eval`
/// at tolerance_wiring_e2e.rs). No re-activation is needed at any point in this
/// test.
///
/// Sequence:
///   (a) eval → activate_purpose → build → capture `ops_after_first`.
///   (b) Sanity check cache IS populated (proves cache-hit WOULD have fired
///       without the edit_param invalidation — makes the final assertion
///       non-vacuous).
///   (c) `edit_param(thickness, 0.005)` — clears the cache.
///   (d) `build_snapshot(...)` — must cold-miss → kernel re-executes.
///   (e) Assert `ops_after_build_snapshot > ops_after_first` (kernel grew,
///       proving cold-miss and fresh execution rather than stale cache-hit).
#[test]
fn edit_param_followed_by_build_snapshot_re_executes_kernel_so_geometry_handle_is_not_stale() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_edit_param_then_build_snapshot_kernel_reruns".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    // Grab the recorder BEFORE moving the kernel into the engine — the Arc
    // keeps it alive across the ownership boundary (established pattern at
    // tolerance_wiring_e2e.rs line ~307/475).
    let ops_handle = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // (a) Cold-start: eval → activate_purpose → build → cache populated.
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");
    engine.build(&module, ExportFormat::Step);
    let ops_after_first = ops_handle.lock().unwrap().len();
    assert!(
        ops_after_first >= 1,
        "test premise: expected first build() to invoke the kernel at least once \
         (cache miss → realization ops dispatched); got ops_after_first={}",
        ops_after_first,
    );

    // (b) Sanity: cache IS populated after first build — proves a cache-hit
    // WOULD have fired on the next build WITHOUT the edit_param invalidation.
    assert!(
        engine
            .realization_cache()
            .lookup("MyDesign", ReprKind::BRep, 1e-6, ContentHash(0))
            .is_some(),
        "sanity: expected RealizationCache to contain an entry at \
         (\"MyDesign\", ReprKind::BRep, 1e-6) after build() — without this the \
         final assertion is vacuous. Cache len={}, dump: {:?}",
        engine.realization_cache().len(),
        engine.realization_cache(),
    );

    // (c) Edit a param — clears the realization cache (task 2874, step-18).
    // No re-activation needed: edit_param does not call eval(), and
    // build_snapshot does not call eval() either.
    let thickness_id = ValueCellId::new("MyDesign", "thickness");
    engine
        .edit_param(thickness_id, Value::Real(0.005))
        .expect("edit_param must succeed against the MyDesign.thickness Real param");

    // (d) build_snapshot must cold-miss on the cleared cache and re-execute
    // the kernel.
    engine.build_snapshot(&module, ExportFormat::Step);
    let ops_after_build_snapshot = ops_handle.lock().unwrap().len();

    // (e) Core assertion: kernel grew — proves build_snapshot cold-missed on
    // the cache (the edit_param invalidation fired correctly) and called the
    // kernel again, producing a fresh GeometryHandleId rather than returning
    // the stale cached one.
    assert!(
        ops_after_build_snapshot > ops_after_first,
        "expected build_snapshot() after edit_param() to cold-miss the \
         RealizationCache and re-invoke the kernel (ops must grow beyond the \
         first-build count). ops_after_first={}, ops_after_build_snapshot={} \
         — kernel op count did not increase, indicating build_snapshot served \
         a stale cache-hit or never reached execute_realization_ops.",
        ops_after_first,
        ops_after_build_snapshot,
    );
}

/// Characterization test: regression pin for the cache-hit short-circuit's
/// attribute-table behaviour.
///
/// **What this test pins:** after the second `build()` call is served from
/// `RealizationCache` (the cache-hit short-circuit at
/// `engine_build.rs::execute_realization_ops` fires), `feature_tag_table` is
/// empty (no entry for the cached handle, table-wide empty).  The root cause
/// is documented in the "Known limitation" docstring on
/// `execute_realization_ops`: the short-circuit returns early before the
/// per-op `feature_tag_table.record(...)` call at line 1547, and the table
/// is reset to `default()` at the start of every `build()`.  The parallel
/// `topology_attribute_table` claim is left to a separate OCCT-gated test
/// because its population path requires real face/edge extraction (see "Why
/// MockGeometryKernel" below).
///
/// **Why MockGeometryKernel:** (1) The test runs unconditionally — no
/// `OCCT_AVAILABLE` skip gate that could hide the regression in OCCT-less CI
/// environments.  (2) `MockGeometryKernel::operations_ref()` exposes the ops
/// counter needed to assert the cache-hit short-circuit actually fired, which
/// is the non-vacuousness premise for the regression assertions.  (3) The
/// regression pin targets the parent-solid-handle level
/// (`feature_tag_table.record(handle.id, tag)` at engine_build.rs:1547),
/// which is independent of the per-face/per-edge seeding that requires real
/// OCCT extraction.
///
/// **This is a CHARACTERIZATION test** — it passes immediately on first run
/// because it pins existing behaviour.  Flipping any assertion to FAIL is the
/// regression signal: it means either the per-build reset stopped firing, or
/// population moved outside the op-loop, or the cache short-circuit stopped
/// firing.
#[test]
fn cache_hit_short_circuit_leaves_feature_tag_table_empty_for_cached_handle() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cache_hit_leaves_feature_tag_table_empty".to_string(),
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

    // SANITY: the op-loop populated feature_tag_table for the parent-solid
    // handle on build #1. This makes the PIN below non-vacuous — if the
    // table were already empty after build #1, asserting it is empty after
    // build #2 would prove nothing.
    assert!(
        engine.feature_tag_table().lookup(cached_handle.id).is_some(),
        "sanity: expected feature_tag_table to contain an entry for \
         cached_handle {:?} after build #1 — the op-loop at \
         engine_build.rs:1547 must record the parent-solid handle. If this \
         fires, the op-loop recording path has changed and the regression \
         PIN assertions below may no longer be meaningful.",
        cached_handle,
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
    // message names `cached_handle` directly and is the more diagnostic signal
    // for the canonical regression (population only of the cached handle),
    // while PIN 2 generalises to catch any new population path.
    //
    // PIN 1 (headline): the cache-hit short-circuit skips the per-op
    // `feature_tag_table.record(handle.id, tag)` call at engine_build.rs:1547,
    // and the per-build reset at the top of build() clears the table before the
    // short-circuit fires. Net effect: the cached handle has no entry.
    assert!(
        engine.feature_tag_table().lookup(cached_handle.id).is_none(),
        "regression PIN: expected feature_tag_table to have NO entry for \
         cached_handle {:?} on the second build — the cache-hit short-circuit \
         at engine_build.rs::execute_realization_ops deliberately skips per-op \
         feature_tag_table.record(); if this fires, either the per-build reset \
         stopped firing or population moved outside the op-loop.",
        cached_handle,
    );

    // PIN 2 (stronger): the entire table is empty — no spurious population from
    // any other source.
    assert!(
        engine.feature_tag_table().is_empty(),
        "regression PIN: expected feature_tag_table to be completely empty \
         after a cache-served build — only the op-loop at engine_build.rs:1547 \
         populates this table, and the cache-hit short-circuit skips that loop \
         entirely. If this fires, a new population path outside the op-loop \
         has been introduced.",
    );
}
