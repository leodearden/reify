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

use reify_compiler::compile;
use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_syntax::parse;
use reify_test_support::mocks::MockGeometryKernel;
use reify_types::{ExportFormat, ModulePath, ReprKind};

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
        .filter(|d| matches!(d.severity, reify_types::Severity::Error))
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
        .filter(|d| matches!(d.severity, reify_types::Severity::Error))
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
#[test]
fn executor_writes_produced_repr_brep_on_build_snapshot() {
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
        .filter(|d| matches!(d.severity, reify_types::Severity::Error))
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
