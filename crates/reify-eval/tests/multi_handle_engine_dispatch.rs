//! Task ε (3436) — multi-handle Engine + per-op dispatch routing.
//!
//! Integration tests for the PRD §8 ε deliverable in
//! `docs/prds/v0_3/multi-kernel-phase-3.md`: the engine carries a
//! `BTreeMap<String, Box<dyn GeometryKernel>>` keyed on kernel name plus a
//! `default_kernel_name: Option<String>` (preserving the v0.2 single-handle
//! BRep-native path), and `execute_realization_ops` routes each op to the
//! `dispatcher::dispatch`-named kernel.
//!
//! This file pins the cross-crate seams; the per-op routing case (step-7/8) +
//! cache-rehit / dispatch-count instrumentation (step-11/12) + produced-repr
//! execution-time write (step-9/10) are added as additional tests in this
//! same file as the steps land. Per-function unit tests for the lower-level
//! helpers (`geometry_op_to_operation`, `plan_output_repr`) live in
//! `crates/reify-eval/src/engine_build.rs::tests` alongside the existing
//! `execute_realization_ops_*` unit-test set.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind, compile};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{CompiledExpr, ExportFormat, ReprKind};
use reify_syntax::parse;
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

/// `Engine::with_registered_kernels(checker)` must build an engine whose
/// `registered_kernel_names()` set matches the inventory registry: when
/// `cfg(has_occt)` is set the OCCT adapter is registered, so `"occt"` is
/// present; when OCCT is unavailable (stub-mode build) the set is empty.
///
/// Mirrors the OCCT-availability gating used by the sibling
/// `engine_with_registered_kernel_picks_occt_for_brep_box_build` integration
/// test in `tests/kernel_registry_inventory.rs`. The skip is announced via
/// `eprintln!` so stub-mode CI produces an observable signal — silent no-op
/// early-returns would let a regression that drops the OCCT submit hide in
/// green logs.
///
/// RED before step-2 impl: both `with_registered_kernels` (plural) and
/// `registered_kernel_names()` are introduced in step-2.
#[test]
fn with_registered_kernels_loads_one_kernel_per_inventory_registration() {
    let checker = SimpleConstraintChecker;
    let engine = Engine::with_registered_kernels(Box::new(checker));

    let names: Vec<String> = engine.registered_kernel_names().map(String::from).collect();

    if reify_kernel_occt::OCCT_AVAILABLE {
        assert!(
            names.iter().any(|n| n == "occt"),
            "with_registered_kernels(checker) must load the OCCT adapter under \
             cfg(has_occt); got names={names:?}"
        );
    } else {
        eprintln!(
            "with_registered_kernels_loads_one_kernel_per_inventory_registration: \
             stub-mode build (cfg(has_occt) off) — asserting empty registered-kernel set"
        );
        assert!(
            names.is_empty(),
            "in stub mode no kernel adapter is registered; got names={names:?}"
        );
    }
}

/// Backward-compat: `Engine::new(checker, Some(MockGeometryKernel))` must keep
/// the single-kernel public signature working end-to-end. The mock kernel is
/// wrapped under the synthetic `DEFAULT_KERNEL_NAME` and used as the default
/// kernel for `build()`'s export-stage call. The mock's `export` writes the
/// fixed `MOCK_EXPORT_DATA` payload, so a non-empty `geometry_output` proves
/// the build pipeline reached the kernel under the new multi-handle field
/// shape.
///
/// RED before step-2 impl: the field reshape + `with_prelude` wrapping land in
/// step-2; before then, the test compiles (signature unchanged) but the
/// `kernel_count()` assertion fails because no accessor exists yet.
#[test]
fn engine_new_with_single_mock_kernel_builds_one_box_realization() {
    let source = "structure S {\n    let b = box(10mm, 10mm, 10mm)\n}\n";
    let parsed = parse(source, ModulePath::single("mock_kernel_box"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {compile_errors:?}"
    );

    let checker = SimpleConstraintChecker;
    let mock = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(mock)));

    // Step-2 contract: a single user-supplied kernel is wrapped under the
    // synthetic DEFAULT_KERNEL_NAME — kernel_count() must report exactly 1.
    assert_eq!(
        engine.kernel_count(),
        1,
        "Engine::new with Some(mock_kernel) must wrap it under the synthetic \
         default name into the multi-handle map; expected kernel_count()==1"
    );

    let result = engine.build(&compiled, ExportFormat::Stl);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build emitted error diagnostics: {errors:?}"
    );
    let output = result.geometry_output.expect(
        "Engine::new(checker, Some(mock)) must execute the box realization on the wrapped mock \
         kernel and surface its dummy export payload as geometry_output",
    );
    assert_eq!(
        &output, b"MOCK_EXPORT_DATA",
        "mock kernel export writes a fixed payload (MOCK_EXPORT_DATA); a different output \
         means the build dispatched to a different kernel than the user-supplied mock"
    );
}

