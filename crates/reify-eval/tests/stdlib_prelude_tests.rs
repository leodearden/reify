//! Tests for stdlib prelude integration with the eval Engine.

use reify_compiler::stdlib_loader;
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity};

// ─── step-7: Engine stores prelude ──────────────────────────────────

/// Engine::new() stores a non-empty prelude from stdlib_loader.
#[test]
fn engine_has_non_empty_prelude() {
    let checker = MockConstraintChecker::new();
    let engine = reify_eval::Engine::new(Box::new(checker), None);
    assert!(
        !engine.prelude().is_empty(),
        "Engine prelude should be non-empty after new()"
    );
}

/// eval() with a user module compiled via compile_with_prelude works for
/// a structure conforming to a prelude trait — values are populated and
/// no error diagnostics.
#[test]
fn eval_with_prelude_trait_conformance() {
    let source = r#"
structure def Steel : Elastic {
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Should have value cells from the Steel structure
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for Steel structure"
    );

    // No error diagnostics from eval
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );
}

// ─── step-9: End-to-end prelude pipeline ─────────────────────────────

/// Full pipeline: .ri source → compile_with_prelude → Engine::eval.
/// User code conforms to both Material and Elastic prelude traits.
/// Asserts: (1) no compile errors, (2) eval returns values for all 5 params,
/// (3) trait_bounds on template include both Material and Elastic.
#[test]
fn end_to_end_material_elastic_conformance() {
    let source = r#"
structure def Steel : Material + Elastic {
    param density : Real = 7800.0
    param name : String = "A36"
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    // (1) No error diagnostics from compilation
    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile should produce no error diagnostics, got: {:?}",
        compile_errors
    );

    // (3) trait_bounds on template include both Material and Elastic
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Steel")
        .expect("Steel template should exist in compiled module");
    assert!(
        template.trait_bounds.contains(&"Material".to_string()),
        "Steel should have 'Material' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "Steel should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );

    // (2) eval returns values for all 5 params
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // Check that all 5 params have values in the result
    let entity = "Steel";
    let expected_params = ["density", "name", "youngs_modulus", "poissons_ratio", "shear_modulus"];
    for param in &expected_params {
        let cell_id = reify_types::ValueCellId::new(entity, *param);
        assert!(
            result.values.get(&cell_id).is_some(),
            "eval should produce a value for param '{}', but it was missing. \
             Available values: {:?}",
            param,
            result.values.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>()
        );
    }
}
