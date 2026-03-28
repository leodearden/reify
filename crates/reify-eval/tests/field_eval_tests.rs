//! Field declaration evaluation tests.
//!
//! Tests for evaluating `field def` declarations into Value::Field values
//! and applying field operations (sample, gradient, etc.).

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{FIELD_ENTITY_PREFIX, ModulePath, Severity, Value, ValueCellId};

/// Helper: parse, compile, and eval source, return eval result.
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("field_eval_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ── Step 21: eval analytical field at point ────────────────────────────

#[test]
fn eval_analytical_field_at_point() {
    let result = eval_source("field def temp : Point3 -> Scalar { source = analytical { |p| p } }");

    // The field should be stored in the values map
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");
    let field_val = result
        .values
        .get(&field_id)
        .unwrap_or_else(|| panic!("field 'temp' not found in eval result values"));

    // Should be a Value::Field with correct types
    match field_val {
        Value::Field {
            domain_type,
            codomain_type,
            source,
            lambda,
            inner_field,
        } => {
            // Domain should be Point3 (StructureRef)
            assert_eq!(format!("{}", domain_type), "Point3");
            // Codomain should be Scalar[m] (length-dimensioned)
            assert_eq!(format!("{}", codomain_type), "Scalar[m]");
            // Source should be Analytical
            assert!(
                matches!(source, reify_types::FieldSourceKind::Analytical),
                "expected Analytical source, got: {:?}",
                source
            );
            // Lambda should be a Lambda value (not Undef)
            assert!(
                matches!(**lambda, Value::Lambda { .. }),
                "expected Lambda value in analytical field, got: {:?}",
                lambda
            );
            // Non-gradient fields should have inner_field = None
            assert!(
                inner_field.is_none(),
                "analytical field should have inner_field = None, got: {:?}",
                inner_field
            );
        }
        other => panic!("expected Value::Field, got: {:?}", other),
    }
}

// ── Step 23: eval sample(field, point) ─────────────────────────────

#[test]
fn eval_sample_field_point() {
    // Define a field and a structure that uses sample() to query it at a point.
    // The analytical field is `|p| p` (identity), so sample(field, 42) should return 42.
    let result = eval_source(
        r#"
field def identity_field : Scalar -> Scalar { source = analytical { |p| p } }

structure S {
    let val = sample(identity_field, 42)
}
"#,
    );

    let val_id = ValueCellId::new("S", "val");
    let val = result
        .values
        .get(&val_id)
        .unwrap_or_else(|| panic!("'val' not found in eval result"));

    // sample(identity_field, 42) should evaluate the lambda |p| p with p=42, returning 42
    match val {
        Value::Int(n) => assert_eq!(*n, 42, "expected 42, got {}", n),
        Value::Real(v) => assert!((v - 42.0).abs() < 1e-12, "expected 42.0, got {}", v),
        other => panic!("expected Int(42) or Real(42.0), got: {:?}", other),
    }
}

// ── Step 27: FIELD_ENTITY_PREFIX constant ──────────────────────────────

#[test]
fn field_entity_prefix_constant() {
    // Verify the constant exists and has the expected value
    assert_eq!(FIELD_ENTITY_PREFIX, "__field");

    // Verify it can be used to construct a ValueCellId matching the field pattern
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");
    assert_eq!(field_id.entity, "__field");
    assert_eq!(field_id.member, "temp");
    assert_eq!(format!("{}", field_id), "__field.temp");
}

// ── Step 31: eval field snapshot consistency ─────────────────────────────

#[test]
fn eval_field_snapshot_consistency() {
    // Evaluate a module with a field and verify the field value appears
    // in snapshot.values (not just the cold values map).
    // This ensures incremental re-evaluation via edit_param/warm-starting
    // can see field values.
    let source = "field def temp : Point3 -> Scalar { source = analytical { |p| p } }";
    let parsed = reify_syntax::parse(source, ModulePath::single("field_snapshot_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _result = engine.eval(&compiled);

    // The field should be in the snapshot values
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, "temp");

    let snapshot_entry = snapshot.values.get(&field_id);
    assert!(
        snapshot_entry.is_some(),
        "field 'temp' not found in snapshot.values — field values must be inserted \
         into the snapshot for incremental re-evaluation to work"
    );

    let (val, det) = snapshot_entry.unwrap();
    // Should be a Value::Field
    assert!(
        matches!(val, Value::Field { .. }),
        "expected Value::Field in snapshot, got: {:?}",
        val
    );
    // Should be Determined
    assert_eq!(
        *det,
        reify_types::DeterminacyState::Determined,
        "field snapshot value should be Determined"
    );
}
