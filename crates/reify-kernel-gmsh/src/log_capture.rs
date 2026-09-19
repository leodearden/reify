//! The crate-wide gmsh message-capture discipline, and the formatter that
//! folds a captured log tail into a failing operation's error.
//!
//! # The invariant
//!
//! Gmsh's message capture is a **process-global** switch, not a per-call
//! buffer. Once armed by [`crate::ffi::logger_start`] it accumulates every
//! line gmsh emits — from any caller, on any thread — until
//! [`crate::ffi::logger_stop`] both stops it and drains the buffer (that
//! drain is measured, and pinned by
//! `tests/ffi_smoke_tests.rs::gmsh_logger_captures_mesh_generate_output_even_with_terminal_silenced`'s
//! post-stop assertion). One SUCCESSFUL `mesh_to_volume` on a unit cube
//! measured 82 captured lines. So:
//!
//! > **Whoever arms the capture must stop it before returning — on every
//! > exit path, early `?`-returns included.**
//!
//! Leaving it armed is not merely untidy: the buffer grows for the life of
//! the process, and the next unrelated gmsh failure would report this
//! call's lines as its own.
//!
//! [`LogCapture`] is how that invariant is kept rather than remembered: a
//! caller arms one guard and the stop rides on `drop`, covering every `?`,
//! every explicit `return Err`, the success path and an unwinding panic
//! alike.
//!
//! # Why capture at all
//!
//! `ffi`'s [`gmsh_call!`](crate::ffi) macro already annotates every failure
//! with `gmshLoggerGetLastError`, but that holds only the last ERROR line.
//! Gmsh's actual diagnosis is routinely an `Info:` line that never reaches
//! it. Measured, on a single open triangle handed to
//! [`crate::kernel_real::GmshKernel::mesh_to_volume`]: the last error is
//! `HXT 3D mesh failed`, while the explanation — `Info: all vertices are
//! coplanar or nearly coplanar`, and before it `Info: Model has 0 non
//! manifold mesh edges and 3 boundary mesh edges` — sits in the captured
//! stream only.
//!
//! Capture is independent of the `"General.Terminal" = 0` that every
//! production mesher in this crate sets to silence gmsh's own stdout; see
//! [`crate::ffi::logger_start`] for that measurement. Silencing the
//! terminal is precisely what makes the capture the ONLY route by which a
//! caller can see gmsh's diagnosis.
//!
//! # What the cap bounds, and what it does not
//!
//! [`MAX_APPENDED_LOG_LINES`] bounds the tail folded into an error message.
//! It does not bound the capture itself: gmsh buffers every line it emits
//! while armed and offers no knob to cap that buffer, so a caller that arms
//! unconditionally — as
//! [`crate::kernel_real::GmshKernel::mesh_to_volume`] does — pays for the
//! buffering on its SUCCESS path too, where not one line is ever read.
//!
//! That cost is accepted, on measurement rather than assumption. Reading the
//! capture inside `mesh_to_volume` itself, just before it returns a unit
//! cube: 188 tets → 82 lines / 3.7 KB; 4,613 tets → 100 lines / 4.7 KB;
//! 63,645 tets → 112 lines / 5.3 KB. Three hundred times the elements cost
//! 37% more log — gmsh narrates meshing PHASES, not elements. The first of
//! those is the size `mesh_to_volume` auto-derives, and is the same 82 quoted
//! above; the absolute counts move with mesh size and with where in the call
//! the capture is read, so what these three points establish is the SLOPE. So
//! the buffer is kilobytes on any mesh this kernel produces, and `drop` frees
//! them at the end of the call that allocated them.
//!
//! Two ways of paying less were considered and declined. Arming only when a
//! caller opts in puts the diagnosis behind a flag that would have to be set
//! BEFORE the failure it explains — and a caller who could predict which
//! mesh fails would not need it. Filtering by `General.Verbosity` discards
//! `Info:` lines, which is to say exactly the lines this module exists to
//! surface (the measured diagnosis is `Info: all vertices are coplanar or
//! nearly coplanar`).

use reify_ir::GeometryError;

/// How many captured lines [`annotated`] appends — the most recent ones.
///
/// Picked from measurement, not taste. Failing runs through `mesh_to_volume`
/// captured 19 lines (zero-area triangle), 26 (single open triangle) and 50
/// (unit cube missing one face); a successful unit cube captured 82. A
/// 40-line tail therefore keeps a small failing run's capture ENTIRE — the
/// first two arrive whole — and for a larger one keeps the part that carries
/// the diagnosis, since gmsh states its conclusion at the END of the stream.
///
/// A cap is needed at all because the capture has no ceiling of its own
/// (see the module doc): how much gmsh narrates is gmsh's choice, and the
/// message it lands in flows on into logs and the GUI, where an unbounded
/// tail is a cost paid by every reader.
///
/// `pub` (like [`crate::mesh_size_clamp`]'s defaults, and for the same
/// reason) so this crate's `tests/` binaries — separate compilation units —
/// can assert against the cap rather than re-declaring a literal that could
/// drift away from the value this module actually applies.
pub const MAX_APPENDED_LOG_LINES: usize = 40;

