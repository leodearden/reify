//! Tests for cross-sub geometry lowering: `self.<sub>.<member>` for a
//! geometry-typed `let` or `param body : Solid` on a child structure must
//! lower to a stable `GeomRef::Sub("<sub_name>.<member>")` reference in the
//! parent template's realization ops.
//!
//! See task 3441: implementing cross-sub geometry composition.

use reify_compiler::{CompiledGeometryOp, GeomRef, TransformKind};
use reify_test_support::compile_source;
use reify_core::Severity;

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
