//! End-to-end tests for `Value::GeometryHandle` value-cell hydration (GHR-γ).
//!
//! Exercises the full source-to-build pipeline (parse → compile → eval → build)
//! and asserts that `Type::Geometry` value cells are stamped with
//! `Value::GeometryHandle` by the realization-execution path in `engine_build.rs`.
//!
//! Step-5: RED — cells exist (post step-2/step-4) but engine does not yet hydrate
//!   them with a real `GeometryHandle`; `build_result.values` returns `Undef`.
//! Step-6: GREEN — engine hydrates the cell immediately after the realization
//!   named_steps + cache inserts.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::RealizationNodeId;
use reify_core::identity::ValueCellId;
use reify_ir::{ExportFormat, GeometryHandleId, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

/// After `Engine::build`, the value cell `ValueCellId::new("Widget", "body")`
/// must be `Value::GeometryHandle` with:
/// - `realization_ref == RealizationNodeId::new("Widget", 0)`
/// - `upstream_values_hash != [0u8; 32]` (non-zero stable hash)
/// - `kernel_handle != GeometryHandleId::INVALID`
///
/// **RED** (step-5): the cell exists post step-2 but `engine_build.rs` does not
/// yet stamp it — the value map returns `Undef` for geometry cells.
/// **GREEN** after step-6 wires the hydration.
#[test]
fn solid_param_evaluates_to_geometry_handle() {
    let source = r#"structure def Widget {
    param body : Solid = box(10mm, 20mm, 30mm)
}"#;
    let compiled = compile_source(source);

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

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time errors; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    let cell_id = ValueCellId::new("Widget", "body");
    let value = result.values.get_or_undef(&cell_id);

    match &value {
        Value::GeometryHandle {
            realization_ref,
            upstream_values_hash,
            kernel_handle,
        } => {
            assert_eq!(
                *realization_ref,
                RealizationNodeId::new("Widget", 0),
                "realization_ref must be Widget#realization[0]"
            );
            assert_ne!(
                *upstream_values_hash, [0u8; 32],
                "upstream_values_hash must be non-zero (blake3 of scalar args)"
            );
            assert_ne!(
                *kernel_handle,
                Some(GeometryHandleId::INVALID),
                "kernel_handle must not be INVALID"
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
