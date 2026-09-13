//! Tests for `geometry_traits_inference` — the per-op trait propagation table
//! and call-site `Bounded`/`Connected`/`Convex` conformance check, implementing
//! PRD `docs/prds/geometry-traits.md` tasks 2 and 3.
//!
//! Scope: the value type `InferredTraits`, the per-`PrimitiveKind` lookup, the
//! pure propagation helpers (`combine_*`), the `CompiledExpr` walk
//! (`infer_traits_for_expr`), the diagnostic-shape helper, and end-to-end
//! positive call-site behaviour (`Foo(g: box(...))` with `param g : Bounded`
//! produces no `GeometryUnbounded`).
//!
//! End-to-end negative ("Unbounded source rejected at a `Bounded` slot") is
//! deferred until a `half_space` / `extrude_infinite` primitive lands — see
//! the `TODO(geometry-traits-task-4-or-later)` block in // ptodo:allow — prose xref, no current owner task
//! `geometry_traits_inference.rs`.
//!
//! The trait-decl behaviour (refinements, `required_members`, defaults) is
//! kept in the sibling file `geometry_traits_tests.rs`; this file is reserved
//! for the inference pipeline.

use reify_compiler::geometry_traits_inference::{
    EmptyLetEnv, GeomDim, GeometryTrait, InferredTraits, LetBindingEnv, SubRefEnv,
    combine_difference, combine_intersection, combine_modify, combine_pattern, combine_sweep,
    combine_transform, combine_union, infer_primitive, infer_traits_for_expr,
    infer_traits_for_expr_in_env, infer_traits_for_op, infer_traits_for_op_in_env,
};
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PatternKind, PrimitiveKind,
    SweepKind, TransformKind,
};
use reify_core::{ContentHash, DiagnosticCode, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, ResolvedFunction, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ─── InferredTraits value type — flag math + has() accessor ─────────────────

#[test]
fn inferred_traits_all_has_all_three_flags_set() {
    let t = InferredTraits::all();
    assert!(t.bounded, "InferredTraits::all() must set bounded");
    assert!(t.connected, "InferredTraits::all() must set connected");
    assert!(t.convex, "InferredTraits::all() must set convex");
}

#[test]
fn inferred_traits_none_has_no_flags_set() {
    let t = InferredTraits::none();
    assert!(!t.bounded, "InferredTraits::none() must clear bounded");
    assert!(!t.connected, "InferredTraits::none() must clear connected");
    assert!(!t.convex, "InferredTraits::none() must clear convex");
}

#[test]
fn inferred_traits_bounded_only_has_only_bounded_set() {
    let t = InferredTraits::bounded_only();
    assert!(t.bounded, "InferredTraits::bounded_only() must set bounded");
    assert!(
        !t.connected,
        "InferredTraits::bounded_only() must clear connected"
    );
    assert!(
        !t.convex,
        "InferredTraits::bounded_only() must clear convex"
    );
}

#[test]
fn inferred_traits_bounded_connected_has_bounded_and_connected_set() {
    let t = InferredTraits::bounded_connected();
    assert!(
        t.bounded,
        "InferredTraits::bounded_connected() must set bounded"
    );
    assert!(
        t.connected,
        "InferredTraits::bounded_connected() must set connected"
    );
    assert!(
        !t.convex,
        "InferredTraits::bounded_connected() must clear convex"
    );
}

#[test]
fn inferred_traits_has_returns_corresponding_flag() {
    let all = InferredTraits::all();
    assert!(all.has(GeometryTrait::Bounded));
    assert!(all.has(GeometryTrait::Connected));
    assert!(all.has(GeometryTrait::Convex));

    let none = InferredTraits::none();
    assert!(!none.has(GeometryTrait::Bounded));
    assert!(!none.has(GeometryTrait::Connected));
    assert!(!none.has(GeometryTrait::Convex));

    let b_only = InferredTraits::bounded_only();
    assert!(b_only.has(GeometryTrait::Bounded));
    assert!(!b_only.has(GeometryTrait::Connected));
    assert!(!b_only.has(GeometryTrait::Convex));
}

// ─── GeomDim + dimension/planar/closed fields (task α data model) ────────────
//
// These tests pin the extended record introduced in step-4. They are RED until
// `GeomDim`, the `dimension`/`planar`/`closed` fields, and the `curve()`
// constructor exist in `geometry_traits_inference.rs`.

/// The three dimensionality variants exist and are pairwise distinct (a shape
/// is exactly one of 1-D / 2-D / 3-D).
#[test]
fn geom_dim_has_three_distinct_variants() {
    assert_ne!(GeomDim::Curve, GeomDim::Surface);
    assert_ne!(GeomDim::Surface, GeomDim::Solid);
    assert_ne!(GeomDim::Curve, GeomDim::Solid);
}

/// Every named constructor (`all`/`none`/`bounded_only`/`bounded_connected`)
/// carries `dimension == GeomDim::Solid` with `planar == false` and
/// `closed == false` — the Solid default that assigns box/cylinder/sphere/tube
/// and all boolean/modify/pattern/sweep results to Solid for free.
#[test]
fn named_constructors_carry_solid_dimension_non_planar_non_closed() {
    assert_eq!(InferredTraits::all().dimension, GeomDim::Solid);
    assert_eq!(InferredTraits::none().dimension, GeomDim::Solid);
    assert_eq!(InferredTraits::bounded_only().dimension, GeomDim::Solid);
    assert_eq!(
        InferredTraits::bounded_connected().dimension,
        GeomDim::Solid
    );

    assert!(!InferredTraits::all().planar, "all() must be planar=false");
    assert!(!InferredTraits::all().closed, "all() must be closed=false");
    assert!(!InferredTraits::none().planar);
    assert!(!InferredTraits::none().closed);
    assert!(!InferredTraits::bounded_only().planar);
    assert!(!InferredTraits::bounded_only().closed);
    assert!(!InferredTraits::bounded_connected().planar);
    assert!(!InferredTraits::bounded_connected().closed);
}

/// `curve()` yields `dimension == GeomDim::Curve` while preserving the prior
/// `all()`-equivalent flags (bounded/connected/convex all true) and keeping
/// planar/closed false — the dedicated constructor for 1-D primitives.
#[test]
fn curve_constructor_is_one_dimensional_preserving_all_flags() {
    let c = InferredTraits::curve();
    assert_eq!(c.dimension, GeomDim::Curve);
    assert!(c.bounded, "curve() must keep bounded=true");
    assert!(c.connected, "curve() must keep connected=true");
    assert!(c.convex, "curve() must keep convex=true");
    assert!(!c.planar, "curve() must be planar=false");
    assert!(!c.closed, "curve() must be closed=false");
}

// ─── dimension assignment across both inference paths (task α) ───────────────
//
// Solid cases already pass after step-4 (named constructors default to Solid);
// the Curve cases are RED until step-6 routes the curve arms to
// `InferredTraits::curve()`.

/// Name-dispatch: primitives and every combinator family (boolean / modify /
/// pattern / sweep) infer `GeomDim::Solid`.
#[test]
fn name_dispatch_assigns_solid_to_primitives_and_combinators() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;
    let g = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_g = || make_function_call("box", vec![g(), g(), g()], Type::Geometry);

    for name in ["box", "cylinder", "sphere", "tube"] {
        let t = try_infer_traits_for_function_call(name, &[g(), g(), g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Solid, "{name} must infer Solid");
    }
    for name in [
        "union",
        "difference",
        "intersection",
        "union_all",
        "intersection_all",
    ] {
        let t = try_infer_traits_for_function_call(name, &[box_g(), box_g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Solid, "{name} must infer Solid");
    }
    for name in [
        "fillet",
        "shell",
        "chamfer",
        "draft",
        "thicken",
        "offset_solid",
    ] {
        let t = try_infer_traits_for_function_call(name, &[box_g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Solid, "{name} must infer Solid");
    }
    for name in ["linear_pattern", "circular_pattern", "mirror"] {
        let t = try_infer_traits_for_function_call(name, &[box_g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Solid, "{name} must infer Solid");
    }
    for name in ["extrude", "revolve", "sweep", "loft"] {
        let t = try_infer_traits_for_function_call(name, &[box_g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Solid, "{name} must infer Solid");
    }
}

/// Name-dispatch: `isosurface` (marching-cubes extraction from a voxel grid)
/// infers `GeomDim::Solid` — mirrors the already-correct op-array arm
/// `CompiledGeometryOp::Isosurface { .. } => InferredTraits::solid()` at
/// geometry_traits_inference.rs:682 (a watertight bounded/connected Solid,
/// not provably convex). The argument content is irrelevant here — like the
/// op-array arm, the name-based arm returns a constant regardless of the
/// (Voxel-repr) grid operand's shape.
///
/// Task 5033 GAP #1(b) RED: falls through the `_ => None` fallback (→
/// `InferredTraits::all()`) until the `"isosurface" => Some(InferredTraits::solid())`
/// name-based arm is added immediately before that fallback.
#[test]
fn name_dispatch_assigns_solid_to_isosurface() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;
    let g = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_g = || make_function_call("box", vec![g(), g(), g()], Type::Geometry);

    let t = try_infer_traits_for_function_call("isosurface", &[box_g()]).unwrap();
    assert_eq!(t.dimension, GeomDim::Solid, "isosurface must infer Solid");
}

/// Name-dispatch: every curve constructor infers `GeomDim::Curve`.
/// RED until step-6 (currently the curve arm returns `all()` → Solid).
#[test]
fn name_dispatch_assigns_curve_to_curve_constructors() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;
    let g = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    for name in ["line_segment", "arc", "helix", "interp", "bezier", "nurbs"] {
        let t = try_infer_traits_for_function_call(name, &[g(), g()]).unwrap();
        assert_eq!(t.dimension, GeomDim::Curve, "{name} must infer Curve");
    }
}

/// Name-dispatch: a transform of a curve PRESERVES Curve (combine_transform is
/// identity), while a transform of a box stays Solid. The Curve half is RED
/// until step-6.
#[test]
fn name_dispatch_transform_preserves_operand_dimension() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;
    let g = || CompiledExpr::literal(Value::Real(10.0), Type::length());

    let arc = make_function_call("arc", vec![g(), g(), g()], Type::Geometry);
    let translate_arc_args = vec![arc, g(), g(), g()];
    let t = try_infer_traits_for_function_call("translate", &translate_arc_args).unwrap();
    assert_eq!(
        t.dimension,
        GeomDim::Curve,
        "translate(arc(...)) must preserve Curve"
    );

    let box_g = make_function_call("box", vec![g(), g(), g()], Type::Geometry);
    let translate_box_args = vec![box_g, g(), g(), g()];
    let t2 = try_infer_traits_for_function_call("translate", &translate_box_args).unwrap();
    assert_eq!(
        t2.dimension,
        GeomDim::Solid,
        "translate(box(...)) must stay Solid"
    );
}

/// Op-array: Primitive / Boolean / Modify / Pattern / Sweep roots infer Solid.
/// These already pass after step-4.
#[test]
fn op_array_assigns_solid_to_primitive_and_combinator_ops() {
    let prim = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![],
    }];
    assert_eq!(infer_traits_for_op(&prim).dimension, GeomDim::Solid);

    let sweep = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&sweep).dimension, GeomDim::Solid);

    let boolean = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        },
    ];
    assert_eq!(infer_traits_for_op(&boolean).dimension, GeomDim::Solid);

    let modify = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Modify {
            kind: ModifyKind::Fillet,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&modify).dimension, GeomDim::Solid);

    let pattern = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&pattern).dimension, GeomDim::Solid);
}

