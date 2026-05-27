//! Tests for graceful handling of geometry build errors.
//!
//! Verifies that Engine::build() produces geometry_output=None and a summary
//! diagnostic when all geometry operations fail, rather than attempting to
//! export with a bogus handle.

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_test_support::*;
use reify_core::{Diagnostic, Type};
use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value};

// ---------------------------------------------------------------------------
// Shared helper: build a CompiledModule with fixed params and optional ops
// ---------------------------------------------------------------------------

/// Builds a `CompiledModule` at `path` with fixed width=80 / height=100 /
/// depth=5 mm parameters.  When `ops` is non-empty, attaches one realization
/// containing those ops; when empty, the template has no realizations
/// (total_ops=0).
///
/// Callers that need kernel/checker flexibility receive the raw `CompiledModule`
/// and wire up their own `Engine` — this helper is intentionally narrow so it
/// does not need to accept kernel or format parameters.
fn build_module_with_ops(path: &str, ops: &[CompiledGeometryOp]) -> reify_compiler::CompiledModule {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let mut builder = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)));

    if !ops.is_empty() {
        builder = builder.realization(e, 0, ops.to_vec());
    }

    let template = builder.build();

    CompiledModuleBuilder::new(reify_core::ModulePath::single(path))
        .template(template)
        .build()
}

/// Creates a compiled module with a single structure containing one box
/// primitive realization, so there is exactly one geometry operation to process.
fn module_with_box_realization() -> reify_compiler::CompiledModule {
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    build_module_with_ops("test_shape", &[box_op])
}

// ---------------------------------------------------------------------------
// Helpers: sentinel test setup and common assertions
// ---------------------------------------------------------------------------

/// Build the standard 3-op sentinel test module used by the three
/// sentinel-continuation tests:
/// - Op 0: Sphere(radius=10) — succeeds
/// - Op 1: Boolean(Union, Step(99), Step(99)) — compile fails (OOB refs)
/// - Op 2: Sphere(radius=5) — succeeds if sentinel continues the loop
///
/// Returns `(module, checker, kernel, ops_ref)` ready for engine construction.
fn make_sentinel_module(
    path: &str,
) -> (
    reify_compiler::CompiledModule,
    MockConstraintChecker,
    MockGeometryKernel,
    std::sync::Arc<std::sync::Mutex<Vec<GeometryOpRecord>>>,
) {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(10.0))],
    };
    let failing_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(99),
        right: GeomRef::Step(99),
    };
    let sphere_op_2 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op_0, failing_op, sphere_op_2])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(path))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    (module, checker, kernel, ops_ref)
}

/// Assert the sentinel-path invariants shared by all three sentinel-continuation
/// tests. Accepts the recorded kernel ops and the full diagnostics slice from
/// the eval result. Asserts: (a) the kernel received exactly 2 Sphere calls
/// (ops 0 and 2 both reached the kernel), and (b) exactly 1 diagnostic whose
/// message contains "failed to compile geometry operation" was produced (counted
/// internally by filtering `diagnostics`).
fn assert_sentinel_invariants(kernel_ops: &[GeometryOpRecord], diagnostics: &[Diagnostic]) {
    let sphere_ops: Vec<_> = kernel_ops
        .iter()
        .filter(|rec| matches!(rec.op, GeometryOp::Sphere { .. }))
        .collect();
    assert_eq!(
        sphere_ops.len(),
        2,
        "expected 2 sphere operations (sentinel allows op 2 to proceed), got {}: kernel_ops={:?}",
        sphere_ops.len(),
        kernel_ops
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    let compile_failure_count = diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .count();
    assert_eq!(
        compile_failure_count, 1,
        "expected exactly 1 compile-failure diagnostic from op 1, got {compile_failure_count}"
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// When all kernel operations fail, build() should return geometry_output=None
/// and include a summary diagnostic indicating that all geometry operations failed.
#[test]
fn build_returns_no_geometry_when_all_kernel_ops_fail() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Should have no geometry output when all ops fail
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when all kernel ops fail, got Some({} bytes)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    // Should contain a summary diagnostic about all ops failing
    let has_summary = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_summary,
        "expected a summary diagnostic about all geometry operations failing, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// When all ops fail to compile (compile_geometry_op returns None for all),
/// build() should also return geometry_output=None with appropriate diagnostics.
#[test]
fn build_returns_no_geometry_when_all_ops_fail_to_compile() {
    // Boolean union referencing Step(0) and Step(1) but no prior primitives,
    // so compile_geometry_op returns None (last_handle is None, resolve_ref fails).
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let module = build_module_with_ops("test_compile_fail", &[union_op]);

    // Use standard MockGeometryKernel — kernel.execute() should never be called
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when all ops fail to compile"
    );

    let has_compile_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("failed to compile geometry operation"));
    assert!(
        has_compile_error,
        "expected per-op compile failure diagnostic"
    );

    let has_summary = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_summary,
        "expected summary diagnostic about all geometry operations failing"
    );
}

// ---------------------------------------------------------------------------
// FailingExportMockGeometryKernel — execute AND export both return Err
// ---------------------------------------------------------------------------

/// A mock whose execute() fails and export() also fails.
/// Used to verify that export is never attempted when all ops fail.
struct FailingExportMockGeometryKernel;

impl GeometryKernel for FailingExportMockGeometryKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(
            "simulated kernel failure".into(),
        ))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Ok(Value::Real(0.0))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::InvalidHandle(GeometryHandleId(0)))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Ok(Mesh {
            vertices: vec![],
            indices: vec![],
            normals: None,
        })
    }
}

/// When all ops fail, export should never be attempted — so there should be
/// NO 'export error' diagnostic even if the kernel's export would fail.
#[test]
fn build_no_export_error_when_all_ops_fail() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = FailingExportMockGeometryKernel;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Export should not have been attempted
    let has_export_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("export error"));
    assert!(
        !has_export_error,
        "export should not be attempted when all ops fail, but got an export error diagnostic"
    );

    // Should still have the summary diagnostic
    let has_summary = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(has_summary, "expected summary diagnostic");
}

/// Regression: modules with no realizations (total_ops=0) should still
/// export successfully, same as the existing build_with_mock_kernel test.
#[test]
fn build_with_no_realizations_still_exports() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert!(
        result.geometry_output.is_some(),
        "modules with no realizations should still produce geometry output"
    );
}

// ---------------------------------------------------------------------------
// Loft end-to-end through compile_geometry_op and Engine::build
// ---------------------------------------------------------------------------

