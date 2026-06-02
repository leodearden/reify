//! E2E integration tests for occurrence evaluation.
//!
//! Tests the complete pipeline: parse → compile → Engine.eval() → check values.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_test_support::mocks::MockConstraintChecker;

/// Parse and compile an occurrence, eval via Engine, and verify
/// that value cells are evaluated correctly.
#[test]
fn e2e_occurrence_eval_basic() {
    let source = r#"
occurrence def Welding {
    param speed : Length = 50mm
    let double_speed = speed * 2
    constraint speed > 0mm
}
"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Verify entity kind
    assert_eq!(compiled.templates.len(), 1);
    assert_eq!(
        compiled.templates[0].entity_kind,
        reify_compiler::EntityKind::Occurrence
    );

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check speed = 50mm = 0.05 SI (metres)
    let speed_id = ValueCellId::new("Welding", "speed");
    let speed_val = result
        .values
        .get(&speed_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", speed_id));
    match speed_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected 0.05 SI, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for speed, got {:?}", other),
    }

    // Check double_speed = speed * 2 = 0.1 SI
    let ds_id = ValueCellId::new("Welding", "double_speed");
    let ds_val = result
        .values
        .get(&ds_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", ds_id));
    match ds_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "expected 0.1 SI, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for double_speed, got {:?}", other),
    }
}
