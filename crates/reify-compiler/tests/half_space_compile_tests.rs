//! Compiler recognition and lowering tests for `half_space` (task #3465 step-3).
//!
//! RED until step-4 adds the `"half_space"` arm to `compile_geometry_call` in
//! `geometry.rs` — until then, `half_space(...)` calls are not recognised and
//! the lowering test fails because no `Primitive { kind: HalfSpace }` op is
//! produced.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind, GEOMETRY_FUNCTION_NAMES};
use reify_test_support::{compile_source_with_stdlib, compile_source_with_stdlib as compile, errors_only};

/// `half_space` must appear in `GEOMETRY_FUNCTION_NAMES` (which backs
/// `is_geometry_function`). Already wired in step-2; this test pins it.
#[test]
fn half_space_is_geometry_function() {
    assert!(
        GEOMETRY_FUNCTION_NAMES.contains(&"half_space"),
        "\"half_space\" must be in GEOMETRY_FUNCTION_NAMES"
    );
}

/// Compiling `half_space(0mm, 0mm, 0mm, 0, 0, 1)` must produce a
/// `CompiledGeometryOp::Primitive { kind: PrimitiveKind::HalfSpace, args }` where
/// `args` carries exactly the six named scalar args: `px`, `py`, `pz`, `nx`, `ny`, `nz`
/// in that order.
///
/// RED until step-4 adds the `"half_space"` compile arm.
#[test]
fn half_space_lowers_to_primitive_half_space_with_six_named_args() {
    let source = r#"
structure S {
    let hs = half_space(0mm, 0mm, 0mm, 0, 0, 1)
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

    assert!(
        half_space_op.is_some(),
        "expected a Primitive(HalfSpace) op in the compiled realizations, got: {:?}",
        template
            .realizations
            .iter()
            .flat_map(|r| r.operations.iter())
            .collect::<Vec<_>>()
    );

    if let Some(CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::HalfSpace,
        args,
    }) = half_space_op
    {
        let arg_names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            arg_names,
            vec!["px", "py", "pz", "nx", "ny", "nz"],
            "HalfSpace Primitive op must carry exactly the six args [px, py, pz, nx, ny, nz] in order, got: {:?}",
            arg_names
        );
    }
}

/// Wrong-arity call `half_space(0mm, 0mm, 0mm)` (only 3 args, expects 6) must
/// emit an arg-count diagnostic naming `"half_space"`. Mirrors the arity tests
/// for cone, wedge, and torus in `geometry_arg_count_span_tests.rs`.
///
/// RED until step-4 adds the `"half_space"` compile arm (which calls
/// `check_arg_count_exact`).
#[test]
fn half_space_wrong_arity_emits_arg_count_diagnostic() {
    let source = r#"
structure S {
    let hs = half_space(0mm, 0mm, 0mm)
}
"#;
    let module = compile(source);
    let errors = errors_only(&module);

    let arg_count_error = errors
        .iter()
        .find(|d| d.message.contains("half_space") && d.message.contains("6"));

    assert!(
        arg_count_error.is_some(),
        "expected an arg-count diagnostic for `half_space()` with 3 args (expects 6), \
         but none was found. All diagnostics: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
