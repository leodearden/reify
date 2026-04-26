//! Worked-example smoke test for field source kinds.
//!
//! Exercises the three user-writeable field source kinds — `analytical`,
//! `sampled`, and `composed` — as defined by §4.1.4 of
//! `docs/reify-language-spec.md` ("Field Declarations").
//!
//! Four-test plan:
//!   1. `composed_stiffness_ri_parses`              — parse only, no errors
//!   2. `composed_stiffness_compiles_with_stdlib`   — compile, three fields present
//!   3. `composed_stiffness_evals_with_three_field_source_kinds` — eval, correct FieldSourceKind per field
//!   4. `composed_stiffness_constraints_all_satisfied` — structure constraints all Satisfied
//!
//! Uses `examples/fields/composed_stiffness.ri` as the fixture file.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{FieldSourceKind, ModulePath, Satisfaction, Severity, Value, FIELD_ENTITY_PREFIX, ValueCellId};

/// Absolute path to the fixture, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/composed_stiffness.ri"
);

/// Read `examples/fields/composed_stiffness.ri` and verify it parses without errors.
#[test]
fn composed_stiffness_ri_parses() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/composed_stiffness.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

/// Compile `examples/fields/composed_stiffness.ri` and verify all three field
/// source kinds are present: `temperature_distribution` (analytical),
/// `material_density` (sampled), and `composed_stiffness` (composed).
#[test]
fn composed_stiffness_compiles_with_stdlib() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/composed_stiffness.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    // Exactly three field defs must be present.
    assert_eq!(
        compiled.fields.len(),
        3,
        "expected 3 fields, got {}: {:?}",
        compiled.fields.len(),
        compiled.fields.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // Verify field names in declaration order.
    let names: Vec<&str> = compiled.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["temperature_distribution", "material_density", "composed_stiffness"],
        "unexpected field names: {:?}",
        names
    );
}

/// Eval the fixture and verify each field's `Value::Field` carries the expected
/// `FieldSourceKind`: Analytical, Sampled, Composed.
#[test]
fn composed_stiffness_evals_with_three_field_source_kinds() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/composed_stiffness.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Each field cell must exist in result.values with the correct FieldSourceKind.
    let expected: &[(&str, FieldSourceKind)] = &[
        ("temperature_distribution", FieldSourceKind::Analytical),
        ("material_density", FieldSourceKind::Sampled),
        ("composed_stiffness", FieldSourceKind::Composed),
    ];

    for (field_name, expected_kind) in expected {
        let cell_id = ValueCellId::new(FIELD_ENTITY_PREFIX, *field_name);
        let val = result
            .values
            .get(&cell_id)
            .unwrap_or_else(|| panic!("no value for field cell {:?}", cell_id));

        match val {
            Value::Field { source, .. } => {
                assert_eq!(
                    source, expected_kind,
                    "field '{}': expected FieldSourceKind::{:?}, got {:?}",
                    field_name, expected_kind, source
                );
            }
            other => panic!(
                "field '{}': expected Value::Field, got: {:?}",
                field_name, other
            ),
        }
    }
}

/// Eval and check `examples/fields/composed_stiffness.ri` and verify all
/// structure constraints (at least 4) are Satisfied.
///
/// The four range constraints come from the `ComposedStiffnessDemo` structure:
///   - `temp_at_p > 399.999` and `temp_at_p < 400.001` (analytical sample at 2.0)
///   - `stiff_at_p > 8.999` and `stiff_at_p < 9.001` (composed sample at 4.0)
#[test]
fn composed_stiffness_constraints_all_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/composed_stiffness.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Check constraints.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 4,
        "expected >=4 constraint results, got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}
