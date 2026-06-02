//! Trait member merging eval tests — task 190.
//!
//! Full pipeline (parse → compile → eval/check) tests verifying that merged
//! trait constraints are actually enforced and let defaults are evaluated.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Satisfaction;
use reify_test_support::assert_no_eval_errors;

// ── Helper ───────────────────────────────────────────────────────────────────

/// Parse `source`, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
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

    compiled
}

// ── step-12 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval test: merged trait constraint is satisfied.
///
/// trait Safe { param x : Length, constraint x > 0mm }
/// structure S : Safe { param x : Length = 5mm }
///
/// 5mm > 0mm → Satisfied.
#[test]
fn merged_trait_constraint_satisfied() {
    let source = r#"
trait Safe {
    param x : Length
    constraint x > 0mm
}

structure def S : Safe {
    param x : Length = 5mm
}
"#;
    let compiled = parse_and_compile(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        !result.constraint_results.is_empty(),
        "expected at least 1 constraint result (trait constraint injected)"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied (5mm > 0mm)",
            entry.id
        );
    }
}

// ── step-13 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval test: merged trait constraint is violated.
///
/// trait Bounded { param x : Length, constraint x > 10mm }
/// structure S : Bounded { param x : Length = 5mm }
///
/// 5mm < 10mm → Violated.
#[test]
fn merged_trait_constraint_violated() {
    let source = r#"
trait Bounded {
    param x : Length
    constraint x > 10mm
}

structure def S : Bounded {
    param x : Length = 5mm
}
"#;
    let compiled = parse_and_compile(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        !result.constraint_results.is_empty(),
        "expected at least 1 constraint result (trait constraint injected)"
    );

    let any_violated = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        any_violated,
        "expected at least one Violated constraint (5mm is not > 10mm), got: {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}

// ── step-14 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval test: multi-trait constraint conjunction — all enforced.
///
/// trait A { param x : Length, constraint x > 0mm }
/// trait B { param x : Length, constraint x < 100mm }
/// structure S : A + B { param x : Length = 5mm }
///
/// 5mm satisfies both bounds → both constraints Satisfied.
#[test]
fn multi_trait_constraints_all_enforced() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

trait B {
    param x : Length
    constraint x < 100mm
}

structure def S : A + B {
    param x : Length = 5mm
}
"#;
    let compiled = parse_and_compile(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        result.constraint_results.len() >= 2,
        "expected at least 2 constraint results (one from each trait), got {}",
        result.constraint_results.len()
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied (5mm satisfies both bounds)",
            entry.id
        );
    }
}

// ── step-16 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval test: multi-trait constraints with partial violation.
///
/// trait A { param x : Length, constraint x > 0mm }
/// trait B { param x : Length, constraint x < 100mm }
/// structure S : A + B { param x : Length = 150mm }
///
/// 150mm satisfies x > 0mm (Satisfied) but violates x < 100mm (Violated).
/// Expect ≥2 constraint results, at least one Satisfied and at least one Violated.
#[test]
fn multi_trait_constraints_partial_violation() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

trait B {
    param x : Length
    constraint x < 100mm
}

structure def S : A + B {
    param x : Length = 150mm
}
"#;
    let compiled = parse_and_compile(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        result.constraint_results.len() >= 2,
        "expected at least 2 constraint results (one from each trait), got {}",
        result.constraint_results.len()
    );

    let any_satisfied = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Satisfied);
    assert!(
        any_satisfied,
        "expected at least one Satisfied constraint (150mm > 0mm), got: {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );

    let any_violated = result
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        any_violated,
        "expected at least one Violated constraint (150mm is not < 100mm), got: {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}

// ── step-15 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval test: let binding from trait is evaluated.
///
/// trait WithComputed { let y = 42 }
/// structure S : WithComputed {}
///
/// The evaluator should inject the let default and evaluate y = 42.
#[test]
fn let_from_trait_evaluated() {
    let source = r#"
trait WithComputed {
    let y = 42
}

structure def S : WithComputed {
}
"#;
    let compiled = parse_and_compile(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Assert no eval-phase errors before accessing result.values — catches eval
    // regressions with a precise failure rather than an opaque panic on missing values.
    assert_no_eval_errors(&result);

    let y_id = ValueCellId::new("S", "y");
    let y_val = result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("expected value for {:?} (let injected from trait)", y_id));

    let numeric = y_val
        .as_f64()
        .unwrap_or_else(|| panic!("expected numeric value for y, got {:?}", y_val));
    assert!(
        (numeric - 42.0).abs() < 1e-10,
        "expected y == 42.0, got {}",
        numeric
    );
}