/// Op-array: a `Curve` op infers Curve, and a Transform of a Curve op preserves
/// Curve. RED until step-6 (currently the `Curve` arm returns `all()` → Solid).
#[test]
fn op_array_assigns_curve_to_curve_op_and_preserves_through_transform() {
    let curve_ops = vec![CompiledGeometryOp::Curve {
        kind: CurveKind::Arc,
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op(&curve_ops).dimension,
        GeomDim::Curve,
        "a Curve op must infer GeomDim::Curve"
    );

    let transform_curve = vec![
        CompiledGeometryOp::Curve {
            kind: CurveKind::Arc,
            args: vec![],
        },
        CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(
        infer_traits_for_op(&transform_curve).dimension,
        GeomDim::Curve,
        "a Transform of a Curve op must preserve GeomDim::Curve"
    );
}

// ─── infer_primitive — per-PrimitiveKind lookup ─────────────────────────────

/// Every current `PrimitiveKind` (Box/Cylinder/Sphere/Tube/Cone) is fully
/// Bounded+Connected+Convex. When an Unbounded primitive lands (e.g.
/// `half_space`, `extrude_infinite`), this test must be updated to
/// expect `InferredTraits::none()` (or the appropriate subset) for those
/// variants — see `TODO(geometry-traits-task-4-or-later)` in the inference // ptodo:allow — prose xref
/// module.
#[test]
fn infer_primitive_kind_yields_all_three_traits() {
    // Iterate every variant via a fixed array so the test is exhaustive.
    // If a new `PrimitiveKind` variant is added, the array length annotation
    // forces a compile error here, which forces the test to be re-considered.
    // RED until step-6 adds PrimitiveKind::Cone to types.rs.
    let cases: [PrimitiveKind; 5] = [
        PrimitiveKind::Box,
        PrimitiveKind::Cylinder,
        PrimitiveKind::Sphere,
        PrimitiveKind::Tube,
        PrimitiveKind::Cone,
    ];
    for kind in cases {
        assert_eq!(
            infer_primitive(kind),
            InferredTraits::all(),
            "PrimitiveKind::{:?} should currently infer all three traits",
            kind
        );
    }
}

/// Torus is the first non-convex primitive: bounded + connected + Solid, but
/// `convex == false` (a ring has a hole). Both inference entry points must
/// agree — the name-keyed `try_infer_traits_for_function_call("torus", ..)` and
/// the kind-keyed `infer_primitive(PrimitiveKind::Torus)`.
///
/// RED until step-6: the name-keyed dispatch has no `"torus"` arm yet and falls
/// through to `None`, so the `.expect(...)` below panics. The `infer_primitive`
/// half is already wired in step-4, so the two-entry-point agreement is what
/// pins the name-keyed arm into existence.
#[test]
fn torus_infers_bounded_connected_nonconvex() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    // Name-keyed dispatch (RED until step-6 adds the `"torus"` arm).
    let by_name = try_infer_traits_for_function_call("torus", &[])
        .expect("torus must be dispatched by name to a trait set");
    assert!(by_name.bounded, "torus is bounded");
    assert!(by_name.connected, "torus is connected");
    assert!(!by_name.convex, "torus is NOT convex (a ring has a hole)");
    assert_eq!(by_name.dimension, GeomDim::Solid, "torus is a Solid");

    // Kind-keyed lookup (already wired in step-4) must agree field-for-field.
    let by_kind = infer_primitive(PrimitiveKind::Torus);
    assert!(by_kind.bounded, "infer_primitive(Torus) is bounded");
    assert!(by_kind.connected, "infer_primitive(Torus) is connected");
    assert!(!by_kind.convex, "infer_primitive(Torus) is NOT convex");
    assert_eq!(
        by_kind.dimension,
        GeomDim::Solid,
        "infer_primitive(Torus) is a Solid"
    );

    // The two entry points must produce the identical flag set.
    assert_eq!(
        by_name, by_kind,
        "name- and kind-keyed inference must agree for torus"
    );
}

// ─── combine_union — bounded if both, connected/convex always dropped ────────

/// `combine_union(all, all)` preserves Bounded but drops Connected and
/// Convex (a union of two non-overlapping shapes is generally disconnected;
/// even when they overlap, the union of two convex sets is not convex in
/// general — IR-level analysis cannot tell).
#[test]
fn combine_union_of_two_full_inputs_is_bounded_only() {
    let result = combine_union(InferredTraits::all(), InferredTraits::all());
    assert_eq!(result, InferredTraits::bounded_only());
}

/// `combine_union` requires Bounded on **both** sides — if either is
/// unbounded, the union is unbounded.
#[test]
fn combine_union_with_one_unbounded_input_is_none() {
    let result = combine_union(InferredTraits::none(), InferredTraits::all());
    assert_eq!(result, InferredTraits::none());

    let result = combine_union(InferredTraits::all(), InferredTraits::none());
    assert_eq!(result, InferredTraits::none());
}

// ─── combine_difference — left-inherit Bounded only ─────────────────────────

/// `combine_difference(left, _)` preserves only `bounded` from the **left**
/// operand: subtracting any geometry from a bounded shape stays bounded
/// (the left bounds the result), but Connected and Convex are not
/// preserved in general.
#[test]
fn combine_difference_inherits_bounded_from_left_only() {
    // Bounded left, unbounded right → Bounded result.
    let result = combine_difference(InferredTraits::all(), InferredTraits::none());
    assert_eq!(result, InferredTraits::bounded_only());
}

/// `combine_difference(unbounded_left, _)` is unbounded — the cutter on
/// the right cannot bound an unbounded body.
#[test]
fn combine_difference_with_unbounded_left_is_none() {
    let result = combine_difference(InferredTraits::none(), InferredTraits::all());
    assert_eq!(result, InferredTraits::none());
}

// ─── combine_intersection — bounded if either, convex if both ───────────────

/// `combine_intersection` preserves Bounded if **either** side is bounded
/// (the bounded one bounds the intersection). Connected is always dropped
/// (intersection of two connected shapes can produce disjoint pieces).
/// Convex is preserved only if **both** sides are convex (a property of
/// convex set theory).
#[test]
fn combine_intersection_with_one_bounded_input_is_bounded() {
    let result = combine_intersection(InferredTraits::all(), InferredTraits::none());
    assert_eq!(result, InferredTraits::bounded_only());
}

/// `combine_intersection(all, all)` preserves Bounded **and** Convex; only
/// Connected is dropped. We don't have a named constructor for
/// "bounded + convex" so we assert against a struct literal.
#[test]
fn combine_intersection_of_two_full_inputs_preserves_bounded_and_convex() {
    let result = combine_intersection(InferredTraits::all(), InferredTraits::all());
    assert_eq!(
        result,
        InferredTraits {
            bounded: true,
            connected: false,
            convex: true,
            dimension: GeomDim::Solid,
            planar: false,
            closed: false,
        }
    );
}

/// `combine_intersection(none, none)` → `none()`: with neither side
/// bounded, the result is unbounded (and trivially neither connected nor
/// convex).
#[test]
fn combine_intersection_of_two_unbounded_inputs_is_none() {
    let result = combine_intersection(InferredTraits::none(), InferredTraits::none());
    assert_eq!(result, InferredTraits::none());
}

// ─── combine_transform — preserve all three ─────────────────────────────────

/// Rigid (and uniform-scale) transforms preserve all three traits — they
/// are bijective continuous maps that take bounded sets to bounded sets,
/// connected sets to connected sets, and convex sets to convex sets.
#[test]
fn combine_transform_preserves_all_three_traits() {
    assert_eq!(
        combine_transform(InferredTraits::all()),
        InferredTraits::all()
    );
    assert_eq!(
        combine_transform(InferredTraits::none()),
        InferredTraits::none()
    );
    assert_eq!(
        combine_transform(InferredTraits::bounded_only()),
        InferredTraits::bounded_only()
    );
}

// ─── combine_modify — Convex dropped ────────────────────────────────────────

/// Modify ops (Fillet/Chamfer/Shell/Draft/Thicken) preserve Bounded and
/// Connected (they operate locally on a single body) but drop Convex —
/// e.g. shelling a sphere produces a hollow non-convex result.
#[test]
fn combine_modify_drops_convex() {
    assert_eq!(
        combine_modify(InferredTraits::all()),
        InferredTraits::bounded_connected()
    );
    assert_eq!(
        combine_modify(InferredTraits::none()),
        InferredTraits::none()
    );
}

// ─── combine_pattern — only Bounded preserved ───────────────────────────────

/// Pattern ops produce multiple disjoint copies, so Connected is always
/// dropped. Convex is dropped (multiple convex pieces ≠ one convex set).
/// Bounded is preserved iff the input was bounded.
#[test]
fn combine_pattern_drops_connected_and_convex() {
    assert_eq!(
        combine_pattern(InferredTraits::all()),
        InferredTraits::bounded_only()
    );
    assert_eq!(
        combine_pattern(InferredTraits::none()),
        InferredTraits::none()
    );
}

// ─── combine_sweep — Bounded+Connected from profile, Convex always dropped ──

/// Sweep ops inherit Bounded and Connected from the profile (a bounded,
/// connected profile swept along a finite path stays bounded and
/// connected). Convex is always false: even a convex profile swept along
/// a curved path produces a non-convex solid in general.
#[test]
fn combine_sweep_preserves_bounded_and_connected_drops_convex() {
    assert_eq!(
        combine_sweep(InferredTraits::all()),
        InferredTraits::bounded_connected()
    );
    assert_eq!(
        combine_sweep(InferredTraits::bounded_connected()),
        InferredTraits::bounded_connected()
    );
}

/// Sweep with an Unbounded profile yields an Unbounded sweep
/// (extruding/lofting cannot bound an originally-unbounded profile).
#[test]
fn combine_sweep_with_unbounded_profile_is_none() {
    assert_eq!(
        combine_sweep(InferredTraits::none()),
        InferredTraits::none()
    );
}

// ─── infer_traits_for_expr — walk CompiledExpr FunctionCall trees ───────────

/// Build a `FunctionCall` `CompiledExpr` for the given function name and args.
///
/// Mirrors the construction pattern used at the call site in
/// `crates/reify-compiler/src/functions.rs`: `ResolvedFunction { name, qualified_name }`
/// where `qualified_name` is `"std::<name>"` for stdlib geometry constructors.
/// The content_hash is a stable byte-derived value (test fixture only — the
/// inference function does not inspect it).
fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: format!("std::{}", name),
            },
            args,
        },
        result_type,
        content_hash: ContentHash::of(name.as_bytes()),
    }
}

