//! Qualified trait access eval tests — task 193.
//!
//! Full pipeline (parse → compile → eval/check) tests verifying that
//! qualified access expressions (`TypeName::member`, `expr.(TypeName::member)`)
//! evaluate correctly and that constraints using qualified access are enforced.
//!
//! Compiler-side qualified access support restored 2026-04-08 from commit
//! 4e8d65153 (lost in the c88ca9635/3a248e07d regression cluster; see
//! project_regression_c88ca9635.md).

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Satisfaction;

// ── Helper ───────────────────────────────────────────────────────────────────

/// Parse `source`, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
fn parse_compile_check(source: &str) -> reify_compiler::CompiledModule {
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

// ── step-7 ───────────────────────────────────────────────────────────────────

/// Full pipeline eval: `let y : Length = A::x` evaluates to the value of `x`.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, let y : Length = A::x }
///
/// Assert: ValueCellId("S", "y") evaluates to 5mm = 0.005 SI.
#[test]
fn qualified_access_returns_correct_value() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    let y : Length = A::x
}
"#;
    let compiled = parse_compile_check(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let y_id = ValueCellId::new("S", "y");
    let y_val = result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("expected value for {:?} (qualified access let)", y_id));

    let numeric = y_val
        .as_f64()
        .unwrap_or_else(|| panic!("expected numeric value for y, got {:?}", y_val));
    // 5mm = 0.005 m (SI base unit)
    assert!(
        (numeric - 0.005).abs() < 1e-10,
        "expected y == 0.005 (5mm in SI), got {}",
        numeric
    );
}

// ── step-8 ───────────────────────────────────────────────────────────────────

/// Full pipeline check: `constraint A::x > 0mm` is satisfied when x = 5mm.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, constraint A::x > 0mm }
///
/// Assert: all constraint results are Satisfied.
#[test]
fn qualified_access_in_constraint_satisfied() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    constraint A::x > 0mm
}
"#;
    let compiled = parse_compile_check(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        !result.constraint_results.is_empty(),
        "expected at least 1 constraint result (qualified access constraint)"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied (5mm > 0mm), got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── step-9 ───────────────────────────────────────────────────────────────────

/// Full pipeline check: `constraint A::x > 10mm` is violated when x = 5mm.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, constraint A::x > 10mm }
///
/// Assert: at least one constraint result is Violated.
#[test]
fn qualified_access_in_constraint_violated() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    constraint A::x > 10mm
}
"#;
    let compiled = parse_compile_check(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    assert!(
        !result.constraint_results.is_empty(),
        "expected at least 1 constraint result (qualified access constraint)"
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

// ── step-10 ──────────────────────────────────────────────────────────────────

/// Full pipeline eval: instance qualified access `part.(A::x)` resolves to
/// the sub-component's trait member value.
///
/// trait A { param x : Length }
/// structure Inner : A { param x : Length = 3mm }
/// structure Outer { sub part = Inner, let val : Length = part.(A::x) }
///
/// Assert: ValueCellId("Outer", "val") evaluates to 3mm = 0.003 SI.
#[test]
fn instance_qualified_access_basic() {
    let source = r#"
trait A {
    param x : Length
}

structure def Inner : A {
    param x : Length = 3mm
}

structure def Outer {
    sub part = Inner()
    let val : Length = part.(A::x)
}
"#;
    let compiled = parse_compile_check(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let val_id = ValueCellId::new("Outer", "val");
    let val = result.values.get(&val_id).unwrap_or_else(|| {
        panic!(
            "expected value for {:?} (instance qualified access)",
            val_id
        )
    });

    let numeric = val
        .as_f64()
        .unwrap_or_else(|| panic!("expected numeric value for val, got {:?}", val));
    // 3mm = 0.003 m (SI base unit)
    assert!(
        (numeric - 0.003).abs() < 1e-10,
        "expected val == 0.003 (3mm in SI), got {}",
        numeric
    );
}

// ── suggestion #16 ───────────────────────────────────────────────────────────

/// Eval test: `A::size` and `B::size` both resolve to the same SI value (5mm)
/// when a structure satisfies two traits that share a member name via disambiguation.
///
/// trait A { param size : Length }
/// trait B { param size : Length }
/// structure def S : A + B {
///     param size : Length = 5mm
///     let a_size : Length = A::size
///     let b_size : Length = B::size
/// }
///
/// Assert: both `a_size` and `b_size` evaluate to 0.005 SI (5mm in SI)
/// and to each other — proving that both qualified accesses resolve to the
/// same underlying `size` value cell.
#[test]
fn disambiguation_qualified_access_same_value() {
    let source = r#"
trait A {
    param size : Length
}

trait B {
    param size : Length
}

structure def S : A + B {
    param size : Length = 5mm
    let a_size : Length = A::size
    let b_size : Length = B::size
}
"#;

    let compiled = parse_compile_check(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let a_id = ValueCellId::new("S", "a_size");
    let b_id = ValueCellId::new("S", "b_size");

    let a_val = result
        .values
        .get(&a_id)
        .unwrap_or_else(|| panic!("expected value for {:?} (A::size qualified access)", a_id));
    let b_val = result
        .values
        .get(&b_id)
        .unwrap_or_else(|| panic!("expected value for {:?} (B::size qualified access)", b_id));

    let a_numeric = a_val
        .as_f64()
        .unwrap_or_else(|| panic!("expected numeric value for a_size, got {:?}", a_val));
    let b_numeric = b_val
        .as_f64()
        .unwrap_or_else(|| panic!("expected numeric value for b_size, got {:?}", b_val));

    // 5mm = 0.005 m (SI base unit)
    assert!(
        (a_numeric - 0.005).abs() < 1e-10,
        "expected a_size == 0.005 (5mm in SI), got {}",
        a_numeric
    );
    assert!(
        (b_numeric - 0.005).abs() < 1e-10,
        "expected b_size == 0.005 (5mm in SI), got {}",
        b_numeric
    );
    assert!(
        (a_numeric - b_numeric).abs() < 1e-10,
        "expected A::size == B::size (both resolve to same cell value), got a={} b={}",
        a_numeric,
        b_numeric
    );
}
