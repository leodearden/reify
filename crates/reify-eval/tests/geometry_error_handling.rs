//! Tests for graceful handling of geometry build errors.
//!
//! Verifies that Engine::build() produces geometry_output=None and a summary
//! diagnostic when all geometry operations fail, rather than attempting to
//! export with a bogus handle.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
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
    let mm_literal =
        |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

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
    let mut engine =
        reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Should have no geometry output when all ops fail
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when all kernel ops fail, got Some({} bytes)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    // Should contain a summary diagnostic about all ops failing
    let has_summary = result.diagnostics.iter().any(|d| {
        d.message.contains("all geometry operations failed")
    });
    assert!(
        has_summary,
        "expected a summary diagnostic about all geometry operations failing, got: {:?}",
        result.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