/// `infer_traits_for_expr` recognises the four primitive constructors
/// (`box`, `cylinder`, `sphere`, `tube`) as fully Bounded+Connected+Convex.
/// The args are non-geometry literals — the inference does not inspect them.
#[test]
fn infer_traits_for_expr_handles_primitive_function_calls() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_expr = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    assert_eq!(infer_traits_for_expr(&box_expr), InferredTraits::all());

    let cylinder_expr = make_function_call("cylinder", vec![ten_mm(), ten_mm()], Type::Geometry);
    assert_eq!(infer_traits_for_expr(&cylinder_expr), InferredTraits::all());

    let sphere_expr = make_function_call("sphere", vec![ten_mm()], Type::Geometry);
    assert_eq!(infer_traits_for_expr(&sphere_expr), InferredTraits::all());

    let tube_expr = make_function_call("tube", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    assert_eq!(infer_traits_for_expr(&tube_expr), InferredTraits::all());
}

/// `infer_traits_for_expr` recurses into geometry-typed args of `union`,
/// applying `combine_union`. Two Bounded boxes union to `bounded_only`
/// (Connected and Convex dropped per `combine_union`).
#[test]
fn infer_traits_for_expr_handles_nested_union_of_boxes() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_a = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let box_b = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let union_expr = make_function_call("union", vec![box_a, box_b], Type::Geometry);
    assert_eq!(
        infer_traits_for_expr(&union_expr),
        InferredTraits::bounded_only()
    );
}

/// `infer_traits_for_expr` of a non-FunctionCall expression (e.g. a
/// `Literal` or `ValueRef`) defaults to `InferredTraits::all()`. This
/// safe-default-Bounded fallback matches the conformance walker's
/// behaviour for non-geometry args: an opaque expression at a Bounded
/// slot is assumed to satisfy the bound (otherwise every `let g = box(...)`
/// passed through a value-ref would spuriously fail).
#[test]
fn infer_traits_for_expr_defaults_to_all_for_non_function_call() {
    let lit = CompiledExpr::literal(Value::Int(42), Type::Int);
    assert_eq!(infer_traits_for_expr(&lit), InferredTraits::all());
}

/// `union_all` is recognised by `is_geometry_function` and routed into
/// `infer_traits_for_expr`, so it MUST have an explicit dispatch arm —
/// without one, future Unbounded primitives fed through `union_all` would
/// silently take the unknown-name `_ => all()` fallback and bypass the
/// Bounded check. This test pins the arm: two Bounded boxes folded under
/// `combine_union` must yield `bounded_only` (Bounded preserved, Connected
/// and Convex dropped). If the arm fell through to the default, the result
/// would be `all()` and the assertion would catch the regression.
#[test]
fn infer_traits_for_expr_handles_variadic_union_all() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_a = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let box_b = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let union_all_expr = make_function_call("union_all", vec![box_a, box_b], Type::Geometry);
    assert_eq!(
        infer_traits_for_expr(&union_all_expr),
        InferredTraits::bounded_only(),
        "union_all of two Bounded boxes must fold combine_union → bounded_only"
    );
}

/// `intersection_all` mirrors `union_all`: it must have an explicit dispatch
/// arm folding `combine_intersection`. With two Bounded+Convex inputs, the
/// fold preserves Bounded and Convex but drops Connected (per the
/// `combine_intersection` rule). If the arm fell through to the default
/// `_ => all()`, the result would still have `connected = true` — so the
/// `connected = false` assertion catches a broken dispatch.
#[test]
fn infer_traits_for_expr_handles_variadic_intersection_all() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_a = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let box_b = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let intersection_all_expr =
        make_function_call("intersection_all", vec![box_a, box_b], Type::Geometry);
    let result = infer_traits_for_expr(&intersection_all_expr);
    assert_eq!(
        result,
        InferredTraits {
            bounded: true,
            connected: false,
            convex: true,
            dimension: GeomDim::Solid,
            planar: false,
            closed: false,
        },
        "intersection_all of two Bounded+Convex boxes must fold combine_intersection \
         (bounded preserved, convex preserved, connected dropped)"
    );
}

// ─── End-to-end: call site with `param g : Bounded` ─────────────────────────

/// Positive end-to-end test: a structure declares `param g : Bounded` and a
/// caller instantiates it with a `box(...)` argument. `box(...)` infers as
/// fully Bounded+Connected+Convex, so the call site must NOT emit
/// `DiagnosticCode::GeometryUnbounded` (or a `TypeNotConformingToTrait`
/// cascade for `g`). This exercises the conformance-walker hook end-to-end.
///
/// The negative end-to-end (an Unbounded primitive rejected at this slot)
/// is deferred until `half_space` / `extrude_infinite` lands — see the
/// `TODO(geometry-traits-task-4-or-later)` block in // ptodo:allow — prose xref
/// `geometry_traits_inference.rs`.
#[test]
fn bounded_param_accepting_box_geometry_emits_no_diagnostic() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            sub x = Foo(g: box(10mm, 10mm, 10mm))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    // Filter to the diagnostics that this test pins: GeometryUnbounded must
    // never fire for a box(...) arg, and TypeNotConformingToTrait must not
    // fire for the `g` arg (a box is fully Bounded).
    let geometry_unbounded: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .collect();
    assert!(
        geometry_unbounded.is_empty(),
        "expected no GeometryUnbounded diagnostic for `box(...)` at a Bounded slot, got: {:?}",
        geometry_unbounded
    );

    let g_conformance_failures: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .filter(|d| d.message.contains("'g'") || d.message.contains("Bounded"))
        .collect();
    assert!(
        g_conformance_failures.is_empty(),
        "expected no TypeNotConformingToTrait diagnostic for the 'g' Bounded slot, got: {:?}",
        g_conformance_failures
    );
}

