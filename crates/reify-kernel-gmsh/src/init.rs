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
//! Because the library's lifetime is this module's business, so is putting
//! it back when the mesher breaks: [`mesh_generate_with_recovery`] recycles
//! gmsh in place after a failed generate, and the recycle is an
//! [`ensure_initialized`] invariant before it is anything else — the
//! OnceLock keeps recording "initialized" across it, so the finalize and the
//! re-initialize must be an inseparable pair or that cached fact becomes a
//! lie.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::sync::{Mutex, MutexGuard, OnceLock};

use reify_ir::GeometryError;

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

/// `ffi::mesh_generate(dim)`, plus the cleanup that makes its failure local.
///
/// # What it repairs
///
/// Measured against libgmsh 4.15.2: once one `gmshModelMeshGenerate` fails,
/// gmsh refuses every later generate in the process — and refuses it
/// *silently*, returning `ierr=0` with no elements rather than an error. The
/// next caller therefore gets a well-formed `Ok` holding zero tetrahedra: a
/// wrong answer that looks like a right one. `gmshClear()` does not lift
/// this; a `gmshFinalize`+`gmshInitialize` cycle does, repeatably. What the
/// surviving state actually *is* was not established — gmsh's sources were
/// not read — so this documents the observed behaviour and nothing more.
///
/// So on failure this recycles the library and returns the ORIGINAL error.
/// The caller's diagnostic stays the mesher's own message; recovery
/// bookkeeping never displaces it.
///
/// # Why it takes the guard
///
/// `_guard` is taken purely for its lifetime — the guard itself is never
/// touched. Finalizing gmsh while another thread is inside the library frees
/// the world out from under it, so "caller must hold [`GMSH_LOCK`]" is
/// enforced by the signature rather than by a comment a refactor can
/// quietly violate: the only way to call this is to already hold the lock.
/// Same idiom, and the same "purely for its lifetime" phrasing, as
/// [`crate::mesh_size_clamp::MeshSizeClampReset::armed`].
///
/// # Panics
///
/// If `gmshInitialize` fails after a successful `gmshFinalize`, leaving the
/// library finalized while [`ensure_initialized`]'s `OnceLock` still records
/// "initialized" — every later gmsh call in the process would then be
/// undefined. This is the stance `ensure_initialized` already takes for its
/// own `gmshInitialize` failure, for the same reason: a process-fatal
/// condition the upper-layer engine cannot meaningfully recover from.
pub fn mesh_generate_with_recovery(
    _guard: &MutexGuard<'_, ()>,
    dim: i32,
) -> Result<(), GeometryError> {
    let original = match ffi::mesh_generate(dim) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    if let Err(finalize_err) = ffi::finalize() {
        // Do NOT initialize() now: the library was never torn down, and a
        // second gmshInitialize over a live one is undefined. Report both
        // facts — the mesh failed, and the process is still poisoned.
        return Err(GeometryError::OperationFailed(format!(
            "{original} (and the mesher could not be recovered: {finalize_err} — \
             later meshing calls in this process may silently produce no elements)"
        )));
    }
    ffi::initialize()
        .expect("gmshInitialize failed while recovering from a failed gmshModelMeshGenerate");

    // `gmshInitialize` resets the process-global option table, so without
    // this the next caller's leading `ffi::clear()` prints "Info: Clearing
    // all models and views..." to stdout before it re-silences the library.
    // Restoring the silence gmsh had on entry is the same "leave nothing
    // behind" discipline `mesh_size_clamp` applies to the size clamp.
    // Best-effort: a failure here must not mask the real result.
    let _ = ffi::option_set_number("General.Terminal", 0.0);

    Err(original)
}
