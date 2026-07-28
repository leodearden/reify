//! Compiler typing tests for the orientation / transform / frame constructor
//! family (task 5344): every member must resolve its call-site cell type to
//! `Type::Orientation(3)` / `Type::Frame(3)` / `Type::Transform(3)` rather than
//! falling through `expr.rs`'s `NoUserFunctions` ladder to the first-arg
//! fallback, and the three zero-arg members must NOT trip the "cannot infer
//! return type of zero-arg function" warning.
//!
//! Structurally mirrors `tests/affine_constructor_typing_tests.rs`, the sibling
//! family's template.
//!
//! Concretely, the fallback mistyped these call sites:
//! - `orient_identity()` / `frame3_identity()` / `transform3_identity()` — zero
//!   args, so the fallback's `unwrap_or_else` fired: typed `Real`, plus one
//!   warning per call site (25 of them in `prj/printer_v01/printer.ri`);
//! - `orient_axis_angle(axis, angle)` — silently `Vector{3}`, adopted from the
//!   rotation-AXIS first argument;
//! - `transform3(orient, vec)` — silently whatever the orientation typed as;
//! - `frame3(point, orient)` — silently `Point`.
//!
//! RED until the `is_orientation_typed_fn` arm is wired into the ladder.

use reify_core::Type;
use reify_test_support::{compile_source, compile_source_with_stdlib, get_let_expr_in};

/// Compile `source` and return the `result_type` of value cell `cell_name` in
/// template `template_name`. Same traversal the affine sibling test does inline.
fn find_cell_type(source: &str, template_name: &str, cell_name: &str) -> Type {
    let compiled = compile_source(source);
    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| panic!("template '{template_name}' not found"));
    let cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == cell_name)
        .unwrap_or_else(|| panic!("value cell '{cell_name}' not found in {template_name}"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{cell_name}' has no default_expr"))
        .result_type
        .clone()
}

/// Dimensionless numeric args keep these sources independent of the stdlib unit
/// registry (`compile_source` has no `unit` decls in scope). The constructor
/// result types are arg-agnostic, so this does not weaken the assertions — for
/// `orient_axis_angle` the axis argument still types as
/// `Vector{3, dimensionless}`, which is exactly the wrong first-arg type the old
/// fallback adopted.
const ORIENT_HOST: &str = r#"
    structure OrientHost {
        let identity    = orient_identity()
        let quaternion  = orient_quaternion(1.0, 0.0, 0.0, 0.0)
        let euler       = orient_euler("xyz", 0.0, 0.0, 0.0)
        let basis       = orient_basis(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0))
        let look_at     = orient_look_at(vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0))
        let axis_angle  = orient_axis_angle(vec3(0.0, 0.0, 1.0), 90.0)
        let exp         = orient_exp(vec3(0.0, 0.0, 0.0))
        let inverse     = orient_inverse(orient_identity())
        let compose     = orient_compose(orient_identity(), orient_identity())
        let slerp       = orient_slerp(orient_identity(), orient_identity(), 0.5)
    }
"#;

/// All ten Orientation producers type as `Type::Orientation(3)`.
#[test]
fn orientation_constructors_type_as_orientation_3() {
    for cell in [
        "identity",
        "quaternion",
        "euler",
        "basis",
        "look_at",
        "axis_angle",
        "exp",
        "inverse",
        "compose",
        "slerp",
    ] {
        assert_eq!(
            find_cell_type(ORIENT_HOST, "OrientHost", cell),
            Type::Orientation(3),
            "cell '{cell}' must type as Orientation(3)"
        );
    }
}

/// Both Frame producers type as `Type::Frame(3)`.
#[test]
fn frame_constructors_type_as_frame_3() {
    let source = r#"
        structure FrameHost {
            let f  = frame3(point3(0.0, 0.0, 0.0), orient_identity())
            let fi = frame3_identity()
        }
    "#;
    for cell in ["f", "fi"] {
        assert_eq!(
            find_cell_type(source, "FrameHost", cell),
            Type::Frame(3),
            "cell '{cell}' must type as Frame(3)"
        );
    }
}

/// All six Transform producers type as `Type::Transform(3)` — explicitly
/// including `frame_to_frame`, which despite its `frame_` prefix returns
/// `Value::Transform` (`geometry.rs:512`), not a Frame.
#[test]
fn transform_constructors_type_as_transform_3() {
    let source = r#"
        structure TransformHost {
            let t        = transform3(orient_identity(), vec3(0.0, 0.0, 0.0))
            let ti       = transform3_identity()
            let compose  = transform_compose(transform3_identity(), transform3_identity())
            let inverse  = transform_inverse(transform3_identity())
            let exp      = transform_exp(transform_log(transform3_identity()))
            let f2f      = frame_to_frame(frame3_identity(), frame3_identity())
        }
    "#;
    for cell in ["t", "ti", "compose", "inverse", "exp", "f2f"] {
        assert_eq!(
            find_cell_type(source, "TransformHost", cell),
            Type::Transform(3),
            "cell '{cell}' must type as Transform(3)"
        );
    }
    // Guard the prefix trap specifically: frame_to_frame must NOT be grouped
    // with the Frame producers.
    assert_ne!(
        find_cell_type(source, "TransformHost", "f2f"),
        Type::Frame(3),
        "frame_to_frame must type as Transform(3), not Frame(3)"
    );
}

