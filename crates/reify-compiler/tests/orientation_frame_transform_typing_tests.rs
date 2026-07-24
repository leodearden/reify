//! Compiler typing tests for the orientation / transform / frame constructor
//! family (task 5344): `orient_identity` / `orient_axis_angle` / `transform3` /
//! `frame3` must resolve their call-site cell type to
//! `Type::Orientation(3)` / `Type::Transform(3)` / `Type::Frame(3)` rather than
//! the first-arg fallback, and the zero-arg `orient_identity()` must NOT trip
//! the "cannot infer return type of zero-arg function" warning.
//!
//! A miniature reproduction of the `prj/printer_v01/printer.ri` acceptance
//! criterion (zero "cannot infer return type" warnings — `orient_identity()` is
//! the sole zero-arg call site in that file).
//!
//! RED until step-6 wires the `is_orientation_typed_fn` arm into the `expr.rs`
//! `NoUserFunctions` ladder: `orient_identity()` falls to the zero-arg fallback
//! (typed `Real` + warning) and `orient_axis_angle(vec3, angle)` adopts its
//! first arg's `Vector{3}` type.

use reify_core::Type;
use reify_test_support::compile_source;

/// Compile a host structure whose value cells exercise each constructor with a
/// mix of nested calls (matching how the printer.ri call sites nest
/// `orient_identity()` inside `transform3`/`frame3`).
fn compile_host() -> reify_compiler::CompiledModule {
    // Dimensionless numeric args keep the test independent of the stdlib unit
    // registry (`compile_source` has no `unit` decls in scope). The constructor
    // result types are arg-agnostic, so this does not weaken the assertions —
    // `vec3(1.0, 0.0, 0.0)` still types as `Vector{3, dimensionless}`, which is
    // exactly the wrong first-arg type the old fallback would have adopted for
    // `orient_axis_angle`.
    let source = r#"
        structure OrientHost {
            let a = orient_identity()
            let b = orient_axis_angle(vec3(1.0, 0.0, 0.0), 90.0)
            let t = transform3(orient_identity(), vec3(0.0, 0.0, 0.0))
            let f = frame3(point3(0.0, 0.0, 0.0), orient_identity())
        }
    "#;
    compile_source(source)
}

/// Fetch the `result_type` of value cell `cell_name` in `OrientHost`.
fn cell_result_type(compiled: &reify_compiler::CompiledModule, cell_name: &str) -> Type {
    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "OrientHost")
        .expect("OrientHost template");
    let cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == cell_name)
        .unwrap_or_else(|| panic!("value cell '{cell_name}' not found in OrientHost"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{cell_name}' has no default_expr"))
        .result_type
        .clone()
}

#[test]
fn orientation_transform_frame_constructors_type_to_their_nominal() {
    let compiled = compile_host();

    // a = orient_identity() → Orientation(3) (zero-arg; the printer.ri warning fix).
    assert_eq!(
        cell_result_type(&compiled, "a"),
        Type::Orientation(3),
        "orient_identity() must type as Orientation(3)"
    );

    // b = orient_axis_angle(vec3, angle) → Orientation(3), NOT the first-arg
    // Vector type (the exact bug this task fixes).
    assert_eq!(
        cell_result_type(&compiled, "b"),
        Type::Orientation(3),
        "orient_axis_angle(vec3, angle) must type as Orientation(3)"
    );
    assert_ne!(
        cell_result_type(&compiled, "b"),
        Type::vec3(Type::dimensionless_scalar()),
        "orient_axis_angle must NOT adopt the first-arg Vector type"
    );

    // t = transform3(orient, vec3) → Transform(3).
    assert_eq!(
        cell_result_type(&compiled, "t"),
        Type::Transform(3),
        "transform3(orient, vec3) must type as Transform(3)"
    );

    // f = frame3(point, orient) → Frame(3).
    assert_eq!(
        cell_result_type(&compiled, "f"),
        Type::Frame(3),
        "frame3(point, orient) must type as Frame(3)"
    );
}

#[test]
fn orient_identity_emits_no_zero_arg_return_type_warning() {
    let compiled = compile_host();

    // The printer.ri acceptance criterion, reproduced in miniature: the zero-arg
    // orient_identity() call sites (including the ones nested inside transform3 /
    // frame3) must NOT emit the zero-arg fallback warning.
    let warning = compiled.diagnostics.iter().find(|d| {
        d.message
            .contains("cannot infer return type of zero-arg function")
    });
    assert!(
        warning.is_none(),
        "orient_identity() must not emit the zero-arg fallback warning, got: {:?}",
        warning.map(|d| &d.message)
    );
}
