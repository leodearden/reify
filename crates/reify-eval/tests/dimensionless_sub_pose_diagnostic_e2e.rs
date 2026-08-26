//! End-to-end gate for task 6099: a **dimensionless** `at` sub-pose must produce
//! a build-failing diagnostic naming the sub, instead of silently placing the
//! whole child subtree at the world origin.
//!
//! # The bug
//!
//! `transform3(orient_identity(), vec3(5.0, 0.0, 0.0))` is a perfectly legal
//! `Value::Transform` — `transform3` performs no dimension validation — but its
//! translation is DIMENSIONLESS. Composing it onto the walk's LENGTH-seeded
//! world pose trips `compose_transforms`' translation-dimension gate, which
//! returns a bare `Value::Undef` with no diagnostic channel to report it. That
//! `Undef` then falls into the placement decomposition's `None` arm, which is
//! indistinguishable from a genuine identity pose: no `ApplyTransform` op is
//! issued, and the sub builds at the origin. The source passed `reify check`
//! clean and exported a wrong STEP file with ZERO diagnostics.
//!
//! # Why these assertions are shaped the way they are
//!
//! Every assertion filters on `Severity::Error`. An unrelated Mock-kernel
//! `Severity::Warning` ("topology-attribute seeding failed ...") is present on
//! this path — measured during premise verification — so asserting on a total
//! diagnostic count would be wrong.
//!
//! AABBs are deliberately NOT asserted: the Mock kernel ignores
//! `ApplyTransform`, so a correctly mm-posed sibling reports the same
//! `([0,0,0],[1,1,1])` box as a misplaced one. The user-observable signal the
//! task specifies is the diagnostic, and asserting on it needs no OCCT.
//!
//! Both surfacing wrappers — `surface_export_bodies` (the STEP export path used
//! by `Engine::build`) and `surface_subtree` (the tessellation/GUI path) —
//! delegate to the same `walk_placed_realizations`, so exercising the
//! tessellation path here covers the STEP export named in the bug report.

use reify_core::Severity;
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// Build a Mock-kernel engine for structural surfacing assertions.
///
/// Same shape as `sub_placement_assembly_gate.rs`'s helper of the same name.
fn mock_engine() -> reify_eval::Engine {
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// The headline repro: one correctly mm-posed sub beside one dimensionless sub.
const MIXED_POSE_SRC: &str = r#"
structure Widget {
    let body = box(2mm, 2mm, 2mm)
}
structure Asm {
    sub good : Widget at transform3(orient_identity(), vec3(5.0mm, 0.0mm, 0.0mm))
    sub bad  : Widget at transform3(orient_identity(), vec3(5.0, 0.0, 0.0))
}"#;

/// Compile `source` and assert it produces zero Error-severity diagnostics.
///
/// The dimensionless pose is a *type-correct* expression, so it MUST keep
/// compiling clean — that is the "passes `reify check` clean" half of the bug
/// report, and this task fixes the build-time drop without waiting on the
/// registry-τ migration that would reject it at compile time.
fn compile_clean(source: &str) -> reify_compiler::CompiledModule {
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "source must still compile clean: {compile_errors:?}"
    );
    compiled
}

/// The Error-severity messages emitted while tessellating `compiled`.
fn tessellate_errors(compiled: &reify_compiler::CompiledModule) -> (Vec<String>, Vec<String>) {
    let mut engine = mock_engine();
    let result = engine.tessellate_realizations(compiled);
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    let paths: Vec<String> = result
        .meshes
        .iter()
        .map(|s| s.entity_path.clone())
        .collect();
    (errors, paths)
}

