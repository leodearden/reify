//! Integration tests for `Engine::ensure_openvdb_kernel` (task 4638).
//!
//! Verifies the lazy-acquisition path that the thickness-DFM arm in
//! `cmd_check` relies on: `with_registered_kernel` builds a single-pick
//! OCCT engine (no OpenVDB); `ensure_openvdb_kernel` idempotently inserts
//! the OpenVDB adapter from the inventory registry while leaving the
//! `default_kernel_name` (OCCT) intact.
//!
//! ## Test structure
//!
//! - `#[cfg(has_openvdb)]` — real assertion: baseline has no OpenVDB, one
//!   call adds it and returns `true`, `default_kernel_name` is unchanged
//!   (OCCT, not "openvdb"), a second call is idempotent.
//! - `#[cfg(not(has_openvdb))]` — skip-stub: `ensure_openvdb_kernel` returns
//!   `false` (registry lacks "openvdb" when the adapter is absent).

// Anchor: ensure the linker passes the reify_kernel_openvdb rlib unconditionally
// so the `inventory::submit!` registration fires at binary startup.
// `extern crate` is more durable than a const read (rustc may inline a const
// without emitting a symbol reference). Mirrors the OCCT/manifold anchors in
// crates/reify-cli/src/main.rs:14,20.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

/// `cfg(has_openvdb)`: `Engine::with_registered_kernel` builds a
/// single-pick OCCT engine (baseline has no OpenVDB kernel).
/// `ensure_openvdb_kernel()` then idempotently inserts the OpenVDB adapter
/// from the registry, returns `true`, and leaves `default_kernel_name`
/// pointing to OCCT (not "openvdb").
///
/// This is the engine-level half of the "doesn't allocate by default"
/// regression pin.
#[cfg(has_openvdb)]
#[test]
fn ensure_openvdb_kernel_adds_openvdb_and_leaves_default_unchanged() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_kernel_openvdb::register::OPENVDB_KERNEL_NAME;

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));

    // Baseline: `with_registered_kernel` is single-pick (OCCT only).
    // The OpenVDB adapter must NOT be in the engine yet.
    assert!(
        !engine
            .registered_kernel_names()
            .any(|n| n == OPENVDB_KERNEL_NAME),
        "baseline: `with_registered_kernel` must NOT pre-load OpenVDB \
         (single-pick alloc-cost contract).\n\
         registered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );

    let baseline_default = engine
        .default_kernel_name()
        .map(str::to_owned)
        .expect("with_registered_kernel must set a default_kernel_name (OCCT)");
    let baseline_count = engine.kernel_count();

    // First call: must return true and insert OpenVDB.
    let first = engine.ensure_openvdb_kernel();
    assert!(
        first,
        "ensure_openvdb_kernel() must return `true` when OpenVDB is in the \
         registry and not yet in the engine"
    );
    assert!(
        engine
            .registered_kernel_names()
            .any(|n| n == OPENVDB_KERNEL_NAME),
        "after ensure_openvdb_kernel(): OpenVDB must now be in registered_kernel_names().\n\
         registered kernels: {:?}",
        engine.registered_kernel_names().collect::<Vec<_>>(),
    );
    assert_eq!(
        engine.kernel_count(),
        baseline_count + 1,
        "kernel_count must increase by exactly 1 after ensure_openvdb_kernel()"
    );

    // default_kernel_name must be unchanged (still OCCT — realize_solid_sdf
    // tessellates via default_kernel_name and voxelizes via openvdb_kernel_name).
    assert_eq!(
        engine.default_kernel_name(),
        Some(baseline_default.as_str()),
        "ensure_openvdb_kernel() must NOT change default_kernel_name \
         (OCCT stays the tessellation default)"
    );

    // Second call: idempotent — must return true and not change the count.
    let second = engine.ensure_openvdb_kernel();
    assert!(
        second,
        "second ensure_openvdb_kernel() call must also return `true` (kernel already present)"
    );
    assert_eq!(
        engine.kernel_count(),
        baseline_count + 1,
        "kernel_count must be unchanged after idempotent second ensure_openvdb_kernel() call"
    );
    assert_eq!(
        engine.default_kernel_name(),
        Some(baseline_default.as_str()),
        "default_kernel_name must remain unchanged after idempotent second call"
    );
}

/// `cfg(not(has_openvdb))`: skip-stub.
///
/// When the OpenVDB adapter is absent (no `has_openvdb` cfg), the registry
/// does not contain "openvdb" → `ensure_openvdb_kernel` returns `false`
/// (graceful C1/D5 degradation).
#[cfg(not(has_openvdb))]
#[test]
fn ensure_openvdb_kernel_returns_false_without_openvdb() {
    use reify_constraints::SimpleConstraintChecker;

    eprintln!(
        "skipping ensure_openvdb_kernel positive assertions: \
         has_openvdb cfg not set — stub-mode build; \
         ensure_openvdb_kernel() must return false (C1/D5 degradation)"
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let result = engine.ensure_openvdb_kernel();
    assert!(
        !result,
        "stub mode: ensure_openvdb_kernel() must return `false` when the \
         OpenVDB adapter is absent from the registry (C1/D5 graceful degradation)"
    );
}