/// Step-9 (task ε / 3436) RED forward-guard: after `engine.build_snapshot()`
/// runs the per-realization op loop, every realization node's
/// `produced_repr` must reflect what `execute_realization_ops` recorded —
/// not just the construction-time `ReprKind::BRep` default that
/// `EvaluationGraph::from_templates` initializes (graph.rs:329, pinned by
/// the eval-only α forward-guard in
/// `tests/realization_produced_repr_pinning.rs`).
///
/// **How this test distinguishes "executor write" from "construction-time
/// default".** Both values are `BRep` in the v0.3-ε BRep baseline, so a
/// naïve `produced_repr == BRep` assertion after `build()` would pass with
/// or without the step-10 executor write. To make this RED before step-10,
/// the test:
///
/// 1. Drives `engine.build()` once, which calls `eval()` internally and
///    creates the snapshot's realization nodes with `produced_repr == BRep`.
/// 2. Reaches into the snapshot via the `test-instrumentation`-gated
///    `snapshot_mut()` accessor and corrupts every realization's
///    `produced_repr` to `ReprKind::Mesh` — a value the BRep baseline can
///    never legitimately produce.
/// 3. Calls `engine.build_snapshot()`, which operates on the existing
///    snapshot (skips `eval()`) and re-runs the per-realization op loop
///    via `execute_realization_ops`.
/// 4. Asserts every realization's `produced_repr` is now `BRep` again.
///
/// Only step-10's caller-write of the executor-returned terminal repr into
/// `eval_state.snapshot.graph.realizations[id].produced_repr` can restore
/// the BRep value once the pre-corruption has set it to Mesh. Before step-10
/// the executor has no channel to surface the terminal repr to the caller,
/// so the Mesh value survives the build and the per-realization assertion
/// fails — the desired RED signal.
///
/// **Step-13/14 (task ε / 3436) — unconditional-execution invariant**: this
/// test DELIBERATELY does NOT gate on `reify_kernel_occt::OCCT_AVAILABLE`
/// (mirroring the OCCT-skip pattern in
/// `with_registered_kernels_loads_one_kernel_per_inventory_registration`).
/// The fixture uses `Engine::new(_, Some(MockGeometryKernel))`, which wraps
/// the mock under the synthetic `Engine::DEFAULT_KERNEL_NAME` sentinel and
/// makes the engine entirely self-contained from any inventory-registered
/// adapter. That synthetic-default-kernel path exists in both stub-mode and
/// OCCT-on builds, so the test MUST exercise the executor-write invariant
/// in both. Before step-14, the test passes incidentally in OCCT-on builds
/// only — when `cfg(has_occt)` is set, the registry carries OCCT and
/// `dispatch(_, PrimitiveBox, BRep, {BRep})` returns
/// `Some(plan{kernel:"occt"})`, the 0-conversion arm falls back to the
/// DEFAULT_KERNEL_NAME-keyed mock (because "occt" is not in the kernels
/// map), `last_plan` is `Some`, and the post-loop `plan_output_repr` reads
/// OCCT's `(PrimitiveBox, BRep)` support to write `BRep`. In stub-mode
/// builds the registry is empty, dispatch returns `None`, the backward-
/// compat fallback arm executes the mock but never sets `last_plan`, so
/// the post-loop write guard short-circuits and the pre-corrupted Mesh
/// value survives. The step-13 unit test
/// `execute_realization_ops_writes_produced_repr_brep_in_none_fallback_backward_compat`
/// in `engine_build.rs` pins the same gap with a synthetic registry that
/// forces the None-fallback arm regardless of build profile. Step-14
/// closes the gap by routing the fallback arm through a parallel
/// `last_produced_repr = Some(BRep)` capture that the post-loop write
/// honours uniformly.
///
/// If a future maintainer is tempted to add `if !reify_kernel_occt::OCCT_AVAILABLE
/// { return; }` to silence a stub-mode failure here, that would hide the
/// production gap behind a test skip — re-read the step-14 plan note in
/// `.task/plan.json` (design_decisions: reviewer option (c)) before doing
/// so. The right fix lives in `execute_realization_ops`, not in this test.
#[test]
fn executor_writes_produced_repr_brep_on_build_snapshot() {
    // Step-13 (task ε / 3436): the unconditional-execution invariant
    // (this test runs in both stub-mode and OCCT-on builds) is documented
    // in the doc comment above. An earlier draft also carried a no-op
    // `assert!(true, …)` block intended to surface the invariant in a
    // place a future maintainer could not miss; that assertion was a
    // wording-meta pattern (testing a documentation decision rather than
    // a behaviour) and was removed during amendment.

    let source = "structure S {\n    let b = box(10mm, 10mm, 10mm)\n}\n";
    let parsed = parse(source, ModulePath::single("produced_repr_executor_write"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {compile_errors:?}"
    );

    let checker = SimpleConstraintChecker;
    let mock = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(mock)));

    // Step (1): seed the snapshot via build (which calls eval internally).
    let _ = engine.build(&compiled, ExportFormat::Stl);

    // Snapshot must contain at least one realization with the construction-
    // time default (pinned by α/`realization_produced_repr_pinning.rs`).
    let realization_ids: Vec<_> = {
        let snap = engine
            .snapshot()
            .expect("snapshot must be Some after a successful build()");
        assert!(
            !snap.graph.realizations.is_empty(),
            "expected at least one realization node in the snapshot graph after build()"
        );
        snap.graph
            .realizations
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    };

    // Step (2): pre-corrupt produced_repr → Mesh on every realization via
    // the test-instrumentation snapshot_mut accessor. Mesh is impossible in
    // the BRep baseline; any later read of BRep here can only come from a
    // step-10 executor-write of the dispatcher-derived repr.
    {
        let snap = engine
            .snapshot_mut()
            .expect("snapshot_mut must be Some after a successful build()");
        for id in &realization_ids {
            let r = snap
                .graph
                .realizations
                .get_mut(id)
                .expect("realization id collected from iter must still be present");
            r.produced_repr = ReprKind::Mesh;
        }
    }

    // Step (3): re-run the per-realization op loop on the existing snapshot
    // (build_snapshot, not build — build calls eval which would rebuild the
    // graph from the module and reset the corrupted produced_repr to the
    // construction default, masking the executor-write signal we're after).
    let _ = engine.build_snapshot(&compiled, ExportFormat::Stl);

    // Step (4): every realization must now carry produced_repr == BRep.
    // Pre-step-10 this fails because the Mesh value we wrote survives the
    // build (the executor has no channel to update the graph node). After
    // step-10 the caller-write restores BRep from the dispatcher's
    // `(PrimitiveBox, BRep)` plan.
    let snap = engine
        .snapshot()
        .expect("snapshot must remain Some after build_snapshot()");
    for (id, r) in snap.graph.realizations.iter() {
        assert_eq!(
            r.produced_repr,
            ReprKind::BRep,
            "realization {id:?}: executor must overwrite the pre-corrupted Mesh value with \
             ReprKind::BRep at execution time (step-10); got {:?}. If this fires after step-10 \
             lands, check that execute_realization_ops returns the terminal repr and the build/\
             build_snapshot caller writes it back into the realization graph node.",
            r.produced_repr,
        );
    }
}

