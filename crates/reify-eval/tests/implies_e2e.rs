//! End-to-end `implies` operator integration test.
//!
//! Exercises the full `compile_with_stdlib` → eval pipeline for the `implies`
//! keyword operator, sourcing `Undef` from a no-default `Bool` param (mirroring
//! `examples/kleene_e2e.ri`) so the fixture remains Bool-typed and does not
//! trigger the compiler's Bool-operand guard.
//!
//! # Spec coverage
//! - §9.2.3 – `a implies b` truth table signal rows:
//!   - `true implies false` → `Bool(false)` (modus ponens, antecedent true)
//!   - `false implies <undef>` → `Bool(true)` (vacuous truth, short-circuit)
//!   - `<undef> implies false` → `Undef` (unknown antecedent, consequent false)
//! - §16 / §9.2.3 right-associativity: `a implies b implies c` parses as
//!   `a implies (b implies c)` (right-assoc, per grammar.js prec.right(-15)).
//! - §16 precedence: `or` binds tighter than `implies`, so
//!   `a or b implies c` evaluates as `(a or b) implies c`.

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{assert_no_eval_errors, make_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Inline `.ri` source for the signal-row tests.
///
/// `u` has no default → `Value::Undef` at eval time.
/// All operands are `Bool`-typed, so the new Bool-operand guard does not reject them.
const SIGNAL_SOURCE: &str = r#"
module ImpliesSignal {
    param u : Bool
    let p1 : Bool = true implies false
    let p2 : Bool = false implies u
    let p3 : Bool = u implies false
}
"#;

/// Inline source for right-associativity: `a implies b implies c` = `a implies (b implies c)`.
///
/// With `a=true, b=false, c=true`:
/// - Right-assoc: `true implies (false implies true)` = `true implies true` = `true`
/// - Left-assoc (wrong):  `(true implies false) implies true` = `false implies true` = `true`
/// Both happen to equal `true` for this choice; pick inputs where they differ:
/// With `a=true, b=true, c=false`:
/// - Right-assoc: `true implies (true implies false)` = `true implies false` = `false`
/// - Left-assoc (wrong):  `(true implies true) implies false` = `true implies false` = `false`
/// Pick `a=false, b=true, c=false`:
/// - Right-assoc: `false implies (true implies false)` = `false implies false` = `true`  (vacuous)
/// - Left-assoc (wrong):  `(false implies true) implies false` = `true implies false` = `false`
const ASSOC_SOURCE: &str = r#"
module ImpliesAssoc {
    let p4 : Bool = false implies true implies false
}
"#;

/// Inline source for precedence: `a or b implies c` = `(a or b) implies c`.
///
/// With `a=false, b=false, c=false`:
/// - `(false or false) implies false` = `false implies false` = `true`  (vacuous)
/// - If `or` had lower precedence (wrong): `false or (false implies false)` = `false or true` = `true`
/// Both equal `true` for this input; choose `a=true, b=false, c=false`:
/// - Correct: `(true or false) implies false` = `true implies false` = `false`
/// - Wrong: `true or (false implies false)` = `true or true` = `true`
const PREC_SOURCE: &str = r#"
module ImpliesPrec {
    let p5 : Bool = true or false implies false
}
"#;

// ── signal-row tests ──────────────────────────────────────────────────────────

/// `true implies false` → `Bool(false)` (modus ponens: antecedent true, consequent false).
#[test]
fn implies_e2e_true_implies_false() {
    let compiled = parse_and_compile_with_stdlib(SIGNAL_SOURCE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    let id = ValueCellId::new("ImpliesSignal", "p1");
    assert_eq!(
        result.values.get(&id).expect("ImpliesSignal.p1 not found"),
        &Value::Bool(false),
        "true implies false should be Bool(false)"
    );
}

/// `false implies <undef>` → `Bool(true)` (vacuous truth: false antecedent short-circuits).
#[test]
fn implies_e2e_false_implies_undef_is_true() {
    let compiled = parse_and_compile_with_stdlib(SIGNAL_SOURCE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    let id = ValueCellId::new("ImpliesSignal", "p2");
    assert_eq!(
        result.values.get(&id).expect("ImpliesSignal.p2 not found"),
        &Value::Bool(true),
        "false implies undef should be Bool(true) (vacuous)"
    );
}

/// `<undef> implies false` → `Undef` (unknown antecedent, consequent is false).
#[test]
fn implies_e2e_undef_implies_false_is_undef() {
    let compiled = parse_and_compile_with_stdlib(SIGNAL_SOURCE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    let id = ValueCellId::new("ImpliesSignal", "p3");
    assert_eq!(
        result.values.get(&id).expect("ImpliesSignal.p3 not found"),
        &Value::Undef,
        "undef implies false should be Undef"
    );
}

// ── associativity / precedence tests (step-9) ─────────────────────────────────

/// `false implies true implies false` parses as `false implies (true implies false)`.
///
/// Right-assoc result: `false implies false` = `true` (vacuous).
/// Left-assoc result (wrong): `(false implies true) implies false` = `true implies false` = `false`.
#[test]
fn implies_e2e_right_associativity() {
    let compiled = parse_and_compile_with_stdlib(ASSOC_SOURCE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    let id = ValueCellId::new("ImpliesAssoc", "p4");
    assert_eq!(
        result.values.get(&id).expect("ImpliesAssoc.p4 not found"),
        &Value::Bool(true),
        "false implies true implies false should be Bool(true) (right-assoc)"
    );
}

/// `true or false implies false` parses as `(true or false) implies false`.
///
/// Correct: `true implies false` = `false`.
/// Wrong (if or had lower prec): `true or true` = `true`.
#[test]
fn implies_e2e_or_binds_tighter_than_implies() {
    let compiled = parse_and_compile_with_stdlib(PREC_SOURCE);
    let mut engine = make_engine();
    let result = engine.eval(&compiled);
    assert_no_eval_errors(&result);
    let id = ValueCellId::new("ImpliesPrec", "p5");
    assert_eq!(
        result.values.get(&id).expect("ImpliesPrec.p5 not found"),
        &Value::Bool(false),
        "true or false implies false should be Bool(false) — or binds tighter than implies"
    );
}
