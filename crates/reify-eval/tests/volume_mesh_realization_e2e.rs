//! End-to-end tests for the VolumeMesh realization demand → execute → read
//! path (task 4743, PRD v0_6/volume-mesh-realization-and-morph-wiring.md α).
//!
//! ## Gmsh dead-strip discipline (CRITICAL)
//!
//! `reify-kernel-gmsh` is a **dev-dependency** of `reify-eval` (not a normal
//! dep — production reify-eval stays gmsh-build-free). A dev-dep rlib is only
//! linked into a test binary when that binary references one of its symbols;
//! otherwise the linker strips it and the crate's
//! `#[cfg(any(has_gmsh, feature = "stub_register"))] inventory::submit!`
//! (register.rs) never fires, leaving the `"gmsh"` registry name invisible to
//! `Engine::ensure_gmsh_kernel()`.
//!
//! The `extern crate reify_kernel_gmsh as _;` anchor below forces the rlib to
//! link unconditionally (more durable than a const read — rustc may inline a
//! const without emitting a symbol reference), mirroring the OpenVDB anchor in
//! `crates/reify-eval/tests/ensure_openvdb_kernel.rs:23`.
//!
//! **Do NOT reference any `reify_kernel_gmsh` symbol from other reify-eval test
//! binaries** — doing so would pull gmsh's `inventory::submit!` into their
//! binaries and break their OCCT-only `kernel_count` / registry-size assertions
//! (the manifold dead-strip discipline noted in `crates/reify-eval/Cargo.toml`).

// Gmsh linker anchor — see the module doc above.
#[cfg(has_gmsh)]
extern crate reify_kernel_gmsh as _;

/// `cfg(has_gmsh)`: `Engine::ensure_gmsh_kernel()` idempotently inserts the
/// Gmsh adapter from the inventory registry.
///
/// A fresh `Engine::new(checker, None)` holds zero kernels. The first
/// `ensure_gmsh_kernel()` looks up `KernelId::Gmsh.as_registry_name()` in
/// `kernel_registry::registry()` (populated by the anchored
/// `inventory::submit!`), inserts the adapter, returns `true`, and increments
/// `kernel_count()` by exactly 1. A second call is idempotent (returns `true`,
/// count unchanged).
///
/// RED before step-10: `Engine::ensure_gmsh_kernel` does not exist.
#[cfg(has_gmsh)]
#[test]
fn ensure_gmsh_kernel_adds_gmsh_and_is_idempotent() {
    use reify_kernel_gmsh::register::GMSH_KERNEL_NAME;
    use reify_test_support::mocks::MockConstraintChecker;

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);

    // Baseline: `Engine::new(checker, None)` holds zero kernels; gmsh absent.
    let baseline = engine.kernel_count();
    assert!(
        !engine
            .registered_kernel_names()
            .any(|n| n == GMSH_KERNEL_NAME),
        "baseline Engine::new must not pre-load gmsh.\nregistered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );

    // First call: inserts gmsh from the inventory registry → true, +1.
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must return true when the gmsh adapter is in the \
         registry and not yet in the engine"
    );
    assert!(
        engine
            .registered_kernel_names()
            .any(|n| n == GMSH_KERNEL_NAME),
        "after ensure_gmsh_kernel(): gmsh must be in registered_kernel_names().\n\
         registered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );
    assert_eq!(
        engine.kernel_count(),
        baseline + 1,
        "kernel_count must increase by exactly 1 after ensure_gmsh_kernel()"
    );

    // Second call: idempotent — returns true, count unchanged.
    assert!(
        engine.ensure_gmsh_kernel(),
        "second ensure_gmsh_kernel() call must also return true (kernel already present)"
    );
    assert_eq!(
        engine.kernel_count(),
        baseline + 1,
        "kernel_count must be unchanged after an idempotent second ensure_gmsh_kernel() call"
    );
}

/// `cfg(not(has_gmsh))`: skip-stub.
///
/// When the gmsh adapter is absent (no `has_gmsh` cfg), the registry does not
/// contain `"gmsh"` → `ensure_gmsh_kernel()` returns `false` (honest absence).
#[cfg(not(has_gmsh))]
#[test]
fn ensure_gmsh_kernel_returns_false_without_gmsh() {
    use reify_test_support::mocks::MockConstraintChecker;

    eprintln!(
        "skipping VolumeMesh-realization gmsh assertions: has_gmsh cfg not set \
         (stub-mode build); ensure_gmsh_kernel() must return false"
    );

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    assert!(
        !engine.ensure_gmsh_kernel(),
        "stub mode: ensure_gmsh_kernel() must return false when the gmsh adapter \
         is absent from the registry (honest degradation)"
    );
}
