//! R2a symbolic geometry eval tests (task #4652).
//!
//! Pins the user-observable signal and §7.1 two-way boundary contract
//! for `Engine::eval` minting kernel-free symbolic `Value::GeometryHandle`
//! cells instead of falling through to `Value::Undef`.
//!
//! ## TDD arc
//!
//! **Step-3 (RED):** `box_eval_yields_symbolic_geometry_handle` — asserts that
//! `Engine::eval` (no build, no kernel) produces `Value::GeometryHandle {
//! kernel_handle: None, realization_ref: Widget#0, .. }` for a `box()` param
//! cell.  FAILS until step-4 adds the symbolic-mint pass.
//!
//! **Step-5 (RED):** `eval_and_build_handles_are_content_hash_equal` and
//! `eval_upstream_values_hash_is_cross_run_stable` — pin §7.1 identity and
//! byte-stability.  Added when step-5 runs; FAIL until step-6 extracts the
//! shared uvh fold.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::{RealizationNodeId, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// Minimal fixture: one geometry param backed by a `box()` realization.
///
/// `body` is the first (index 0) realization of `Widget`, so its
/// `RealizationNodeId` is `RealizationNodeId::new("Widget", 0)`.
const WIDGET_SRC: &str = r#"structure def Widget {
    param width  : Length = 10mm
    param height : Length = 20mm
    param depth  : Length = 30mm
    param body   : Solid  = box(width, height, depth)
}"#;

/// After `Engine::eval` (no build, no kernel), the geometry value cell
/// `Widget.body` must be `Value::GeometryHandle { kernel_handle: None, .. }`
/// — NOT `Value::Undef`.
///
/// **RED** until step-4 adds `mint_symbolic_geometry_handles_into_values` on
/// the eval path.  Currently `eval_builtin("box", …)` falls through to
/// `Value::Undef` because reify-stdlib has no `RealizationNodeId` access.
#[test]
fn box_eval_yields_symbolic_geometry_handle() {
    let compiled = compile_source(WIDGET_SRC);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time errors; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Engine::eval — kernel-free, no build.
    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);

    let cell_id = ValueCellId::new("Widget", "body");
    let value = result.values.get_or_undef(&cell_id);

    match &value {
        Value::GeometryHandle {
            realization_ref,
            kernel_handle,
            ..
        } => {
            assert_eq!(
                *realization_ref,
                RealizationNodeId::new("Widget", 0),
                "realization_ref must be Widget#realization[0]"
            );
            assert_eq!(
                *kernel_handle, None,
                "kernel-free eval must yield kernel_handle == None (symbolic)"
            );
        }
        Value::Undef => {
            panic!(
                "Engine::eval returned Undef for Widget.body — symbolic-mint pass \
                 not yet wired (step-4 will fix this)"
            );
        }
        other => {
            panic!(
                "expected Value::GeometryHandle for Widget.body, got {:?}",
                other
            );
        }
    }
}

