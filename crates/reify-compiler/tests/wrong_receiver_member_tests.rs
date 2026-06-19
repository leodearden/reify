//! Wrong-receiver diagnostic tests for aggregation members and struct-ref member access
//! (ds-sentinel L4, task #4649).
//!
//! ## Coverage
//!
//! - `.sum` applied to a non-`List` receiver (e.g. `(5kg).sum`) → must emit
//!   `DiagnosticCode::AggregationReceiverNotCollection` + `Type::Error` poison.
//! - `.keys` applied to a non-`Map` receiver → same code.
//! - `.values` applied to a non-`Map` receiver → same code.
//! - `w.nonexistent` where `w : Widget` (entity-scope `StructureRef`) and `Widget`
//!   has no member `nonexistent` → must emit an error with "has no member" in the message
//!   + `Type::Error` poison (step-3 / step-4).
//! - Non-regression GUARD: `w.mass` (a valid Widget member) must resolve cleanly with no error.
//!
//! RED state before step-2 / step-4: the `_` arms return dimensionless `Real` / `List<Real>`
//! with NO diagnostic, so `result_type ≠ Error` and zero error diagnostics.
//! GREEN after the corresponding impl step.

use reify_compiler::CompiledModule;
use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::{compile_source, get_let_expr, get_let_expr_in};

// ── helpers ───────────────────────────────────────────────────────────────────

/// True iff some `Severity::Error` diagnostic in `m` carries the given code.
fn has_error_code(m: &CompiledModule, code: DiagnosticCode) -> bool {
    m.diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code == Some(code))
}