/// Exercises the full compile -> eval path for Loft.
/// Creates a module with 3 ops: Sphere(0), Sphere(1), Loft([Step(0), Step(1)]).
/// Verifies that the Loft operation receives distinct profile handle IDs
/// (handle from op 0 and handle from op 1), not duplicates.
#[test]
fn loft_through_full_eval_pipeline() {
    use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};

    let e = "TestLoft";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (produces handle at step index 0)
    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(10.0))],
    };

    // Op 1: Sphere (produces handle at step index 1)
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 2: Loft referencing Step(0) and Step(1) as profiles
    let loft_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Loft,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op_0, sphere_op_1, loft_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_loft"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    // Inspect the recorded operations
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        3,
        "expected 3 geometry operations, got {}",
        ops.len()
    );

    // Op 0: Sphere → handle 1
    // Op 1: Sphere → handle 2
    // Op 2: Loft → should have profiles [handle_1, handle_2]
    let handle_0 = ops[0].result_handle;
    let handle_1 = ops[1].result_handle;

    match &ops[2].op {
        GeometryOp::Loft { profiles } => {
            assert_eq!(
                profiles.len(),
                2,
                "Loft should have 2 profiles, got {}",
                profiles.len()
            );
            assert_eq!(
                profiles[0], handle_0,
                "Loft profiles[0] should be handle from op 0 ({:?}), got {:?}",
                handle_0, profiles[0]
            );
            assert_eq!(
                profiles[1], handle_1,
                "Loft profiles[1] should be handle from op 1 ({:?}), got {:?}",
                handle_1, profiles[1]
            );
            // Verify they're distinct
            assert_ne!(
                profiles[0], profiles[1],
                "Loft profiles should be distinct handles, but both are {:?}",
                profiles[0]
            );
        }
        other => panic!("expected GeometryOp::Loft at op index 2, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Tests: sentinel placeholder behavior for compile failures
// ---------------------------------------------------------------------------

/// When a realization contains multiple ops that all fail to compile, all
/// failures should be reported. With the sentinel fix, the loop pushes
/// GeometryHandleId::INVALID and continues (does not break), so all ops are
/// attempted and each emits its own diagnostic.
/// The realization is still rolled back because had_failure=true.
#[test]
fn cascading_compile_failures_aborted_after_first() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Three Boolean(Union) ops, all referencing non-existent Step indices.
    // compile_geometry_op returns None for each because step_handles is empty.
    // With the sentinel fix, all 3 ops are attempted → 3 compile-failure diagnostics.
    let union_op_0 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    let union_op_1 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    let union_op_2 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![union_op_0, union_op_1, union_op_2])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_cascade"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // All 3 ops are attempted with sentinel — each emits its own diagnostic
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();

    assert_eq!(
        compile_failures.len(),
        3,
        "expected exactly 3 compile-failure diagnostics (sentinel continues after each failure), \
         got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Realization must be rolled back (had_failure=true) — no geometry output
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when all ops fail (realization rolled back)"
    );
}

/// When a realization contains multiple ops and kernel.execute fails for all,
/// only the first kernel error diagnostic should be emitted. The loop should
/// abort after the first Err from kernel.execute, preventing cascading errors.
#[test]
fn cascading_kernel_failures_aborted_after_first() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Three Box primitives — all will compile successfully but fail at kernel.execute
    let box_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(10.0)),
            ("height".into(), mm_literal(20.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    let box_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(30.0)),
            ("height".into(), mm_literal(40.0)),
            ("depth".into(), mm_literal(15.0)),
        ],
    };
    let box_op_2 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(50.0)),
            ("height".into(), mm_literal(60.0)),
            ("depth".into(), mm_literal(25.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![box_op_0, box_op_1, box_op_2])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_kernel_cascade"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let kernel_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("geometry error"))
        .collect();

    assert_eq!(
        kernel_errors.len(),
        1,
        "expected exactly 1 geometry error diagnostic (abort after first), got {}: {:?}",
        kernel_errors.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Aborting a failing realization must not prevent subsequent realizations from
/// executing. Realization 1 has a broken Boolean op (compile failure), while
/// realization 2 has a valid Box primitive. After the fix, realization 1 emits
/// exactly 1 compile-failure diagnostic and realization 2 succeeds normally.
#[test]
fn realization_abort_is_per_realization() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Realization 0: Boolean union referencing non-existent steps → compile failure
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    // Realization 1: valid Box primitive → should succeed
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![union_op])
        .realization(e, 1, vec![box_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_per_realization"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) exactly 1 compile-failure diagnostic from realization 0
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();
    assert_eq!(
        compile_failures.len(),
        1,
        "expected exactly 1 compile-failure diagnostic, got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (b) MockGeometryKernel received exactly 1 execute call (the Box from realization 1)
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "expected 1 kernel execute call (Box from realization 1), got {}",
        ops.len()
    );

    // (c) the 'all geometry operations failed' summary diagnostic is NOT present
    let has_all_failed = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        !has_all_failed,
        "should NOT have 'all geometry operations failed' since realization 1 succeeded"
    );
}

/// The tessellate path (tessellate_realizations → tessellate_from_values) should
/// also use the sentinel approach: all 3 ops are attempted, each emits a
/// compile-failure diagnostic, and no meshes are produced for the failing
/// realization because it is rolled back.
#[test]
fn tessellate_aborts_cascading_compile_failures() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Three Boolean(Union) ops, all referencing non-existent Step indices.
    // With the sentinel fix, all 3 are attempted → 3 compile-failure diagnostics.
    let union_op_0 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    let union_op_1 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };
    let union_op_2 = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![union_op_0, union_op_1, union_op_2])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_tess_cascade"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.tessellate_realizations(&module);

    // All 3 ops attempted with sentinel → 3 compile-failure diagnostics
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();

    assert_eq!(
        compile_failures.len(),
        3,
        "expected exactly 3 compile-failure diagnostics from tessellate \
         (sentinel continues after each failure), got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Realization rolled back — no meshes
    assert!(
        result.meshes.is_empty(),
        "expected no meshes when all ops fail to compile (realization rolled back)"
    );
}

