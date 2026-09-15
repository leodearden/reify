//! Process-global initialisation + serialisation primitives for libgmsh.
//!
//! Gmsh's runtime state is process-wide: `gmshClear()` wipes the current
//! model, `gmshOptionSetNumber("Mesh.Algorithm3D", …)` mutates a global
//! option table, `gmshModelMeshGenerate(3)` operates on whatever model is
//! current. Two threads concurrently calling [`crate::GmshKernel::mesh_to_volume`]
//! would race on this state. [`GMSH_LOCK`] is the single static `Mutex<()>`
//! behind every gmsh library call; [`lock`] is how this crate acquires it.
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
//! [`read_tet_connectivity`] is that recovery's companion on the way out: a
//! broken mesher's characteristic output is an EMPTY element buffer, so all
//! three meshers read their tets back through one shared, checked call.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::ops::Deref;
use std::sync::{Mutex, MutexGuard, OnceLock};

use reify_ir::{ElementOrderTag, GeometryError};

use crate::ffi;

/// Process-global serialisation lock for every gmsh library call.
///
/// This crate's own entry points take it through [`lock`]. The static stays
/// `pub` for this crate's integration test binaries — separate compilation
/// units that cannot reach `pub(crate)` symbols — whose need is only to
/// serialise their own raw-FFI access against the production path.
pub static GMSH_LOCK: Mutex<()> = Mutex::new(());

