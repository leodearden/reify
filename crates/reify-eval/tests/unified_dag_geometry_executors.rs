//! Integration harness + gates for the unified build-DAG geometry-path
//! executors (task 4358 ε).
//!
//! δ (task 4357) landed `run_unified_pass` as a PURE planner and wired
//! `Engine::build()` to forward its diagnostics under
//! [`BuildScheduler::UnifiedDag`] (proven byte-preserving on acyclic modules by
//! `tests/unified_dag_cycle_contract.rs`). ε wires the schedule onto three
//! geometry-path executors (realization / selector-query / constraint), retires
//! the frozen pre-geometry `constraint_results` ("C7"), and lands the
//! auto-constraint guard decline — all behind the same scheduler flag.
//!
//! This file mirrors the `build_under` pattern from
//! `tests/unified_dag_cycle_contract.rs`, but the ε tests assert on geometry
//! ops, constraint verdicts, and diagnostics, so the shared helpers return the
//! FULL [`BuildResult`] (not just projected diagnostic triples). The scheduler
//! is selected through the deterministic `Engine::set_build_scheduler` test seam
//! (a `#[cfg(any(test, feature = "test-instrumentation"))]` setter reached via
//! the self-dev-dep with `test-instrumentation` enabled — see
//! `crates/reify-eval/Cargo.toml`), so these tests stay parallel-safe and
//! independent of the `unified-dag` cargo feature.
//!
//! The mock kernel's `with_query_result` / bbox / volume builders let a
//! geometry-backed constraint reach a DEFINITE verdict without OCCT; the
//! OCCT-dependent headline e2e tests (verdict-FLIP / volume-≠-all-fillet) are
//! owned by η, not ε.

// The shared `build_*` helpers below are consumed incrementally as the ε steps
// land their RED integration tests (steps 5/7/9/11). Until every helper has a
// caller, an unused helper would trip `-D warnings`; this scaffolding allow is
// intentional and is the prerequisite (`pre-1`) deliverable.
#![allow(dead_code)]

use reify_constraints::SimpleConstraintChecker;
use reify_eval::{BuildResult, BuildScheduler, Engine};
use reify_ir::{ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// Compile `source`, build it on a FRESH engine under the given `scheduler`
/// with the supplied `kernel`, and return the full [`BuildResult`]
/// (`values`, `constraint_results`, `geometry_output`, `diagnostics`).
///
/// A fresh engine per call guarantees the cold-start `eval()` path runs (which
/// populates `eval_state.trace_map` that `run_unified_pass` consumes); a second
/// build on the same engine would hit the `eval_cached` path.
///
/// The `kernel` is taken by `Box<dyn GeometryKernel>` so callers can pass a
/// `MockGeometryKernel` pre-seeded with `with_query_result` / `with_bbox_result`
/// / `with_volume_result` replies (the ε constraint tests) OR the real
/// eval-test kernel.
pub fn build_with_kernel(
    source: &str,
    scheduler: BuildScheduler,
    kernel: Box<dyn GeometryKernel>,
) -> BuildResult {
    let compiled = compile_source(source);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), Some(kernel));
    engine.set_build_scheduler(scheduler);
    engine.build(&compiled, ExportFormat::Step)
}

/// Convenience over [`build_with_kernel`] using a default (unseeded)
/// [`MockGeometryKernel`] — for tests that only inspect recorded geometry ops
/// or diagnostics and need no canned query replies.
pub fn build_under(source: &str, scheduler: BuildScheduler) -> BuildResult {
    build_with_kernel(source, scheduler, Box::new(MockGeometryKernel::new()))
}

// ─────────────────────────────────────────────────────────────────────────────
// step-5 (RED): in-loop curated-fillet edge resolution under UnifiedDag.
// ─────────────────────────────────────────────────────────────────────────────

