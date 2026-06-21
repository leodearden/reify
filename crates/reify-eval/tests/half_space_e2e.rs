//! End-to-end eval tests for the `half_space` primitive (task #3465, step-7).
//!
//! # RED until step-8
//!
//! All tests here panic at the `todo!("half_space runtime dispatch …")`
//! stub in `geometry_ops.rs:862` until step-8 wires `GeometryOp::HalfSpace` /
//! `Operation::PrimitiveHalfSpace` through the full eval + kernel stack.
//!
//! The compiler-side arm (PrimitiveKind::HalfSpace, step-2/4) is already wired,
//! so source parses and compiles cleanly; the panic happens at `engine.build()`.
//!
//! Tests:
//!   (a) bounded_intersection_step_export — `intersection(half_space(…), box(…))`
//!       produces a valid STEP file (bounded solid from an unbounded operand).
//!   (b) bare_half_space_is_constructible — a bare `half_space(…)` let realizes
//!       a geometry handle without panic (constructible when unbounded).

use reify_core::{ModulePath, Severity};
use reify_ir::ExportFormat;
use reify_kernel_occt::OCCT_AVAILABLE;

// ---------------------------------------------------------------------------
// Shared pipeline helper (mirrors boolean_ops_e2e.rs)
// ---------------------------------------------------------------------------

/// Compile + Engine::build the given source and return the STEP bytes.
///
/// Asserts no parse errors, no compile errors, and that the build produces
/// non-empty STEP output containing the ISO-10303-21 header.
///
/// Returns `None` if OCCT is not available (test is silently skipped).
fn build_step(source: &str) -> Option<Vec<u8>> {
    if !OCCT_AVAILABLE {
        eprintln!("skipping (OCCT unavailable)");
        return None;
    }

    let parsed = reify_syntax::parse(source, ModulePath::single("half_space_e2e"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let output = result
        .geometry_output
        .expect("engine.build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    let step_str = std::str::from_utf8(&output).expect("STEP should be valid UTF-8");
    assert!(
        step_str.contains("ISO-10303-21"),
        "STEP output must contain ISO-10303-21 header"
    );
    // The result must be a solid (not an empty / degenerate shape).
    assert!(
        step_str.contains("MANIFOLD_SOLID_BREP") || step_str.contains("CLOSED_SHELL"),
        "STEP output must contain a solid body, got header: {}…",
        &step_str[..step_str.len().min(500)]
    );

    Some(output)
}

// ---------------------------------------------------------------------------
// (a) G2.3: bounded intersection produces valid STEP export
// ---------------------------------------------------------------------------

/// `intersection(half_space(0mm,0mm,0mm, 0,0,1), box(20mm,20mm,20mm))`:
///   - The box (20mm cube centered at origin) is bisected by the z=0 plane.
///   - The retained half is bounded → must produce a valid STEP solid.
///
/// Exact volume check (½·(20mm)³) is done by the kernel-direct integration
/// test in `crates/reify-kernel-occt/tests/half_space_integration.rs`; here
/// we verify the full source→STEP pipeline succeeds.
///
/// RED: panics at `todo!()` in geometry_ops.rs until step-8.
#[test]
fn bounded_intersection_step_export() {
    build_step(r#"
structure S {
    let result = intersection(half_space(0mm, 0mm, 0mm, 0, 0, 1), box(20mm, 20mm, 20mm))
}
"#);
}

// ---------------------------------------------------------------------------
// (b) Bare half_space is constructible without panic
// ---------------------------------------------------------------------------

/// A bare `let hs = half_space(…)` let must realize a geometry handle without
/// panic, even though the result is unbounded (Bounded = false).
///
/// The test checks:
///   - Source compiles without errors (no E_GEOMETRY_UNBOUNDED on a bare let,
///     only on a Bounded-typed slot).
///   - `engine.build()` completes without panic.
///
/// RED: panics at `todo!()` in geometry_ops.rs until step-8.
#[test]
fn bare_half_space_is_constructible() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping bare_half_space_is_constructible: OCCT unavailable");
        return;
    }

    let source = r#"
structure S {
    let hs = half_space(0mm, 0mm, 0mm, 0, 0, 1)
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("half_space_e2e_bare"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    // A bare half_space let must not produce compile errors (E_GEOMETRY_UNBOUNDED
    // fires only when the result is passed to a Bounded-typed slot).
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors for bare half_space: {:?}",
        compile_errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    // Must not panic — an unbounded geometry handle IS constructible.
    let result = engine.build(&compiled, ExportFormat::Step);
    // No build-level errors.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "unexpected build errors for bare half_space: {:?}",
        build_errors
    );
}