/// Sentinel behavior for mixed-failure sequence: op0 (Box) succeeds, op1
/// (Boolean referencing non-existent steps) fails to compile → INVALID sentinel
/// pushed at step[1], loop continues. op2 (Fillet on Step(1)) is attempted;
/// Step(1) resolves to INVALID so compile_geometry_op returns None → second
/// compile-failure diagnostic. Kernel only receives the Box from op0.
#[test]
fn mixed_failure_then_dependent_ops_aborted() {
    use reify_compiler::ModifyKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box (succeeds) → handle at step[0]
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Boolean union referencing non-existent Step(5) and Step(6) → compile failure
    // Sentinel INVALID pushed at step[1], loop continues.
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(6),
    };

    // Op 2: Fillet on Step(1) — IS attempted (sentinel loop continues), but
    // Step(1) is INVALID so compile_geometry_op returns None → second failure.
    let fillet_op = CompiledGeometryOp::Modify {
        kind: ModifyKind::Fillet,
        target: GeomRef::Step(1),
        args: vec![("radius".into(), mm_literal(2.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op, union_op, fillet_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_mixed_abort"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) kernel received exactly 1 execute call (the Box from op0)
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "expected 1 kernel execute call (Box from op0), got {}",
        ops.len()
    );

    // (b) 2 compile-failure diagnostics: op1 (bad refs) and op2 (INVALID target)
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();
    assert_eq!(
        compile_failures.len(),
        2,
        "expected 2 compile-failure diagnostics (op1 bad refs + op2 INVALID target), \
         got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (c) no kernel errors — op2's compile failure happens before kernel dispatch
    let kernel_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("geometry error"))
        .collect();
    assert_eq!(
        kernel_errors.len(),
        0,
        "expected no geometry error diagnostics (kernel was never called for op1 or op2)"
    );
}

// ---------------------------------------------------------------------------
// Tests: partial-failure handle leakage (step-32)
// ---------------------------------------------------------------------------

/// When a realization has [Box (succeeds), Boolean union with bad refs (compile
/// failure)], tessellate_realizations should produce NO meshes — the partial
/// success should not leak the intermediate Box handle into the output.
///
/// BUG: Before the fix, the `break` leaves the Box handle in `step_handles`.
/// `step_handles.len() > handle_start` is true so the intermediate box gets
/// tessellated and added to meshes — callers receive an incorrect partial mesh.
#[test]
fn partial_failure_tessellate_produces_no_mesh() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box primitive (compiles and executes OK)
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Boolean union referencing non-existent Step(5) and Step(6) → compile failure
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(6),
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op, union_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_partial_tess"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.tessellate_realizations(&module);

    // (a) The failing realization should produce NO mesh — the intermediate
    //     Box handle must be discarded when the realization partially fails.
    assert!(
        result.meshes.is_empty(),
        "expected no meshes when realization partially fails (Box OK, Union FAIL), \
         but got {} mesh(es) — intermediate handles leaked",
        result.meshes.len()
    );

    // (b) Should have exactly 1 compile-failure diagnostic
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();
    assert_eq!(
        compile_failures.len(),
        1,
        "expected exactly 1 compile-failure diagnostic, got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// When op 1 of 3 fails to compile in tessellate_from_values, a sentinel should
/// be pushed and op 2 should still be attempted. This mirrors
/// sentinel_placeholder_continues_independent_ops (the build() path) and verifies
/// the same sentinel logic in the tessellate_from_values loop.
///
/// Op 0: Sphere(radius=10) — succeeds, kernel gets Sphere call.
/// Op 1: Boolean(Union, Step(99), Step(99)) — compile fails (OOB refs).
/// Op 2: Sphere(radius=5) — succeeds because sentinel allows loop to continue.
///
/// Assertions:
/// (a) kernel receives 2 Sphere calls (ops 0 and 2).
/// (b) meshes.is_empty() — rollback because had_failure=true.
/// (c) exactly 1 compile-failure diagnostic from op 1.
#[test]
fn tessellate_sentinel_placeholder_continues_independent_ops() {
    let (module, checker, kernel, ops_ref) = make_sentinel_module("test_tess_sentinel_continues");
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.tessellate_realizations(&module);

    let kernel_ops = ops_ref.lock().unwrap();
    assert_sentinel_invariants(&kernel_ops, &result.diagnostics);

    // Rollback: no meshes produced when had_failure=true.
    assert!(
        result.meshes.is_empty(),
        "expected no meshes (sentinel rollback in tessellate_from_values), \
         but got {} mesh(es)",
        result.meshes.len()
    );
}

/// When a realization has [Box (succeeds), Boolean union with bad refs (compile
/// failure)], build() should return geometry_output=None — the partial success
/// should not leak the intermediate Box handle into the export.
///
/// BUG: Before the fix, `step_handles.last()` returns the intermediate Box
/// handle so the partially-complete geometry gets exported.
#[test]
fn partial_failure_build_produces_no_geometry() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box primitive (compiles and executes OK)
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Boolean union referencing non-existent Step(5) and Step(6) → compile failure
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(6),
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op, union_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_partial_build"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) geometry_output should be None — the intermediate Box handle must
    //     be discarded when the realization partially fails.
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when realization partially fails \
         (Box OK, Union FAIL), but got Some({} bytes) — intermediate handles leaked",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    // (b) Should have compile-failure diagnostic
    let has_compile_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("failed to compile geometry operation"));
    assert!(
        has_compile_error,
        "expected compile-failure diagnostic for the Boolean union"
    );

    // (c) Should have 'all geometry operations failed' summary since partial
    //     failure means the realization contributed no handles (after fix)
    let has_summary = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_summary,
        "expected summary diagnostic about all geometry operations failing \
         (partial failure should discard all intermediate handles)"
    );
}

/// Two realizations in one template: realization 0 partially fails (Box OK,
/// Boolean union compile failure), realization 1 succeeds (Box only). After the
/// fix, realization 0's partial failure contributes nothing and realization 1's
/// output is isolated and correct.
#[test]
fn partial_failure_does_not_contaminate_subsequent_realization() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Realization 0: [Box (succeeds), Boolean union with bad refs (compile failure)]
    let box_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(6),
    };

    // Realization 1: [Box only (succeeds)]
    let box_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(40.0)),
            ("height".into(), mm_literal(50.0)),
            ("depth".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op_0, union_op])
        .realization(e, 1, vec![box_op_1])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_partial_contaminate"))
            .template(template)
            .build();

    // --- tessellate path ---
    {
        let checker = MockConstraintChecker::new();
        let kernel = MockGeometryKernel::new();
        let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
        let result = engine.tessellate_realizations(&module);

        // (a) exactly 1 mesh produced (from realization 1 only)
        assert_eq!(
            result.meshes.len(),
            1,
            "expected exactly 1 mesh (from realization 1), got {}",
            result.meshes.len()
        );

        // (b) the mesh's entity ID matches realization 1's ID (not realization 0's)
        let mesh_id = &result.meshes[0].0;
        assert!(
            mesh_id.contains("1"),
            "expected mesh entity ID to be from realization 1, got '{}'",
            mesh_id
        );

        // (c) exactly 1 compile-failure diagnostic (from realization 0)
        let compile_failures: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("failed to compile geometry operation"))
            .collect();
        assert_eq!(
            compile_failures.len(),
            1,
            "expected exactly 1 compile-failure diagnostic, got {}",
            compile_failures.len()
        );
    }

    // --- build path ---
    {
        let checker = MockConstraintChecker::new();
        let kernel = MockGeometryKernel::new();
        let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
        let result = engine.build(&module, ExportFormat::Step);

        // (d) geometry_output is Some (realization 1 succeeded)
        assert!(
            result.geometry_output.is_some(),
            "expected geometry_output to be Some since realization 1 succeeded"
        );

        // (e) no 'all geometry operations failed' summary (realization 1 succeeded)
        let has_all_failed = result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("all geometry operations failed"));
        assert!(
            !has_all_failed,
            "should NOT have 'all geometry operations failed' since realization 1 succeeded"
        );
    }
}

// ---------------------------------------------------------------------------
// Regression guard: missing primitive arg → no kernel call, no 'geometry error'
// ---------------------------------------------------------------------------

/// When a Box primitive is missing a required arg ('width'), build() should:
/// 1. Short-circuit: return geometry_output=None (kernel never called).
/// 2. Emit a Warning: "missing required geometry argument" mentioning 'width'.
/// 3. Emit an Error: "failed to compile geometry operation".
/// 4. NOT emit any diagnostic containing "geometry error" (kernel was never reached).
///
/// Assertions are delegated to `assert_rejected_at_compile` with `None` for the
/// primitive predicate, indicating zero preceding kernel calls are expected.
#[test]
fn build_primitive_missing_arg_no_kernel_error() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Box with height and depth present, but 'width' deliberately omitted
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
            // width deliberately omitted
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_missing_width"))
        .template(template)
        .build();

    // Standard MockGeometryKernel — if execute() were called it would succeed,
    // but it should never be reached.
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Only test exercising the None (zero kernel ops) path of assert_rejected_at_compile.
    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        None,
        &["missing required geometry argument", "width"],
    );
}

// ---------------------------------------------------------------------------
// Anti-cascade regression lock: one Warning + one Error, no downstream cascade
// ---------------------------------------------------------------------------