/// Proof that its holder acquired [`GMSH_LOCK`] — not merely *a* mutex.
///
/// [`lock`] is the only constructor and the field is private, so a function
/// that asks for a `&GmshGuard` cannot be reached without the real
/// process-global lock held. [`mesh_generate_with_recovery`] needs exactly
/// that: it finalizes libgmsh, which no thread inside the library survives. A
/// `&MutexGuard<'_, ()>` parameter would have been satisfied by a guard
/// borrowed from any `Mutex<()>` the caller cared to declare.
///
/// Derefs to the guard it wraps, so it still serves as the lifetime witness
/// [`crate::mesh_size_clamp::MeshSizeClampReset::armed`] borrows.
pub struct GmshGuard(MutexGuard<'static, ()>);

impl Deref for GmshGuard {
    type Target = MutexGuard<'static, ()>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Acquire [`GMSH_LOCK`] for a gmsh entry point.
///
/// Recovers from a poisoned lock rather than propagating the failure: every
/// entry point in this crate opens with `ffi::clear()`, which wipes whatever
/// half-built model state a panicked prior call left behind. Without this, one
/// panic anywhere under the lock would disable meshing for the rest of the
/// process lifetime.
pub fn lock() -> GmshGuard {
    GmshGuard(GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
}

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
/// enforced by the signature rather than by a comment a refactor can quietly
/// violate. [`GmshGuard`] is what makes that enforcement real rather than
/// suggestive: one constructor, private field, so the only way to reach this
/// function is to already hold the process-global lock.
/// [`crate::mesh_size_clamp::MeshSizeClampReset::armed`] is the same idiom one
/// notch weaker — it accepts any `&MutexGuard` — which is proportionate there,
/// where a violation restores two options at the wrong moment, and is not here,
/// where it tears the library down.
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
/// holding [`GMSH_LOCK`], and [`lock`] deliberately recovers from a poisoned
/// lock, so the next caller would walk straight into the finalized library
/// instead of being stopped. The unwind is unsound in its own right: it drops
/// [`crate::mesh_size_clamp::MeshSizeClampReset`], whose `Drop` calls back
/// into gmsh to restore the size clamp.
pub fn mesh_generate_with_recovery(_guard: &GmshGuard, dim: i32) -> Result<(), GeometryError> {
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

/// Read this call's tetrahedra back out of gmsh, rejecting a buffer that
/// cannot be a real mesh.
///
/// Both numbers the element order fixes come from one [`tet_element_spec`]
/// lookup. Derived separately — the type code at the readback, the stride at
/// the check — they were two facts taken from one dimension of variability at
/// four sites, and a site that asked gmsh for 10-node tets while validating
/// against a 4-node stride would mis-slice the connectivity into plausible
/// nonsense.
///
/// Two ways the buffer can be unusable are therefore rejected here rather than
/// returned:
///
///   - a length that is not a whole number of tets; and
///   - an EMPTY buffer, which would become an `Ok` `VolumeMesh` holding no
///     tetrahedra — a wrong answer no caller can tell from a right one.
///
/// The emptiness half is the output-side twin of the empty-INPUT rejection in
/// [`crate::GmshKernel::mesh_to_volume`]: gmsh accepts the degenerate case and
/// yields a zero-tet mesh, which is never a useful caller outcome. Stating it
/// once, here, is what keeps that invariant uniform across the three readbacks
/// rather than enforced in whichever of them was edited last.
///
/// Since every `mesh_generate` in this crate routes through
/// [`mesh_generate_with_recovery`], no path measured today reaches the empty
/// case. It is the backstop for the ones not measured: a future gmsh version,
/// a mesher added later that forgets the recovery wrapper, an HXT that
/// reports `ierr=0` having produced nothing at all.
///
/// `caller` names the entry point in the error message — three of them share
/// this readback, and the buffer says nothing about where it came from.
pub fn read_tet_connectivity(
    caller: &str,
    element_order: ElementOrderTag,
) -> Result<Vec<u64>, GeometryError> {
    let (elem_type, _) = tet_element_spec(element_order);
    let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;
    verify_tet_readback(caller, &elem_node_tags, element_order)?;
    Ok(elem_node_tags)
}

/// The two numbers a tet's element order fixes: gmsh's element-type code for
/// [`ffi::get_elements_by_type`], and the nodes-per-element stride of the flat
/// buffer that call returns.
///
/// One table because they are one fact — gmsh numbers its element types by
/// shape AND order (4 = 4-node tet, 11 = 10-node tet), so the code and the
/// stride are never independently chosen.
fn tet_element_spec(element_order: ElementOrderTag) -> (i32, usize) {
    match element_order {
        ElementOrderTag::P1 => (4, 4),
        ElementOrderTag::P2 => (11, 10),
    }
}

/// The half of [`read_tet_connectivity`] that the buffer alone decides — split
/// out so both rejections are reachable from a unit test with no live gmsh
/// model behind them.
fn verify_tet_readback(
    caller: &str,
    elem_node_tags: &[u64],
    element_order: ElementOrderTag,
) -> Result<(), GeometryError> {
    let (_, nodes_per_elem) = tet_element_spec(element_order);
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

/// Both [`verify_tet_readback`] rejections, over both element orders.
///
/// The empty-buffer rejection is unreachable from any measured production path
/// (see [`read_tet_connectivity`]), so without these a guard inverted or
/// deleted outright would ship green.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_tet_buffer_is_rejected_and_names_its_caller() {
        let err = verify_tet_readback("some_mesher", &[], ElementOrderTag::P1)
            .expect_err("an empty element buffer is never a real mesh");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("no P1 tetrahedra"),
            "the empty case must say the model holds no tets of the requested order; got: {msg}"
        );
        assert!(
            msg.contains("some_mesher"),
            "three meshers share this check, so the message must name which one; got: {msg}"
        );
    }

    #[test]
    fn a_buffer_that_is_not_a_whole_number_of_tets_is_rejected() {
        let err = verify_tet_readback("some_mesher", &[1, 2, 3], ElementOrderTag::P1)
            .expect_err("3 node tags cannot be a whole number of 4-node tets");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("stride mismatch"),
            "a partial tet must be reported as a stride mismatch, not as an empty \
             readback; got: {msg}"
        );
    }

    #[test]
    fn the_stride_is_read_from_the_element_order() {
        verify_tet_readback("some_mesher", &[1, 2, 3, 4], ElementOrderTag::P1)
            .expect("four node tags are exactly one P1 tet");
        let err = verify_tet_readback("some_mesher", &[1, 2, 3, 4], ElementOrderTag::P2)
            .expect_err("a P2 tet is 10 nodes, so the same four tags are a partial one");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("stride mismatch") && msg.contains("multiple of 10"),
            "the P2 stride must come from the order, not from a P1 constant; got: {msg}"
        );
    }
}
