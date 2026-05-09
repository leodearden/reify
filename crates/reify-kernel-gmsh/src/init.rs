//! Process-global initialisation + serialisation primitives for libgmsh.
//!
//! Gmsh's runtime state is process-wide: `gmshClear()` wipes the current
//! model, `gmshOptionSetNumber("Mesh.Algorithm3D", …)` mutates a global
//! option table, `gmshModelMeshGenerate(3)` operates on whatever model is
//! current. Two threads concurrently calling [`crate::GmshKernel::mesh_to_volume`]
//! would race on this state. [`GMSH_LOCK`] is the single static `Mutex<()>`
//! we acquire at every public entry point that touches the gmsh library.
//!
//! [`ensure_initialized`] OnceLock-guards the `gmshInitialize` call so
//! repeated `mesh_to_volume` invocations pay a one-cached-cell branch
//! instead of the FFI roundtrip on every call. Mirrors
//! `crates/reify-kernel-openvdb/src/init.rs:22-37`.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::sync::{Mutex, OnceLock};

use crate::ffi;

/// Process-global serialisation lock for every gmsh library call.
///
/// Acquire at the head of any public method that touches gmsh state — FFI
/// reads, FFI writes, or both. The lock is exposed `pub` so this crate's
/// integration test binaries (separate compilation units that cannot reach
/// `pub(crate)` symbols) can serialise their own gmsh access against the
/// production code path.
pub static GMSH_LOCK: Mutex<()> = Mutex::new(());

/// `OnceLock`-guarded `gmshInitialize`. Idempotent: the first caller pays
/// the FFI cost; subsequent callers hit the cached `()` and return
/// immediately.
///
/// Panics on initialisation failure rather than threading a `Result` up
/// every call site — `gmshInitialize` is documented to fail only on
/// resource exhaustion, which is a process-fatal condition the upper-layer
/// engine cannot meaningfully recover from.
pub fn ensure_initialized() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        ffi::initialize().expect("gmshInitialize failed during ensure_initialized");
    });
}