/// §7.1 two-way boundary: `Engine::eval` (symbolic, `kernel_handle=None`) and
/// `Engine::build` (realized, `kernel_handle=Some(...)`) on the SAME source
/// must produce `content_hash`-equal and `PartialEq`-equal
/// `Value::GeometryHandle` values (GHR-β: `content_hash`/`PartialEq` exclude
/// `kernel_handle`).
///
/// **Step-5 (RED):** fails if the `upstream_values_hash` fold in step-4's
/// `mint_symbolic_geometry_handles_into_values` diverges in even one byte from
/// the build-path fold in `post_process_geometry_handle_cells`.  Becomes green
/// after step-6 extracts both into a single shared free fn that guarantees
/// byte-identical output.
#[test]
fn eval_and_build_handles_are_content_hash_equal() {
    let compiled = compile_source(WIDGET_SRC);
    let cell_id = ValueCellId::new("Widget", "body");

    // Path A: pure eval (no kernel) — symbolic handle (kernel_handle = None).
    let mut eval_engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let eval_result = eval_engine.eval(&compiled);
    let eval_value = eval_result.values.get_or_undef(&cell_id);
    let (eval_rr, eval_uvh) = match &eval_value {
        Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => {
            assert_eq!(
                *kernel_handle,
                None,
                "eval path must yield kernel_handle=None (symbolic)"
            );
            (realization_ref.clone(), *upstream_values_hash)
        }
        other => panic!("eval path: expected Value::GeometryHandle, got {other:?}"),
    };

    // Path B: build with mock kernel — realized handle (kernel_handle = Some).
    let kernel = MockGeometryKernel::new();
    let mut build_engine =
        Engine::new(Box::new(SimpleConstraintChecker), Some(Box::new(kernel)));
    let build_result = build_engine.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        build_errors.is_empty(),
        "build must succeed with MockGeometryKernel; got: {build_errors:?}"
    );
    let build_value = build_result.values.get_or_undef(&cell_id);
    let (build_rr, build_uvh) = match &build_value {
        Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => {
            assert!(
                kernel_handle.is_some(),
                "build path must yield kernel_handle=Some(...)"
            );
            (realization_ref.clone(), *upstream_values_hash)
        }
        other => panic!("build path: expected Value::GeometryHandle, got {other:?}"),
    };

    // §7.1: realization_ref must match between eval and build paths.
    assert_eq!(
        eval_rr,
        build_rr,
        "realization_ref must match between eval and build paths (§7.1)"
    );

    // §7.1: upstream_values_hash must be byte-identical (same fold algorithm).
    assert_eq!(
        eval_uvh,
        build_uvh,
        "upstream_values_hash must be byte-identical between eval-mint and build-path fold \
         (step-6 extracts the shared fn that guarantees this)"
    );

    // GHR-β: content_hash excludes kernel_handle — symbolic == realized.
    assert_eq!(
        eval_value.content_hash(),
        build_value.content_hash(),
        "content_hash must be equal between symbolic (eval) and realized (build) handles \
         (GHR-β: kernel_handle excluded from content_hash)"
    );

    // PartialEq also excludes kernel_handle (GHR-β §DD).
    assert_eq!(
        eval_value,
        build_value,
        "PartialEq must hold between symbolic (eval) and realized (build) handles (GHR-β)"
    );
}

/// Cross-run stability: two independent `Engine::eval` runs on the same
/// compiled source yield byte-identical `upstream_values_hash` and
/// `content_hash` for `Widget.body`.
///
/// **Step-5 (RED):** fails if the uvh fold is non-deterministic across
/// independent runs.  Becomes green after step-6 hardens the shared fn.
#[test]
fn eval_upstream_values_hash_is_cross_run_stable() {
    let compiled = compile_source(WIDGET_SRC);
    let cell_id = ValueCellId::new("Widget", "body");

    // Run 1 — fresh Engine.
    let mut engine1 = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result1 = engine1.eval(&compiled);
    let value1 = result1.values.get_or_undef(&cell_id);
    let (uvh1, ch1) = match &value1 {
        Value::GeometryHandle {
            upstream_values_hash,
            ..
        } => (*upstream_values_hash, value1.content_hash()),
        other => panic!("run1: expected Value::GeometryHandle, got {other:?}"),
    };

    // Run 2 — separate Engine instance, same compiled module.
    let mut engine2 = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result2 = engine2.eval(&compiled);
    let value2 = result2.values.get_or_undef(&cell_id);
    let (uvh2, ch2) = match &value2 {
        Value::GeometryHandle {
            upstream_values_hash,
            ..
        } => (*upstream_values_hash, value2.content_hash()),
        other => panic!("run2: expected Value::GeometryHandle, got {other:?}"),
    };

    assert_eq!(
        uvh1,
        uvh2,
        "upstream_values_hash must be byte-identical across independent Engine::eval runs"
    );
    assert_eq!(
        ch1,
        ch2,
        "content_hash must be byte-identical across independent Engine::eval runs"
    );
}