/// Positive end-to-end smoke: `intersection(box, box)` at a `Bounded` slot
/// must NOT emit `E_GEOMETRY_UNBOUNDED`.
///
/// Per `combine_intersection`, the result is Bounded if **either** operand is
/// Bounded — and `box(...)` is fully Bounded. So the worked example from the
/// PRD (`volume(intersection(half_space, box))` not erroring on the `box`
/// half) is exercised in the half that is reachable today: substituting two
/// boxes for `(half_space, box)`. When `half_space` lands, the negative end-
/// to-end test (`intersection(half_space, half_space)` rejected) becomes a
/// one-source-string change away.
///
/// **Scope:** this test only proves the call site does not spuriously fail
/// for nested geometry. It does NOT pin the specific dispatch arm: with two
/// fully-Bounded boxes, both `combine_intersection(all, all)` and the
/// unknown-name `_ => all()` fallback yield a result with `bounded == true`,
/// so a broken `intersection` arm would still pass this assertion. The
/// dispatch arm itself is pinned by
/// [`infer_traits_for_expr_pins_intersection_dispatch_via_connected_drop`]
/// below, which constructs a hand-built `intersection(...)` `CompiledExpr`
/// and asserts `connected == false` (a property only `combine_intersection`
/// guarantees).
#[test]
fn intersection_of_bounded_with_anything_remains_bounded_at_call_site() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            sub x = Foo(g: intersection(box(10mm, 10mm, 10mm), box(5mm, 5mm, 5mm)))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    let geometry_unbounded: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .collect();
    assert!(
        geometry_unbounded.is_empty(),
        "expected no GeometryUnbounded diagnostic for `intersection(box, box)` at a Bounded slot, got: {:?}",
        geometry_unbounded
    );

    let g_conformance_failures: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .filter(|d| d.message.contains("'g'") || d.message.contains("Bounded"))
        .collect();
    assert!(
        g_conformance_failures.is_empty(),
        "expected no TypeNotConformingToTrait diagnostic for the 'g' Bounded slot, got: {:?}",
        g_conformance_failures
    );
}

/// Asymmetric pin on the `"intersection"` dispatch arm in
/// `infer_traits_for_function_call`. With two fully-Bounded box arguments,
/// `combine_intersection(all, all)` drops `connected` (intersection of two
/// connected sets can be disconnected) but preserves `bounded` and
/// `convex`. The unknown-name fallback `_ => all()` would instead leave
/// `connected == true`. Asserting `connected == false` therefore catches
/// both:
///   - a regression that removes the explicit `"intersection"` arm and
///     lets the call fall through to the default,
///   - a regression that swaps to the wrong combine helper (e.g.
///     `combine_transform`, which preserves all three traits).
///
/// This is the unit-level companion to
/// [`intersection_of_bounded_with_anything_remains_bounded_at_call_site`],
/// which is intentionally a positive smoke (its bounded-only assertion
/// passes under either dispatch path).
#[test]
fn infer_traits_for_expr_pins_intersection_dispatch_via_connected_drop() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_a = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let box_b = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let intersection_expr = make_function_call("intersection", vec![box_a, box_b], Type::Geometry);
    let result = infer_traits_for_expr(&intersection_expr);
    assert_eq!(
        result,
        InferredTraits {
            bounded: true,
            connected: false,
            convex: true,
            dimension: GeomDim::Solid,
            planar: false,
            closed: false,
        },
        "intersection dispatch must fold combine_intersection — bounded+convex \
         preserved, connected dropped (regressing to the `_ => all()` fallback \
         would leave connected = true)"
    );
}

// ─── E2E: inline-form baseline (already-rejected path) ──────────────────────

/// Pin the inline rejection baseline: `intersection(box, box)` at a
/// `Connected` param emits exactly one `TypeNotConformingToTrait` diagnostic.
///
/// `combine_intersection(all, all)` drops `connected`, so the inlined call
/// is correctly rejected today. This test locks the inline-form behaviour so
/// that the let-bound test (step-7) can assert symmetric parity.
#[test]
fn inline_intersection_of_boxes_at_connected_param_emits_diagnostic() {
    let source = r#"
        structure def Foo {
            param g : Connected
        }
        structure def Top {
            sub x = Foo(g: intersection(box(10mm, 10mm, 10mm), box(5mm, 5mm, 5mm)))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    let conformance_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .filter(|d| d.message.contains("'g'") || d.message.contains("Connected"))
        .collect();
    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly one TypeNotConformingToTrait for inline \
         intersection(box,box) at a Connected slot, got: {:?}",
        conformance_errors
    );
}

// ─── E2E: let-bound rejection (soundness gap fix) ───────────────────────────

/// RED test: let-bound `intersection(box, box)` at a `Connected` param must
/// emit the same `TypeNotConformingToTrait` diagnostic as the inline form.
///
/// Currently fails (no diagnostic) because `ValueRef` falls through
/// `_ => InferredTraits::all()` in the env-less `infer_traits_for_expr`.
/// Becomes green after step-8 wires `infer_traits_for_expr_in_env` with a
/// `RealizationLetEnv` in the conformance walker.
#[test]
fn let_bound_intersection_at_connected_param_emits_same_diagnostic_as_inline_form() {
    let source = r#"
        structure def Foo {
            param g : Connected
        }
        structure def Top {
            let g = intersection(box(10mm, 10mm, 10mm), box(5mm, 5mm, 5mm))
            sub x = Foo(g: g)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    let conformance_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .filter(|d| d.message.contains("'g'") || d.message.contains("Connected"))
        .collect();
    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly one TypeNotConformingToTrait for let-bound \
         intersection(box,box) at a Connected slot — parity with inline form; \
         currently no diagnostic because ValueRef falls through to all() \
         (soundness gap fixed by step-8). Got: {:?}",
        conformance_errors
    );
}

/// Positive companion: `let g = box(...); sub x = Foo(g: g)` with
/// `param g : Bounded` must NOT emit any diagnostic — the env resolves the
/// let to a box (all traits) and the Bounded check passes.
#[test]
fn let_bound_box_at_bounded_param_emits_no_diagnostic() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let g = box(10mm, 10mm, 10mm)
            sub x = Foo(g: g)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    let geometry_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::GeometryUnbounded)
                || d.code == Some(DiagnosticCode::TypeNotConformingToTrait)
        })
        .collect();
    assert!(
        geometry_errors.is_empty(),
        "expected no geometry diagnostics for let-bound box at a Bounded slot, \
         got: {:?}",
        geometry_errors
    );
}

// ─── E2E: chained-let rejection ─────────────────────────────────────────────

/// Regression lock: chained-let `let h = union(box, box); let g = h` at a
/// `Connected` param emits exactly one `TypeNotConformingToTrait` diagnostic
/// — symmetric to the inline and direct-let baselines above.
///
/// Pins the load-bearing claim that `compile_geometry_call` handles chaining
/// at the AST level (geometry.rs:249-272: Ident arm recurses through
/// `geometry_lets`) so the realization for `g` reuses `h`'s ops. A future
/// refactor of that arm — or of `is_geometry_let`'s Ident arm — would silently
/// regress chained-let conformance without tripping the inline / direct-let
/// E2E tests or the unit-level `lookup_chained_noncyclic_let_propagates_terminal_traits`
/// env test (conformance/mod.rs:4027); this test closes that integration gap.
///
/// Trait math: `combine_union(all, all) = bounded_only` (Connected dropped),
/// so `g`'s inferred traits lack `Connected` and the conformance walker emits
/// exactly one `TypeNotConformingToTrait` for the 'g' slot.
#[test]
fn chained_let_bound_union_at_connected_param_emits_one_diagnostic() {
    let source = r#"
        structure def Foo {
            param g : Connected
        }
        structure def Top {
            let h = union(box(10mm, 10mm, 10mm), box(5mm, 5mm, 5mm))
            let g = h
            sub x = Foo(g: g)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let errors = errors_only(&compiled);

    let conformance_errors: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .filter(|d| d.message.contains("'g'") || d.message.contains("Connected"))
        .collect();
    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly one TypeNotConformingToTrait for chained-let \
         union(box,box) at a Connected slot — parity with inline and direct-let \
         forms; regression lock for compile_geometry_call's AST-level chaining. \
         Got: {:?}",
        conformance_errors
    );
}

// ─── GEOMETRY_FUNCTION_NAMES ↔ infer_traits_for_function_call coverage ──────

// ─── infer_traits_for_op — op-array walker unit tests ───────────────────────
//
// These tests exercise `infer_traits_for_op` introduced in step-3. They are
// RED until that function is added to `geometry_traits_inference.rs`.

/// A single `Primitive` op at the root → `InferredTraits::all()`.
/// All current primitive kinds are fully Bounded+Connected+Convex.
#[test]
fn infer_traits_for_op_handles_primitive_root() {
    let ops = vec![CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![],
    }];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::all());
}

/// Boolean `Union` at root: two box primitives via `GeomRef::Step`.
/// `combine_union(all, all)` → `bounded_only` (connected+convex dropped).
#[test]
fn infer_traits_for_op_handles_boolean_union_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        },
    ];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::bounded_only());
}

/// Boolean `Intersection` at root: `combine_intersection(all, all)` →
/// bounded+convex (connected dropped).
#[test]
fn infer_traits_for_op_handles_boolean_intersection_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Intersection,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        },
    ];
    let result = infer_traits_for_op(&ops);
    assert_eq!(
        result,
        InferredTraits {
            bounded: true,
            connected: false,
            convex: true,
            dimension: GeomDim::Solid,
            planar: false,
            closed: false,
        },
        "intersection of two all-trait ops must be bounded+convex (connected dropped)"
    );
}

/// Boolean `Difference` at root: `combine_difference(all, all)` →
/// bounded_only (bounded inherited from left only).
#[test]
fn infer_traits_for_op_handles_boolean_difference_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Difference,
            left: GeomRef::Step(0),
            right: GeomRef::Step(1),
        },
    ];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::bounded_only());
}

/// `Modify` root (e.g. Fillet): `combine_modify(all)` →
/// bounded_connected (convex dropped).
#[test]
fn infer_traits_for_op_handles_modify_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Modify {
            kind: ModifyKind::Fillet,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_connected()
    );
}

/// `Transform` root: `combine_transform(all)` → all three preserved.
#[test]
fn infer_traits_for_op_handles_transform_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Transform {
            kind: TransformKind::Translate,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::all());
}

