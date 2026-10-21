//! Stress tests for geometry pattern operations via pattern_composition.ri fixture.
//!
//! Covers:
//!   - smoke test: fixture parses, compiles, evaluates without errors (non-empty values)
//!   - expected templates: at least 7 structures all present in compiled output
//!   - count_zero: degenerate linear_pattern_2d with count=0 compiles without ICE
//!   - count_one: single-instance pattern compiles and evals
//!   - 10x10 grid: large pattern compiles with Pattern2D kind realization
//!   - composed patterns: arbitrary_pattern in same structure as linear_pattern_2d
//!   - boolean fold: union_all over multiple box() primitives

use std::fs;

use reify_compiler::PatternKind;
use reify_test_support::{assert_no_eval_errors, compile_source_named, errors_only, make_engine};

/// Absolute path to the fixture file.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/pattern_composition.ri"
);

// ── Helper ────────────────────────────────────────────────────────────────────

/// Load a .ri file, parse, compile (asserting no errors), and evaluate.
/// Returns the full EvalResult for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let compiled = compile_source_named(&source, module_name);
    let errs = errors_only(&compiled);
    assert!(errs.is_empty(), "compile errors in {}: {:?}", path, errs);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    // Note: assert_no_eval_errors omits the file path from the panic message;
    // the path is visible via `path` in the backtrace when this assertion fails.
    assert_no_eval_errors(&result);
    result
}

/// Compile the fixture without evaluating. Used for structural assertions.
fn compile_ri_file(path: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    compile_source_named(&source, module_name)
}

// ── step-1: smoke test ────────────────────────────────────────────────────────

/// Load pattern_composition.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn smoke_compiles_and_evals() {
    let result = eval_ri_file(FIXTURE_PATH, "pattern_composition");
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for pattern_composition.ri"
    );
}

// ── step-3: structural assertions ─────────────────────────────────────────────

/// At least 7 expected structures present in compiled templates.
#[test]
fn has_expected_templates() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    assert!(
        compiled.templates.len() >= 7,
        "expected >=7 templates, got {}: {:?}",
        compiled.templates.len(),
        compiled
            .templates
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    let expected_names = [
        "BaseElement",
        "PatternRow",
        "PatternCountZero",
        "PatternCountOne",
        "PatternGrid10x10",
        "PatternComposed",
        "BooleanFold",
    ];
    let actual_names: Vec<&str> = compiled.templates.iter().map(|t| t.name.as_str()).collect();
    for name in &expected_names {
        assert!(
            actual_names.contains(name),
            "expected template '{}' in compiled output, got {:?}",
            name,
            actual_names
        );
    }
}

/// PatternCountZero compiles without Error diagnostics (may produce warnings
/// or zero-instance geometry, but must not ICE or emit errors).
#[test]
fn count_zero_compiles() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let errors = errors_only(&compiled);
    assert!(
        errors.is_empty(),
        "PatternCountZero should compile without errors, but got: {:?}",
        errors
    );
    assert!(
        compiled
            .templates
            .iter()
            .any(|t| t.name == "PatternCountZero"),
        "PatternCountZero template should be present"
    );
}

/// PatternCountOne has exactly 1 realization with Linear2D pattern kind.
#[test]
fn count_one_single_instance() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "PatternCountOne")
        .expect("PatternCountOne template should be present");
    assert_eq!(
        template.realizations.len(),
        1,
        "PatternCountOne should have 1 realization (the linear_pattern_2d call), got {}",
        template.realizations.len()
    );
    assert!(
        !template.realizations[0].operations.is_empty(),
        "PatternCountOne realization should have at least one operation"
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear2D,
                ..
            }
        ),
        "PatternCountOne realization should be Pattern(Linear2D), got {:?}",
        op
    );
}

/// PatternGrid10x10 template has exactly 1 realization with Pattern2D kind.
#[test]
fn grid_10x10_compiles() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "PatternGrid10x10")
        .expect("PatternGrid10x10 template should be present");
    assert_eq!(
        template.realizations.len(),
        1,
        "PatternGrid10x10 should have 1 realization (the linear_pattern_2d call), got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Pattern {
                kind: PatternKind::Linear2D,
                ..
            }
        ),
        "PatternGrid10x10 realization should be Pattern(Linear2D), got {:?}",
        op
    );
}

/// PatternComposed template has >=2 realizations (one linear_pattern_2d + one
/// arbitrary_pattern, each producing one realization in the compiled output).
#[test]
fn composed_patterns_eval() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "PatternComposed")
        .expect("PatternComposed template should be present");
    assert!(
        template.realizations.len() >= 2,
        "PatternComposed should have >=2 realizations (linear + arbitrary), got {}",
        template.realizations.len()
    );
}

/// BooleanFold template has at least one realization with Union operations.
#[test]
fn boolean_fold_eval() {
    let compiled = compile_ri_file(FIXTURE_PATH, "pattern_composition");
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "BooleanFold")
        .expect("BooleanFold template should be present");
    assert!(
        !template.realizations.is_empty(),
        "BooleanFold should have at least one realization"
    );
    let has_union = template.realizations[0].operations.iter().any(|op| {
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Boolean {
                op: reify_compiler::BooleanOp::Union,
                ..
            }
        )
    });
    assert!(
        has_union,
        "BooleanFold realization should contain at least one Union operation"
    );
}
