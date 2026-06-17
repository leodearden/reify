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
