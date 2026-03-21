//! Field declaration compilation tests.
//!
//! Tests for compiling `field def` declarations into CompiledField entries.

/// Helper: parse and compile source, return compiled module.
fn compile_module(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("field_compile_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

// ── Step 13: compile analytical field ──────────────────────────────────

#[test]
fn compile_field_analytical() {
    let module = compile_module(
        "field def temp : Point3 -> Scalar { source = analytical { |p| p } }",
    );
    assert!(module.diagnostics.is_empty(), "diagnostics: {:?}", module.diagnostics);
    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");

    let field = &module.fields[0];
    assert_eq!(field.name, "temp");
    assert!(!field.is_pub);

    // Domain and codomain types should be resolved
    // Point3 is not a built-in type, so it resolves to StructureRef
    assert_eq!(format!("{}", field.domain_type), "Point3");
    // Scalar resolves to Type::length() which displays as "Scalar[m]"
    assert_eq!(format!("{}", field.codomain_type), "Scalar[m]");

    // Source should be analytical with a compiled lambda expression
    match &field.source {
        reify_compiler::CompiledFieldSource::Analytical { expr } => {
            // The expression should be a lambda
            assert!(
                matches!(expr.kind, reify_types::CompiledExprKind::Lambda { .. }),
                "expected Lambda expression in analytical source, got: {:?}",
                expr.kind
            );
        }
        other => panic!("expected Analytical source, got: {:?}", other),
    }
}

// ── Step 15: compile sampled field ──────────────────────────────────

#[test]
fn compile_field_sampled() {
    let module = compile_module(
        "field def pressure : Point3 -> Scalar { source = sampled { resolution = 100 interpolation = linear } }",
    );
    assert!(module.diagnostics.is_empty(), "diagnostics: {:?}", module.diagnostics);
    assert_eq!(module.fields.len(), 1, "expected 1 compiled field");

    let field = &module.fields[0];
    assert_eq!(field.name, "pressure");

    // Source should be sampled with config key-value pairs
    match &field.source {
        reify_compiler::CompiledFieldSource::Sampled { config } => {
            assert_eq!(config.len(), 2, "expected 2 config entries");
            assert_eq!(config[0].0, "resolution");
            assert_eq!(config[1].0, "interpolation");
        }
        other => panic!("expected Sampled source, got: {:?}", other),
    }
}