/// Regression lock for the fail-fast / anti-cascade contract (tasks 1196, 1198).
///
/// When a primitive op is missing a required arg, `compile_geometry_op` must
/// emit exactly one Warning (from `eval_named_arg*`'s `.ok_or_else` path) and
/// exactly one Error (from the caller in `engine_build.rs`: "failed to compile
/// geometry operation: {err}"). It must NOT produce a second Warning from any
/// downstream "expected Geometry, found Undef" / type-coercion cascade.
///
/// The invariants pinned here:
/// - `.filter(|d| Severity::Warning).count() == 1` — one (not two) warnings.
/// - That Warning contains 'missing required geometry argument' and 'width'.
/// - Exactly one `failed to compile geometry operation` Error diagnostic.
/// - No diagnostic contains 'expected Geometry' or 'found Undef' (no cascade).
///
/// This test will break if a future refactor re-introduces the Undef-value
/// cascade — whether by computing a post-error value-cell type check, by
/// downgrading `.ok_or_else` to `.unwrap_or(Value::Undef)`, or by otherwise
/// letting the compile pipeline emit a second diagnostic for the same op.
#[test]
fn build_primitive_missing_arg_emits_exactly_one_compile_warning() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
            // width deliberately omitted
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_anti_cascade_single_warning",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) Exactly one Warning containing the missing-arg message and 'width'.
    let matching_warnings: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("missing required geometry argument")
                && d.message.contains("width")
        })
        .collect();
    assert_eq!(
        matching_warnings.len(),
        1,
        "expected exactly one Warning about missing 'width'; got {}: {:?}",
        matching_warnings.len(),
        result
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // (c) "one Warning, no cascade" invariant — total Warning count is also 1.
    let warning_count = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Warning)
        .count();
    assert_eq!(
        warning_count,
        1,
        "anti-cascade invariant: expected exactly one Warning diagnostic across the whole realization; got {}: {:?}",
        warning_count,
        result
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // (b) Exactly one Error containing 'failed to compile geometry operation'.
    let compile_errors: Vec<&Diagnostic> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Error
                && d.message.contains("failed to compile geometry operation")
        })
        .collect();
    assert_eq!(
        compile_errors.len(),
        1,
        "expected exactly one 'failed to compile geometry operation' Error; got {}: {:?}",
        compile_errors.len(),
        result
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // (d) No diagnostic mentions 'expected Geometry' or 'found Undef' (cascade).
    for d in &result.diagnostics {
        assert!(
            !d.message.contains("expected Geometry"),
            "anti-cascade invariant violated: a diagnostic contained 'expected Geometry' (downstream cascade): {:?}",
            d.message
        );
        assert!(
            !d.message.contains("found Undef"),
            "anti-cascade invariant violated: a diagnostic contained 'found Undef' (downstream cascade): {:?}",
            d.message
        );
    }
}

// ---------------------------------------------------------------------------
// Regression guard: missing modify arg → no Modify kernel call, no 'geometry error'
// ---------------------------------------------------------------------------

/// When a Fillet modify op is missing its required 'radius' arg, build() should:
/// 1. Call kernel exactly once — for the preceding Box that provides the target
///    handle — but never call kernel for the Fillet itself.
/// 2. Return geometry_output=None (compile failure short-circuits the realization).
/// 3. Emit a Warning whose message contains 'missing required geometry argument',
///    'radius', and 'Fillet'.
/// 4. Emit an Error: "failed to compile geometry operation".
/// 5. NOT emit any diagnostic containing "geometry error" (kernel was never
///    called for the Fillet op).
///
/// Unlike the primitive test (`build_primitive_missing_arg_no_kernel_error`),
/// this test uses a two-op realization [Box, Fillet(missing radius)] because
/// `compile_geometry_op` resolves `Modify { target, .. }` via
/// `step_handles.get(idx).copied()?` *before* reaching arg-validation — so a
/// lone Fillet with an empty step_handles would short-circuit at target lookup
/// without ever emitting the warning. The Box is the minimum setup needed to
/// populate step_handles[0] so the Fillet's target resolves and the
/// arg-validation path is exercised.
///
/// The unit-level counterpart is
/// `compile_geometry_op_modify_missing_arg_returns_none` in lib.rs:4955-5007.
#[test]
fn build_modify_missing_arg_no_kernel_error() {
    build_modify_missing_arg_case(reify_compiler::ModifyKind::Fillet, "radius", "fillet");
}

/// Drive an engine.build() through a Modify op whose required arg is omitted,
/// then assert compile-time rejection via `assert_rejected_at_compile`.
///
/// The realization is a two-op sequence [Box, Modify(kind, empty args)] because
/// `compile_geometry_op` resolves `Modify { target, .. }` via
/// `step_handles.get(idx).copied()?` *before* reaching arg-validation — so a
/// lone Modify with an empty step_handles would short-circuit at target lookup
/// without ever emitting the missing-arg warning. The Box is the minimum setup
/// needed to populate step_handles[0] so the Modify's target resolves and the
/// arg-validation path is exercised.
///
/// Callers pass:
/// - `kind` — the ModifyKind variant under test (Fillet/Chamfer/Shell/Draft/Thicken).
/// - `missing_arg_for_warning` — the arg name expected in the Warning message
///   (e.g. `"radius"` for Fillet, `"thickness"` for Shell).
/// - `kind_name_for_warning` — the lowercase Display name of the kind
///   (e.g. `"fillet"`, `"shell"`, `"thicken"`, `"draft"`, `"chamfer"`).
fn build_modify_missing_arg_case(
    kind: reify_compiler::ModifyKind,
    missing_arg_for_warning: &str,
    kind_name_for_warning: &str,
) {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box primitive with all three required args — provides step_handles[0]
    // as the Modify op's target.
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Modify op with its required arg deliberately omitted.
    let modify_op = CompiledGeometryOp::Modify {
        kind,
        target: GeomRef::Step(0),
        args: vec![],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, modify_op])
        .build();
    let module_path = format!(
        "test_modify_{}_missing_{}",
        kind_name_for_warning, missing_arg_for_warning
    );
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(&module_path))
        .template(template)
        .build();

    // Standard MockGeometryKernel — if execute() were called for the Modify it
    // would succeed, but it should never be reached for that op.
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Box { .. })),
        &[
            "missing required geometry argument",
            missing_arg_for_warning,
            kind_name_for_warning,
        ],
    );
}

// ---------------------------------------------------------------------------
// Helper: assert compile-time rejection of a geometry operation
// ---------------------------------------------------------------------------

/// Asserts that a geometry operation was rejected at compile time (before kernel
/// dispatch), producing the expected 5-signal pattern in `result` / `ops`.
///
/// Arguments:
/// - `result` — the `BuildResult` returned by `engine.build()`.
/// - `ops` — the slice from `ops_ref.lock().unwrap()` (the kernel's recorded ops).
/// - `expected_primitive` — `None` means expect zero kernel ops (the rejected op was
///   a bare primitive — kernel should never be called at all); `Some(f)` means expect
///   exactly one preceding kernel op whose recorded op satisfies `f`
///   (e.g. `Some(|op| matches!(op, GeometryOp::Box { .. }))`).
/// - `warning_needles` — every string that must appear in the Warning message
///   (e.g. `&["scale dropped", "negative"]`).
///
/// The five assertions are:
/// 1. Kernel received zero ops (`None`) or exactly one op matching `expected_primitive` (`Some`).
/// 2. `result.geometry_output` is `None`.
/// 3. A `Severity::Warning` diagnostic exists whose message contains every needle.
/// 4. An `Error`-level diagnostic exists containing 'failed to compile geometry operation'.
/// 5. No diagnostic contains 'geometry error' (kernel was never called for the rejected op).
fn assert_rejected_at_compile(
    result: &reify_eval::BuildResult,
    ops: &[reify_test_support::GeometryOpRecord],
    expected_primitive: Option<fn(&reify_ir::GeometryOp) -> bool>,
    warning_needles: &[&str],
) {
    // (1) Kernel op count and identity
    match expected_primitive {
        None => {
            assert!(
                ops.is_empty(),
                "kernel.execute() should never be called when compile_geometry_op returns None, \
                 but got {} kernel ops",
                ops.len()
            );
        }
        Some(f) => {
            assert_eq!(
                ops.len(),
                1,
                "kernel.execute() should be called only for the preceding primitive (not the rejected op), \
                 got {} kernel ops",
                ops.len()
            );
            assert!(
                f(&ops[0].op),
                "expected the only recorded kernel op to match expected_primitive, got: {:?}",
                ops[0].op
            );
        }
    }

    // (2) No geometry output — rejected op caused the whole realization to fail
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when the op is rejected at compile time"
    );

    // (3) Warning containing all provided needles
    let has_warning = result.diagnostics.iter().any(|d| {
        d.severity == reify_core::Severity::Warning
            && warning_needles
                .iter()
                .all(|needle| d.message.contains(needle))
    });
    assert!(
        has_warning,
        "expected a Warning diagnostic containing all needles {:?}, got: {:?}",
        warning_needles,
        result
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    // (4) Error about failed compile
    let has_compile_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("failed to compile geometry operation"));
    assert!(
        has_compile_error,
        "expected an Error diagnostic 'failed to compile geometry operation', got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (5) No 'geometry error' diagnostic — kernel was never called for the rejected op
    let has_kernel_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("geometry error"));
    assert!(
        !has_kernel_error,
        "should NOT have a 'geometry error' diagnostic (kernel was never called for the rejected op), \
         but got: {:?}",
        result
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("geometry error"))
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Regression guard: negative scale factor → compile-time rejection, specific Warning
// ---------------------------------------------------------------------------