/// `Pattern` root: `combine_pattern(all)` → bounded_only
/// (connected+convex dropped — multiple disjoint copies).
#[test]
fn infer_traits_for_op_handles_pattern_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Pattern {
            kind: PatternKind::Linear,
            target: GeomRef::Step(0),
            args: vec![],
        },
    ];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::bounded_only());
}

/// `Sweep` root (e.g. Extrude): `combine_sweep(all)` →
/// bounded_connected (convex dropped).
#[test]
fn infer_traits_for_op_handles_sweep_root() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Sweep {
            kind: SweepKind::Extrude,
            profiles: vec![GeomRef::Step(0)],
            args: vec![],
        },
    ];
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_connected()
    );
}

/// `Curve` root → `InferredTraits::curve()` (1-D primitive: preserves the
/// all()-equivalent bounded/connected/convex flags but carries
/// `dimension == GeomDim::Curve`). The dedicated dimension assertion lives in
/// `op_array_assigns_curve_to_curve_op_and_preserves_through_transform`; this
/// full-struct pin guards the complete record.
#[test]
fn infer_traits_for_op_handles_curve_root() {
    let ops = vec![CompiledGeometryOp::Curve {
        kind: CurveKind::Arc,
        args: vec![],
    }];
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::curve());
}

/// `GeomRef::Sub(_)` in a boolean op returns the safe default `all()` —
/// we don't chase sub-component geometry through the call stack.
/// Here, Union(Sub("x"), Step(0)): left = all() (Sub safe-default),
/// right = all() (Box), combine_union(all, all) = bounded_only.
#[test]
fn infer_traits_for_op_geom_ref_sub_defaults_to_all() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub("x".to_string()),
            right: GeomRef::Step(0),
        },
    ];
    // Sub("x") defaults to all(), combine_union(all, all) = bounded_only
    assert_eq!(infer_traits_for_op(&ops), InferredTraits::bounded_only());
}

// ─── SubRefEnv: op-array Sub-ref resolution seam (#7060) ────────────────────

/// Test stub for the op-array env seam: resolves the single sibling name `"h"`
/// to an unbounded half-space trait set, and declines every other name.
///
/// Mirrors the `LetBindingEnv` stubs used by the `infer_traits_for_expr_in_env`
/// tests above — the two entry points share one env idiom.
struct StubSubRefEnv;

impl SubRefEnv for StubSubRefEnv {
    fn lookup_sub(&self, name: &str) -> Option<InferredTraits> {
        match name {
            "h" => Some(InferredTraits::unbounded_convex()),
            _ => None,
        }
    }
}

/// A `GeomRef::Sub(name)` the env RESOLVES propagates the referent's real
/// traits through `combine_transform` (an identity passthrough), so an
/// unbounded sibling yields `bounded == false` instead of the `all()`
/// safe-default.
///
/// This is the seam that makes the sibling-let → `GeomRef::Sub` lowering
/// (#4668) visible to trait inference at all.
#[test]
fn infer_traits_for_op_in_env_resolves_sub_ref_through_env() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("h".to_string()),
        args: vec![],
    }];
    let inferred = infer_traits_for_op_in_env(&ops, &StubSubRefEnv);
    assert!(
        !inferred.bounded,
        "Sub(\"h\") resolves through the env to unbounded_convex(); combine_transform is an \
         identity passthrough, so translate(h, ..) must infer bounded=false, got {inferred:?}"
    );
    assert_eq!(
        inferred,
        InferredTraits::unbounded_convex(),
        "combine_transform preserves the operand's full trait set unchanged"
    );
}

/// A `GeomRef::Sub(name)` the env DECLINES keeps today's `InferredTraits::all()`
/// safe-default — the `None` → `all()` collapse is the same convention every
/// other defensive arm in the module uses.
#[test]
fn infer_traits_for_op_in_env_unresolved_sub_ref_keeps_safe_default() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("nope".to_string()),
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op_in_env(&ops, &StubSubRefEnv),
        InferredTraits::all(),
        "an unresolved Sub name must safe-default to all(), not leak a partial trait set"
    );
}

/// The env-less `infer_traits_for_op` alias is behaviour-preserving: over the
/// very op-array that resolves to `bounded=false` under `StubSubRefEnv`, it
/// still returns `InferredTraits::all()`.
///
/// This locks the blast radius of the seam to call sites that explicitly opt in
/// by passing an env.
#[test]
fn infer_traits_for_op_without_env_still_defaults_sub_ref_to_all() {
    let ops = vec![CompiledGeometryOp::Transform {
        kind: TransformKind::Translate,
        target: GeomRef::Sub("h".to_string()),
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::all(),
        "infer_traits_for_op must stay a thin alias over the empty env: Sub(\"h\") is \
         unresolvable without an env, so the safe default all() stands"
    );
}

/// Empty op array → safe default `InferredTraits::all()`.
#[test]
fn infer_traits_for_op_empty_array_defaults_to_all() {
    assert_eq!(infer_traits_for_op(&[]), InferredTraits::all());
}

/// Out-of-range `GeomRef::Step(idx)` defaults to `InferredTraits::all()`.
///
/// The `infer_geom_ref` helper uses `ops.get(*idx)` which returns `None` for
/// an out-of-range index, falling back to `InferredTraits::all()`. This test
/// pins that defensive behaviour: if a future refactor changes `.get()` to
/// `[idx]` the resulting panic (or wrong result) would be caught here.
///
/// Array: `[Primitive(Box), Boolean { Union, left: Step(99), right: Step(0) }]`.
/// Root (`ops.last()`) = Boolean Union.
/// - `infer_geom_ref(Step(99), ops)` → out of range → `all()`
/// - `infer_geom_ref(Step(0), ops)` → Primitive(Box) → `all()`
/// - `combine_union(all, all)` → `bounded_only`
#[test]
fn infer_traits_for_op_geom_ref_step_out_of_range_defaults_to_all() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Step(99), // deliberately out of range
            right: GeomRef::Step(0),
        },
    ];
    // left = all() (out-of-range safe default), right = all() (Box)
    // combine_union(all, all) = bounded_only
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_only(),
        "out-of-range GeomRef::Step must default to all(); \
         combine_union(all, all) = bounded_only"
    );
}

/// Self-referential `GeomRef::Step(self_idx)` short-circuits to the
/// safe-default `InferredTraits::all()` rather than stack-overflowing.
///
/// # Regression lock (task 2549)
///
/// Array: `[Boolean { Union, left: Step(0), right: Step(0) }]`.
/// Root = `ops[0]` at `current_position = 0`.
///
/// Under the `idx < current_position` guard introduced in task 2549:
/// - `infer_geom_ref(Step(0), ops, current_position=0)`:
///   `0 < 0` is false → guard fires → returns `all()` (no recursion).
/// - Both sides return `all()`, so `combine_union(all, all) = bounded_only`.
///
/// **Without the guard** each `Step(0)` would recurse back into the same op,
/// causing an infinite-recursion stack-overflow (SIGSEGV / fatal runtime error).
/// This test exists specifically to lock the cycle-guard behaviour: any future
/// refactor that removes or narrows the guard (e.g. changing `>=` to `==`)
/// will resurface the stack-overflow regression here.
#[test]
fn infer_traits_for_op_self_referential_step_short_circuits_to_safe_default() {
    let ops = vec![CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Step(0),  // self-reference: idx == current_position
        right: GeomRef::Step(0), // same
    }];
    // Each Step(0) fires the cycle guard → all().
    // combine_union(all, all) = bounded_only.
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_only(),
        "self-referential GeomRef::Step must short-circuit to all(); \
         combine_union(all, all) = bounded_only"
    );
}

/// Forward-chain `GeomRef::Step` cycles short-circuit to the safe-default
/// `InferredTraits::all()` rather than stack-overflowing.
///
/// # Regression lock (task 2549)
///
/// This test explicitly locks the broader "any forward-reference, not just
/// exact self-reference" case. A future refactor that narrows the guard to
/// `idx == current_position` (exact self-reference only) would pass the
/// self-referential test above but fail here, surfacing the regression loudly.
///
/// Array:
/// ```
/// ops[0] = Modify { Fillet, target: Step(1) }  // forward-reference
/// ops[1] = Modify { Fillet, target: Step(0) }  // back-edge → cycle
/// ```
/// Root = `ops[1]` at `current_position = 1`.
///
/// Under the `idx < current_position` guard:
/// - `infer_op(ops[1], ops, 1)` → `infer_geom_ref(Step(0), ops, 1)`:
///   `0 < 1` → OK, recurse into `infer_op(ops[0], ops, 0)`.
/// - `infer_op(ops[0], ops, 0)` → `infer_geom_ref(Step(1), ops, 0)`:
///   `1 < 0` is false → guard fires → `all()`.
/// - So `ops[0]` resolves to `combine_modify(all) = bounded_connected`.
/// - Back at root: `combine_modify(bounded_connected) = bounded_connected`.
///
/// **Without the guard** `ops[0]` would recurse into `ops[1]` which recurses
/// back into `ops[0]`, causing an infinite-recursion stack-overflow.
#[test]
fn infer_traits_for_op_forward_chain_step_short_circuits_to_safe_default() {
    let ops = vec![
        CompiledGeometryOp::Modify {
            kind: ModifyKind::Fillet,
            target: GeomRef::Step(1), // forward-reference: idx > current_position
            args: vec![],
        },
        CompiledGeometryOp::Modify {
            kind: ModifyKind::Fillet,
            target: GeomRef::Step(0), // back-edge to ops[0]
            args: vec![],
        },
    ];
    // ops[0] resolves via guard: Step(1) at pos=0 → all() → combine_modify(all) = bounded_connected
    // root ops[1]: Step(0) at pos=1 → OK → combine_modify(bounded_connected) = bounded_connected
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_connected(),
        "forward-chain GeomRef::Step must short-circuit via guard; \
         combine_modify(combine_modify(all)) = bounded_connected"
    );
}

