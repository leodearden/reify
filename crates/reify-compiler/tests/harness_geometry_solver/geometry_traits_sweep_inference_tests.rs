//! Sweep-family trait inference: `combine_sweep` × `SweepKind`, through both
//! inference walkers (by-name `try_infer_traits_for_function_call` and the
//! op-array `infer_traits_for_op` that resolves let-bound geometry).

use reify_compiler::geometry_traits_inference::{InferredTraits, infer_traits_for_op};
use reify_compiler::{CompiledGeometryOp, GeomRef, ProfileKind, SweepKind};
use reify_core::DiagnosticCode;
use reify_test_support::compile_source_with_stdlib;

/// Number of `code` diagnostics `source` compiles to.
fn diagnostic_count(source: &str, code: DiagnosticCode) -> usize {
    compile_source_with_stdlib(source)
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .count()
}

/// `structure def Foo { param g : <slot_trait> }`, fed `g_arg` from `Top`
/// after `top_lets`.
fn trait_slot_source(slot_trait: &str, top_lets: &str, g_arg: &str) -> String {
    format!(
        "structure def Foo {{ param g : {slot_trait} }}\n\
         structure def Top {{\n{top_lets}\nsub x = Foo(g: {g_arg})\n}}"
    )
}

const EXTRUDE_INFINITE_OF_CIRCLE: &str = r#"extrude_infinite(circle(5mm), 0, 0, 1, "positive")"#;

#[test]
fn op_array_extrude_infinite_of_a_bounded_profile_is_unbounded() {
    let ops = [
        CompiledGeometryOp::Profile {
            kind: ProfileKind::Circle,
            args: vec![],
        },
        CompiledGeometryOp::Sweep {
            kind: SweepKind::ExtrudeInfinite,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::none());
}

#[test]
fn let_bound_extrude_infinite_at_bounded_param_matches_inline() {
    let inline = trait_slot_source("Bounded", "", EXTRUDE_INFINITE_OF_CIRCLE);
    let let_bound = trait_slot_source(
        "Bounded",
        &format!("let e = {EXTRUDE_INFINITE_OF_CIRCLE}"),
        "e",
    );
    assert_eq!(
        diagnostic_count(&inline, DiagnosticCode::GeometryUnbounded),
        1,
        "inline"
    );
    assert_eq!(
        diagnostic_count(&let_bound, DiagnosticCode::GeometryUnbounded),
        1,
        "let-bound"
    );
}

#[test]
fn let_bound_extrude_infinite_at_connected_param_matches_inline() {
    let inline = trait_slot_source("Connected", "", EXTRUDE_INFINITE_OF_CIRCLE);
    let let_bound = trait_slot_source(
        "Connected",
        &format!("let e = {EXTRUDE_INFINITE_OF_CIRCLE}"),
        "e",
    );
    assert_eq!(
        diagnostic_count(&inline, DiagnosticCode::TypeNotConformingToTrait),
        1,
        "inline"
    );
    assert_eq!(
        diagnostic_count(&let_bound, DiagnosticCode::TypeNotConformingToTrait),
        1,
        "let-bound"
    );
}

#[test]
fn let_bound_finite_extrude_of_a_bounded_profile_at_bounded_param_is_clean() {
    let source = trait_slot_source("Bounded", "let e = extrude(circle(5mm), 10mm)", "e");
    assert_eq!(
        diagnostic_count(&source, DiagnosticCode::GeometryUnbounded),
        0
    );
}
