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
//! Because the library's lifetime is this module's business, so is putting it
//! back when the mesher breaks (`mesh_generate_with_recovery`) and refusing
//! to keep going when it cannot be put back ([`lock`]).
//!
//! `read_tet_connectivity` is that recovery's companion on the way out: a
//! broken mesher's characteristic output is an EMPTY element buffer, so all
//! three tet meshers read their tets back through one shared, checked call.
//!
//! Only compiled when `cfg(has_gmsh)` is set by `build.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use reify_ir::{ElementOrderTag, GeometryError};

use crate::ffi;

/// Process-global serialisation lock for every gmsh library call.
///
/// This crate's own entry points take it through [`lock`]. The static stays
/// `pub` for this crate's integration test binaries — separate compilation
/// units that cannot reach `pub(crate)` symbols — whose need is only to
/// serialise their own raw-FFI access against the production path.
///
/// Taking it directly buys serialisation and nothing else. The refusal that
/// keeps a caller out of a library finalized beyond recovery lives in [`lock`],
/// so the raw acquirers in `tests/` bypass it and would call into a finalized
/// library — tolerable only because a run that reaches that state has already
/// failed something louder.
pub static GMSH_LOCK: Mutex<()> = Mutex::new(());

/// Set once libgmsh has been finalized with nothing live behind it. Sticky by
/// design: nothing clears it, because nothing can repair the library
/// afterwards. Written only under [`GMSH_LOCK`], by
/// [`mesh_generate_with_recovery`]; read by [`lock`].
static GMSH_DEAD: AtomicBool = AtomicBool::new(false);

/// What a caller is told once [`GMSH_DEAD`] is set. One string, so the message
/// the recovery site emits and the one every later entry point emits cannot
/// drift apart.
const GMSH_DEAD_MESSAGE: &str = "libgmsh is finalized and could not be \
     re-initialized; meshing is disabled for the rest of this process";

/// Proof that its holder acquired [`GMSH_LOCK`] — not merely *a* mutex — and
/// that libgmsh was still usable when they took it.
///
/// [`lock`] is the only constructor and the field is private, so a function
/// that asks for a `&GmshGuard` cannot be reached without the real
/// process-global lock held. `mesh_generate_with_recovery` needs exactly
/// that: it finalizes libgmsh, which no thread inside the library survives. A
/// `&MutexGuard<'_, ()>` parameter would have been satisfied by a guard
/// borrowed from any `Mutex<()>` the caller cared to declare.
/// [`crate::mesh_size_clamp::MeshSizeClampReset::armed`] is that same idiom
/// one notch weaker — proportionate there, where a violation restores two
/// options at the wrong moment, and not here, where it tears the library down.
pub struct GmshGuard(MutexGuard<'static, ()>);

impl GmshGuard {
    /// The weaker witness [`crate::mesh_size_clamp::MeshSizeClampReset::armed`]
    /// still asks for, and the only way to obtain one from a [`GmshGuard`].
    ///
    /// A named `pub(crate)` accessor rather than an `impl Deref`: a
    /// `&MutexGuard<'_, ()>` is exactly the witness this type was introduced to
    /// stop handing out, so the one site that still needs it names it, and no
    /// caller outside this crate can reach one at all.
    pub(crate) fn clamp_reset_witness(&self) -> &MutexGuard<'static, ()> {
        &self.0
    }
}

