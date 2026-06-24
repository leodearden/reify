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

// OCCT linker anchor. The body `box(...)` realization needs a real BRep kernel
// as the lex-min default so it tessellates into a closed surface the gmsh tet
// path can volume-mesh. `make_occt_engine()` below references
// `reify_kernel_occt::OcctKernelHandle` directly (dev-dep, mirrors the gmsh
// dev-dep), so this `extern crate` is belt-and-suspenders for the link.
#[cfg(has_gmsh)]
extern crate reify_kernel_occt as _;

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

/// Build a fresh `Engine` backed by a real OCCT kernel as the lex-min BRep
/// default (so `box(...)` realizes into a tessellatable closed surface),
/// mirroring `as_printed_body_realization_e2e.rs::make_occt_engine`.
#[cfg(has_gmsh)]
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

/// `cfg(has_gmsh)`: the demand → execute call edge writes VolumeMesh + Gmsh.
///
/// Build the `volume_mesh_box.ri` fixture (a `box` body consumed by the
/// `@optimized("test::vm-demand-probe")` `vm_probe`) through a real OCCT engine
/// with the probe target registered VolumeMesh-demanding and gmsh acquired.
/// The module-static demand pass overrides `body`'s demanded `ReprKind` to
/// `VolumeMesh`; `execute_realization_ops` then tessellates the terminal OCCT
/// BRep handle and routes it through `dispatch_volume_mesh` (tet path) → gmsh
/// `store_volume_mesh`, re-terminating the `body` realization on the gmsh
/// kernel.
///
/// Asserts via the public `realization_kernel_provenance()` that exactly the
/// call edge's writes landed: a realization with `repr == VolumeMesh` AND
/// `kernel == Gmsh`. (The `volume_mesh()` *content* round-trip — tet structure,
/// `% 4 == 0`, P1 tag — is the probe-read path, covered by the step-13 e2e;
/// `realization_handles` itself is a private field, so the public provenance
/// surface is the dead-strip-safe assertion here.)
///
/// RED before step-12: no call edge exists, so `body` falls back to a BRep box
/// on the OCCT kernel (`repr == BRep`, `kernel == Occt`) and no VolumeMesh+Gmsh
/// realization is present.
#[cfg(has_gmsh)]
#[test]
fn call_edge_writes_volume_mesh_repr_and_gmsh_kernel_for_demanded_body() {
    use reify_core::KernelId;
    use reify_ir::{ExportFormat, ReprKind};

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping call_edge_writes_volume_mesh_repr_and_gmsh_kernel_for_demanded_body: \
             OCCT not available (no BRep kernel to build the box body)"
        );
        return;
    }

    let compiled = reify_test_support::parse_and_compile_with_stdlib(include_str!(
        "fixtures/volume_mesh_box.ri"
    ));

    let mut engine = make_occt_engine();
    engine.register_volume_mesh_demand("test::vm-demand-probe");
    assert!(
        engine.ensure_gmsh_kernel(),
        "ensure_gmsh_kernel() must acquire the gmsh adapter from the registry"
    );

    engine.build(&compiled, ExportFormat::Step);

    // Public provenance read: the body realization must have been re-terminated
    // on the gmsh kernel at VolumeMesh by the call edge.
    let provenance = engine.realization_kernel_provenance();
    assert!(
        provenance
            .iter()
            .any(|p| p.repr == ReprKind::VolumeMesh && p.kernel == KernelId::Gmsh),
        "after build, the VolumeMesh-demanded `body` realization must produce \
         repr == VolumeMesh on kernel == Gmsh (the execute_realization_ops → \
         dispatch_volume_mesh call edge). provenance: {:?}",
        provenance
            .iter()
            .map(|p| (p.realization.clone(), p.repr, p.kernel))
            .collect::<Vec<_>>()
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
