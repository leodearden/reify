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
use reify_ir::Value;
use reify_test_support::compile_source;

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