/// Acquire [`GMSH_LOCK`] for a gmsh entry point, refusing once libgmsh is
/// unrecoverable.
///
/// Recovers from a poisoned lock rather than propagating the failure: every
/// entry point in this crate opens with `ffi::clear()`, which wipes whatever
/// half-built model state a panicked prior call left behind. Without this, one
/// panic anywhere under the lock would disable meshing for the rest of the
/// process lifetime.
///
/// `GMSH_DEAD` is read AFTER the lock is taken, because its only writer sets it
/// while holding the lock. Every entry point in `src/` reaches its first FFI
/// call through here, so for this crate's own callers this one check is the
/// whole enforcement; the `tests/` binaries that take [`GMSH_LOCK`] directly
/// are outside it, as that static's doc records.
pub fn lock() -> Result<GmshGuard, GeometryError> {
    let guard = GmshGuard(GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
    if GMSH_DEAD.load(Ordering::Acquire) {
        return Err(GeometryError::OperationFailed(GMSH_DEAD_MESSAGE.into()));
    }
    Ok(guard)
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
/// `_guard` is taken purely for its lifetime, never touched, so that "caller
/// must hold [`GMSH_LOCK`]" is enforced by the signature rather than by a
/// comment a refactor can quietly violate — see [`GmshGuard`].
///
/// # When the recycle itself fails
///
/// A failed `gmshFinalize` leaves the library up and the mesher still
/// poisoned; a failed `gmshInitialize` after a successful finalize leaves no
/// library at all. Neither is repairable from here, so the first is reported
/// in the returned error and the second additionally sets [`GMSH_DEAD`],
/// after which every entry point refuses at [`lock`] instead of calling into
/// a finalized library.
///
/// Both paths RETURN; neither panics and neither aborts the process. A panic
/// would unwind out holding [`GMSH_LOCK`], which [`lock`] deliberately
/// un-poisons, so the next caller would sail past the refusal. An abort would
/// hold the same property the flag already buys, at the cost of killing a
/// `reify-gui` session — this crate is linked into one — over a library fault
/// the user could otherwise have saved their model through. The
/// [`crate::mesh_size_clamp::MeshSizeClampReset`] drop that runs on the way
/// out is harmless either way: measured on libgmsh 4.15.2, an FFI call after
/// `gmshFinalize` returns `ierr=1` ("Gmsh has not been initialized") and does
/// nothing.
///
/// # Why the diagnosis is read here
///
/// Gmsh states WHY a mesh failed in its captured message stream, not in the
/// last-error line the `ffi` macro annotates with — see [`crate::log_capture`].
/// That stream lives INSIDE libgmsh, and this function is the only thing in
/// this crate that destroys libgmsh. So it reads the capture before the
/// teardown and folds it into the error it returns, and is therefore the sole
/// annotator of its own failure: a caller's own
/// [`crate::log_capture::LogCapture`] seam must not also cover this call, or
/// the same lines land in the message twice.
///
/// That the read has to happen first is not hypothetical, but neither is it
/// what the measurement showed. Measured on libgmsh 4.15.2: the capture
/// SURVIVES a `gmshFinalize`+`gmshInitialize` cycle — lines captured before
/// the recycle are still readable after it — and `gmshLoggerGet` between the
/// two returns `ierr=1`. Gmsh documents neither, so reading first is what
/// keeps the diagnosis independent of a behaviour that could change under us.
///
/// A caller that armed no capture pays nothing: [`ffi::logger_get`] on a
/// logger that was never started returns an empty `Vec`, which is
/// [`crate::log_capture::annotated`]'s pass-through path. The read is on the
/// failure path only — the success path returns before reaching it.
///
/// Three of this function's four callers sit in exactly that position today,
/// and the asymmetry is easier to miss from their side than from here. Only
/// [`crate::kernel_real::GmshKernel::mesh_to_volume`] arms a
/// [`crate::log_capture::LogCapture`], so a failure reached through
/// `refine_volume::refine_volume_with_size_field`,
/// `mesh_boundary::mesh_surface_to_volume_with_attribution` or
/// `mesh_profile_2d::mesh_plane_2d` still reports nothing beyond the
/// last-error line — and each of those silences `"General.Terminal"` just as
/// `mesh_to_volume` does, which is precisely what leaves the capture as the
/// only route to gmsh's diagnosis there too. Each is one
/// `LogCapture::armed(&_guard)` after its own `"General.Terminal"` write away
/// from parity; all three already hold the [`GmshGuard`] that call asks for.
/// Left undone because those three files are outside task #6969's scope, not
/// because arming them was judged wrong.
pub(crate) fn mesh_generate_with_recovery(
    _guard: &GmshGuard,
    dim: i32,
) -> Result<(), GeometryError> {
    let original = match ffi::mesh_generate(dim) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let captured = ffi::logger_get().unwrap_or_default();

    if let Err(finalize_err) = ffi::finalize() {
        // Do NOT initialize() now: the library was never torn down, and a
        // second gmshInitialize over a live one is undefined. Report both
        // facts — the mesh failed, and the process is still poisoned.
        return Err(crate::log_capture::annotated(
            GeometryError::OperationFailed(format!(
                "{original} (and the mesher could not be recovered: {finalize_err} — \
                 later meshing calls in this process may silently produce no elements)"
            )),
            &captured,
        ));
    }
    if let Err(init_err) = ffi::initialize() {
        GMSH_DEAD.store(true, Ordering::Release);
        return Err(crate::log_capture::annotated(
            GeometryError::OperationFailed(format!(
                "{original} (and {GMSH_DEAD_MESSAGE}: {init_err})"
            )),
            &captured,
        ));
    }

    // `gmshInitialize` resets the process-global option table, so without
    // this the next caller's leading `ffi::clear()` prints "Info: Clearing
    // all models and views..." to stdout before it re-silences the library.
    // Restoring the silence gmsh had on entry is the same "leave nothing
    // behind" discipline `mesh_size_clamp` applies to the size clamp.
    // Best-effort: a failure here must not mask the real result.
    let _ = ffi::option_set_number("General.Terminal", 0.0);

    Err(crate::log_capture::annotated(original, &captured))
}

/// Read this call's tetrahedra back out of gmsh, rejecting a buffer that
/// cannot be a real mesh: a length that is not a whole number of tets, or an
/// EMPTY buffer.
///
/// The emptiness half is the output-side twin of the empty-INPUT rejection in
/// [`crate::GmshKernel::mesh_to_volume`]: gmsh accepts the degenerate case and
/// yields a zero-tet mesh, which is never a useful caller outcome. Stating it
/// once, here, is what keeps that rejection uniform across the three tet
/// meshers rather than enforced in whichever of them was edited last. This
/// crate's fourth mesher, [`crate::mesh_profile_2d::mesh_plane_2d`], reads
/// back triangles and quads instead of tets, so it cannot share this call; it
/// carries the 2D form of the same rejection at its own readback.
///
/// Since every `mesh_generate` in this crate routes through
/// [`mesh_generate_with_recovery`], no path measured today reaches the empty
/// case. It is the backstop for the ones not measured: a future gmsh version,
/// a mesher added later that forgets the recovery wrapper, an HXT that
/// reports `ierr=0` having produced nothing at all.
///
/// `caller` names the entry point in the error message — three of them share
/// this readback, and the buffer says nothing about where it came from.
pub(crate) fn read_tet_connectivity(
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
/// One table because for this readback they are one fact — gmsh numbers its
/// element types by shape AND order (4 = 4-node tet, 11 = 10-node tet), so a
/// site that asked gmsh for 10-node tets while validating against a 4-node
/// stride would mis-slice the connectivity into plausible nonsense.
///
/// The gmsh type code is local to this crate. The stride is NOT: it is a
/// fourth copy of P1→4 / P2→10, alongside `reify_ir::VolumeMesh::nodes_per_element`
/// (the same table over a wider element family) and the `match` in each of
/// [`crate::through_thickness`] and [`crate::fill_metrics`]. Collapsing those
/// belongs next to the `reify_ir` element-order type that owns the fact, not
/// here.
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

/// The decisions this module makes with no live gmsh model behind them: the
/// dead-library refusal at [`lock`], and both [`verify_tet_readback`]
/// rejections over both element orders.
///
/// Neither is reachable from a measured production path — the empty-buffer
/// rejection for the reason [`read_tet_connectivity`] records, and the refusal
/// because its flag is only ever set by a `gmshInitialize` that failed. So
/// without these, a guard inverted or deleted outright would ship green.
#[cfg(test)]
mod tests {
    use super::*;

    /// Once [`GMSH_DEAD`] is set, [`lock`] must refuse instead of handing back
    /// a guard onto a finalized library, and must say why in the one message
    /// the recovery site also emits.
    ///
    /// The flag is set here directly: its production writer needs a
    /// `gmshInitialize` failure, which no test can provoke. It is cleared
    /// again before the first assertion, and the window it spans is one
    /// [`lock`] call that cannot panic, so a failure here cannot leak a sticky
    /// refusal into the rest of this binary.
    #[test]
    fn a_library_finalized_beyond_recovery_is_refused_at_the_lock() {
        GMSH_DEAD.store(true, Ordering::Release);
        let refusal = lock().err();
        GMSH_DEAD.store(false, Ordering::Release);

        let err = refusal.expect(
            "lock must refuse while GMSH_DEAD is set — otherwise every later entry \
             point calls into a library that is no longer there",
        );
        let msg = format!("{err:?}");
        assert!(
            msg.contains(GMSH_DEAD_MESSAGE),
            "the refusal must carry the shared dead-library message, so a caller \
             reads the same explanation wherever it surfaces; got: {msg}"
        );
    }

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
