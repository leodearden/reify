//! Tests for Engine::tessellate_realizations() — tessellation API for GUI mesh generation.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_test_support::*;
use reify_types::{
    ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel,
    GeometryOp, GeometryQuery, Mesh, ModulePath, QueryError, TessError, Value,
};

/// When the module has no realizations and no geometry kernel,
/// tessellate_realizations() should return empty meshes and populated values.
#[test]
fn tessellate_no_realizations_no_kernel_returns_empty_meshes() {
    use reify_types::Type;

    let e = "SimpleParam";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Module with a param but no realizations
    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_literal(42.0)))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_no_realization"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    let result = engine.tessellate_realizations(&module);

    // Should have empty meshes
    assert!(
        result.meshes.is_empty(),
        "expected no meshes when no realizations exist"
    );

    // Values should still be populated from eval
    assert!(
        !result.values.is_empty(),
        "expected values to be populated from eval"
    );
}

/// Helper: build a CompiledModule with one box primitive realization.
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

    CompiledModuleBuilder::new(ModulePath::single("test_shape"))
        .template(template)
        .build()
}

/// tessellate_realizations returns a mesh for a single box realization.
#[test]
fn tessellate_single_box_realization() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.tessellate_realizations(&module);

    assert_eq!(
        result.meshes.len(),
        1,
        "expected exactly one mesh for one realization"
    );

    let (entity_path, mesh) = &result.meshes[0];
    assert_eq!(entity_path, "TestShape#realization[0]");
    assert!(
        !mesh.vertices.is_empty(),
        "mesh should have non-empty vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "mesh should have non-empty indices"
    );
}

/// tessellate_realizations with two realizations returns two meshes with distinct entity paths.
#[test]
fn tessellate_multiple_realizations() {
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

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(10.0))],
    };

    let template = TopologyTemplateBuilder::new("TestShape")
        .param(e, "width", Type::length(), Some(mm_literal(80.0)))
        .realization(e, 0, vec![box_op])
        .realization(e, 1, vec![sphere_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_multi"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.tessellate_realizations(&module);

    assert_eq!(
        result.meshes.len(),
        2,
        "expected two meshes for two realizations"
    );

    let paths: Vec<&str> = result.meshes.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths[0], "TestShape#realization[0]");
    assert_eq!(paths[1], "TestShape#realization[1]");
}

/// tessellate_realizations returns empty meshes (no panic, no error) when
/// geometry_kernel is None but module has realizations.
#[test]
fn tessellate_no_kernel_with_realizations_returns_empty_meshes() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    // No geometry kernel
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    let result = engine.tessellate_realizations(&module);

    assert!(
        result.meshes.is_empty(),
        "expected no meshes when kernel is absent"
    );

    // No tessellation-related error diagnostics
    let has_tess_diag = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("tessellation") || d.message.contains("geometry error"));
    assert!(
        !has_tess_diag,
        "expected no tessellation diagnostics when kernel absent, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// FailingMockGeometryKernel — execute() always returns Err
// ---------------------------------------------------------------------------

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
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Ok(())
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed("should not reach".into()))
    }
}

/// tessellate_realizations records geometry execution errors as diagnostics
/// when kernel operations fail.
#[test]
fn tessellate_records_geometry_errors_as_diagnostics() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.tessellate_realizations(&module);

    // No meshes should be produced
    assert!(
        result.meshes.is_empty(),
        "expected no meshes when all kernel ops fail"
    );

    // Should have geometry error diagnostics
    let has_geom_error = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("geometry error"));
    assert!(
        has_geom_error,
        "expected geometry error diagnostic, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// tessellate_snapshot returns None when no prior eval() has been called.
#[test]
fn tessellate_snapshot_returns_none_without_prior_eval() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let result = engine.tessellate_snapshot(&module);
    assert!(
        result.is_none(),
        "expected None when no eval() has been called"
    );
}

/// tessellate_snapshot returns tessellated meshes from the current snapshot after eval().
#[test]
fn tessellate_snapshot_returns_meshes_after_eval() {
    let module = module_with_box_realization();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // Initial eval to populate snapshot
    let _eval_result = engine.eval(&module);

    let result = engine
        .tessellate_snapshot(&module)
        .expect("tessellate_snapshot should return Some after eval()");

    assert_eq!(
        result.meshes.len(),
        1,
        "expected one mesh from one realization"
    );
    let (entity_path, mesh) = &result.meshes[0];
    assert_eq!(entity_path, "TestShape#realization[0]");
    assert!(
        !mesh.vertices.is_empty(),
        "mesh should have non-empty vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "mesh should have non-empty indices"
    );
}