/// Count of `Severity::Error` diagnostics in `m`.
fn error_count(m: &CompiledModule) -> usize {
    m.diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

// ── step-1 (RED → GREEN after step-2): aggregation wrong-receiver tests ───────

/// `.sum` on a non-`List` receiver must emit `AggregationReceiverNotCollection`
/// and produce `Type::Error` (anti-cascade).
///
/// RED today: `_` arm returns `Type::dimensionless_scalar()`, no diagnostic.
#[test]
fn sum_on_non_list_receiver_emits_error() {
    let source = r#"
structure S {
    let broken = (5kg).sum
}
"#;
    let m = compile_source(source);

    // (a) anti-cascade: result_type must be Error
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for (5kg).sum (wrong receiver), got {:?}",
        expr.result_type,
    );

    // (b) exactly one Severity::Error, carrying AggregationReceiverNotCollection
    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected an error with code AggregationReceiverNotCollection; diagnostics = {:#?}",
        m.diagnostics,
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 Severity::Error (no cascade); diagnostics = {:#?}",
        m.diagnostics,
    );
}

/// `.keys` on a non-`Map` receiver must emit `AggregationReceiverNotCollection`
/// and produce `Type::Error` (anti-cascade).
///
/// RED today: `_` arm returns `Type::List(Box<Type::dimensionless_scalar()>)`, no diagnostic.
#[test]
fn keys_on_non_map_receiver_emits_error() {
    let source = r#"
structure S {
    let broken = (5kg).keys
}
"#;
    let m = compile_source(source);

    // (a) anti-cascade: result_type must be Error
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for (5kg).keys (wrong receiver), got {:?}",
        expr.result_type,
    );

    // (b) exactly one Severity::Error, carrying AggregationReceiverNotCollection
    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected an error with code AggregationReceiverNotCollection; diagnostics = {:#?}",
        m.diagnostics,
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 Severity::Error (no cascade); diagnostics = {:#?}",
        m.diagnostics,
    );
}

/// `.values` on a non-`Map` receiver must emit `AggregationReceiverNotCollection`
/// and produce `Type::Error` (anti-cascade).
///
/// RED today: `_` arm returns `Type::List(Box<Type::dimensionless_scalar()>)`, no diagnostic.
#[test]
fn values_on_non_map_receiver_emits_error() {
    let source = r#"
structure S {
    let broken = (5kg).values
}
"#;
    let m = compile_source(source);

    // (a) anti-cascade: result_type must be Error
    let expr = get_let_expr(&m, "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for (5kg).values (wrong receiver), got {:?}",
        expr.result_type,
    );

    // (b) exactly one Severity::Error, carrying AggregationReceiverNotCollection
    assert!(
        has_error_code(&m, DiagnosticCode::AggregationReceiverNotCollection),
        "expected an error with code AggregationReceiverNotCollection; diagnostics = {:#?}",
        m.diagnostics,
    );
    assert_eq!(
        error_count(&m),
        1,
        "expected exactly 1 Severity::Error (no cascade); diagnostics = {:#?}",
        m.diagnostics,
    );
}

// ── Aggregation happy-path guards (over-poison regression guard) ─────────────
//
// These tests confirm that a CORRECT receiver for each aggregation member
// (List for `.sum`, Map for `.keys`/`.values`) produces NO error diagnostic
// and a non-Error result_type.  They guard against a future regression where
// the wrong-receiver poison accidentally fires on a valid collection receiver.

/// GUARD: `.sum` on a `List` receiver must resolve cleanly with no error
/// and produce a non-Error result_type.
///
/// Regression guard for step-2: confirms the List success arm is intact after
/// the wrong-receiver poison was added to the `_` arm.
#[test]
fn sum_on_list_receiver_resolves_cleanly() {
    let source = r#"
structure S {
    let items = [1, 2, 3]
    let s = items.sum
}
"#;
    let m = compile_source(source);

    // No errors — valid receiver must not trigger AggregationReceiverNotCollection
    assert_eq!(
        error_count(&m),
        0,
        "expected 0 Severity::Error for items.sum on a List receiver; diagnostics = {:#?}",
        m.diagnostics,
    );

    // result_type must not be Error
    let expr = get_let_expr(&m, "s");
    assert_ne!(
        expr.result_type,
        Type::Error,
        "valid items.sum must not produce Type::Error; got {:?}",
        expr.result_type,
    );
}

/// GUARD: `.keys` on a `Map` receiver must resolve cleanly with no error
/// and produce a non-Error result_type.
///
/// Regression guard for step-2: confirms the Map success arm is intact after
/// the wrong-receiver poison was added to the `_` arm.
#[test]
fn keys_on_map_receiver_resolves_cleanly() {
    let source = r#"
structure S {
    let m = map{"a" => 1}
    let k = m.keys
}
"#;
    let m = compile_source(source);

    // No errors — valid receiver must not trigger AggregationReceiverNotCollection
    assert_eq!(
        error_count(&m),
        0,
        "expected 0 Severity::Error for m.keys on a Map receiver; diagnostics = {:#?}",
        m.diagnostics,
    );

    // result_type must not be Error
    let expr = get_let_expr(&m, "k");
    assert_ne!(
        expr.result_type,
        Type::Error,
        "valid m.keys must not produce Type::Error; got {:?}",
        expr.result_type,
    );
}

/// GUARD: `.values` on a `Map` receiver must resolve cleanly with no error
/// and produce a non-Error result_type.
///
/// Regression guard for step-2: confirms the Map success arm is intact after
/// the wrong-receiver poison was added to the `_` arm.
#[test]
fn values_on_map_receiver_resolves_cleanly() {
    let source = r#"
structure S {
    let m = map{"a" => 1}
    let v = m.values
}
"#;
    let m = compile_source(source);

    // No errors — valid receiver must not trigger AggregationReceiverNotCollection
    assert_eq!(
        error_count(&m),
        0,
        "expected 0 Severity::Error for m.values on a Map receiver; diagnostics = {:#?}",
        m.diagnostics,
    );

    // result_type must not be Error
    let expr = get_let_expr(&m, "v");
    assert_ne!(
        expr.result_type,
        Type::Error,
        "valid m.values must not produce Type::Error; got {:?}",
        expr.result_type,
    );
}

// ── step-3 (RED → GREEN after step-4): StructureRef missing-member tests ─────

/// Accessing a nonexistent member on a StructureRef-typed value must emit an error
/// and produce `Type::Error` (anti-cascade, SIR-α entity-scope path :3432).
///
/// RED today: the `:3445` `.unwrap_or(dimensionless_scalar())` silently accepts
/// the missing member — result_type is `dimensionless Real`, no diagnostic.
#[test]
fn structref_missing_member_emits_error() {
    let source = r#"
structure Widget {
    param mass : Mass = 5kg
}
structure Holder {
    let w = Widget()
    let broken = w.nonexistent
}
"#;
    let m = compile_source(source);

    // (a) anti-cascade: result_type of the broken access must be Error
    let expr = get_let_expr_in(&m, "Holder", "broken");
    assert_eq!(
        expr.result_type,
        Type::Error,
        "expected result_type == Type::Error for w.nonexistent (missing member), got {:?}",
        expr.result_type,
    );

    // (b) exactly one Severity::Error carrying StructureMemberNotFound — asserted by
    // stable code rather than message wording so the test is robust to rewording.
    assert!(
        has_error_code(&m, DiagnosticCode::StructureMemberNotFound),
        "expected an error with code StructureMemberNotFound; diagnostics = {:#?}",
        m.diagnostics,
    );
    // supplementary: message should name the struct and the missing member
    let errors: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    let has_member_msg = errors
        .iter()
        .any(|d| d.message.contains("has no member") && d.message.contains("nonexistent"));
    assert!(
        has_member_msg,
        "expected error message containing 'has no member' and 'nonexistent'; errors = {:#?}",
        errors,
    );
    // anti-cascade: at most one error (the missing-member diagnostic, no cascade)
    assert!(
        errors.len() <= 1,
        "expected at most 1 Severity::Error (no cascade); diagnostics = {:#?}",
        m.diagnostics,
    );
}

/// GUARD: accessing a sub-component name via a StructureRef-typed receiver must NOT
/// trigger a false "has no member" poison (ds-sentinel L4 amendment, task #4649).
///
/// `child` is in `template.sub_components` for Outer but absent from `value_cells`.
/// The SIR-α branch must recognise it as "known" (via `template_has_member`) and
/// keep the permissive `dimensionless_scalar()` fallback rather than poisoning.
///
/// The fixture uses a Container that holds an `Outer` instance so that `o.child`
/// goes through the SIR-α branch (entity-scope StructureRef member access), which
/// is the path this guard is protecting.  A bare `let x = child` inside `Outer`
/// never reaches SIR-α and therefore would not exercise the widened absence check.
#[test]
fn structref_sub_name_not_poisoned() {
    let source = r#"
structure Inner {
    param r : Real = 1.0
}
structure Outer {
    sub child = Inner()
}
structure Container {
    let o = Outer()
    let x = o.child
}
"#;
    let m = compile_source(source);

    // No StructureMemberNotFound error — sub name must not be poisoned by SIR-α
    let has_false_poison = m.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.code == Some(DiagnosticCode::StructureMemberNotFound)
    });
    assert!(
        !has_false_poison,
        "unexpected StructureMemberNotFound on sub-component name; diagnostics = {:#?}",
        m.diagnostics,
    );
}

