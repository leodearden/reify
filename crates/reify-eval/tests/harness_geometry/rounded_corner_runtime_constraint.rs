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

/// A violation must say WHICH constructor was at fault and name the argument.
///
/// The label is the only designer-visible text: `SimpleConstraintChecker`
/// emits no span, and `Engine::labeled_diagnostics` substitutes the label for
/// the raw `ConstraintNodeId` in the message. So the constructor name has to
/// live in the label or it reaches nobody.
///
/// RED before the label lands: it is a fixed placeholder naming neither
/// `rounded_box` nor `corner_r`.
#[test]
fn rounded_box_violation_message_names_the_constructor_and_arg() {
    let source = r#"structure def S {
    param corner_r: Length = 25mm
    let body = rounded_box(40mm, 30mm, 20mm, corner_r)
}"#;
    let result = check_source(source);

    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.satisfaction == Satisfaction::Violated)
        .unwrap_or_else(|| {
            panic!(
                "2*0.025 = 0.05 is not < min(0.04, 0.03) — expected a Violated \
                 constraint, got: {:?}",
                result.constraint_results
            )
        });

    let label = entry
        .label
        .as_deref()
        .expect("a synthesized constraint must carry a label");
    assert!(
        label.contains("rounded_box"),
        "the label must name the constructor at fault, got: {label:?}"
    );
    assert!(
        label.contains("corner_r"),
        "the label must name the offending argument, got: {label:?}"
    );

    // And the label must actually reach the designer-facing message.
    let messages: Vec<&str> = result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .filter(|m| m.contains(label))
        .collect();
    assert!(
        !messages.is_empty(),
        "the label must be substituted into the violation message; \
         diagnostics were: {:#?}",
        result.diagnostics
    );
}
