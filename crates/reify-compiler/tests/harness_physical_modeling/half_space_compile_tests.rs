//! Compiler recognition and lowering tests for `half_space` (task #3465), in its
//! `half_space(origin: Point3<Length>, normal: Direction)` form (direction-typing
//! ι-c).

use reify_compiler::{CompiledGeometryOp, GEOMETRY_FUNCTION_NAMES, PrimitiveKind};
use reify_test_support::{
    compile_source_with_stdlib, compile_source_with_stdlib as compile, errors_only,
};

/// `half_space` must appear in `GEOMETRY_FUNCTION_NAMES` (which backs
/// `is_geometry_function`). Already wired in step-2; this test pins it.
#[test]
fn half_space_is_geometry_function() {
    assert!(
        GEOMETRY_FUNCTION_NAMES.contains(&"half_space"),
        "\"half_space\" must be in GEOMETRY_FUNCTION_NAMES"
    );
}

/// Compiling `half_space(point3(0mm, 0mm, 0mm), dir(0, 0, 1))` must produce a
/// `CompiledGeometryOp::Primitive { kind: PrimitiveKind::HalfSpace, args }` whose
/// `args` are exactly the Point3 `origin` and the Direction `normal`, in that
/// order (direction-typing ι-c).
#[test]
fn half_space_lowers_to_primitive_half_space_with_origin_and_normal() {
    let source = r#"
structure S {
    let hs = half_space(point3(0mm, 0mm, 0mm), dir(0, 0, 1))
}
"#;
    let module = compile_source_with_stdlib(source);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected compile errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let half_space_op = template
        .realizations
        .iter()
        .flat_map(|r| r.operations.iter())
        .find(|op| {
            matches!(
                op,
                CompiledGeometryOp::Primitive {
                    kind: PrimitiveKind::HalfSpace,
                    ..
                }
            )
        });

    let Some(CompiledGeometryOp::Primitive { args, .. }) = half_space_op else {
        panic!(
            "expected a Primitive(HalfSpace) op in the compiled realizations, got: {:?}",
            template
                .realizations
                .iter()
                .flat_map(|r| r.operations.iter())
                .collect::<Vec<_>>()
        );
    };
    let arg_names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        arg_names,
        vec!["origin", "normal"],
        "the HalfSpace Primitive op must carry exactly [origin, normal] in order"
    );
}

/// The retired scalar `half_space(0mm, 0mm, 0mm, 0, 0, 1)` is an arity error
/// that names the Direction form (PRD direction-typing B13).
#[test]
fn half_space_retired_6_arg_call_names_the_direction_form() {
    let source = r#"
structure S {
    let hs = half_space(0mm, 0mm, 0mm, 0, 0, 1)
}
"#;
    let module = compile(source);
    let expected =
        "half_space() expects 2 arguments: half_space(point3(x, y, z), dir(x, y, z)), got 6";
    assert!(
        errors_only(&module).iter().any(|d| d.message == expected),
        "expected the Error {expected:?}; got: {:?}",
        errors_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