/// The task's named acceptance test, at the call-site level: the rotation AXIS
/// is the first argument and types as `Vector{3}`, so the old first-arg fallback
/// silently gave the whole call that Vector type.
#[test]
fn orient_axis_angle_does_not_type_as_vector() {
    let ty = find_cell_type(ORIENT_HOST, "OrientHost", "axis_angle");
    assert_eq!(
        ty,
        Type::Orientation(3),
        "orient_axis_angle(axis, angle) must type as Orientation(3)"
    );
    assert!(
        !matches!(ty, Type::Vector { .. }),
        "orient_axis_angle must NOT adopt the first-arg (axis) Vector type, got {ty:?}"
    );
}

/// The three zero-arg members are the only names in the family that can trip the
/// zero-arg warning — it is emitted solely from the first-arg fallback's
/// `unwrap_or_else` branch, reached only when `compiled_args.first()` is `None`.
#[test]
fn zero_arg_orientation_constructors_emit_no_infer_warning() {
    let source = r#"
        structure ZeroArgHost {
            let o = orient_identity()
            let f = frame3_identity()
            let t = transform3_identity()
        }
    "#;
    let compiled = compile_source(source);
    let warning = compiled
        .diagnostics
        .iter()
        .find(|d| d.message.contains("cannot infer return type"));
    assert!(
        warning.is_none(),
        "zero-arg orientation constructors must not emit the infer warning, got: {:?}",
        warning.map(|d| &d.message)
    );
}

/// The `prj/printer_v01/printer.ri` shape: a zero-arg `orient_identity()` nested
/// inside `transform3`. Under the old fallback the inner call typed `Real`, and
/// the outer `transform3` then adopted that `Real` through the first-arg chain —
/// so this pins that the fix composes through nesting, not just at leaf sites.
#[test]
fn nested_transform3_over_orient_identity_types_as_transform_3() {
    let source = r#"
        structure NestedHost {
            let t = transform3(orient_identity(), vec3(0.0, 0.0, 0.0))
        }
    "#;
    let ty = find_cell_type(source, "NestedHost", "t");
    assert_eq!(
        ty,
        Type::Transform(3),
        "transform3(orient_identity(), vec3(..)) must type as Transform(3)"
    );
    assert!(
        !matches!(ty, Type::Scalar { .. }),
        "nested transform3 must not inherit the inner call's fallback Real type, got {ty:?}"
    );
}

/// The task's headline acceptance criterion, pinned inside the suite so it
/// cannot silently regress independently of a manual `reify check` run.
///
/// Reproduces the `prj/printer_v01/printer.ri` usage shape — repeated
/// `orient_identity()` bindings, `orient_axis_angle(axis, angle)`,
/// `transform3(orient_identity(), vec3(..))`, `frame3(point3(..),
/// orient_identity())`, and a nested `transform_compose(transform3(..),
/// transform3(..))` — and asserts BOTH halves of the criterion:
///
/// (a) ZERO "cannot infer return type" diagnostics. Verified achievable by this
///     task alone: `orient_identity()` is the only zero-arg call syntax present
///     in printer.ri (25 sites), and that warning is emitted solely from the
///     first-arg fallback's `unwrap_or_else` branch.
/// (b) The nested cells carry the right type END-TO-END, proving the fix
///     composes through nesting rather than only at leaf call sites.
///
/// Uses `compile_source_with_stdlib` so the dimensioned literals (`1m`, `90deg`,
/// `0mm`) printer.ri actually passes resolve — the arguments are the real thing,
/// not dimensionless stand-ins.
#[test]
fn printer_shaped_source_emits_zero_infer_warnings() {
    let source = r#"
        structure PrinterShaped {
            let rot_to_y = orient_axis_angle(vec3(1m, 0mm, 0mm), 90deg)
            let a_upper = transform3(rot_to_y, vec3(0mm, 0mm, 10mm))
            let a_lower = transform3(orient_identity(), vec3(0mm, 0mm, 20mm))
            let b_upper = transform3(orient_identity(), vec3(0mm, 0mm, 30mm))
            let b_lower = transform3(orient_identity(), vec3(0mm, 0mm, 40mm))
            let stacked = transform_compose(
                transform3(orient_identity(), vec3(0mm, 0mm, 50mm)),
                transform3(orient_identity(), vec3(0mm, 0mm, 60mm))
            )
            let mount = frame3(point3(0mm, 0mm, 0mm), orient_identity())
        }
    "#;
    let module = compile_source_with_stdlib(source);

    // (a) the headline criterion.
    let infer_warnings: Vec<&str> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("cannot infer return type"))
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        infer_warnings.is_empty(),
        "printer-shaped source must emit ZERO 'cannot infer return type' \
         warnings (was 25 across prj/printer_v01/printer.ri); got: {infer_warnings:#?}"
    );

    // (b) the types compose through nesting, not just at leaf call sites.
    for cell in ["a_upper", "a_lower", "b_upper", "b_lower", "stacked"] {
        assert_eq!(
            find_stdlib_cell_type(source, "PrinterShaped", cell),
            Type::Transform(3),
            "cell '{cell}' must type as Transform(3) end-to-end"
        );
    }
    assert_eq!(
        find_stdlib_cell_type(source, "PrinterShaped", "rot_to_y"),
        Type::Orientation(3),
        "orient_axis_angle(axis, angle) must type as Orientation(3), not the \
         axis argument's Vector type"
    );
    assert_eq!(
        find_stdlib_cell_type(source, "PrinterShaped", "mount"),
        Type::Frame(3),
        "frame3(point3(..), orient_identity()) must type as Frame(3), not the \
         point argument's type"
    );
}