/// When a Transform::Scale op has a negative factor (-1.0), build() should:
/// 1. Call kernel exactly once — for the preceding Box that provides the target
///    handle — but never call kernel for the Scale op itself.
/// 2. Return geometry_output=None (the realization as a whole fails).
/// 3. Emit a Warning whose message contains both 'scale dropped' and 'negative'
///    (the site-specific diagnostic from lib.rs:3551).
/// 4. Emit an Error containing 'failed to compile geometry operation'.
/// 5. NOT emit any diagnostic containing "geometry error" (kernel was never
///    called for the Scale op).
///
/// This is a regression-coverage test for lib.rs:3546-3555 (the negative-factor
/// check in the Transform::Scale branch of compile_geometry_op). It drives the
/// full Engine::build() path to verify the Warning propagates to BuildResult.diagnostics.
#[test]
fn build_scale_negative_factor_emits_diagnostic() {
    use reify_compiler::TransformKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    // Op 0: Box primitive with all three required args — provides step_handles[0]
    // as the Scale op's target
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Scale with factor=-1.0 — rejected because negative factor produces
    // inside-out (point-symmetry) geometry
    let scale_op = CompiledGeometryOp::Transform {
        kind: TransformKind::Scale,
        target: GeomRef::Step(0),
        args: vec![("factor".into(), real_literal(-1.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op, scale_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_scale_negative"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Box { .. })),
        &["scale dropped", "negative"],
    );
}

// ---------------------------------------------------------------------------
// Regression guard: non-finite extrude distance → compile-time rejection, specific Warning
// ---------------------------------------------------------------------------

/// When a Sweep::Extrude op has a NaN distance, build() should:
/// 1. Call kernel exactly once — for the preceding Sphere that provides the
///    profile handle — but never call kernel for the Extrude op itself.
/// 2. Return geometry_output=None (the realization as a whole fails).
/// 3. Emit a Warning whose message contains 'extrude dropped', 'degenerate', and 'NaN'
///    (the site-specific diagnostic from lib.rs:3670).
/// 4. Emit an Error containing 'failed to compile geometry operation'.
/// 5. NOT emit any diagnostic containing "geometry error" (kernel was never
///    called for the Extrude op).
///
/// Uses NaN rather than 0.0 to exercise the non-finite arm of the check at
/// lib.rs:3667-3678, complementing the existing zero-distance test in
/// stress_sweep_degenerate.rs which covers the near-zero arm.
#[test]
fn build_extrude_nonfinite_distance_emits_diagnostic() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere primitive with radius — provides step_handles[0] as the
    // Extrude's profile handle
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Extrude with NaN distance — rejected because NaN is non-finite
    let nan_distance = reify_ir::CompiledExpr::literal(
        reify_ir::Value::Scalar {
            si_value: f64::NAN,
            dimension: reify_core::DimensionVector::LENGTH,
        },
        Type::length(),
    );
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![("distance".into(), nan_distance)],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "radius", Type::length(), Some(mm_literal(50.0)))
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extrude_nan_distance"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["extrude dropped", "degenerate", "NaN"],
    );
}

// ---------------------------------------------------------------------------
// Regression guard: degenerate revolve axis → compile-time rejection, specific Warning
// ---------------------------------------------------------------------------

/// When a Sweep::Revolve op has a zero-length axis (ax=ay=az=0.0), build() should:
/// 1. Call kernel exactly once — for the preceding Sphere that provides the
///    profile handle — but never call kernel for the Revolve op itself.
/// 2. Return geometry_output=None (the realization as a whole fails).
/// 3. Emit a Warning whose message contains both 'revolve dropped' and 'axis'
///    (the site-specific diagnostic from lib.rs:3699).
/// 4. Emit an Error containing 'failed to compile geometry operation'.
/// 5. NOT emit any diagnostic containing "geometry error" (kernel was never
///    called for the Revolve op).
///
/// This specifically exercises the axis-magnitude check at lib.rs:3698, which
/// is distinct from the angle check tested in build_revolve_zero_angle_emits_diagnostic.
#[test]
fn build_revolve_degenerate_axis_emits_diagnostic() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    // Op 0: Sphere primitive — provides step_handles[0] as the Revolve's profile handle
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Revolve with ax=ay=az=0.0 (zero-length axis) — rejected before angle is evaluated
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(0.0)),
            ("angle".into(), real_literal(std::f64::consts::PI)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "radius", Type::length(), Some(mm_literal(50.0)))
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_revolve_degenerate_axis",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["revolve dropped", "axis"],
    );
}

// ---------------------------------------------------------------------------
// Regression guard: zero revolve angle → compile-time rejection, specific Warning
// ---------------------------------------------------------------------------

/// When a Sweep::Revolve op has a zero angle (0.0 rad) with a valid z-axis,
/// build() should:
/// 1. Call kernel exactly once — for the preceding Sphere that provides the
///    profile handle — but never call kernel for the Revolve op itself.
/// 2. Return geometry_output=None (the realization as a whole fails).
/// 3. Emit a Warning whose message contains both 'revolve dropped' and 'angle'
///    (the site-specific diagnostic from lib.rs:3711).
/// 4. Emit an Error containing 'failed to compile geometry operation'.
/// 5. NOT emit any diagnostic containing "geometry error" (kernel was never
///    called for the Revolve op).
///
/// This is the canonical guard for the angle-degenerate check at lib.rs:3710.
/// The weaker revolve_zero_angle test that previously existed in
/// stress_sweep_degenerate.rs has been removed — this test supersedes it with
/// stricter assertions (kernel-ops count + specific Warning + compile Error +
/// no kernel error).
#[test]
fn build_revolve_zero_angle_emits_diagnostic() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    // Op 0: Sphere primitive — provides step_handles[0] as the Revolve's profile handle
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Revolve with valid z-axis (az=1.0) but angle=0.0 — rejected because
    // |angle| < 1e-12 cannot produce a meaningful revolve
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "radius", Type::length(), Some(mm_literal(50.0)))
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_revolve_zero_angle"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["revolve dropped", "angle"],
    );
}

// ---------------------------------------------------------------------------
// Tests for sentinel placeholder (task-612): compile_geometry_op returning None
// should push INVALID placeholder and continue, not break.
// ---------------------------------------------------------------------------

/// When op 1 of 3 fails to compile (returns None), a sentinel should be pushed
/// and op 2 should still be attempted. This verifies that the kernel receives
/// 2 sphere calls (ops 0 and 2), not just 1.
///
/// Currently fails because the loop `break`s on the first None.
/// After the fix: sentinel pushed at index 1, loop continues, op 2 succeeds.
/// The realization is rolled back because had_failure=true.
#[test]
fn sentinel_placeholder_continues_independent_ops() {
    let (module, checker, kernel, ops_ref) = make_sentinel_module("test_sentinel_continues");
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let kernel_ops = ops_ref.lock().unwrap();
    assert_sentinel_invariants(&kernel_ops, &result.diagnostics);

    // The realization should be rolled back (had_failure=true) → no geometry output.
    assert!(
        result.geometry_output.is_none(),
        "realization should be rolled back when any op fails, but got geometry output"
    );
}

