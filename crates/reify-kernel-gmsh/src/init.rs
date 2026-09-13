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
//! [`verify_tet_readback`] is the other half of that ownership. A broken
//! mesher's characteristic output is an EMPTY element buffer, so every entry
//! point that meshes a volume reads its tets back through one shared check
//! rather than three near-identical copies of it.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::sync::{Mutex, MutexGuard, OnceLock};

use reify_ir::{ElementOrderTag, GeometryError};

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
/// # Aborts
///
/// If `gmshInitialize` fails after a successful `gmshFinalize`, this aborts
/// the process. gmsh is then finalized while [`ensure_initialized`]'s
/// `OnceLock` still records "initialized", so every later gmsh call in the
/// process is undefined.
///
/// A panic would not contain that, which is why this is deliberately not the
/// stance [`ensure_initialized`] takes for its own `gmshInitialize` failure.
/// That panic leaves state consistent — the `OnceLock` cell stays unset, gmsh
/// stays uninitialized, a retry re-attempts init — whereas this one unwinds
/// holding [`GMSH_LOCK`], and every entry point in this crate deliberately
/// recovers from a poisoned lock (`unwrap_or_else(|e| e.into_inner())`), so
/// the next caller would walk straight into the finalized library instead of
/// being stopped. The unwind is unsound in its own right: it drops
/// [`crate::mesh_size_clamp::MeshSizeClampReset`], whose `Drop` calls back
/// into gmsh to restore the size clamp.
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
    if let Err(init_err) = ffi::initialize() {
        eprintln!(
            "reify-kernel-gmsh: gmshInitialize failed while recovering from a failed \
             gmshModelMeshGenerate ({init_err}); gmsh is finalized but still recorded as \
             initialized, so every later gmsh call would be undefined — aborting"
        );
        std::process::abort();
    }

    // `gmshInitialize` resets the process-global option table, so without
    // this the next caller's leading `ffi::clear()` prints "Info: Clearing
    // all models and views..." to stdout before it re-silences the library.
    // Restoring the silence gmsh had on entry is the same "leave nothing
    // behind" discipline `mesh_size_clamp` applies to the size clamp.
    // Best-effort: a failure here must not mask the real result.
    let _ = ffi::option_set_number("General.Terminal", 0.0);

    Err(original)
}

/// Rejects a tet readback that cannot be a real mesh.
///
/// Every entry point in this crate that meshes a volume calls
/// `gmshModelMeshGetElementsByType` and then walks the returned buffer flat,
/// so both ways that buffer can be unusable are checked here instead of being
/// re-derived at each of them:
///
///   - a length that is not a whole number of tets, which would silently
///     mis-slice the connectivity into plausible nonsense; and
///   - an EMPTY buffer, which would become an `Ok` `VolumeMesh` holding no
///     tetrahedra — a wrong answer no caller can tell from a right one.
///
/// The emptiness half is the output-side twin of the empty-INPUT rejection in
/// [`crate::GmshKernel::mesh_to_volume`]: gmsh accepts the degenerate case and
/// yields a zero-tet mesh, which is never a useful caller outcome. Stating it
/// once, here, is what keeps that invariant uniform across the three
/// readbacks rather than enforced in whichever of them was edited last.
///
/// Since every `mesh_generate` in this crate routes through
/// [`mesh_generate_with_recovery`], no path measured today reaches the empty
/// case. It is the backstop for the ones not measured: a future gmsh version,
/// a mesher added later that forgets the recovery wrapper, an HXT that
/// reports `ierr=0` having produced nothing at all.
///
/// `caller` names the entry point in the error message — three of them share
/// this check, and the buffer says nothing about where it came from.
pub fn verify_tet_readback(
    caller: &str,
    elem_node_tags: &[u64],
    element_order: ElementOrderTag,
) -> Result<(), GeometryError> {
    let nodes_per_elem: usize = match element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };
    if !elem_node_tags.len().is_multiple_of(nodes_per_elem) {
        return Err(GeometryError::OperationFailed(format!(
            "{caller}: gmsh get_elements_by_type stride mismatch: \
             elem_node_tags.len()={} is not a multiple of {nodes_per_elem} \
             (expected {nodes_per_elem} nodes per {element_order:?} tet)",
            elem_node_tags.len(),
        )));
    }
    if elem_node_tags.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "{caller}: gmshModelMeshGenerate reported success but the model holds \
             no {element_order:?} tetrahedra — returning an empty VolumeMesh would \
             be a silent wrong answer. Known cause: a mesher left unusable by an \
             earlier failed mesh_generate in this process"
        )));
    }
    Ok(())
}
