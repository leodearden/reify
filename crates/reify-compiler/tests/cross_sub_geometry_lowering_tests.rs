//! Tests for cross-sub geometry lowering: `self.<sub>.<member>` for a
//! geometry-typed `let` or `param body : Solid` on a child structure must
//! lower to a stable `GeomRef::Sub("<sub_name>.<member>")` reference in the
//! parent template's realization ops.
//!
//! See task 3441: implementing cross-sub geometry composition.

use reify_compiler::{CompiledGeometryOp, GeomRef, TransformKind};
use reify_test_support::compile_source;
use reify_core::Severity;
use reify_ir::{CompiledExprKind, Value};

/// Compile `Outer` whose `placed` realization translates `self.inner.body`.
/// Assert (a) no Error diagnostics and (b) the lowered translate op targets
/// `GeomRef::Sub("inner.body")` — i.e. the compound-key pointing at the
/// child template's named realization handle.
///
/// RED until step-2 (compile-side lowering) and step-4 (eval-side plumbing) land.
#[test]
fn cross_sub_geometry_lowers_to_geom_ref_sub_with_compound_key() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (a) No Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (b) Outer's `placed` realization contains a Translate transform whose
    //     target is `GeomRef::Sub("inner.body")`.
    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");
    let placed = outer
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("placed"))
        .expect("Outer.placed realization should be present");

    let has_expected_translate = placed.operations.iter().any(|op| {
        matches!(
            op,
            CompiledGeometryOp::Transform {
                kind: TransformKind::Translate,
                target: GeomRef::Sub(name),
                ..
            } if name == "inner.body"
        )
    });
    assert!(
        has_expected_translate,
        "expected a Translate op targeting GeomRef::Sub(\"inner.body\"); \
         got: {:?}",
        placed.operations
    );
}

/// Bare cross-sub geometry let `let body = self.inner.body` must lower to an
/// identity-translate `RealizationDecl` on Outer: exactly one
/// `CompiledGeometryOp::Transform { kind: Translate, target: GeomRef::Sub("inner.body"), ... }`
/// with dx/dy/dz each evaluating to numeric zero.
///
/// Pins the lowering shape so a future refactor (different TransformKind,
/// dropped GeomRef::Sub key, extra ops) trips loudly.
///
/// May already be GREEN from step-2; if so this is a regression guard only.
#[test]
fn bare_cross_sub_geometry_alias_lowers_to_identity_translate_realization() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let body = self.inner.body
}"#;
    let compiled = compile_source(source);

    // (a) No Error diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let outer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("Outer template should be present");

    // (b) Exactly one RealizationDecl named "body".
    let body_realizations: Vec<_> = outer
        .realizations
        .iter()
        .filter(|r| r.name.as_deref() == Some("body"))
        .collect();
    assert_eq!(
        body_realizations.len(),
        1,
        "expected exactly 1 RealizationDecl named 'body' on Outer; got: {:#?}",
        body_realizations
    );
    let body_real = body_realizations[0];

    // (c) Exactly one op: a Translate targeting GeomRef::Sub("inner.body")
    //     with args keyed ["target","dx","dy","dz"], dx/dy/dz numeric zero.
    assert_eq!(
        body_real.operations.len(),
        1,
        "expected exactly 1 op in Outer.body realization (identity-translate); \
         got: {:#?}",
        body_real.operations
    );

    let is_numeric_zero = |e: &reify_ir::CompiledExpr| match &e.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => *si_value == 0.0,
        CompiledExprKind::Literal(Value::Real(v)) => *v == 0.0,
        CompiledExprKind::Literal(Value::Int(0)) => true,
        _ => false,
    };

    match &body_real.operations[0] {
        CompiledGeometryOp::Transform { kind, target, args } => {
            assert_eq!(
                *kind,
                TransformKind::Translate,
                "expected TransformKind::Translate, got {:?}",
                kind
            );
            assert_eq!(
                *target,
                GeomRef::Sub("inner.body".to_string()),
                "expected target GeomRef::Sub(\"inner.body\"), got {:?}",
                target
            );
            let keys: Vec<&str> = args.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(
                keys,
                vec!["target", "dx", "dy", "dz"],
                "expected args keys [target, dx, dy, dz], got {:?}",
                keys
            );
            for (key, expr) in &args[1..] {
                assert!(
                    is_numeric_zero(expr),
                    "expected {} arg to evaluate to numeric zero; got kind: {:?}",
                    key,
                    expr.kind
                );
            }
        }
        other => panic!(
            "expected a Transform op for Outer.body identity-translate; got: {:?}",
            other
        ),
    }
}
