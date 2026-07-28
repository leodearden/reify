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
use reify_test_support::compile_source;

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
