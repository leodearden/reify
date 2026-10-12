//! Sweep-family trait inference: `combine_sweep` × `SweepKind`, through both
//! inference walkers (by-name `try_infer_traits_for_function_call` and the
//! op-array `infer_traits_for_op` that resolves let-bound geometry).

use reify_compiler::geometry_traits_inference::{
    GeomDim, InferredTraits, combine_sweep, infer_traits_for_op,
    try_infer_traits_for_function_call,
};
use reify_compiler::{CompiledGeometryOp, CurveKind, GeomRef, ProfileKind, SweepKind};
use reify_core::{ContentHash, DiagnosticCode, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction};
use reify_test_support::compile_source_with_stdlib;

const ALL_SWEEP_KINDS: [SweepKind; 9] = [
    SweepKind::Loft,
    SweepKind::Extrude,
    SweepKind::Revolve,
    SweepKind::Sweep,
    SweepKind::ExtrudeSymmetric,
    SweepKind::ExtrudeInfinite,
    SweepKind::SweepGuided,
    SweepKind::LoftGuided,
    SweepKind::Pipe,
];

/// The kinds whose first operand is a section that the sweep raises by one
/// dimension.
const SECTION_LIFTING_KINDS: [SweepKind; 7] = [
    SweepKind::Extrude,
    SweepKind::ExtrudeSymmetric,
    SweepKind::ExtrudeInfinite,
    SweepKind::Revolve,
    SweepKind::Sweep,
    SweepKind::SweepGuided,
    SweepKind::LoftGuided,
];

/// Every sweep builtin name, paired with the `SweepKind` geometry.rs lowers it to.
const SWEEP_NAME_KINDS: [(&str, SweepKind); 12] = [
    ("extrude", SweepKind::Extrude),
    ("extrude_symmetric", SweepKind::ExtrudeSymmetric),
    ("extrude_infinite", SweepKind::ExtrudeInfinite),
    ("revolve", SweepKind::Revolve),
    ("revolve_full", SweepKind::Revolve),
    ("sweep", SweepKind::Sweep),
    ("sweep_guided", SweepKind::SweepGuided),
    ("loft", SweepKind::Loft),
    ("loft_guided", SweepKind::LoftGuided),
    ("pipe", SweepKind::Pipe),
    ("zone_cylinder", SweepKind::Pipe),
    ("zone_annulus", SweepKind::Pipe),
];

/// A no-arg geometry builtin call, typed with the `dimensionless_scalar`
/// placeholder expr.rs gives every geometry builtin call.
fn geometry_call(name: &str) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: name.to_string(),
            },
            args: vec![],
        },
        result_type: Type::dimensionless_scalar(),
        content_hash: ContentHash::of(name.as_bytes()),
    }
}

fn line_segment_op() -> CompiledGeometryOp {
    CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![],
    }
}

fn circle_op() -> CompiledGeometryOp {
    CompiledGeometryOp::Profile {
        kind: ProfileKind::Circle,
        args: vec![],
    }
}

fn sweep_op(kind: SweepKind, profiles: Vec<GeomRef>) -> CompiledGeometryOp {
    CompiledGeometryOp::Sweep {
        kind,
        profiles,
        args: vec![],
    }
}

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

// ─── combine_sweep — Bounded+Connected from profile, Convex always dropped ──

/// Sweep ops inherit Bounded and Connected from the profile (a bounded,
/// connected profile swept along a finite path stays bounded and
/// connected). Convex is always false: even a convex profile swept along
/// a curved path produces a non-convex solid in general.
#[test]
fn combine_sweep_preserves_bounded_and_connected_drops_convex() {
    assert_eq!(
        combine_sweep(SweepKind::Extrude, InferredTraits::all()),
        InferredTraits::bounded_connected()
    );
    assert_eq!(
        combine_sweep(SweepKind::Extrude, InferredTraits::bounded_connected()),
        InferredTraits::bounded_connected()
    );
}

/// Sweep with an Unbounded profile yields an Unbounded sweep
/// (extruding/lofting cannot bound an originally-unbounded profile).
#[test]
fn combine_sweep_with_unbounded_profile_is_none() {
    assert_eq!(
        combine_sweep(SweepKind::Extrude, InferredTraits::none()),
        InferredTraits::none()
    );
}

// ─── extrude_infinite — unbounded in both walkers ───────────────────────────