// ---------------------------------------------------------------------------
// Tests for rollback correctness with sentinel (step-8)
// ---------------------------------------------------------------------------

/// Rollback correctness: op0 (Sphere) succeeds and produces a valid handle, but
/// op1 (Boolean referencing non-existent Step(5)) fails to compile and sets
/// had_failure=true. Even though op0's handle is in step_handles at the time
/// of failure, the realization's handle range must be truncated to handle_start
/// because had_failure=true triggers the rollback condition.
///
/// Verifies: geometry_output is None despite op0 producing a valid handle, and
/// a compile-failure diagnostic is emitted for op1.
#[test]
fn sentinel_had_failure_triggers_rollback_despite_partial_success() {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (compiles and executes OK) → valid handle in step_handles[0]
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(10.0))],
    };

    // Op 1: Boolean referencing non-existent Step(5) → compile failure, sets had_failure
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(5),
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "radius", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![sphere_op, union_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_had_failure_rollback"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) kernel received exactly 1 execute call (Sphere from op0)
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "expected 1 kernel execute call (Sphere from op0), got {}",
        ops.len()
    );

    // (b) had_failure rollback: geometry_output must be None even though op0 succeeded
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None — had_failure=true must trigger rollback \
         even though op0 produced a valid Sphere handle"
    );

    // (c) exactly 1 compile-failure diagnostic (from op1)
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();
    assert_eq!(
        compile_failures.len(),
        1,
        "expected 1 compile-failure diagnostic (op1 bad refs), got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Regression test for the Draft plane resolution missing INVALID filter.
///
/// Scenario: 3 ops in a realization:
///   Op 0 — Sphere → succeeds, pushes a valid handle into step_handles[0]
///   Op 1 — Boolean(Step(5), Step(5)) → fails compile (indices OOB) → pushes
///           INVALID sentinel into step_handles[1]
///   Op 2 — Draft(target=Step(0)) → target_id resolves to the sphere handle (valid),
///           but step_handles.last() is INVALID (the sentinel from op 1).
///           After the fix, the INVALID filter causes plane_id to be None,
///           so compile_geometry_op returns None for the Draft.
///
/// Before the fix: INVALID was forwarded as `plane` to the kernel, leading to
/// undefined behaviour. After the fix: the Draft is also treated as a compile
/// failure (sentinel pushed, had_failure=true), geometry_output remains None.
#[test]
fn draft_plane_invalid_sentinel_causes_compile_failure() {
    use reify_compiler::ModifyKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    // Op 0: Sphere — succeeds, produces a valid handle at step_handles[0]
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(10.0))],
    };

    // Op 1: Boolean with out-of-bounds Step refs — fails compile → INVALID sentinel
    // pushed at step_handles[1], had_failure=true.
    let bad_bool_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(5),
        right: GeomRef::Step(5),
    };

    // Op 2: Draft targeting Step(0) (the sphere).
    //   target_id resolves to the sphere handle (valid, not INVALID).
    //   step_handles.last() = INVALID (sentinel from op 1).
    //   After the fix: .filter(|h| *h != GeometryHandleId::INVALID) → None
    //   → compile_geometry_op returns None → another sentinel pushed.
    let draft_op = CompiledGeometryOp::Modify {
        kind: ModifyKind::Draft,
        target: GeomRef::Step(0),
        args: vec![
            // "target" arg (not used for target_id resolution — that comes from `target` field)
            ("target".into(), mm_literal(10.0)),
            // "angle" arg must evaluate to a Value so the `eval_arg("angle")?` succeeds
            ("angle".into(), real_literal(5.0)),
            // "plane" arg is in args but not used for plane resolution (step_handles.last() is used)
            ("plane".into(), real_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "radius", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![sphere_op, bad_bool_op, draft_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_draft_invalid_plane"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // (a) Kernel received exactly 1 execute call — only the Sphere from op0.
    //     Op1 fails compile (not sent to kernel). Op2 fails compile because
    //     plane_id is INVALID (not sent to kernel).
    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "expected only 1 kernel execute call (Sphere), got {}: {:?}",
        ops.len(),
        ops
    );

    // (b) geometry_output is None: all handles are rolled back because had_failure=true.
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None — Draft with INVALID plane must trigger rollback"
    );

    // (c) Exactly 2 compile-failure diagnostics: one for op1 (bad bool refs) and one
    //     for op2 (Draft with INVALID plane).
    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();
    assert_eq!(
        compile_failures.len(),
        2,
        "expected 2 compile-failure diagnostics (op1 bad refs + op2 INVALID plane), got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Tests for sentinel in build_snapshot inline loop (step-2)
// ---------------------------------------------------------------------------

/// When op 1 of 3 fails to compile in build_snapshot's inline loop, a sentinel
/// should be pushed and op 2 should still be attempted. Mirrors
/// sentinel_placeholder_continues_independent_ops (build() path) for the
/// build_snapshot code path.
///
/// Setup: eval() populates the snapshot, then build_snapshot() exercises the
/// inline geometry loop that is separate from tessellate_from_values.
///
/// Op 0: Sphere(radius=10) — succeeds, kernel gets Sphere call.
/// Op 1: Boolean(Union, Step(99), Step(99)) — compile fails (OOB refs).
/// Op 2: Sphere(radius=5) — succeeds because sentinel allows loop to continue.
///
/// Assertions:
/// (a) kernel receives 2 Sphere calls (ops 0 and 2).
/// (b) geometry_output is None — rollback because had_failure=true.
/// (c) exactly 1 compile-failure diagnostic from op 1.
#[test]
fn build_snapshot_sentinel_placeholder_continues_independent_ops() {
    let (module, checker, kernel, ops_ref) =
        make_sentinel_module("test_build_snapshot_sentinel_continues");
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // Populate the snapshot first so build_snapshot returns Some(...).
    // Eval runs constraint solving only, not geometry compilation, so the bad
    // GeomRef indices in op 1 do not produce diagnostics here.
    let eval_result = engine.eval(&module);
    assert!(
        eval_result.diagnostics.is_empty(),
        "eval() produced unexpected diagnostics: {:?}",
        eval_result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    // Isolate kernel ops for the build_snapshot call: clear any ops that might
    // have been accumulated during eval() so the count assertion below is exact.
    ops_ref.lock().unwrap().clear();

    let result = engine
        .build_snapshot(&module, ExportFormat::Step)
        .expect("build_snapshot should return Some after eval()");

    let kernel_ops = ops_ref.lock().unwrap();
    assert_sentinel_invariants(&kernel_ops, &result.diagnostics);

    // Rollback: geometry_output is None when had_failure=true.
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output=None (build_snapshot rollback when any op fails)"
    );
}

// ---------------------------------------------------------------------------
// Tests: zero-ops module (no realizations) — task-1780
// ---------------------------------------------------------------------------

/// Helper: build a zero-ops module (no realizations, total_ops=0) at the
/// given module path and return the BuildResult.
fn build_zero_ops_result(path: &str) -> reify_eval::BuildResult {
    // `build_module_with_ops(path, &[])` produces a template with no
    // realizations — total_ops stays 0.
    let module = build_module_with_ops(path, &[]);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    engine.build(&module, ExportFormat::Step)
}

/// When a module has no geometry operations at all (template has no realizations,
/// so total_ops=0), build() should return geometry_output=None.
///
/// Bug: the previous `else` branch evaluated
/// `step_handles.last().copied().unwrap_or(GeometryHandleId(0))`, producing
/// handle 0 — which MockGeometryKernel happily exported — so build() incorrectly
/// returned Some. After the fix, `step_handles.is_empty()` alone gates the None
/// path, so zero-ops modules cleanly return None without attempting any export.
#[test]
fn build_no_geometry_returns_none_when_zero_ops() {
    let result = build_zero_ops_result("test_zero_ops");

    // No geometry ops → no geometry output (not even a spurious GeometryHandleId(0) export)
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when no geometry ops exist (total_ops=0), \
         got Some({} bytes) — engine incorrectly tried to export GeometryHandleId(0)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );
}

/// When a module has no geometry operations (total_ops=0), build() should NOT
/// emit an 'all geometry operations failed' diagnostic — no ops were attempted,
/// so the absence of geometry is not an error condition.
#[test]
fn build_no_geometry_no_spurious_diagnostic_when_zero_ops() {
    let result = build_zero_ops_result("test_zero_ops_diag");

    // Must NOT emit the 'all geometry operations failed' diagnostic — no ops were attempted
    let has_all_failed = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        !has_all_failed,
        "must NOT emit 'all geometry operations failed' when total_ops=0 \
         (no ops were attempted), but found the diagnostic: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Must also NOT emit 'export error' — no export should be attempted
    let has_export_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("export error"));
    assert!(
        !has_export_error,
        "must NOT emit 'export error' when total_ops=0, but found: {:?}",
        result
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("export error"))
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Regression guard: when total_ops > 0 but all kernel ops fail (step_handles
/// ends up empty because every execute() call returned Err), build() should
/// return geometry_output=None AND emit 'all geometry operations failed'.
///
/// This verifies the diagnostic is still emitted after the refactor that
/// nested the `if total_ops > 0` guard inside the `step_handles.is_empty()`
/// branch — ensuring the diagnostic path was not accidentally removed.
#[test]
fn build_all_ops_fail_diagnostic_emitted_after_refactor() {
    let module = module_with_box_realization(); // total_ops=1
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel; // all execute() calls return Err
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output=None when all ops fail, got Some({} bytes)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    let has_summary = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_summary,
        "expected 'all geometry operations failed' diagnostic when total_ops>0 \
         and all kernel ops fail, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Regression guards: Extrude distance threshold boundary (DEGENERATE_LENGTH_M)
// ---------------------------------------------------------------------------

/// Boundary test: distance=1e-13 is strictly less than DEGENERATE_LENGTH_M=1e-12
/// and must be rejected at compile time with an "extrude dropped" Warning.
/// Pins the strictly-less-than floor semantics so a future refactor to `>` or
/// a different constant can't silently let sub-picometer distances through.
#[test]
fn build_extrude_distance_just_below_threshold_rejected() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let length_literal = |si_value: f64| {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value,
                dimension: reify_core::DimensionVector::LENGTH,
            },
            Type::length(),
        )
    };

    // Op 0: Sphere primitive — provides step_handles[0] as the Extrude's profile handle
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Extrude with distance=1e-13 m — strictly below DEGENERATE_LENGTH_M=1e-12
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![("distance".into(), length_literal(1e-13))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_extrude_below_threshold",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["extrude dropped", "degenerate"],
    );
}

/// Boundary test: distance=1e-12 is exactly DEGENERATE_LENGTH_M, the documented
/// floor (`v.abs() >= DEGENERATE_LENGTH_M`), and must be accepted — the Extrude
/// op should be forwarded to the kernel. Pins the inclusive `>=` boundary
/// semantics so a future refactor to `>` would be caught.
#[test]
fn build_extrude_distance_at_threshold_accepted() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let length_literal = |si_value: f64| {
        reify_ir::CompiledExpr::literal(
            reify_ir::Value::Scalar {
                si_value,
                dimension: reify_core::DimensionVector::LENGTH,
            },
            Type::length(),
        )
    };

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Extrude with distance=1e-12 m — exactly at the floor, must pass
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![("distance".into(), length_literal(1e-12))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extrude_at_threshold"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Sphere + Extrude) when distance is exactly at the floor, got {}",
        ops.len()
    );
    assert!(
        matches!(ops[0].op, reify_ir::GeometryOp::Sphere { .. }),
        "expected ops[0] to be Sphere, got: {:?}",
        ops[0].op
    );
    assert!(
        matches!(ops[1].op, reify_ir::GeometryOp::Extrude { .. }),
        "expected ops[1] to be Extrude (accepted at threshold), got: {:?}",
        ops[1].op
    );

    // No 'extrude dropped' Warning should fire at the inclusive boundary
    let spurious_drop = result.diagnostics.iter().any(|d| {
        d.severity == reify_core::Severity::Warning && d.message.contains("extrude dropped")
    });
    assert!(
        !spurious_drop,
        "expected no 'extrude dropped' Warning at the inclusive floor, got: {:?}",
        result
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("extrude dropped"))
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Regression guards: Revolve angle threshold boundary (DEGENERATE_ANGLE_RAD)
// ---------------------------------------------------------------------------

/// Boundary test: angle=1e-13 rad with a valid z-axis is strictly below
/// DEGENERATE_ANGLE_RAD=1e-12 and must be rejected at compile time.
/// Augments `build_revolve_zero_angle_emits_diagnostic` (which only covers
/// exactly 0.0) by pinning the `|angle| < 1e-12` near-zero boundary.
#[test]
fn build_revolve_angle_just_below_threshold_rejected() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    // Op 1: Revolve with valid z-axis (az=1.0) but angle=1e-13 rad —
    // strictly below DEGENERATE_ANGLE_RAD=1e-12
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(1e-13)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_revolve_angle_below_threshold",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["revolve dropped", "angle", "degenerate"],
    );
}

