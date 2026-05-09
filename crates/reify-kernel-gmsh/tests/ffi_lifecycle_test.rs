//! Pin the gmsh-library lifecycle FFI surface against libgmsh 4.15.2.
//!
//! Lives in its OWN integration-test binary (separate from
//! `ffi_smoke_tests.rs`) so it gets its own `cargo test` process. That
//! isolation matters because this test calls `ffi::initialize()` /
//! `ffi::finalize()` directly — bypassing the `OnceLock<()>` inside
//! `init::ensure_initialized` — and gmsh's runtime state is
//! process-wide. Sharing a binary with tests that route through
//! `ensure_initialized` would couple test ordering to FFI semantics
//! (a finalize here could leave a cached OnceLock pointing at a
//! finalized library; a subsequent direct `gmshInitialize` without an
//! intervening finalize is undefined). One test per binary side-steps
//! both failure modes.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On
//! stub builds (no `/opt/reify-deps`) the file is empty and this test
//! binary contains zero tests — preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::ffi;
use reify_kernel_gmsh::init;

/// Round-trip the gmsh library lifecycle through our extern "C" wrappers.
///
/// 1. Acquire the process-global `init::GMSH_LOCK` — gmsh has process-wide
///    state that other tests in this binary may also touch.
/// 2. `ffi::initialize()` — boxes `gmshInitialize(0, null, 0, 0, &mut ierr)`.
/// 3. Assert `ffi::is_initialized()` returns `true`.
/// 4. `ffi::finalize()` — drops the gmsh runtime state.
/// 5. Assert `ffi::is_initialized()` returns `false`.
///
/// Pins the four lifecycle bindings and the GMSH_LOCK plumbing in a single
/// scope so a future binding regression (wrong ABI, missing extern, etc.)
/// surfaces here before reaching `mesh_to_volume`.
#[test]
fn gmsh_initialize_and_finalize_round_trip() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    ffi::initialize().expect("ffi::initialize failed");
    assert!(
        ffi::is_initialized(),
        "ffi::is_initialized must return true immediately after ffi::initialize",
    );

    ffi::finalize().expect("ffi::finalize failed");
    assert!(
        !ffi::is_initialized(),
        "ffi::is_initialized must return false immediately after ffi::finalize",
    );
}