/// Under `UnifiedDag`, a curated fillet `fillet(b, edges_at_height(b, …), r)`
/// must dispatch with its edge selector ALREADY resolved — the recorded
/// `GeometryOp::Fillet` must carry a non-empty, curated `edges` list.
///
/// Mechanism: `let e = edges_at_height(b, …)` is a `Value::Selector` cell that
/// resolves to a `List<Geometry>` only in the topology-selector post-process
/// (legacy "P4"), which the legacy build loop runs AFTER every realization. So
/// when the fillet realization dispatches (legacy "P2"), `e` is still unresolved
/// and the 3-arg fillet eval arm (`geometry_ops.rs::compile_geometry_op`,
/// `ModifyKind::Fillet`) hits its `other => Err("curated edge selection is not
/// yet available on the current build pipeline …")` branch → the fillet
/// realization is rolled back (C9) and NO `Fillet` op reaches the kernel.
///
/// ε's schedule-driven driver (step-6) consumes `run_unified_pass`'s Kahn order,
/// which — because the fillet realization's dependency trace reads cell `e`
/// (`deps::extract_realization_dependencies` over the op args) — schedules the
/// `e` selector cell BEFORE the consuming fillet realization. The selector is
/// hydrated at its scheduled slot, so the fillet dispatches with curated edges.
///
/// RED until step-6: today (even under `UnifiedDag`, which δ wired only as an
/// additive diagnostic pass over the still-legacy build loop) the selector is
/// unresolved at dispatch, so `find_ops(Fillet)` is empty and the
/// `fillets.len() == 1` assertion fails.
///
/// Structural assertion ONLY (`edges` non-empty) — the OCCT volume-≠-all-fillet
/// e2e is η's, per PRD §8 / the ε test-strategy design decision.
#[test]
fn unified_dag_curated_fillet_resolves_edges_in_loop() {
    // `let e` is a named selector cell so the fillet realization's trace reads it
    // (an inlined selector would have no cell to schedule before the fillet).
    let source = r#"pub structure S {
    let b = box(10mm, 10mm, 10mm)
    let e = edges_at_height(b, 5mm, 1mm)
    let f = fillet(b, e, 2mm)
}"#;

    // The box is the first (and only successful) kernel `execute()` → handle 1,
    // so it is the parent solid the `edges_at_height` selector extracts against
    // (mirrors the "id=1 is the parent solid" convention in
    // tests/topology_filtered_selectors_mock.rs). Edge sub-handle ids are chosen
    // high (50/51/52) to avoid colliding with realization result handles.
    let parent = GeometryHandleId(1);
    let e0 = GeometryHandleId(50);
    let e1 = GeometryHandleId(51);
    let e2 = GeometryHandleId(52);

    // A flat-bbox JSON whose z-extents both sit exactly on `z` (SI metres), so
    // every edge passes the `edges_at_height(b, 5mm, 1mm)` window
    // (|zmin - 0.005| ≤ 0.001 && |zmax - 0.005| ≤ 0.001). Format mirrors
    // tests/topology_filtered_selectors_mock.rs's `bbox_json`.
    let bbox_at = |z: f64| {
        Value::String(format!(
            "{{\"xmin\":0.0,\"ymin\":0.0,\"zmin\":{z},\
              \"xmax\":0.01,\"ymax\":0.01,\"zmax\":{z}}}"
        ))
    };

    let kernel = MockGeometryKernel::new()
        .with_extracted_edges(parent, vec![e0, e1, e2])
        .with_bbox_result(e0, bbox_at(0.005))
        .with_bbox_result(e1, bbox_at(0.005))
        .with_bbox_result(e2, bbox_at(0.005));
    // Capture the recorder BEFORE the kernel is boxed/moved into the engine.
    let ops_ref = kernel.operations_ref();

    let result = build_with_kernel(source, BuildScheduler::UnifiedDag, Box::new(kernel));

    let ops = ops_ref.lock().unwrap().clone();
    let fillets: Vec<&GeometryOp> = ops
        .iter()
        .map(|rec| &rec.op)
        .filter(|op| matches!(op, GeometryOp::Fillet { .. }))
        .collect();

    assert_eq!(
        fillets.len(),
        1,
        "UnifiedDag must dispatch exactly one curated Fillet op (the selector must \
         resolve in-loop before the fillet realization); recorded ops={:?}, \
         diagnostics={:?}",
        ops.iter().map(|r| &r.op).collect::<Vec<_>>(),
        result.diagnostics,
    );

    match fillets[0] {
        GeometryOp::Fillet { edges, .. } => assert!(
            !edges.is_empty(),
            "curated fillet must dispatch with a resolved, non-empty edge list \
             in-loop under UnifiedDag (an empty list is the all-edges back-compat \
             path / an unresolved selector)"
        ),
        _ => unreachable!("filtered to Fillet above"),
    }
}