/// Fold a captured gmsh log tail into `err`, returning the annotated error.
///
/// Pure: takes the lines already read, calls no gmsh function, and needs no
/// lock — which is what lets the format be tested without touching the
/// process-global capture switch.
///
/// Two callers. [`LogCapture::annotate`] folds in the LIVE capture, for a
/// failure that leaves libgmsh standing. `init::mesh_generate_with_recovery`
/// folds in a copy it read before recycling libgmsh, because the capture
/// lives inside the library it destroys — see its "Why the diagnosis is read
/// here", which is also why the mesher failure is deliberately outside
/// `mesh_to_volume`'s `LogCapture` seam.
///
/// Two inputs are passed through untouched: an empty `lines` (so a
/// best-effort arm that failed costs the caller nothing but the capture it
/// never got), and any variant other than
/// [`GeometryError::OperationFailed`] — that is the only one carrying a
/// gmsh message worth extending.
///
/// Otherwise the original message is kept as the prefix — the
/// `gmshLoggerGetLastError` annotation is ADDED to, never replaced — and
/// the last [`MAX_APPENDED_LOG_LINES`] entries follow it, one per line,
/// under a `gmsh log ({shown} of {total} lines):` header. That header is
/// emitted in the same form whether or not lines were elided, so there is
/// no branch to get wrong and a reader never has to infer whether the tail
/// is the whole capture.
pub fn annotated(err: GeometryError, lines: &[String]) -> GeometryError {
    match err {
        GeometryError::OperationFailed(message) if !lines.is_empty() => {
            let kept = &lines[lines.len().saturating_sub(MAX_APPENDED_LOG_LINES)..];
            let mut message_with_log = format!(
                "{message}\ngmsh log ({} of {} lines):",
                kept.len(),
                lines.len(),
            );
            for line in kept {
                message_with_log.push_str("\n  ");
                message_with_log.push_str(line);
            }
            GeometryError::OperationFailed(message_with_log)
        }
        unannotated => unannotated,
    }
}

/// RAII arm/stop of gmsh's process-global message capture, covering the
/// early-`?`-return paths as well as success.
///
/// A caller holding [`crate::init::GMSH_LOCK`] arms one of these, then folds
/// the capture into any error it is about to return via [`Self::annotate`].
/// The stop rides on `drop`, so the module invariant above is kept
/// structurally rather than re-derived at each of the caller's exit points.
///
/// # Why it borrows the lock guard
///
/// The FFI call in `drop` flips a process-global gmsh switch and must
/// therefore happen while [`crate::init::GMSH_LOCK`] is held. The
/// `PhantomData<&'g GmshGuard>` makes that structural rather than a comment a
/// refactor can quietly violate: [`Self::armed`] can only be called with a
/// live guard in hand, so the binding cannot be hoisted above the
/// `let _guard = init::lock()?` line, and because this type has a `Drop` impl
/// (no `#[may_dangle]`) dropck requires the borrow to still be live when it
/// drops — which forces the stop to land *before* the lock is released.
///
/// The witness is [`crate::init::GmshGuard`], not the weaker
/// `&MutexGuard<'_, ()>` that [`crate::mesh_size_clamp::MeshSizeClampReset`]
/// still takes. `GmshGuard`'s own doc calls that weak form "exactly the
/// witness this type was introduced to stop handing out, so the one site that
/// still needs it names it", and names `MeshSizeClampReset` as that one site —
/// so reaching for `GmshGuard::clamp_reset_witness` here would falsify it. The
/// strong witness is the honest one anyway: a `GmshGuard` proves libgmsh was
/// alive when the lock was taken, and a capture is a read of a buffer that
/// lives inside the library.
///
/// # The one contract a caller can still break
///
/// Gmsh's capture is a SINGLE process-global switch, not a stack, so two
/// live `LogCapture`s under one lock hold would not nest: the inner one's
/// `drop` stops the outer one's capture and drains the buffer out from under
/// it, leaving the outer `annotate` with nothing. Arm at most one per lock
/// hold. Today there is exactly one call site,
/// [`crate::kernel_real::GmshKernel::mesh_to_volume`] — and its seam
/// deliberately does not span the one call that recycles libgmsh, which
/// annotates its own failure instead.
pub struct LogCapture<'g>(std::marker::PhantomData<&'g crate::init::GmshGuard>);

impl<'g> LogCapture<'g> {
    /// Arm the capture. Takes the live [`crate::init::GmshGuard`] by reference
    /// purely for its lifetime — the guard itself is never touched.
    ///
    /// Best-effort on purpose: a diagnostic that cannot be armed must never
    /// turn a mesh that would have succeeded into a failure, so a
    /// `logger_start` error is dropped rather than propagated. The
    /// degradation is graceful and needs no branch downstream —
    /// [`crate::ffi::logger_get`] on a logger that was never started returns
    /// an empty `Vec` (measured; see its doc), which is exactly
    /// [`annotated`]'s pass-through path, so the caller gets back the error
    /// it would have got without capture at all.
    pub fn armed(_guard: &'g crate::init::GmshGuard) -> Self {
        let _ = crate::ffi::logger_start();
        Self(std::marker::PhantomData)
    }

    /// Fold everything captured so far into `err`.
    ///
    /// Reads without draining — the drain is `drop`'s job — so this may be
    /// called at any point while the guard is live.
    pub fn annotate(&self, err: GeometryError) -> GeometryError {
        annotated(err, &crate::ffi::logger_get().unwrap_or_default())
    }
}

impl Drop for LogCapture<'_> {
    fn drop(&mut self) {
        // Best-effort, like `MeshSizeClampReset::drop`: a failure here cannot
        // be reported from `drop` and must not mask the real result.
        let _ = crate::ffi::logger_stop();
    }
}
