//! Compiler recognition and lowering tests for `zone_slab` (task 4477 step-5).
//!
//! RED until step-6 adds `ModifyKind::ZoneSlab` — this file references that
//! variant directly, causing a compile error until it exists.

use reify_compiler::{CompiledGeometryOp, ModifyKind, GEOMETRY_FUNCTION_NAMES};
use reify_compiler::geometry_traits_inference::{GeomDim, try_infer_traits_for_function_call};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// zone_slab must appear in GEOMETRY_FUNCTION_NAMES (which backs is_geometry_function).
#[test]
fn zone_slab_is_geometry_function() {
    assert!(
        GEOMETRY_FUNCTION_NAMES.contains(&"zone_slab"),
        "\"zone_slab\" must be in GEOMETRY_FUNCTION_NAMES"
    );
}

/// Compiling `zone_slab(rectangle(...), w)` must produce a
/// `CompiledGeometryOp::Modify { kind: ModifyKind::ZoneSlab, ... }` realization
/// with the face as the geometry target and the width as the sole named arg.
#[test]
fn zone_slab_lowers_to_modify_zone_slab() {
    let source = r#"
structure S {
    let f = rectangle(width: 40mm, height: 20mm)
    let s = zone_slab(f, 2mm)
}
"#;
    let module = compile_source_with_stdlib(source);
    assert!(
        errors_only(&module).is_empty(),
        "unexpected compile errors: {:?}",
        errors_only(&module)
    );

    let template = &module.templates[0];
    let zone_op = template
        .realizations
        .iter()
        .flat_map(|r| r.operations.iter())
        .find(|op| matches!(op, CompiledGeometryOp::Modify { kind: ModifyKind::ZoneSlab, .. }));

    assert!(
        zone_op.is_some(),
        "expected a Modify(ZoneSlab) op in the compiled realizations, got: {:?}",
        template
            .realizations
            .iter()
            .flat_map(|r| r.operations.iter())
            .collect::<Vec<_>>()
    );

    // Verify the width arg is present.
    if let Some(CompiledGeometryOp::Modify { kind: ModifyKind::ZoneSlab, args, .. }) = zone_op {
        assert!(
            args.iter().any(|(name, _)| name == "width"),
            "ZoneSlab Modify op must carry a \"width\" arg, got: {:?}",
            args.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
        );
    }
}

/// zone_slab must infer GeomDim::Solid — offsetting a face ±w/2 and capping
/// produces a solid, not a surface.
#[test]
fn zone_slab_infers_solid_dimension() {
    let t = try_infer_traits_for_function_call("zone_slab", &[])
        .expect("zone_slab must have an explicit dispatch arm in geometry_traits_inference");
    assert_eq!(
        t.dimension,
        GeomDim::Solid,
        "zone_slab must infer GeomDim::Solid"
    );
}
