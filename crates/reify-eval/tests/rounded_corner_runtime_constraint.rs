//! Runtime semantics of the corner-radius constraint the compiler synthesizes
//! for param-driven `rounded_rect`/`rounded_box` args (task #5665).
//!
//! The compiler-layer tests (`reify-compiler/tests/rounded_primitives_tests.rs`)
//! pin that a constraint is emitted and how it is labelled. These pin what it
//! actually *decides* at runtime, so the predicate's semantics are asserted
//! rather than its AST shape.
//!
//! No kernel is needed: the constraint reads only params, so `Engine::check`
//! resolves it definitely — before any geometry is executed, which is exactly
//! the point of synthesizing it (a named violation ahead of the opaque OCCT
//! failure the oversized radius would otherwise produce).
//!
//! Every bound here is an exact metric literal (40mm → 0.04, 30mm → 0.03,
//! 25mm → 0.025, 5mm → 0.005 SI) compared with the same strict `<` / `>` that
//! `eval_cmp` applies at runtime, so no tolerance is involved.

use reify_ir::Satisfaction;
use reify_test_support::check_source;

/// `rounded_rect(40mm, 30mm, corner_r)` with `corner_r` a param defaulting to
/// `radius`.
fn rounded_rect_source(radius: &str) -> String {
    format!(
        r#"structure def S {{
    param corner_r: Length = {radius}
    let body = rounded_rect(40mm, 30mm, corner_r)
}}"#
    )
}

/// The single synthesized constraint's satisfaction.
///
/// Located by label rather than by index so the assertion does not silently
/// re-target if the entity ever gains another constraint.
fn corner_constraint(source: &str) -> Satisfaction {
    let result = check_source(source);
    let matching: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|e| e.label.as_deref().is_some_and(|l| l.contains("corner")))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "expected exactly one corner-radius constraint, got: {:?}",
        result.constraint_results
    );
    matching[0].satisfaction
}

/// 2*25mm = 50mm ≥ min(40mm, 30mm) — the radius does not fit, so the
/// synthesized constraint must catch it.
///
/// This is the case the whole task exists for: with a literal `25mm` the
/// compiler errors statically, but behind a param it used to sail through to
/// OCCT.
#[test]
fn oversized_param_corner_r_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("25mm")),
        Satisfaction::Violated,
        "2*0.025 = 0.05 is not < min(0.04, 0.03) — must be Violated"
    );
}

/// A zero radius is degenerate — the positivity conjunct must reject it.
#[test]
fn zero_param_corner_r_violates() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("0mm")),
        Satisfaction::Violated,
        "corner_r = 0 is not > 0 — must be Violated"
    );
}

/// 2*5mm = 10mm < 30mm ≤ 40mm and 5mm > 0 — a perfectly ordinary radius must
/// NOT be flagged. The no-false-positive case: the constraint has to stay
/// silent on valid designs or it is worse than the silent skip it replaced.
#[test]
fn valid_param_corner_r_is_satisfied() {
    assert_eq!(
        corner_constraint(&rounded_rect_source("5mm")),
        Satisfaction::Satisfied,
        "2*0.005 = 0.01 < 0.03 and 0.005 > 0 — must be Satisfied"
    );
}