/// Build a `MyDesign`-shaped [`reify_compiler::TopologyTemplate`] that carries
/// a single named realization producing one `Box` primitive op. Mirrors the
/// private helper of the same name in `tests/tolerance_wiring_e2e.rs` — the
/// realization id is `(entity = "MyDesign", index = 0)` and the realization's
/// name is `"body"` so the post-realization `named_steps` map is populated.
///
/// Cache-rehit tests need exactly one op (one dispatch on cold path, zero on
/// the cache-hit short-circuit) so the `last_dispatch_count()` assertion is
/// unambiguous.
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

/// Step-11(a) (task ε / 3436) RED forward-guard: the dispatch-count
/// instrumentation counter (`Engine::last_dispatch_count()`) must report >0 on
/// a cold-cache first build (one realization → one op → one `dispatch(...)`
/// call inside `execute_realization_ops`) and exactly 0 on a second build of
/// the same module with the same demanded tolerance (cache short-circuit at
/// the top of `execute_realization_ops` returns BEFORE the per-op loop, so
/// `dispatch(...)` is never reached).
///
/// Setup mirrors the cache-hit fixture in
/// `tests/tolerance_wiring_e2e.rs::second_build_with_unchanged_purpose_and_module_short_circuits_kernel_via_cache_hit`:
/// `STEPOutput(1µm)` + `MyDesign` (one Box op named `"body"`) + manufacturing
/// purpose at 1µm. The first `build()` populates the `RealizationCache` at
/// `("MyDesign", BRep, 1e-6)`; the second `build()` with the same purpose
/// re-activated hits the cache and short-circuits the op loop.
///
/// Why exactly 0 (not just `<= ops_after_first`)? The cache-hit branch at the
/// top of `execute_realization_ops` returns BEFORE the `for op in operations`
/// loop, and the dispatch-count counter is incremented INSIDE that loop. A
/// cumulative-count regression where the counter doesn't reset at the build
/// entry would surface as a non-zero second-build value. Pinning `== 0`
/// rather than just `<` gives precise failure attribution.
///
/// RED before step-12 because `last_dispatch_count()` doesn't exist yet.
/// After step-12 wires the counter increment inside the dispatcher arm and
/// adds the `#[cfg(any(test, feature = "test-instrumentation"))]` accessor,
/// the test compiles and passes.
#[test]
fn last_dispatch_count_zero_on_cache_hit_second_build() {
    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_last_dispatch_count_zero_on_cache_hit".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(my_design_template_with_box_realization())
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    // First build: cold cache → one dispatch call (one Box op in the
    // MyDesign realization). STEPOutput has no realization ops.
    let _build1 = engine.build(&module, ExportFormat::Step);
    let dispatches_after_first = engine.last_dispatch_count();
    assert!(
        dispatches_after_first >= 1,
        "expected first build() to invoke the dispatcher at least once \
         (cold cache → realization op loop runs → dispatch(...) called per op); \
         got last_dispatch_count()={dispatches_after_first}",
    );

    // Re-activate purpose so the second build's pre-`check()` precompute
    // sees the same `demanded_tol = Some(1e-6)` that populated the cache
    // on the first build. Without re-activation eval() clears
    // `active_purpose_bindings` and the cache lookup at the top of
    // `execute_realization_ops` would not even fire — defeating the test
    // premise. (Mirrors the pattern in tolerance_wiring_e2e.rs.)
    engine.activate_purpose("manufacturing", "MyDesign");

    // Second build: cache-hit short-circuit at the top of
    // `execute_realization_ops` returns BEFORE the op loop, so the
    // dispatch counter (incremented inside the loop) reports 0.
    let _build2 = engine.build(&module, ExportFormat::Step);
    let dispatches_after_second = engine.last_dispatch_count();
    assert_eq!(
        dispatches_after_second, 0,
        "expected second build() to be served entirely from the RealizationCache: \
         the cache-hit short-circuit returns before the per-op loop, so the \
         dispatch counter must reset to 0 at the build entry AND no \
         dispatch(...) call must fire. Got last_dispatch_count()={dispatches_after_second} \
         (first build saw {dispatches_after_first}). A nonzero value means either the \
         counter is not resetting at build() entry, or the cache-hit short-circuit \
         is bypassed and dispatch(...) is still called per op.",
    );
}