/// `Sweep` root with empty `profiles` → `bounded_connected` (combine_sweep(all)).
///
/// The `Sweep` arm calls `profiles.first()` which returns `None` for an empty
/// vec, falling back to `InferredTraits::all()` via `unwrap_or`. Then
/// `combine_sweep(all)` = `bounded_connected`. This test pins that the
/// documented "safe all() default" branch is actually reached and produces
/// the expected result.
#[test]
fn infer_traits_for_op_sweep_with_empty_profiles_defaults_to_bounded_connected() {
    let ops = vec![CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![], // deliberately empty
        args: vec![],
    }];
    assert_eq!(
        infer_traits_for_op(&ops),
        InferredTraits::bounded_connected(),
        "Sweep with empty profiles must fall back to combine_sweep(all) = bounded_connected"
    );
}

// ─── LetBindingEnv / EmptyLetEnv / infer_traits_for_expr_in_env ─────────────
//
// These tests exercise the env API introduced in step-5. They are RED until
// `LetBindingEnv`, `EmptyLetEnv`, and `infer_traits_for_expr_in_env` are added
// to `geometry_traits_inference.rs`.

/// `EmptyLetEnv::lookup` must return `None` for any `ValueCellId`.
#[test]
fn empty_let_env_returns_none_for_any_id() {
    let env = EmptyLetEnv;
    let id = ValueCellId::new("AnyEntity", "any_member");
    assert!(
        env.lookup(&id).is_none(),
        "EmptyLetEnv must return None for every ValueCellId"
    );
}

/// `infer_traits_for_expr_in_env` with `EmptyLetEnv` must return `all()` for
/// a `ValueRef` — same as the env-less `infer_traits_for_expr` wrapper.
#[test]
fn infer_traits_for_expr_in_env_with_empty_env_matches_legacy_value_ref_behaviour() {
    let value_ref = CompiledExpr::value_ref(ValueCellId::new("E", "g"), Type::Geometry);
    assert_eq!(
        infer_traits_for_expr_in_env(&value_ref, &EmptyLetEnv),
        InferredTraits::all(),
        "infer_traits_for_expr_in_env(ValueRef, EmptyLetEnv) must equal \
         infer_traits_for_expr(ValueRef) = all()"
    );
}

/// `infer_traits_for_expr_in_env` with `EmptyLetEnv` must return `all()` for
/// a box FunctionCall — same as the env-less `infer_traits_for_expr` wrapper.
#[test]
fn infer_traits_for_expr_in_env_with_empty_env_matches_legacy_function_call_behaviour() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_expr = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    assert_eq!(
        infer_traits_for_expr_in_env(&box_expr, &EmptyLetEnv),
        InferredTraits::all(),
        "infer_traits_for_expr_in_env(box(...), EmptyLetEnv) must equal \
         infer_traits_for_expr(box(...)) = all()"
    );
}

/// A custom env that resolves one specific `ValueCellId` to `bounded_only()`.
/// Used by the next two tests.
struct FixtureEnv {
    id: ValueCellId,
    traits: InferredTraits,
}
impl LetBindingEnv for FixtureEnv {
    fn lookup(&self, id: &ValueCellId) -> Option<InferredTraits> {
        if id == &self.id {
            Some(self.traits)
        } else {
            None
        }
    }
}

/// `infer_traits_for_expr_in_env` resolves a `ValueRef` whose id matches the
/// env entry, returning the env's answer instead of the safe all() default.
#[test]
fn infer_traits_for_expr_in_env_resolves_value_ref_via_custom_env() {
    let target_id = ValueCellId::new("Top", "g");
    let env = FixtureEnv {
        id: target_id.clone(),
        traits: InferredTraits::bounded_only(),
    };

    // Matching ValueRef → env answer
    let matching_ref = CompiledExpr::value_ref(target_id.clone(), Type::Geometry);
    assert_eq!(
        infer_traits_for_expr_in_env(&matching_ref, &env),
        InferredTraits::bounded_only(),
        "infer_traits_for_expr_in_env must return env answer for a matching ValueRef"
    );

    // Non-matching ValueRef → safe all() default
    let other_ref = CompiledExpr::value_ref(ValueCellId::new("Top", "h"), Type::Geometry);
    assert_eq!(
        infer_traits_for_expr_in_env(&other_ref, &env),
        InferredTraits::all(),
        "infer_traits_for_expr_in_env must fall back to all() for an unbound ValueRef"
    );
}

/// `infer_traits_for_expr_in_env` threads the env through recursive geometry
/// arg resolution. Build `union(ValueRef(g), box(...))` where the env says
/// `g` resolves to `none()`. `combine_union(none, all)` = `none()` — the env
/// propagation through recursive arg traversal is locked.
#[test]
fn infer_traits_for_expr_in_env_threads_env_through_function_call_args() {
    let target_id = ValueCellId::new("Top", "g");
    let env = FixtureEnv {
        id: target_id.clone(),
        traits: InferredTraits::none(),
    };

    let g_ref = CompiledExpr::value_ref(target_id, Type::Geometry);
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_expr = make_function_call("box", vec![ten_mm(), ten_mm(), ten_mm()], Type::Geometry);
    let union_expr = make_function_call("union", vec![g_ref, box_expr], Type::Geometry);

    // combine_union(none(), all()) = none() because both must be bounded
    assert_eq!(
        infer_traits_for_expr_in_env(&union_expr, &env),
        InferredTraits::none(),
        "infer_traits_for_expr_in_env(union(ValueRef(none), box), env) must thread \
         env into arg resolution: combine_union(none, all) = none"
    );
}

// ─── Tripwire / expectation-pinning: ValueRef safe-default ──────────────────

/// Documents the soundness gap: `infer_traits_for_expr` returns
/// `InferredTraits::all()` (safe-default-Bounded) for any `ValueRef` — i.e.
/// the env-less wrapper has no way to resolve `let g = union(box, box); Foo(g: g)`
/// and therefore assumes the ref is fully Bounded+Connected+Convex.
///
/// This test MUST remain green throughout and after the env-aware fix
/// (step-5 / step-8 below) because `infer_traits_for_expr` continues to use
/// `EmptyLetEnv` after the refactor — the env-less wrapper preserves the
/// `_ => all()` safe-default for unbound `ValueRef`s. The actual fix is
/// exercised through `infer_traits_for_expr_in_env` with a non-empty env,
/// which is tested in the `infer_traits_for_expr_in_env_*` suite below.
#[test]
fn infer_traits_for_expr_value_ref_returns_all_safe_default() {
    let value_ref = CompiledExpr::value_ref(ValueCellId::new("E", "g"), Type::Geometry);
    assert_eq!(
        infer_traits_for_expr(&value_ref),
        InferredTraits::all(),
        "infer_traits_for_expr(ValueRef) must return InferredTraits::all() \
         (safe-default-Bounded) when no env is supplied — this is the deliberate \
         soundness gap that the env-aware extension fixes"
    );
}

/// Every name in `GEOMETRY_FUNCTION_NAMES` must hit an **explicit** arm in
/// `infer_traits_for_function_call` — `try_infer_traits_for_function_call`
/// must return `Some(_)` for each, never `None`.
///
/// # Why this matters
///
/// The private `infer_traits_for_function_call` has a `_ => InferredTraits::all()`
/// fallback that silently treats any unknown function name as fully Bounded.
/// Today this is safe because every name in `GEOMETRY_FUNCTION_NAMES` is
/// explicitly dispatched. But when a future Unbounded primitive lands —
/// e.g. `half_space`, `extrude_infinite` — a developer adding it to
/// `GEOMETRY_FUNCTION_NAMES` (so `is_geometry_function` recognises it)
/// without also adding an explicit arm in the dispatch match would silently
/// produce `InferredTraits::all()` (Bounded) instead of the correct
/// `InferredTraits::none()` (Unbounded). The conformance walker would then
/// never emit `E_GEOMETRY_UNBOUNDED` for calls like `Foo(g: half_space(...))`
/// with `param g : Bounded`, defeating the whole check.
///
/// `try_infer_traits_for_function_call` returns `None` precisely for the
/// unknown-name fallback and `Some(_)` for every explicitly-dispatched arm,
/// so iterating `GEOMETRY_FUNCTION_NAMES` and asserting `Some(_)` turns
/// that silent gap into a loud test failure.
///
/// # Why empty `&[]` args are sufficient
///
/// Every dispatch arm either returns a constant (`InferredTraits::all()`)
/// or recurses on the geometry-typed subset of `args`. The helpers
/// `first_geometry_arg`, `first_two_geometry_args`, and `fold_geometry_args`
/// all defensively `unwrap_or(InferredTraits::all())` when no geometry arg
/// is present — so every arm produces a value even with an empty slice.
/// The test's only goal is to verify the name is **dispatched**, not to
/// check the specific trait set produced.
///
/// # Synchronisation guarantee
///
/// Because this test is driven from `GEOMETRY_FUNCTION_NAMES` (the same
/// const that backs `is_geometry_function`), it stays in sync automatically:
/// adding a name to `GEOMETRY_FUNCTION_NAMES` without adding a dispatch arm
/// in `geometry_traits_inference.rs` causes **this** test to fail.
#[test]
fn every_geometry_function_name_has_explicit_dispatch_arm() {
    use reify_compiler::GEOMETRY_FUNCTION_NAMES;
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    assert!(
        !GEOMETRY_FUNCTION_NAMES.is_empty(),
        "GEOMETRY_FUNCTION_NAMES must not be empty"
    );

    for name in GEOMETRY_FUNCTION_NAMES {
        let result = try_infer_traits_for_function_call(name, &[]);
        assert!(
            result.is_some(),
            "GEOMETRY_FUNCTION_NAMES recognises {name:?} but it falls through the \
             unknown-name fallback in infer_traits_for_function_call — add an explicit \
             arm in crates/reify-compiler/src/geometry_traits_inference.rs"
        );
    }
}

