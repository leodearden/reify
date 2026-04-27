//! Tests for `geometry_traits_inference` — the per-op trait propagation table
//! and call-site `Bounded`/`Connected`/`Convex` conformance check, implementing
//! PRD `docs/prds/geometry-traits.md` tasks 2 and 3.
//!
//! Scope: the value type `InferredTraits`, the per-`PrimitiveKind` lookup, the
//! pure propagation helpers (`combine_*`), the op-array walk
//! (`infer_traits_for_op`), the `CompiledExpr` walk (`infer_traits_for_expr`),
//! the diagnostic-shape helper, and end-to-end positive call-site behaviour
//! (`Foo(g: box(...))` with `param g : Bounded` produces no
//! `GeometryUnbounded`).
//!
//! End-to-end negative ("Unbounded source rejected at a `Bounded` slot") is
//! deferred until a `half_space` / `extrude_infinite` primitive lands — see
//! the `TODO(geometry-traits-task-4-or-later)` block in
//! `geometry_traits_inference.rs`.
//!
//! The trait-decl behaviour (refinements, `required_members`, defaults) is
//! kept in the sibling file `geometry_traits_tests.rs`; this file is reserved
//! for the inference pipeline.

use reify_compiler::geometry_traits_inference::{
    GeometryTrait, InferredTraits, combine_difference, combine_intersection, combine_modify,
    combine_pattern, combine_sweep, combine_transform, combine_union, infer_curve, infer_primitive,
    infer_traits_for_expr, infer_traits_for_op,
};
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PrimitiveKind,
};
use reify_types::{
    CompiledExpr, CompiledExprKind, ContentHash, ResolvedFunction, Type, Value,
};

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

// ─── infer_primitive — per-PrimitiveKind lookup ─────────────────────────────

/// Every current `PrimitiveKind` (Box/Cylinder/Sphere/Tube) is fully
/// Bounded+Connected+Convex. When an Unbounded primitive lands (e.g.
/// `half_space`, `extrude_infinite`), this test must be updated to
/// expect `InferredTraits::none()` (or the appropriate subset) for those
/// variants — see `TODO(geometry-traits-task-4-or-later)` in the inference
/// module.
#[test]
fn infer_primitive_kind_yields_all_three_traits() {
    // Iterate every variant via a fixed array so the test is exhaustive.
    // If a new `PrimitiveKind` variant is added, the array length annotation
    // forces a compile error here, which forces the test to be re-considered.
    let cases: [PrimitiveKind; 4] = [
        PrimitiveKind::Box,
        PrimitiveKind::Cylinder,
        PrimitiveKind::Sphere,
        PrimitiveKind::Tube,
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
    assert_eq!(combine_transform(InferredTraits::all()), InferredTraits::all());
    assert_eq!(combine_transform(InferredTraits::none()), InferredTraits::none());
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

// ─── infer_curve — every curve constructor is "all three" ───────────────────

/// All current `CurveKind` variants (line_segment, arc, helix, interp,
/// bezier, nurbs) are finite, single-piece, and treated as Convex from the
/// inference table's perspective: a curve is a 1-D primitive used as a
/// sweep input, where the propagation through `combine_sweep` will drop
/// Convex anyway. Documenting them as `all()` here keeps the table
/// uniform and lets `combine_sweep` remain the single decision point for
/// sweep-output convexity.
#[test]
fn infer_curve_kind_yields_all_three_traits() {
    let cases: [CurveKind; 6] = [
        CurveKind::LineSegment,
        CurveKind::Arc,
        CurveKind::Helix,
        CurveKind::InterpCurve,
        CurveKind::BezierCurve,
        CurveKind::NurbsCurve,
    ];
    for kind in cases {
        assert_eq!(
            infer_curve(kind),
            InferredTraits::all(),
            "CurveKind::{:?} should currently infer all three traits",
            kind
        );
    }
}

// ─── infer_traits_for_op — walk Step-chain in op array ──────────────────────

/// `infer_traits_for_op` walks a `GeomRef::Step` chain across the op array.
/// `Boolean::Union` of two Box primitives propagates to `bounded_only` per
/// the Boolean rule (Connected and Convex dropped).
#[test]
fn infer_traits_for_op_walks_union_of_two_boxes() {
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
    assert_eq!(infer_traits_for_op(2, &ops), InferredTraits::bounded_only());
}

/// `Modify::Fillet` of a Box primitive propagates to `bounded_connected`
/// per `combine_modify` (Convex dropped, Bounded+Connected preserved).
#[test]
fn infer_traits_for_op_walks_modify_of_box() {
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
        infer_traits_for_op(1, &ops),
        InferredTraits::bounded_connected()
    );
}

/// `GeomRef::Sub(_)` defaults to `InferredTraits::all()` — the safe v0.1
/// assumption for cross-component geometry references (sub-component
/// realizations are not visible from this op array). When future work
/// wires cross-component inference, this test will break deliberately,
/// flagging the design-decision change for review. Pinned per plan
/// design decision §3.
#[test]
fn infer_traits_for_op_treats_geomref_sub_as_bounded() {
    let ops = vec![CompiledGeometryOp::Boolean {
        op: BooleanOp::Union,
        left: GeomRef::Sub("child".to_string()),
        right: GeomRef::Sub("other".to_string()),
    }];
    // Both Sub references default to all(); Union preserves only Bounded.
    assert_eq!(infer_traits_for_op(0, &ops), InferredTraits::bounded_only());
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
    let box_expr = make_function_call(
        "box",
        vec![ten_mm(), ten_mm(), ten_mm()],
        Type::Geometry,
    );
    assert_eq!(infer_traits_for_expr(&box_expr), InferredTraits::all());

    let cylinder_expr = make_function_call(
        "cylinder",
        vec![ten_mm(), ten_mm()],
        Type::Geometry,
    );
    assert_eq!(infer_traits_for_expr(&cylinder_expr), InferredTraits::all());

    let sphere_expr = make_function_call("sphere", vec![ten_mm()], Type::Geometry);
    assert_eq!(infer_traits_for_expr(&sphere_expr), InferredTraits::all());

    let tube_expr = make_function_call(
        "tube",
        vec![ten_mm(), ten_mm(), ten_mm()],
        Type::Geometry,
    );
    assert_eq!(infer_traits_for_expr(&tube_expr), InferredTraits::all());
}

/// `infer_traits_for_expr` recurses into geometry-typed args of `union`,
/// applying `combine_union`. Two Bounded boxes union to `bounded_only`
/// (Connected and Convex dropped per `combine_union`).
#[test]
fn infer_traits_for_expr_handles_nested_union_of_boxes() {
    let ten_mm = || CompiledExpr::literal(Value::Real(10.0), Type::length());
    let box_a = make_function_call(
        "box",
        vec![ten_mm(), ten_mm(), ten_mm()],
        Type::Geometry,
    );
    let box_b = make_function_call(
        "box",
        vec![ten_mm(), ten_mm(), ten_mm()],
        Type::Geometry,
    );
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
