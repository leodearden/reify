//! Tests for graceful handling of geometry build errors.
//!
//! Verifies that Engine::build() produces geometry_output=None and a summary
//! diagnostic when all geometry operations fail, rather than attempting to
//! export with a bogus handle.

use reify_compiler::{BooleanOp, CompiledGeometryOp, GeomRef, PrimitiveKind};
use reify_test_support::*;
use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, QueryError, TessError, Value,
};

// ---------------------------------------------------------------------------
// FailingMockGeometryKernel — execute() always returns Err
// ---------------------------------------------------------------------------

/// A mock geometry kernel whose `execute` always fails.
/// Other methods return Ok with dummy data (intentionally — to demonstrate
/// the current bug where export succeeds with a never-created handle).
struct FailingMockGeometryKernel;

impl GeometryKernel for FailingMockGeometryKernel {
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
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        // Intentionally succeeds — exposes the bug where export runs on a bogus handle
        writer
            .write_all(b"BOGUS_EXPORT")
            .map_err(|e| ExportError::IoError(e.to_string()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Ok(Mesh {
            vertices: vec![],
            indices: vec![],
            normals: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Helper: build a CompiledModule with one box primitive realization
// ---------------------------------------------------------------------------

/// Creates a compiled module with a single structure containing one box
/// primitive realization, so there is exactly one geometry operation to process.
fn module_with_box_realization() -> reify_compiler::CompiledModule {
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(80.0)),
            ("height".into(), mm_literal(100.0)),
            ("depth".into(), mm_literal(5.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new("TestShape")
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .param(e, "height", Type::length(), Some(mm_literal(100.0)))
        .param(e, "depth", Type::length(), Some(mm_literal(5.0)))
        .realization(e, 0, vec![box_op])
        .build();

    CompiledModuleBuilder::new(reify_types::ModulePath::single("test_shape"))
        .template(template)
        .build()
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
    use reify_types::Type;

    // Boolean union referencing Step(0) and Step(1) but no prior primitives,
    // so compile_geometry_op returns None (last_handle is None, resolve_ref fails).
    let union_op = CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),
        right: GeomRef::Step(1),
    };

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    let template = TopologyTemplateBuilder::new("TestShape")
        .param(e, "width", Type::length(), Some(mm_literal(10.0)))
        .realization(e, 0, vec![union_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_compile_fail"))
        .template(template)
        .build();

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
    use reify_types::Type;

    let e = "TestLoft";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_loft"))
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
// Tests: abort realization on first geometry failure
// ---------------------------------------------------------------------------

/// When a realization contains multiple ops that all fail to compile, only the
/// first failure should be reported. The loop should abort after the first
/// compile_geometry_op returns None, preventing cascading diagnostics from
/// downstream ops that reference the missing step handle.
#[test]
fn cascading_compile_failures_aborted_after_first() {
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Three Boolean(Union) ops, all referencing non-existent Step indices.
    // compile_geometry_op returns None for each because step_handles is empty.
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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_cascade"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();

    assert_eq!(
        compile_failures.len(),
        1,
        "expected exactly 1 compile-failure diagnostic (abort after first), got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// When a realization contains multiple ops and kernel.execute fails for all,
/// only the first kernel error diagnostic should be emitted. The loop should
/// abort after the first Err from kernel.execute, preventing cascading errors.
#[test]
fn cascading_kernel_failures_aborted_after_first() {
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_kernel_cascade"))
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
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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
        CompiledModuleBuilder::new(reify_types::ModulePath::single("test_per_realization"))
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
/// also abort on the first compile failure, producing exactly 1 diagnostic and
/// no meshes for the failing realization.
#[test]
fn tessellate_aborts_cascading_compile_failures() {
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Three Boolean(Union) ops, all referencing non-existent Step indices.
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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_tess_cascade"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.tessellate_realizations(&module);

    let compile_failures: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("failed to compile geometry operation"))
        .collect();

    assert_eq!(
        compile_failures.len(),
        1,
        "expected exactly 1 compile-failure diagnostic from tessellate, got {}: {:?}",
        compile_failures.len(),
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    assert!(
        result.meshes.is_empty(),
        "expected no meshes when all ops fail to compile"
    );
}

/// The exact scenario from the task description: a mid-sequence compile failure
/// prevents downstream dependent ops from running. op0 (Box) succeeds, op1
/// (Boolean referencing non-existent steps) fails to compile, and op2 (Fillet
/// on Step(1)) would also fail but should never execute because the loop aborted.
#[test]
fn mixed_failure_then_dependent_ops_aborted() {
    use reify_compiler::ModifyKind;
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box (succeeds)
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

    // Op 2: Fillet on Step(1) — depends on the boolean result, should never run
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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_mixed_abort"))
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

    // (b) exactly 1 compile-failure diagnostic (from op1)
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

    // (c) op2 never ran — no second compile-failure diagnostic
    // (already verified by count == 1 above, but also verify no kernel errors)
    let kernel_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("geometry error"))
        .collect();
    assert_eq!(
        kernel_errors.len(),
        0,
        "expected no geometry error diagnostics (op2 should not have run)"
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
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_partial_tess"))
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

/// When a realization has [Box (succeeds), Boolean union with bad refs (compile
/// failure)], build() should return geometry_output=None — the partial success
/// should not leak the intermediate Box handle into the export.
///
/// BUG: Before the fix, `step_handles.last()` returns the intermediate Box
/// handle so the partially-complete geometry gets exported.
#[test]
fn partial_failure_build_produces_no_geometry() {
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_partial_build"))
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
    use reify_types::Type;

    let e = "TestShape";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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
        CompiledModuleBuilder::new(reify_types::ModulePath::single("test_partial_contaminate"))
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
