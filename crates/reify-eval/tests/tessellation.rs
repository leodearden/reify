//! Tests for Engine::tessellate_realizations() — tessellation API for GUI mesh generation.

use reify_test_support::*;
use reify_types::ModulePath;

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
    assert!(result.meshes.is_empty(), "expected no meshes when no realizations exist");

    // Values should still be populated from eval
    assert!(!result.values.is_empty(), "expected values to be populated from eval");
}
