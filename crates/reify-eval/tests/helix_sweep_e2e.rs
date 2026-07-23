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
//! test in `reify-kernel-occt/tests/helix_sweep_integration.rs`: it
//! additionally covers the compile → eval geometry-error diagnostic channel
//! and the mesh / STEP output path.
//!
//! Profile choice — why a bare `circle(3mm)` rather than a posed
//! `translate(rotate(circle(3mm), …), …)`: the compiler's profile-precondition
//! check (`GeometryProfileRequired`, crates/reify-compiler/src/geometry.rs)
//! requires a `sweep` profile operand that is a statically-known Surface
//! producer to be Closed ∧ Planar. An *inline* `translate(rotate(circle(…)))`
//! operand is a known-combinator `FunctionCall` whose nested profile is opaque
//! to the static inference, so it is rejected at compile time — that guard is
//! independent of this task. A bare `circle(3mm)` is a direct Surface producer
//! that passes the check and still sweeps cleanly along the helix (OCCT
//! `MakePipe` transports the section along the spine frame), producing exactly
//! one non-empty solid. The posed, volume-accurate case (π·r²·L within 5%) is
//! covered by the kernel integration test; here we only need the full-pipeline
//! no-error + non-empty-geometry acceptance.
//!
//! Modeled on the full-pipeline section of `tube_pipe_e2e.rs`.

use reify_core::{ModulePath, Severity};
use reify_ir::ExportFormat;

/// Full-pipeline e2e: `sweep(circle(3mm), helix(24mm, 7mm, 63mm))` must flow
/// through parse → compile → tessellate → STEP export with no Error
/// diagnostics and non-empty mesh / STEP output.
#[test]
fn helix_sweep_through_full_pipeline_succeeds() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let groove = sweep(circle(3mm), helix(24mm, 7mm, 63mm))
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
    assert_eq!(
        tess_result.meshes.len(),
        1,
        "expected 1 mesh (single realization), got {}",
        tess_result.meshes.len()
    );
    let mesh = &tess_result.meshes[0].mesh;
    assert!(
        !mesh.vertices.is_empty(),
        "helix-swept mesh should have vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "helix-swept mesh should have triangles"
    );

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