/// GUARD: accessing a port name via a StructureRef-typed receiver must NOT trigger
/// a false "has no member" poison (ds-sentinel L4 amendment, task #4649).
///
/// `p` is in `template.ports` for HasPort but absent from `value_cells`.
/// The SIR-α branch must recognise it as "known" (via `template_has_member`) and
/// keep the permissive `dimensionless_scalar()` fallback rather than poisoning.
///
/// Symmetric to `structref_sub_name_not_poisoned` — covers the `ports` clause of
/// `template_has_member` so both sub-component and port widening are exercised.
#[test]
fn structref_port_name_not_poisoned() {
    let source = r#"
trait T { param d : Length }
structure HasPort {
    port p : out T { param d : Length = 5mm }
}
structure Container {
    let o = HasPort()
    let x = o.p
}
"#;
    let m = compile_source(source);

    // No StructureMemberNotFound error — port name must not be poisoned by SIR-α
    let has_false_poison = m.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.code == Some(DiagnosticCode::StructureMemberNotFound)
    });
    assert!(
        !has_false_poison,
        "unexpected StructureMemberNotFound on port name; diagnostics = {:#?}",
        m.diagnostics,
    );
}

/// GUARD: a valid member access on a StructureRef must resolve cleanly with no
/// "has no member" error.  Guards against step-4 over-poisoning a valid member.
///
/// GREEN today (must stay green after step-4).
#[test]
fn structref_valid_member_resolves_cleanly() {
    let source = r#"
structure Widget {
    param mass : Mass = 5kg
}
structure Holder {
    let w = Widget()
    let ok = w.mass
}
"#;
    let m = compile_source(source);

    // No "has no member" error — valid member must not be poisoned
    let has_no_member_err = m.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.message.contains("has no member")
    });
    assert!(
        !has_no_member_err,
        "unexpected 'has no member' error on valid member w.mass; diagnostics = {:#?}",
        m.diagnostics,
    );

    // result_type must not be Error
    let expr = get_let_expr_in(&m, "Holder", "ok");
    assert_ne!(
        expr.result_type,
        Type::Error,
        "valid member w.mass must not produce Type::Error; got {:?}",
        expr.result_type,
    );
}
