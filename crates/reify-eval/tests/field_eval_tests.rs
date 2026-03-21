//! Field declaration evaluation tests.
//!
//! Tests for evaluating `field def` declarations into Value::Field values
//! and applying field operations (sample, gradient, etc.).

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity, Value, ValueCellId};

/// Helper: parse, compile, and eval source, return eval result.
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("field_eval_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
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
    let result = eval_source(
        "field def temp : Point3 -> Scalar { source = analytical { |p| p } }",
    );

    // The field should be stored in the values map
    let field_id = ValueCellId::new("__field", "temp");
    let field_val = result
        .values
        .get(&field_id)
        .unwrap_or_else(|| panic!("field 'temp' not found in eval result values"));

    // Should be a Value::Field with correct types
    match field_val {
        Value::Field { domain_type, codomain_type, source, lambda } => {
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
        }
        other => panic!("expected Value::Field, got: {:?}", other),
    }
}
