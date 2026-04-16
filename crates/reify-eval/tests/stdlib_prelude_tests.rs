//! Tests for stdlib prelude integration with the eval Engine.

use reify_compiler::stdlib_loader;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{collect_errors, steel_elastic_source, steel_material_elastic_source};
use reify_types::{ModulePath, ValueCellId};

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
/// no error diagnostics. Verifies the 3 specific Elastic params
/// (youngs_modulus, poissons_ratio, shear_modulus) are present.
#[test]
fn eval_with_prelude_trait_conformance() {
    let source = steel_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No error diagnostics from eval
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // Verify all 3 Elastic params are present with correct values
    let entity = "Steel";
    let expected_params: &[(&str, f64)] = &[
        ("youngs_modulus", 200.0),
        ("poissons_ratio", 0.3),
        ("shear_modulus", 77.0),
    ];
    for (param, expected_val) in expected_params {
        let cell_id = ValueCellId::new(entity, *param);
        let value = result.values.get(&cell_id).unwrap_or_else(|| {
            panic!(
                "eval should produce a value for Elastic param '{}', but it was missing. \
                 Available values: {:?}",
                param,
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
        let actual = value.as_f64().unwrap_or_else(|| {
            panic!(
                "Elastic param '{}' should be numeric, got: {:?}",
                param, value
            )
        });
        assert!(
            (actual - expected_val).abs() < 1e-9,
            "Elastic param '{}' should be {}, got {}",
            param,
            expected_val,
            actual
        );
    }
}

// ─── step-1: Shadowing regression ────────────────────────────────────

/// Regression guard: user-defined functions shadow prelude functions with
/// identical signatures. A user-defined `symmetric_tolerance` that returns
/// `nominal - deviation` (subtraction) must win over the prelude's
/// `nominal + deviation` (addition) implementation.
///
/// With `5mm, 2mm`:
///   - user impl → 3mm = 0.003 m  (expected)
///   - prelude impl → 7mm = 0.007 m  (would indicate shadowing is broken)
#[test]
fn user_function_shadows_prelude_function() {
    let source = r#"
fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {
    nominal - deviation
}

structure S {
    let v : Length = symmetric_tolerance(5mm, 2mm)
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    let cell_id = ValueCellId::new("S", "v");
    let value = result.values.get(&cell_id).unwrap_or_else(|| {
        panic!(
            "eval should produce a value for S.v, but it was missing. \
             Available values: {:?}",
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });
    let actual = value.as_f64().unwrap_or_else(|| {
        panic!("S.v should be numeric, got: {:?}", value)
    });
    // User impl: 5mm - 2mm = 3mm = 0.003 m
    // Prelude impl (if shadowing were broken): 5mm + 2mm = 7mm = 0.007 m
    assert!(
        (actual - 0.003).abs() < 1e-9,
        "user function should shadow prelude: expected 0.003 (3mm), got {} (prelude would give 0.007)",
        actual
    );
}

// ─── step-3: Eval idempotency (caching regression) ───────────────────

/// Regression guard: calling `eval()` twice on the same engine with the
/// same module must produce identical results. This guards against a
/// regression where `self.functions` could accumulate prelude functions
/// across calls (e.g., if `eval()` appended instead of replacing).
///
/// A user-defined `symmetric_tolerance` with the same signature as the
/// prelude function (addition body) is used. The prelude version would
/// also return 7mm, so any ordering shift from accumulation would be
/// visible if it changed the number of functions found or their types.
/// Both calls must return 0.007 m (7mm = 5mm + 2mm).
#[test]
fn eval_is_idempotent_for_prelude_functions() {
    let source = r#"
fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {
    nominal + deviation
}

structure S {
    let v : Length = symmetric_tolerance(5mm, 2mm)
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval
    let result1 = engine.eval(&compiled);
    let eval_errors1 = collect_errors(&result1.diagnostics);
    assert!(
        eval_errors1.is_empty(),
        "first eval: no error diagnostics expected, got: {:?}",
        eval_errors1
    );
    let cell_id = ValueCellId::new("S", "v");
    let v1 = result1
        .values
        .get(&cell_id)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("S.v missing or non-numeric on first eval"));

    // Second eval on same engine
    let result2 = engine.eval(&compiled);
    let eval_errors2 = collect_errors(&result2.diagnostics);
    assert!(
        eval_errors2.is_empty(),
        "second eval: no error diagnostics expected, got: {:?}",
        eval_errors2
    );
    let v2 = result2
        .values
        .get(&cell_id)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("S.v missing or non-numeric on second eval"));

    assert!(
        (v1 - v2).abs() < 1e-9,
        "eval() must be idempotent: first={} second={} (differ by {})",
        v1,
        v2,
        (v1 - v2).abs()
    );
    // User impl (addition, matching prelude): 5mm + 2mm = 7mm = 0.007 m
    assert!(
        (v1 - 0.007).abs() < 1e-9,
        "symmetric_tolerance(5mm, 2mm) should be 0.007 m (7mm), got {}",
        v1
    );
}

// ─── step-9: End-to-end prelude pipeline ─────────────────────────────

/// Full pipeline: .ri source → compile_with_prelude → Engine::eval.
/// User code conforms to both Material and Elastic prelude traits.
/// Asserts: (1) no compile errors, (2) eval returns values for all 5 params,
/// (3) trait_bounds on template include both Material and Elastic.
#[test]
fn end_to_end_material_elastic_conformance() {
    let source = steel_material_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // (1) No error diagnostics from compilation
    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let compile_errors = collect_errors(&compiled.diagnostics);
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

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // Check that all 5 params have values in the result
    let entity = "Steel";
    let expected_params = [
        "density",
        "name",
        "youngs_modulus",
        "poissons_ratio",
        "shear_modulus",
    ];
    for param in &expected_params {
        let cell_id = reify_types::ValueCellId::new(entity, *param);
        assert!(
            result.values.get(&cell_id).is_some(),
            "eval should produce a value for param '{}', but it was missing. \
             Available values: {:?}",
            param,
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        );
    }
}