#[test]
fn dimensionless_sub_pose_is_a_build_error() {
    let compiled = compile_clean(MIXED_POSE_SRC);
    let (errors, _) = tessellate_errors(&compiled);

    assert_eq!(
        errors.len(),
        1,
        "exactly one Error-severity diagnostic expected, got {errors:?}"
    );
    let msg = errors[0].to_lowercase();
    assert!(
        msg.contains("bad"),
        "the diagnostic must name the offending sub `bad`: {}",
        errors[0]
    );
    assert!(
        msg.contains("asm"),
        "the diagnostic must name the enclosing structure `Asm`: {}",
        errors[0]
    );
    assert!(
        msg.contains("dimensionless"),
        "the diagnostic must name the offending dimension: {}",
        errors[0]
    );
    assert!(
        !msg.contains("good"),
        "the correctly mm-posed sibling must not be implicated: {}",
        errors[0]
    );
}

#[test]
fn all_length_posed_assembly_emits_no_pose_errors() {
    // False-positive control: a depth-2 all-mm assembly, plus one sub with NO
    // `at` clause at all (covering `eval_sub_pose`'s identity `None` arm).
    let source = r#"
structure Leaf {
    let body = box(1mm, 1mm, 1mm)
}
structure Mid {
    sub leaf : Leaf at transform3(orient_identity(), vec3(2.0mm, 0.0mm, 0.0mm))
    sub bare : Leaf
}
structure Asm {
    sub mid : Mid at transform3(orient_identity(), vec3(10.0mm, 0.0mm, 0.0mm))
}"#;
    let compiled = compile_clean(source);
    let (errors, paths) = tessellate_errors(&compiled);

    assert!(
        errors.is_empty(),
        "a fully LENGTH-posed assembly must emit no pose errors: {errors:?}"
    );
    for expected in ["Asm.mid.leaf#realization[0]", "Asm.mid.bare#realization[0]"] {
        assert!(
            paths.iter().any(|p| p == expected),
            "composed entity path `{expected}` must still surface; got {paths:?}"
        );
    }
}

#[test]
fn dimensionless_sub_pose_deep_subtree_errors_exactly_once() {
    // `Asm` poses `mid` with a bare dimensionless vec3; `Mid` in turn poses
    // `leaf` correctly in mm. The `Undef` world pose propagates down and would
    // poison `leaf`'s composition too, so without the parent-`Undef`
    // origination guard this would report once per depth level.
    let source = r#"
structure Leaf {
    let body = box(1mm, 1mm, 1mm)
}
structure Mid {
    sub leaf : Leaf at transform3(orient_identity(), vec3(2.0mm, 0.0mm, 0.0mm))
}
structure Asm {
    sub mid : Mid at transform3(orient_identity(), vec3(5.0, 0.0, 0.0))
}"#;
    let compiled = compile_clean(source);
    let (errors, _) = tessellate_errors(&compiled);

    assert_eq!(
        errors.len(),
        1,
        "one authoring mistake must yield exactly one error, not one per depth \
         level, got {errors:?}"
    );
    let msg = errors[0].to_lowercase();
    assert!(
        msg.contains("mid"),
        "the diagnostic must name the sub the author has to edit (`mid`): {}",
        errors[0]
    );
    assert!(
        !msg.contains("leaf"),
        "the poisoned descendant `leaf` must not be reported: {}",
        errors[0]
    );
}

#[test]
fn posed_subtree_still_surfaces_after_diagnostic() {
    // "Diagnose, do not skip the subtree": the fix is purely additive to the
    // diagnostics vector, leaving placement/surfacing shape byte-identical.
    // An implementation that added a `continue` to drop the offending subtree
    // would break this and silently change mesh counts for existing gates.
    let compiled = compile_clean(MIXED_POSE_SRC);
    let (_, paths) = tessellate_errors(&compiled);

    for expected in ["Asm.good#realization[0]", "Asm.bad#realization[0]"] {
        assert!(
            paths.iter().any(|p| p == expected),
            "`{expected}` must still surface; got {paths:?}"
        );
    }
    assert_eq!(
        paths.len(),
        2,
        "exactly two surfaces expected; got {paths:?}"
    );
}