#[test]
fn op_array_extrude_infinite_of_a_bounded_profile_is_unbounded() {
    let ops = [
        circle_op(),
        sweep_op(SweepKind::ExtrudeInfinite, vec![GeomRef::Step(0)]),
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

// ─── dimension — a sweep lifts its swept section ────────────────────────────

#[test]
fn combine_sweep_lifts_a_curve_section_to_a_surface_and_a_face_to_a_solid() {
    for kind in SECTION_LIFTING_KINDS {
        assert_eq!(
            combine_sweep(kind, InferredTraits::curve()).dimension,
            GeomDim::Surface,
            "{kind:?} of a curve"
        );
        assert_eq!(
            combine_sweep(kind, InferredTraits::surface()).dimension,
            GeomDim::Solid,
            "{kind:?} of a face"
        );
        assert_eq!(
            combine_sweep(kind, InferredTraits::all()).dimension,
            GeomDim::Solid,
            "{kind:?} of an opaque operand"
        );
    }
}

/// Pipe's first operand is its PATH and Loft caps every section, so neither
/// lifts a curve operand to a Surface.
#[test]
fn combine_sweep_of_pipe_and_loft_is_solid_over_a_curve_operand() {
    for kind in [SweepKind::Pipe, SweepKind::Loft] {
        assert_eq!(
            combine_sweep(kind, InferredTraits::curve()).dimension,
            GeomDim::Solid,
            "{kind:?}"
        );
    }
}

/// Keeps the extrude/revolve/loft profile precondition refusing every sweep
/// result.
#[test]
fn combine_sweep_never_claims_closed_planar_or_convex() {
    let operands = [
        InferredTraits::curve(),
        InferredTraits::surface(),
        InferredTraits::surface_nonconvex(),
        InferredTraits::all(),
        InferredTraits::none(),
    ];
    for kind in ALL_SWEEP_KINDS {
        for operand in operands {
            let swept = combine_sweep(kind, operand);
            assert!(
                !swept.closed && !swept.planar && !swept.convex,
                "{kind:?} of {operand:?} gave {swept:?}"
            );
        }
    }
}

#[test]
fn op_array_sweep_of_a_curve_profile_infers_surface() {
    let ops = [
        line_segment_op(),
        line_segment_op(),
        sweep_op(SweepKind::Sweep, vec![GeomRef::Step(0), GeomRef::Step(1)]),
    ];
    let swept = infer_traits_for_op(&ops);
    assert_eq!(swept.dimension, GeomDim::Surface);
    assert!(swept.bounded && swept.connected, "{swept:?}");
    assert!(!swept.convex && !swept.closed && !swept.planar, "{swept:?}");
}

#[test]
fn op_array_sweep_of_a_face_profile_infers_solid() {
    let ops = [
        circle_op(),
        line_segment_op(),
        sweep_op(SweepKind::Sweep, vec![GeomRef::Step(0), GeomRef::Step(1)]),
    ];
    assert_eq!(infer_traits_for_op(&ops).dimension, GeomDim::Solid);
}

#[test]
fn op_array_pipe_along_a_curve_path_infers_solid() {
    let ops = [
        line_segment_op(),
        sweep_op(SweepKind::Pipe, vec![GeomRef::Step(0)]),
    ];
    assert_eq!(infer_traits_for_op(&ops).dimension, GeomDim::Solid);
}

/// Locks the name→SweepKind mapping and that both walkers apply one rule.
#[test]
fn by_name_and_op_array_sweep_inference_agree_per_kind() {
    for (name, kind) in SWEEP_NAME_KINDS {
        let op_array = infer_traits_for_op(&[
            line_segment_op(),
            sweep_op(kind, vec![GeomRef::Step(0)]),
        ]);
        assert_eq!(
            try_infer_traits_for_function_call(name, &[geometry_call("line_segment")]),
            Some(op_array),
            "{name} vs {kind:?}"
        );
    }
}

/// The let-bound sweep's realization, which conformance infers for `b`, is the
/// un-capped shell OCCT builds. The curve profile is inline because a let-bound
/// one reaches the walker as an unchased `GeomRef::Sub` (task #7060).
#[test]
fn let_bound_sweep_of_a_curve_profile_realization_infers_surface() {
    let compiled = compile_source_with_stdlib(
        "structure def Top {\n\
         let b = sweep(line_segment(0mm, 0mm, 0mm, 20mm, 0mm, 0mm), \
         line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 200mm))\n\
         }",
    );
    let profile_required = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryProfileRequired))
        .count();
    assert_eq!(profile_required, 1, "the inline curve profile is refused");

    let top = compiled
        .templates
        .iter()
        .find(|t| t.name == "Top")
        .expect("Top template");
    let b = top
        .realizations
        .iter()
        .find(|r| r.name.as_deref() == Some("b"))
        .expect("Top.b realization");
    let swept = infer_traits_for_op(&b.operations);
    assert_eq!(swept.dimension, GeomDim::Surface, "{:?}", b.operations);
    assert!(!swept.closed, "{swept:?}");
}
