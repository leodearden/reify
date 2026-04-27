//! Field declaration compilation tests.
//!
//! Tests for compiling `field def` declarations into CompiledField entries.

use reify_test_support::{compile_source, errors_only};
use reify_types::DiagnosticCode;

// ── Step 13: compile analytical field ──────────────────────────────────

#[test]
fn compile_field_analytical() {
    let module = compile_source(
        "field def temp : Point3 -> Scalar { source = analytical { |p| 1.0m } }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    let module = compile_source(
        "field def pressure : Point3 -> Scalar { source = sampled { resolution = 100 interpolation = linear } }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    let module = compile_source(
        r#"
field def f1 : Point3 -> Scalar { source = analytical { |p| 1.0m } }
field def f2 : Scalar -> Scalar { source = analytical { |x| 1.0m } }
field def composed : Point3 -> Scalar { source = composed { |p| f2(f1(p)) } }
"#,
    );
    // Should compile without type errors (warnings for StructureRef types are OK)
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
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
    let module = compile_source(
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
        d.message.contains("mismatch")
            || d.message.contains("compose")
            || d.message.contains("field")
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
    let module = compile_source(
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
        d.message.contains("mismatch")
            || d.message.contains("compose")
            || d.message.contains("field")
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
    let module = compile_source(
        r#"
field def temp : Point3 -> Scalar { source = analytical { |p| p } }
field def temp : Scalar -> Scalar { source = analytical { |x| x } }
"#,
    );
    // Should emit a diagnostic about duplicate entity definition (covers field-vs-field collision
    // now that fields participate in the unified entity namespace per spec §4.2.1).
    let has_dup_error = module
        .diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate entity definition") && d.message.contains("temp"));
    assert!(
        has_dup_error,
        "expected 'duplicate entity definition' diagnostic for 'temp', got: {:?}",
        module.diagnostics
    );
    // Should only compile the first field (duplicate skipped)
    assert_eq!(
        module.fields.len(),
        1,
        "expected only 1 compiled field (duplicate should be skipped)"
    );
}

// ── Step 2336: analytical field codomain type-check ─────────────────────────

#[test]
fn compile_field_analytical_codomain_dimension_mismatch_emits_diagnostic() {
    // Body returns Real (param x has default Real type), codomain declared as Scalar[m].
    // implicitly_converts_to(Real, Scalar[LENGTH]) is false → FieldCodomainMismatch.
    let module =
        compile_source("field def temp : Real -> Scalar { source = analytical { |x| x } }");

    let has_mismatch = module.diagnostics.iter().any(|d| {
        d.severity == reify_types::Severity::Error
            && d.code == Some(DiagnosticCode::FieldCodomainMismatch)
    });
    assert!(
        has_mismatch,
        "expected DiagnosticCode::FieldCodomainMismatch error for codomain mismatch, got: {:?}",
        module.diagnostics
    );

    // The diagnostic message should use the canonical phrasing, naming both sides.
    // Checking the full phrase rather than bare type names avoids false positives
    // from substrings like "Vector<Real>", "Scalar<Temperature>", etc.
    //
    // `Scalar` in the source resolves to Type::length() (type_resolution.rs:374-376),
    // which Displays as `Scalar[m]` (ty.rs:296-302 + dimension.rs:308-327: the LENGTH
    // basis dimension emits "m"). Pinning the exact `Scalar[m]` rendering ensures a
    // future change to `Scalar`'s default dimension (e.g. switching to dimensionless
    // or to a different SI base) causes this assertion to fail loudly rather than
    // silently passing with a changed rendering.
    let mismatch_diag = module
        .diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch))
        .unwrap();
    assert!(
        mismatch_diag.message.contains("declared codomain `Scalar[m]`"),
        "expected message to contain 'declared codomain `Scalar[m]`', got: {}",
        mismatch_diag.message
    );
    assert!(
        mismatch_diag.message.contains("lambda body produces `Real`"),
        "expected message to contain 'lambda body produces `Real`', got: {}",
        mismatch_diag.message
    );
}

// ── Step 2336: positive-path guard — matching codomain does not emit mismatch ─

#[test]
fn compile_field_analytical_matching_codomain_does_not_emit_mismatch() {
    // Body returns Real (2.5 * x + 1.0), codomain declared as Real — types match.
    // No FieldCodomainMismatch diagnostic should be emitted.
    let module = compile_source(
        "field def linear : Real -> Real { source = analytical { |x| 2.5 * x + 1.0 } }",
    );

    let has_mismatch = module
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch));
    assert!(
        !has_mismatch,
        "expected NO FieldCodomainMismatch for Real->Real field with Real body, got: {:?}",
        module.diagnostics
    );
}

// ── Task 2414 step-1: pin Int→Real widening arm in field_codomain_compatible ──

#[test]
fn compile_field_analytical_int_body_widens_to_real_codomain() {
    // Body literal `1` is typed as Type::Int (expr.rs:257-258): whole-number
    // literals without a unit suffix always produce Int, not Real.
    // Codomain is Real (Type::Real).
    //
    // implicitly_converts_to(Int, Real) returns false (type_compat.rs:52-169:
    // identity check fails because Int != Real; none of rules 1a/1b/2a/2b/2c/3
    // match the Int→Real direction; the default arm returns false).
    //
    // The dedicated `(Type::Int, Type::Real)` arm at functions.rs:170-171 is
    // the *only* thing that keeps this source valid. Removing that arm would
    // cause field_codomain_compatible to return false and emit
    // DiagnosticCode::FieldCodomainMismatch, making this test fail.
    let module = compile_source(
        "field def f : Real -> Real { source = analytical { |x| 1 } }",
    );

    let has_mismatch = module
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::FieldCodomainMismatch));
    assert!(
        !has_mismatch,
        "expected NO FieldCodomainMismatch for Real->Real field with Int literal body \
         (Int→Real widening must hold), got: {:?}",
        module.diagnostics
    );
}

// ── Step 2344: imported field emits v0.2 deferral diagnostic ────────────────

#[test]
fn compile_field_imported_emits_v02_deferral_diagnostic() {
    let module = compile_source(
        r#"field def data : Point3 -> Scalar { source = imported { "data.vtu" } }"#,
    );

    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected at least one error for imported field source, got: {:?}",
        module.diagnostics
    );

    let has_code_and_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::FieldImportedV02)
            && d.message.contains("v0.2")
            && d.message.contains("imported")
    });
    assert!(
        has_code_and_msg,
        "expected DiagnosticCode::FieldImportedV02 with message containing 'v0.2' and 'imported', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::FieldImportedV02))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}