/// Boundary test: angle=-1e-13 rad must be rejected for the same reason as
/// the positive near-zero case, proving the guard is sign-symmetric via
/// `angle_rad.abs() < DEGENERATE_ANGLE_RAD`. A future refactor dropping the
/// `.abs()` would silently accept small negative angles — this test catches
/// that regression.
#[test]
fn build_revolve_angle_negative_just_below_threshold_rejected() {
    use reify_compiler::SweepKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal =
        |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::Real);

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(50.0))],
    };

    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(-1e-13)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_revolve_angle_negative_below_threshold",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Sphere { .. })),
        &["revolve dropped", "angle", "degenerate"],
    );
}

// ---------------------------------------------------------------------------
// Circular / Mirror pattern missing-arg coverage
// ---------------------------------------------------------------------------

/// Circular pattern dispatch rejects a missing `count` argument at compile
/// time. The Warning message identifies both the arg name and the kind name
/// (lowercase `circular` via the Display impl).
#[test]
fn build_circular_pattern_missing_count_no_kernel_error() {
    use reify_compiler::PatternKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::Real);

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    // Circular pattern with full ox/oy/oz/ax/ay/az/angle but no `count`.
    let circular_op = CompiledGeometryOp::Pattern {
        kind: PatternKind::Circular,
        target: GeomRef::Step(0),
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("angle".into(), real_literal(90.0)),
            // count deliberately omitted
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, circular_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(
        "test_circular_missing_count",
    ))
    .template(template)
    .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Box { .. })),
        &["missing required geometry argument", "count", "circular"],
    );
}

/// Circular pattern dispatch rejects a missing `ax` (first axis-direction
/// component) at compile time. `ax` is read after ox/oy/oz; this exercises
/// the Circular arm after the origin components already resolved.
#[test]
fn build_circular_pattern_missing_axis_no_kernel_error() {
    use reify_compiler::PatternKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::Real);

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    // Circular pattern missing `ax` (axis X-component).
    let circular_op = CompiledGeometryOp::Pattern {
        kind: PatternKind::Circular,
        target: GeomRef::Step(0),
        args: vec![
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            // ax deliberately omitted
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)),
            ("count".into(), real_literal(3.0)),
            ("angle".into(), real_literal(90.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, circular_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_circular_missing_ax"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Box { .. })),
        &["missing required geometry argument", "ax", "circular"],
    );
}

