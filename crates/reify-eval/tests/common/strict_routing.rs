//! The OCCT strict-routing STEP gate, shared by the `affine_tapered_spacer_gate`
//! and `nurbs_surface_e2e` test binaries.
//!
//! A module cannot be shared across integration-test crate roots, so each declares
//! `#[path = "common/strict_routing.rs"] mod strict_routing;`, following the
//! `guarded_groups.rs` convention.

use reify_core::Severity;
use reify_ir::{ExportFormat, Operation};
use reify_kernel_occt::register::OCCT_KERNEL_NAME;

/// Asserts that `compiled` builds non-empty STEP under
/// `Engine::with_registered_kernels`, where `op` reaches OCCT only if OCCT's
/// capability descriptor declares it: an undeclared op is a NoKernelChain
/// Error, never a silent fallback to a default kernel.
///
/// Each failure names its own cause: OCCT missing from the registry, an
/// Error-severity build diagnostic, or absent STEP output.
pub fn assert_step_builds_under_registered_kernels(
    compiled: &reify_compiler::CompiledModule,
    op: Operation,
) {
    let registered: Vec<&str> = reify_eval::registry().keys().map(String::as_str).collect();
    assert!(
        registered.contains(&OCCT_KERNEL_NAME),
        "kernel {OCCT_KERNEL_NAME:?} is not registered, so strict routing cannot reach it \
         for {op:?}; registered kernels: {registered:?}"
    );

    let mut engine = reify_eval::Engine::with_registered_kernels(
        reify_eval::SolverPosture::SolverFree,
        Box::new(reify_constraints::SimpleConstraintChecker),
    );
    let result = engine.build(compiled, ExportFormat::Step);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "build errors under strict routing (is ({op:?}, BRep) declared in \
         occt_capability_descriptor?): {errors:?}"
    );

    let output = result
        .geometry_output
        .unwrap_or_else(|| panic!("no STEP output for {op:?} under strict routing"));
    assert!(
        !output.is_empty(),
        "STEP output for {op:?} must be non-empty"
    );
}