// ─── 2-D profile face producers: rectangle + circle (task-4160) ─────────────
//
// Both producers must infer Surface-dimension traits (planar, closed, bounded,
// connected, convex) so that violates_profile_requirement() accepts them at
// extrude/revolve/loft profile slots and rejects them at Curve path slots.
//
// RED until step-6 adds InferredTraits::surface(), the "rectangle"|"circle"
// dispatch arm in geometry_traits_inference.rs, and the infer_op Profile arm.

/// `rectangle(_, _)` → Surface, planar, closed, bounded, connected, convex.
///
/// RED until step-6 adds the surface() constructor and the dispatch arm.
#[test]
fn rectangle_infers_surface_dimension_with_full_face_traits() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    let t = try_infer_traits_for_function_call("rectangle", &[])
        .expect("rectangle must have an explicit dispatch arm");
    assert_eq!(
        t.dimension,
        GeomDim::Surface,
        "rectangle must infer GeomDim::Surface, got {:?}",
        t.dimension
    );
    assert!(t.planar, "rectangle face must be planar");
    assert!(t.closed, "rectangle face must be closed");
    assert!(t.bounded, "rectangle face must be bounded");
    assert!(t.connected, "rectangle face must be connected");
    assert!(t.convex, "rectangle face must be convex");
}

/// `circle(_, _)` → Surface, planar, closed, bounded, connected, convex.
///
/// RED until step-6 adds the surface() constructor and the dispatch arm.
#[test]
fn circle_infers_surface_dimension_with_full_face_traits() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    let t = try_infer_traits_for_function_call("circle", &[])
        .expect("circle must have an explicit dispatch arm");
    assert_eq!(
        t.dimension,
        GeomDim::Surface,
        "circle must infer GeomDim::Surface, got {:?}",
        t.dimension
    );
    assert!(t.planar, "circle face must be planar");
    assert!(t.closed, "circle face must be closed");
    assert!(t.bounded, "circle face must be bounded");
    assert!(t.connected, "circle face must be connected");
    assert!(t.convex, "circle face must be convex");
}

// ─── task-4161: polygon + ellipse Surface inference ──────────────────────────

/// `polygon(&[])` → Surface, planar, closed, bounded, connected, convex=FALSE.
///
/// A general polygon may be concave, so `surface_nonconvex()` (convex=false) must
/// be returned instead of `surface()` (convex=true).
///
/// RED until step-6 adds `InferredTraits::surface_nonconvex()` and the
/// `"polygon"` arm in the name-based dispatch.
#[test]
fn polygon_infers_surface_dimension_with_nonconvex_traits() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    let t = try_infer_traits_for_function_call("polygon", &[])
        .expect("polygon must have an explicit dispatch arm");
    assert_eq!(
        t.dimension,
        GeomDim::Surface,
        "polygon must infer GeomDim::Surface, got {:?}",
        t.dimension
    );
    assert!(t.planar, "polygon face must be planar");
    assert!(t.closed, "polygon face must be closed");
    assert!(t.bounded, "polygon face must be bounded");
    assert!(t.connected, "polygon face must be connected");
    assert!(
        !t.convex,
        "polygon face must NOT be convex (general polygon may be concave)"
    );
}

/// `ellipse(&[])` → Surface, planar, closed, bounded, connected, convex=TRUE.
///
/// An ellipse is always convex, so `surface()` (convex=true) must be returned.
///
/// RED until step-6 adds the `"ellipse"` arm in the name-based dispatch.
#[test]
fn ellipse_infers_surface_dimension_with_full_face_traits() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    let t = try_infer_traits_for_function_call("ellipse", &[])
        .expect("ellipse must have an explicit dispatch arm");
    assert_eq!(
        t.dimension,
        GeomDim::Surface,
        "ellipse must infer GeomDim::Surface, got {:?}",
        t.dimension
    );
    assert!(t.planar, "ellipse face must be planar");
    assert!(t.closed, "ellipse face must be closed");
    assert!(t.bounded, "ellipse face must be bounded");
    assert!(t.connected, "ellipse face must be connected");
    assert!(t.convex, "ellipse face must be convex");
}

// ─── task-3465: half_space — Bounded=false producer ──────────────────────────
//
// RED until step-2 adds `PrimitiveKind::HalfSpace`, `InferredTraits::unbounded_convex()`,
// and the `"half_space"` dispatch arm in `geometry_traits_inference.rs`.

/// `infer_primitive(PrimitiveKind::HalfSpace)` must return `InferredTraits::unbounded_convex()`:
/// bounded=false, connected=true, convex=true, dimension=Solid.
///
/// A half-space is an infinite convex region on one side of a boundary plane;
/// it is single-component (connected) and convex, but unbounded.
///
/// RED until step-2 adds `PrimitiveKind::HalfSpace` and the `infer_primitive` arm.
#[test]
fn half_space_infer_primitive_returns_unbounded_convex() {
    use reify_compiler::geometry_traits_inference::try_infer_traits_for_function_call;

    let by_kind = infer_primitive(PrimitiveKind::HalfSpace);
    assert!(!by_kind.bounded, "half_space is NOT bounded");
    assert!(
        by_kind.connected,
        "half_space is connected (single component)"
    );
    assert!(by_kind.convex, "half_space is convex");
    assert_eq!(by_kind.dimension, GeomDim::Solid, "half_space is a Solid");
    assert!(!by_kind.planar, "half_space is not planar");
    assert!(!by_kind.closed, "half_space is not closed");

    // Both entry points must agree.
    let by_name = try_infer_traits_for_function_call("half_space", &[])
        .expect("half_space must be dispatched by name to a trait set");
    assert_eq!(
        by_kind, by_name,
        "infer_primitive(HalfSpace) and try_infer_traits_for_function_call(\"half_space\", ..) must agree"
    );

    // Verify it is identical to InferredTraits::unbounded_convex().
    let expected = InferredTraits::unbounded_convex();
    assert_eq!(
        by_kind, expected,
        "half_space must == InferredTraits::unbounded_convex()"
    );
}

/// End-to-end negative conformance test: a structure declares `param g : Bounded`
/// and a caller passes `half_space(...)`. Since `half_space` infers `bounded=false`,
/// the conformance walker must emit `DiagnosticCode::GeometryUnbounded`.
///
/// This is the inverse of the existing `bounded_param_accepting_box_geometry_emits_no_diagnostic`
/// positive test.
///
/// RED until step-2 adds the "half_space" dispatch arm (and step-4 adds the
/// compile arm so the source parses correctly).
#[test]
fn half_space_at_bounded_param_emits_geometry_unbounded_diagnostic() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            sub x = Foo(g: half_space(0mm, 0mm, 0mm, 0, 0, 1))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);

    let geometry_unbounded: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .collect();

    assert!(
        !geometry_unbounded.is_empty(),
        "expected at least one GeometryUnbounded diagnostic for half_space(...) at a \
         Bounded slot, but got no such diagnostic. All diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ─── extrude_infinite: Bounded=false producer (task #3466) ──────────────────

/// Negative end-to-end: `extrude_infinite(...)` at a `param g : Bounded` slot
/// MUST emit `DiagnosticCode::GeometryUnbounded`.
///
/// RED until step-2 registers "extrude_infinite" in `GEOMETRY_FUNCTION_NAMES`
/// and adds the `InferredTraits::none()` dispatch arm — until then the unknown
/// name falls back to `InferredTraits::all()` (bounded=true) and no diagnostic
/// is emitted, causing this assertion to fail.
#[test]
fn extrude_infinite_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            sub x = Foo(g: extrude_infinite(circle(5mm), 0, 0, 1, "positive"))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let all_diags = errors_only(&compiled);

    let geometry_unbounded: Vec<_> = all_diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .collect();
    assert!(
        !geometry_unbounded.is_empty(),
        "expected a GeometryUnbounded diagnostic for `extrude_infinite(...)` at a \
         Bounded slot, but got none. Full diagnostics: {:?}",
        all_diags
    );
}

/// Positive end-to-end: `intersection(extrude_infinite(...), box(...))` at a
/// `param g : Bounded` slot MUST NOT emit `GeometryUnbounded` because the
/// intersection with a bounded operand restores the Bounded trait.
///
/// The assertion itself passes even in the RED phase (unknown name → all() →
/// bounded), but it documents the expected steady-state behaviour and will
/// keep passing after step-2 lands the `InferredTraits::none()` arm.
#[test]
fn intersection_of_extrude_infinite_with_box_at_bounded_slot_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            sub x = Foo(g: intersection(extrude_infinite(circle(5mm), 0, 0, 1, "positive"), box(20mm, 20mm, 10mm)))
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let all_diags = errors_only(&compiled);

    let geometry_unbounded: Vec<_> = all_diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .collect();
    assert!(
        geometry_unbounded.is_empty(),
        "expected NO GeometryUnbounded diagnostic when `extrude_infinite(...)` result \
         is bounded by intersection with `box(...)`, but got: {:?}",
        geometry_unbounded
    );
}