// ── Datum-neighbour audit (the same defect, one resolver over) ────────────────
//
// `plane_xy`/`plane_xz`/`plane_yz` and `axis_x`/`axis_y`/`axis_z` share the
// orientation family's first-arg-fallback defect but belong to the DATUM
// vocabulary, so they are claimed by `units.rs::datum_constructor_result_type`
// (alongside `midplane`/`plane_through`/`axis_through`/`frame_at`) rather than
// by `ORIENTATION_TYPED_FN_NAMES` — the two families stay cleanly disjoint.
//
// These six take exactly ONE argument, not zero: `reify_stdlib::geometry`'s
// `make_plane` (:1112) and `make_axis` (:1152) both hard-check `args.len() != 1`.
// So unlike the three zero-arg orientation constructors they mistype SILENTLY —
// they never trip the "cannot infer return type" warning, which is precisely why
// the defect went unnoticed here. Do NOT take the LSP completion table
// (`crates/reify-lsp/src/completion.rs`) as the source of truth: it declares
// `plane_xy() -> Frame` and `axis_x() -> Vector`, wrong in BOTH arity and return
// type. The evaluator is the source of truth.
//
// These use `compile_source_with_stdlib` so dimensioned literals (`10mm`)
// resolve — the wrong types the fallback adopted (`Scalar<Length>` /
// `Point3<Length>`) are only reproducible with real dimensioned arguments.

/// Compile with the stdlib prelude and return `cell_name`'s `result_type`.
fn find_stdlib_cell_type(source: &str, template_name: &str, cell_name: &str) -> Type {
    let module = compile_source_with_stdlib(source);
    get_let_expr_in(&module, template_name, cell_name)
        .result_type
        .clone()
}

/// `plane_xy(10mm)` types as `Type::Plane`, NOT the `Scalar<Length>` of its
/// offset argument.
#[test]
fn axis_aligned_plane_constructors_type_as_plane() {
    let source = r#"
        structure PlaneHost {
            let pxy = plane_xy(10mm)
            let pxz = plane_xz(0mm)
            let pyz = plane_yz(5mm)
        }
    "#;
    for cell in ["pxy", "pxz", "pyz"] {
        let ty = find_stdlib_cell_type(source, "PlaneHost", cell);
        assert_eq!(
            ty,
            Type::Plane,
            "cell '{cell}' must type as Plane, not the offset argument's type"
        );
        assert!(
            !matches!(ty, Type::Scalar { .. }),
            "cell '{cell}' must not adopt the offset argument's Scalar<Length> \
             (the old first-arg fallback), got {ty:?}"
        );
    }
}

/// `axis_x(point3(0mm, 0mm, 0mm))` types as `Type::Axis`, NOT a type derived
/// from its origin argument.
///
/// Measured pre-fix type: `Scalar<Length>`, not `Point3<Length>` — the fallback
/// chains, because `point3` is itself unclaimed and adopts `0mm`'s
/// `Scalar<Length>` first. (`point3`/`vec3` stay unclaimed here on purpose:
/// `Type::Point`/`Type::Vector` carry an argument-DEPENDENT quantity slot, so
/// they need a different resolver shape and are deferred to a follow-up.) The
/// assertion below is therefore written against `Type::Axis` plus a negative on
/// `Type::Point`, both of which hold regardless of which link in that chain the
/// wrong type came from.
#[test]
fn axis_aligned_axis_constructors_type_as_axis() {
    let source = r#"
        structure AxisHost {
            let ax = axis_x(point3(0mm, 0mm, 0mm))
            let ay = axis_y(point3(0mm, 0mm, 0mm))
            let az = axis_z(point3(0mm, 0mm, 0mm))
        }
    "#;
    for cell in ["ax", "ay", "az"] {
        let ty = find_stdlib_cell_type(source, "AxisHost", cell);
        assert_eq!(
            ty,
            Type::Axis,
            "cell '{cell}' must type as Axis, not the origin argument's type"
        );
        assert!(
            !matches!(ty, Type::Point { .. } | Type::Scalar { .. }),
            "cell '{cell}' must not inherit an argument type through the old \
             first-arg fallback chain, got {ty:?}"
        );
    }
}
