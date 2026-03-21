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

// ── Step 17: compose type check valid ───────────────────────────────

#[test]
fn compile_field_compose_type_check_valid() {
    // Field<Point3, Scalar> composed with Field<Scalar, Scalar> is valid:
    // codomain of first (Scalar) matches domain of second (Scalar).
    // Result should be Field<Point3, Scalar>.
    let module = compile_module(
        r#"
field def f1 : Point3 -> Scalar { source = analytical { |p| p } }
field def f2 : Scalar -> Scalar { source = analytical { |x| x } }
field def composed : Point3 -> Scalar { source = composed { |p| f2(f1(p)) } }
"#,
    );
    // Should compile without type errors
    assert!(module.diagnostics.is_empty(), "diagnostics: {:?}", module.diagnostics);
    assert_eq!(module.fields.len(), 3, "expected 3 compiled fields");

    let composed = &module.fields[2];
    assert_eq!(composed.name, "composed");
    assert_eq!(format!("{}", composed.domain_type), "Point3");
    assert_eq!(format!("{}", composed.codomain_type), "Scalar[m]");

    match &composed.source {
        reify_compiler::CompiledFieldSource::Composed { expr } => {
            // Should have compiled the composition lambda
            assert!(
                matches!(expr.kind, reify_types::CompiledExprKind::Lambda { .. }),
                "expected Lambda expression in composed source, got: {:?}",
                expr.kind
            );
        }
        other => panic!("expected Composed source, got: {:?}", other),
    }
}

// ── Step 19: compose type mismatch ──────────────────────────────────

#[test]
fn compile_field_compose_type_mismatch() {
    // Field<Point3, Vector3> composed with Field<Scalar, Scalar> is INVALID:
    // codomain of first (Vector3) != domain of second (Scalar).
    // Should produce a type error diagnostic.
    let module = compile_module(
        r#"
field def f1 : Point3 -> Vector3 { source = analytical { |p| p } }
field def f2 : Scalar -> Scalar { source = analytical { |x| x } }
field def bad_compose : Point3 -> Scalar { source = composed { |p| f2(f1(p)) } }
"#,
    );
    // Should have at least one diagnostic about field composition type mismatch
    assert!(
        !module.diagnostics.is_empty(),
        "expected a type mismatch diagnostic for mismatched field composition"
    );
    let has_mismatch_error = module.diagnostics.iter().any(|d| {
        d.message.contains("mismatch") || d.message.contains("compose") || d.message.contains("field")
    });
    assert!(
        has_mismatch_error,
        "expected field composition type mismatch diagnostic, got: {:?}",
        module.diagnostics
    );
}

// ── Step 29: compose type check nested in match ─────────────────────────

#[test]
fn compose_type_check_nested_in_match() {
    // Field composition mismatch nested inside a match arm body.
    // The current walk_field_composition misses Match variants;
    // after rewriting to use CompiledExpr::walk, it will be caught.
    let module = compile_module(
        r#"
enum Mode { A B }

field def f1 : Point3 -> Vector3 { source = analytical { |p| p } }
field def f2 : Scalar -> Scalar { source = analytical { |x| x } }
field def bad_nested : Point3 -> Scalar {
    source = composed { |p| match Mode.A { A => f2(f1(p)) B => f2(f1(p)) } }
}
"#,
    );
    // Should detect the type mismatch even though it's inside a match arm
    let has_mismatch_error = module.diagnostics.iter().any(|d| {
        d.message.contains("mismatch") || d.message.contains("compose") || d.message.contains("field")
    });
    assert!(
        has_mismatch_error,
        "expected field composition type mismatch diagnostic inside match arm, got: {:?}",
        module.diagnostics
    );
}

// ── Step 33: duplicate field names ───────────────────────────────────────

#[test]
fn compile_duplicate_field_names() {
    let module = compile_module(
        r#"
field def temp : Point3 -> Scalar { source = analytical { |p| p } }
field def temp : Scalar -> Scalar { source = analytical { |x| x } }
"#,
    );
    // Should emit a diagnostic about duplicate field name
    let has_dup_error = module.diagnostics.iter().any(|d| {
        d.message.contains("duplicate field name")
    });
    assert!(
        has_dup_error,
        "expected 'duplicate field name' diagnostic, got: {:?}",
        module.diagnostics
    );
    // Should only compile the first field (duplicate skipped)
    assert_eq!(
        module.fields.len(),
        1,
        "expected only 1 compiled field (duplicate should be skipped)"
    );
}