/// Step-11(b) (task ε / 3436) RED end-to-end pin: a module with two `Box`
/// realizations plus a `Union` realization, built through
/// `Engine::with_registered_kernels(checker)`, must (i) emit geometry output
/// (proving the inventory-driven multi-handle constructor instantiates the
/// OCCT adapter and routes per-op dispatch to it) and (ii) write
/// `produced_repr == ReprKind::BRep` to every realization graph node — most
/// importantly to the terminal Union realization, since the dispatcher's
/// `(BooleanUnion, BRep, {BRep})` plan must resolve to the BRep-native OCCT
/// kernel under the v0.3-ε baseline.
///
/// Source shape:
/// ```ignore
/// structure S {
///     let a = box(10mm, 10mm, 10mm)
///     let b = box(5mm, 5mm, 5mm)
///     let r = union(a, b)
/// }
/// ```
/// The reify compiler emits one realization per top-level geometry-typed
/// `let` binding, so this compiles to three realizations: two Box-only
/// realizations and one Union realization whose ops reference the prior
/// two via cross-step lookup.
///
/// Skipped in stub mode: with `cfg(has_occt)` off `with_registered_kernels`
/// loads an empty kernel set, so the per-op dispatcher would fail at the
/// `no_kernel_chain_diagnostic` branch and no STEP would emerge — there is
/// nothing meaningful to assert. Mirror's the gating pattern in
/// `tests/kernel_registry_inventory.rs`.
///
/// RED today only if the instrumentation accessor in (a) is missing —
/// otherwise the (b) assertions exercise step-10's already-landed
/// `produced_repr` write through the inventory-driven kernel path. Pairing
/// them in the same step-11 RED commit keeps the cache-rehit + end-to-end
/// signals in a single failing test binary so step-12 can flip them green
/// together.
#[test]
fn with_registered_kernels_end_to_end_two_boxes_plus_union() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping with_registered_kernels_end_to_end_two_boxes_plus_union: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let source = r#"structure S {
    let a = box(10mm, 10mm, 10mm)
    let b = box(5mm, 5mm, 5mm)
    let r = union(a, b)
}"#;

    let parsed = parse(source, ModulePath::single("multi_handle_end_to_end"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {compile_errors:?}"
    );

    // Verify the realization shape pre-build: expect 3 realizations
    // (two boxes + one union) so the produced_repr assertion below is
    // unambiguous about WHICH realization carries the Union output.
    assert_eq!(
        compiled.templates.len(),
        1,
        "expected one structure template"
    );
    assert_eq!(
        compiled.templates[0].realizations.len(),
        3,
        "expected three realizations (two Box, one Union); got {}",
        compiled.templates[0].realizations.len(),
    );

    let checker = SimpleConstraintChecker;
    let mut engine = Engine::with_registered_kernels(Box::new(checker));

    // STEP rather than STL: OCCT's `OcctKernelHandle::export` returns
    // `unsupported export format: Stl` for `ExportFormat::Stl`. STEP is the
    // only BRep-native export OCCT supports; the round-trip pin needs a
    // format that actually round-trips.
    let result = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build emitted error diagnostics: {errors:?}"
    );

    let output = result.geometry_output.expect(
        "Engine::with_registered_kernels(checker) must instantiate the OCCT adapter and \
         route the per-op dispatch plan to it; the terminal Union handle must export to \
         non-empty STEP via the default kernel",
    );
    assert!(
        !output.is_empty(),
        "STEP geometry_output must be non-empty — empty output indicates the registered \
         kernel was not actually instantiated or the per-op dispatch routing dropped the \
         terminal Union handle"
    );

    // Every realization graph node must carry produced_repr == BRep, written
    // by step-10's executor-write of `plan_output_repr(plan, op)` for the
    // (op, BRep) dispatcher plan. Most critically the Union realization
    // — its terminal op's plan is `(BooleanUnion, BRep, {BRep})` →
    // BRep-native OCCT kernel under the v0.3-ε baseline.
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    assert!(
        !snap.graph.realizations.is_empty(),
        "expected at least one realization node in the snapshot graph after build()"
    );
    for (id, r) in snap.graph.realizations.iter() {
        assert_eq!(
            r.produced_repr,
            ReprKind::BRep,
            "realization {id:?}: with_registered_kernels build of two-boxes-plus-union must \
             write ReprKind::BRep (the dispatcher's terminal (op, BRep) plan resolves to \
             the OCCT adapter for every op under the v0.3-ε baseline); got {:?}",
            r.produced_repr,
        );
    }
}