// ─── Sibling-let GeomRef::Sub chasing, end to end (#7060) ───────────────────
//
// A bare sibling-let geometry operand lowers to `GeomRef::Sub(<name>)` rather
// than an inlined `GeomRef::Step` (#4668). Before #7060 the inference walker
// safe-defaulted every `Sub` to `InferredTraits::all()`, so an unbounded
// producer reached through a `let` silently satisfied a `Bounded` slot while
// the identical inline shape diagnosed correctly. These cases pin the
// end-to-end behaviour in both directions.

/// Count the `GeometryUnbounded` diagnostics a source produces, returning the
/// full error list alongside so a failure message can show what DID fire.
fn geometry_unbounded_count(source: &str) -> (usize, Vec<String>) {
    let compiled = compile_source_with_stdlib(source);
    let all_diags = errors_only(&compiled);
    let count = all_diags
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::GeometryUnbounded))
        .count();
    let rendered = all_diags.iter().map(|d| format!("{d:?}")).collect();
    (count, rendered)
}

/// One hop: `let h = half_space(..)` then `let t = translate(h, ..)`, with `t`
/// passed to a `param g : Bounded` slot.
///
/// `t` lowers to `[Transform { Translate, target: Sub("h") }]` via the #4668
/// sibling-let pre-check. `combine_transform` is an identity passthrough, so
/// once the `Sub("h")` operand resolves to `half_space`'s `bounded=false` the
/// slot must be diagnosed — exactly as the inline `Foo(g: half_space(..))`
/// form already is.
#[test]
fn translate_of_unbounded_sibling_let_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let t = translate(h, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert!(
        count >= 1,
        "expected a GeometryUnbounded diagnostic for `translate(h, ..)` where `h` is an \
         unbounded sibling let: the operand lowers to GeomRef::Sub(\"h\") (#4668) and must be \
         chased through the sibling realization. Got {count}. All diagnostics: {diags:?}"
    );
}

/// Two hops: `h` → `let t1 = translate(h, ..)` → `let t2 = translate(t1, ..)`,
/// with `t2` at the `Bounded` slot.
///
/// Each hop is its own realization holding a `GeomRef::Sub` back to the
/// previous one, so this fails if resolution stops at depth 1 rather than
/// recursing through the sibling-let chain (#4668).
#[test]
fn two_hop_transform_chain_of_unbounded_sibling_let_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let t1 = translate(h, 1mm, 0mm, 0mm)
            let t2 = translate(t1, 0mm, 1mm, 0mm)
            sub x = Foo(g: t2)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert!(
        count >= 1,
        "expected a GeometryUnbounded diagnostic for a two-hop sibling-let chain \
         (h -> t1 -> t2): each hop lowers to its own GeomRef::Sub (#4668), so chasing must \
         recurse rather than stop at depth 1. Got {count}. All diagnostics: {diags:?}"
    );
}

/// Modify family: `let s = shell(h, 1mm)` at the `Bounded` slot.
///
/// `shell` lowers to `CompiledGeometryOp::Modify`, a different `infer_op` arm
/// from `Transform`, so this pins that the `GeomRef::Sub` (#4668) chasing is a
/// property of `infer_geom_ref` rather than of one op variant.
#[test]
fn shell_of_unbounded_sibling_let_at_bounded_slot_emits_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let h = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let s = shell(h, 1mm)
            sub x = Foo(g: s)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert!(
        count >= 1,
        "expected a GeometryUnbounded diagnostic for `shell(h, ..)` where `h` is an unbounded \
         sibling let: the Modify arm resolves its target through the same GeomRef::Sub (#4668) \
         path as Transform. Got {count}. All diagnostics: {diags:?}"
    );
}

/// False-positive guard: a BOUNDED sibling let must stay silent.
///
/// `let b = box(..)` / `let t = translate(b, ..)` lowers to the identical
/// `GeomRef::Sub("b")` (#4668) shape as the must-fire cases above. Chasing it
/// resolves to `box`'s `bounded=true`, so the slot is satisfied and NO
/// diagnostic may be emitted. This is what distinguishes chasing the Sub from
/// pessimising it.
#[test]
fn translate_of_bounded_sibling_let_at_bounded_slot_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let b = box(20mm, 20mm, 20mm)
            let t = translate(b, 1mm, 0mm, 0mm)
            sub x = Foo(g: t)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 0,
        "expected NO GeometryUnbounded for `translate(b, ..)` where `b` is a BOUNDED sibling \
         let: chasing GeomRef::Sub(\"b\") (#4668) must resolve to box's bounded=true, not \
         pessimise the operand. Got {count}. All diagnostics: {diags:?}"
    );
}

// ─── Regression locks for the #7060 Sub-chasing fix ────────────────────────

/// (a) Cycle termination. Two sibling lets that name each other compile on main
/// today — `let a = translate(b, ..)` / `let b = translate(a, ..)` lowers to
/// `a = [Xform(.., Sub("b"))]` / `b = [Xform(.., Sub("a"))]`, a genuine 2-cycle
/// in the sibling-let graph (#4668). Chasing those Subs without a visited set
/// recurses forever.
///
/// This test asserts only that compilation RETURNS and safe-defaults. It is the
/// executable proof of the `in_flight` guard's relocation above the realization
/// arm: remove that guard and this does not fail, it aborts the whole test
/// binary with a stack overflow.
#[test]
fn mutually_referential_sibling_lets_terminate_instead_of_overflowing() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let a = translate(b, 1mm, 0mm, 0mm)
            let b = translate(a, 1mm, 0mm, 0mm)
            sub x = Foo(g: a)
        }
    "#;
    // Reaching the next line at all is the assertion: a missing cycle guard
    // overflows the stack here rather than returning.
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 0,
        "a sibling-let cycle must terminate at the in_flight guard and safe-default to \
         InferredTraits::all() (bounded=true), not invent a diagnostic. Got {count}. \
         All diagnostics: {diags:?}"
    );
}

/// (b) Compound cross-sub keys are still not chased.
///
/// `GeomRef::Sub` is overloaded: besides bare sibling names (#4668) it also
/// carries `"<sub>.<member>"` for cross-sub `self.<sub>.<member>` references,
/// whose referent lives on a DIFFERENT template. The consumer's `contains('.')`
/// carve-out declines those, and a declined name collapses to the `all()` safe
/// default here — so cross-sub behaviour is exactly what it was before #7060.
///
/// Op shape mirrors the lowering pinned by
/// `harness_type_checking::boolean_arg_cross_sub_diagnostic_tests::
/// boolean_op_with_working_path_cross_sub_arg_lowers_without_diagnostic`.
#[test]
fn compound_cross_sub_key_declined_by_env_keeps_safe_default() {
    let ops = vec![
        CompiledGeometryOp::Primitive {
            kind: PrimitiveKind::Box,
            args: vec![],
        },
        CompiledGeometryOp::Boolean {
            op: BooleanOp::Union,
            left: GeomRef::Sub("inner.body".to_string()),
            right: GeomRef::Step(0),
        },
    ];
    // StubSubRefEnv declines every name but "h", standing in for the consumer
    // declining every name containing a dot.
    assert_eq!(
        infer_traits_for_op_in_env(&ops, &StubSubRefEnv),
        InferredTraits::bounded_only(),
        "a compound cross-sub key must collapse to all() (making Union(all, Box) = \
         bounded_only), identical to the pre-#7060 behaviour — #7060 chases bare sibling \
         names only and must not widen into cross-entity resolution"
    );
}

/// (b, end to end) The cross-sub source shape itself is behaviourally untouched:
/// a `self.<sub>.<member>` geometry reference at a `Bounded` slot still emits no
/// `GeometryUnbounded`, because its compound `GeomRef::Sub` key is not chased.
#[test]
fn cross_sub_geometry_reference_at_bounded_slot_is_unchanged_by_sub_chasing() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Inner {
            let body = half_space(0mm, 0mm, 0mm, 0, 0, 1)
        }
        structure def Top {
            sub inner = Inner()
            let combined = union(self.inner.body, box(20mm, 20mm, 20mm))
            sub x = Foo(g: combined)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 0,
        "a cross-sub `self.inner.body` reference lowers to the compound key \
         GeomRef::Sub(\"inner.body\"), which #7060 deliberately does NOT chase — behaviour \
         must be identical to before the fix. Got {count}. All diagnostics: {diags:?}"
    );
}

/// (c) #6449 widening lock. `intersection(cutter, box(..))` where `cutter` is an
/// unbounded sibling let IS bounded, so the `Bounded` slot must stay silent.
///
/// Green today for a reason that is about to change: boolean operands still
/// inline as `GeomRef::Step`, so this never touches the #7060 chasing path. Once
/// #6449 converts bare-ident boolean operands to `GeomRef::Sub` — the same
/// lowering transform/modify already use (#4668) — `cutter` becomes a chased Sub
/// and `combine_intersection` is what must keep this green by masking the
/// unbounded operand against the bounded one.
///
/// Landing #7060 first is what stops #6449 from silently widening the dropped
/// diagnostic; this test is the executable half of that claim, and must be
/// re-read (not merely re-run) when #6449 lands.
#[test]
fn intersection_of_unbounded_sibling_let_with_box_emits_no_geometry_unbounded() {
    let source = r#"
        structure def Foo {
            param g : Bounded
        }
        structure def Top {
            let cutter = half_space(0mm, 0mm, 0mm, 0, 0, 1)
            let upper = intersection(cutter, box(20mm, 20mm, 20mm))
            sub x = Foo(g: upper)
        }
    "#;
    let (count, diags) = geometry_unbounded_count(source);
    assert_eq!(
        count, 0,
        "intersection with a bounded operand IS bounded, so no GeometryUnbounded may fire \
         even though `cutter` is unbounded. Got {count}. All diagnostics: {diags:?}"
    );
}
