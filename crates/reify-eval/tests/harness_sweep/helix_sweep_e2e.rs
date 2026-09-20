//! End-to-end acceptance test for sweeping a profile along a `helix()` spine
//! (task #5342 — the task's explicitly-named ".ri test pins it").
//!
//! Exercises the full source → parse → compile → Engine(real
//! OcctKernelHandle) → tessellate / build(STEP) pipeline. Before the
//! `make_helix_wire` fix the helix wire carried only a pcurve-on-cylinder
//! with no `Geom_Curve` 3D representation, so `BRepOffsetAPI_MakePipe`
//! (behind `GeometryOp::Sweep`) failed — surfacing as an Error-severity
//! geometry diagnostic with no mesh/STEP produced. After the fix the sweep
//! succeeds end-to-end. This is a higher layer than the kernel integration
//! test in `reify-kernel-occt/tests/harness_occt/helix_sweep_integration.rs`: it
//! additionally covers the compile → eval geometry-error diagnostic channel
//! and the mesh / STEP output path.
//!
//! Profile choice — why the profile is posed through a NAMED BINDING
//! (`aux let prof = translate(rotate(circle(3mm), …), …)`) rather than either a
//! bare `circle(3mm)` or an *inline* `translate(rotate(circle(…)))`. Two
//! independent constraints leave exactly one legal spelling:
//!
//! 1. A bare `circle(3mm)` carries its own +Z normal, which for the
//!    `helix(24mm, 7mm, 63mm)` spine sits only ~2.66° off the spine's start
//!    tangent (|t̂·n̂| = 0.046370259, measured). `sweep` transports the profile
//!    AS POSED, so that near-coplanar section sweeps edge-on: the body was
//!    always degenerate here — its volume came back negative and `volume()`
//!    downgraded it to Undef — and this test passed only because it asserts
//!    non-emptiness, never a value. Task #7248 made that pose an Error-severity
//!    geometry diagnostic, so a bare circle is now rejected outright.
//! 2. An inline `translate(rotate(circle(…)))` operand is rejected at COMPILE
//!    time, independently of this task: the compiler's profile-precondition
//!    check (`GeometryProfileRequired`, crates/reify-compiler/src/geometry.rs)
//!    requires a statically-known Surface producer to be Closed ∧ Planar, and a
//!    known-combinator `FunctionCall` hides its nested profile from that
//!    inference.
//!
//! The named binding satisfies both: the `let` operand is opaque to the profile
//! precondition (which admits it), and the pose puts the section normal to the
//! helix start tangent, which the pose guard admits and which sweeps to a real
//! tube — 3.836681796674759e-5 m³, matching π·r²·L. Here we still assert only
//! the full-pipeline no-error + non-empty-geometry acceptance; the
//! volume-accurate case (π·r²·L within 5%) stays covered by the kernel
//! integration test.
//!
//! Modeled on the full-pipeline section of `tube_pipe_e2e.rs`.

use reify_core::{ModulePath, Severity};
use reify_ir::ExportFormat;

/// Full-pipeline e2e: a circle posed normal to the helix start tangent, swept
/// along `helix(24mm, 7mm, 63mm)`, must flow through parse → compile →
/// tessellate → STEP export with no Error diagnostics and non-empty mesh /
/// STEP output.
#[test]
fn helix_sweep_through_full_pipeline_succeeds() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    aux let prof = translate(rotate(circle(3mm), 1.0, 0.0, 0.0, -90deg), 24mm, 0mm, 0mm)
    let groove = sweep(prof, helix(24mm, 7mm, 63mm))
}"#;

    // ---- Parse ----
    let parsed = reify_syntax::parse(source, ModulePath::single("test_helix_sweep_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // ---- Compile ----
    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors: {:?}",
        compile_errors
    );

    // ---- Tessellate with a real OCCT kernel via SingleKernelHolder ----
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let tess_result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = tess_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors sweeping circle along helix: {:?}",
        geom_errors
    );
    // Posing through a named binding costs a second realization: the section
    // `prof` realizes alongside the swept body `groove`. Asserting over ALL
    // surfaces covers the swept body without pinning it to a realization index.
    assert_eq!(
        tess_result.meshes.len(),
        2,
        "expected 2 meshes (posed section + helix-swept body), got {}",
        tess_result.meshes.len()
    );
    for surface in &tess_result.meshes {
        assert!(
            !surface.mesh.vertices.is_empty(),
            "{} should have vertices",
            surface.entity_path
        );
        assert!(
            !surface.mesh.indices.is_empty(),
            "{} should have triangles",
            surface.entity_path
        );
    }

    // ---- STEP export through the same pipeline (fresh engine/planner;
    // tessellate and build each consume the single registered kernel
    // handle in SingleKernelHolder) ----
    let checker2 = reify_constraints::SimpleConstraintChecker;
    let mut planner2 = reify_geometry::SingleKernelHolder::new();
    planner2.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine2 = reify_eval::Engine::new(Box::new(checker2), Some(Box::new(planner2)));

    let build_result = engine2.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "unexpected build errors: {:?}",
        build_errors
    );
    let step = build_result
        .geometry_output
        .expect("helix sweep should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");
}