/// Mirror pattern dispatch rejects a missing `ox` (plane origin X) at
/// compile time. First f64 arg in the Mirror arm; exercises the arm's
/// entry.
#[test]
fn build_mirror_pattern_missing_plane_origin_no_kernel_error() {
    use reify_compiler::PatternKind;
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::Real);

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    let mirror_op = CompiledGeometryOp::Pattern {
        kind: PatternKind::Mirror,
        target: GeomRef::Step(0),
        args: vec![
            // ox deliberately omitted
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("nx".into(), real_literal(0.0)),
            ("ny".into(), real_literal(0.0)),
            ("nz".into(), real_literal(1.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, mirror_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_mirror_missing_ox"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    assert_rejected_at_compile(
        &result,
        &ops_ref.lock().unwrap(),
        Some(|op| matches!(op, reify_ir::GeometryOp::Box { .. })),
        &["missing required geometry argument", "ox", "mirror"],
    );
}

// ---------------------------------------------------------------------------
// Modify missing-arg coverage: Shell / Thicken / Draft / Chamfer
// ---------------------------------------------------------------------------

/// Shell modify op rejects a missing `thickness` arg at compile time.
#[test]
fn build_modify_shell_missing_thickness_no_kernel_error() {
    build_modify_missing_arg_case(reify_compiler::ModifyKind::Shell, "thickness", "shell");
}

/// Thicken modify op rejects a missing `offset` arg at compile time.
#[test]
fn build_modify_thicken_missing_offset_no_kernel_error() {
    build_modify_missing_arg_case(reify_compiler::ModifyKind::Thicken, "offset", "thicken");
}

/// Draft modify op rejects a missing `angle` arg at compile time.
#[test]
fn build_modify_draft_missing_angle_no_kernel_error() {
    build_modify_missing_arg_case(reify_compiler::ModifyKind::Draft, "angle", "draft");
}

/// Chamfer modify op rejects a missing `distance` arg at compile time.
#[test]
fn build_modify_chamfer_missing_distance_no_kernel_error() {
    build_modify_missing_arg_case(reify_compiler::ModifyKind::Chamfer, "distance", "chamfer");
}

// ---------------------------------------------------------------------------
// Boolean unresolved-ref coverage: Union / Difference / Intersection
// ---------------------------------------------------------------------------
//
// `compile_geometry_op`'s Boolean arm resolves `left` first, then `right`,
// using `?` so the first Err short-circuits. The resulting Err string is
// turned into a `Diagnostic::error("failed to compile geometry operation:
// {err}")` by `engine_build.rs` — no Warning is emitted on the unresolved-ref
// path (unlike missing-arg, which does emit a Warning from `eval_named_arg*`).
// These tests therefore assert on the Error diagnostic's message directly.

/// Assert the 4-signal unresolved-ref rejection shape for a single failing
/// Boolean op preceded by a Box primitive:
/// 1. Kernel received exactly one recorded op (the Box — Boolean never reached).
/// 2. `result.geometry_output` is None.
/// 3. An Error diagnostic contains `"failed to compile geometry operation"`
///    and every string in `error_needles`.
/// 4. No diagnostic contains `"geometry error"` (kernel was never called
///    for the Boolean op).
fn assert_boolean_unresolved_ref_rejected(
    result: &reify_eval::BuildResult,
    ops: &[reify_test_support::GeometryOpRecord],
    error_needles: &[&str],
) {
    assert_eq!(
        ops.len(),
        1,
        "expected exactly one kernel op (the preceding Box), got {}",
        ops.len()
    );
    assert!(
        matches!(ops[0].op, reify_ir::GeometryOp::Box { .. }),
        "expected the only recorded kernel op to be a Box, got: {:?}",
        ops[0].op
    );

    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when the Boolean op is rejected at compile time"
    );

    let has_compile_error = result.diagnostics.iter().any(|d| {
        d.severity == reify_core::Severity::Error
            && d.message.contains("failed to compile geometry operation")
            && error_needles
                .iter()
                .all(|needle| d.message.contains(needle))
    });
    assert!(
        has_compile_error,
        "expected an Error diagnostic containing 'failed to compile geometry operation' and all needles {:?}; got: {:?}",
        error_needles,
        result
            .diagnostics
            .iter()
            .map(|d| (&d.severity, &d.message))
            .collect::<Vec<_>>()
    );

    let has_kernel_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("geometry error"));
    assert!(
        !has_kernel_error,
        "should NOT have a 'geometry error' diagnostic (kernel was never called for the Boolean op), but got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Drive an engine.build() through [Box, Boolean(op, left, right)] where at
/// least one of `left`/`right` is an out-of-bounds `GeomRef::Step(99)`.
fn run_boolean_unresolved_ref_case(
    op: BooleanOp,
    left: GeomRef,
    right: GeomRef,
    module_path: &str,
) -> (
    reify_eval::BuildResult,
    std::sync::Arc<std::sync::Mutex<Vec<GeometryOpRecord>>>,
) {
    let e = "TestShape";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };
    let boolean_op = CompiledGeometryOp::Boolean { op, left, right };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, boolean_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single(module_path))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);
    (result, ops_ref)
}

/// Union with an out-of-bounds `left` ref (Step(99)) — the first unresolved
/// ref short-circuits compile via `?` before `right` is even consulted.
#[test]
fn build_boolean_union_unresolved_left_no_kernel_error() {
    let (result, ops_ref) = run_boolean_unresolved_ref_case(
        BooleanOp::Union,
        GeomRef::Step(99),
        GeomRef::Step(0),
        "test_boolean_union_unresolved_left",
    );
    assert_boolean_unresolved_ref_rejected(
        &result,
        &ops_ref.lock().unwrap(),
        &["unresolvable GeomRef::Step", "99"],
    );
}

/// Difference with an out-of-bounds `right` ref (Step(99)) — after `left`
/// resolves, resolving `right` returns Err and compile short-circuits.
#[test]
fn build_boolean_difference_unresolved_right_no_kernel_error() {
    let (result, ops_ref) = run_boolean_unresolved_ref_case(
        BooleanOp::Difference,
        GeomRef::Step(0),
        GeomRef::Step(99),
        "test_boolean_difference_unresolved_right",
    );
    assert_boolean_unresolved_ref_rejected(
        &result,
        &ops_ref.lock().unwrap(),
        &["unresolvable GeomRef::Step", "99"],
    );
}

/// Intersection with both refs out-of-bounds — fail-fast invariant: the first
/// unresolved ref (`left`) short-circuits via `?` so only one
/// "unresolvable GeomRef::Step" Error is emitted, not two.
#[test]
fn build_boolean_intersection_unresolved_both_no_kernel_error() {
    let (result, ops_ref) = run_boolean_unresolved_ref_case(
        BooleanOp::Intersection,
        GeomRef::Step(99),
        GeomRef::Step(99),
        "test_boolean_intersection_unresolved_both",
    );
    assert_boolean_unresolved_ref_rejected(
        &result,
        &ops_ref.lock().unwrap(),
        &["unresolvable GeomRef::Step", "99"],
    );

    // Fail-fast: only the first unresolved ref produces an Error — not two.
    let unresolved_count = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Step"))
        .count();
    assert_eq!(
        unresolved_count,
        1,
        "fail-fast invariant: expected exactly one 'unresolvable GeomRef::Step' Error (short-circuit on `left`), got {}: {:?}",
        unresolved_count,
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
