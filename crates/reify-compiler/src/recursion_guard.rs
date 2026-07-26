//! Bounded compiler expression-recursion depth (task #5337).
//!
//! The compiler's expression recursion (`compile_expr_guarded_with_expected`
//! in `expr.rs`) and its geometry counterpart (`compile_geometry_call` in
//! `geometry.rs`) are mutually recursive and, on deeply-nested input (e.g.
//! `difference(difference(…), …)` boolean geometry), can descend arbitrarily
//! deep. On a small-stack embedder thread that overflows the guard page and
//! SIGSEGVs the whole process — an *uncatchable* hardware fault that no
//! `catch_unwind` can intercept. The motivating case is the GUI, which compiles
//! inside the synchronous `open_file_engine` Tauri command: that runs on a
//! tokio worker, whose stack is tokio's 2 MiB default because nothing under
//! `gui/src-tauri` sets `stack_size`/`thread_stack_size`.
//!
//! This module provides the mechanisms that convert that crash into a clean,
//! bounded outcome, shared by BOTH recursion entry points through the single
//! [`with_recursion_guard`] wrapper:
//!
//! 1. **On-demand stack growth** via [`RECURSION_RED_ZONE`] /
//!    [`RECURSION_STACK_GROWTH`] passed to `stacker::maybe_grow`, so realistic
//!    (and moderately deep) input compiles successfully even on a 2 MiB stack.
//! 2. **A hard depth cap** ([`MAX_COMPILE_RECURSION_DEPTH`]) tracked by the
//!    [`RecursionDepthGuard`] RAII counter: past the cap the compiler refuses
//!    loudly with an `E_EXPR_NESTING_TOO_DEEP` diagnostic instead of recursing
//!    (and eventually OOM/overflowing) further.
//!
//! The depth is tracked with a thread-local counter rather than a threaded
//! `depth: usize` parameter because the recursion fans out across ~30 match
//! arms and ~10 call sites and *resets* across `compile_expr` boundaries
//! (`compile_geometry_call` → `compile_expr`); a thread-local shared by both
//! entry points bounds their combined on-stack depth uniformly with zero
//! signature churn. Compilation is single-threaded per thread, so the
//! thread-local has no contention. The RAII `Drop` decrements on the normal
//! return, on the poison early-return, and on panic-unwind alike.
//!
//! **Reporting contract:** at most ONE `E_EXPR_NESTING_TOO_DEEP` diagnostic per
//! outermost recursion entry. Over-deep siblings (a wide node whose children
//! all sit past the cap) each still fail — poison / `None` — but only the first
//! pushes a diagnostic; the latch re-arms when the depth counter returns to 0,
//! i.e. once the outermost compile entry has returned.

use std::cell::Cell;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Maximum combined recursion depth across the two compiler expression
/// recursion entry points before the cap fires.
///
/// Rationale: realistic designs nest at most a few dozen levels
/// (`designs/litter_tray/bottom_deck_split.ri` ≈ 9 boolean levels), and even
/// the 8 MiB CLI main thread only fits ~55 fat debug frames today — so any
/// input that currently compiles is far below 256. The cap therefore never
/// regresses currently-working input, *rescues* the ~55–256 range (previously
/// a CLI stack overflow) via on-demand stack growth, and cleanly *rejects*
/// anything beyond with a diagnostic.
///
/// Resource bound at the cap, from the per-level measurement recorded on
/// [`RECURSION_RED_ZONE`]: a full 256-level descent consumes
/// ≈ 256 × 143 KiB ≈ 36 MiB of stack in a **debug** build (far less in release,
/// where frames are much smaller). With [`RECURSION_STACK_GROWTH`] = 16 MiB
/// that descent spans ~3 live `stacker` segments, i.e. ~48 MiB of *mapped*
/// stack — lazily faulted, so resident pages are only the ones actually
/// touched. It is transient: the cap bails at 256 and every segment is unmapped
/// as the recursion returns.
pub(crate) const MAX_COMPILE_RECURSION_DEPTH: usize = 256;

/// Compile-time pin: the cap must keep generous headroom over realistic nesting
/// (a few dozen levels) so it can only ever fire on pathological input.
const _: () = assert!(
    MAX_COMPILE_RECURSION_DEPTH > 128,
    "MAX_COMPILE_RECURSION_DEPTH must exceed realistic-nesting headroom (128)"
);

/// `stacker::maybe_grow` red zone: if fewer than this many bytes of stack
/// remain on entry to a recursion level, grow before descending.
///
/// Sized from a measurement, not a guess. Instrumenting both entry points with
/// `stacker::remaining_stack()` and compiling a nested `difference(…)` AST and a
/// nested `BinOp` AST (debug, x86_64-unknown-linux-gnu, rustc opt-level 0)
/// gives the stack consumed between two consecutive checks:
///
/// | transition                                        | bytes per level     |
/// |---------------------------------------------------|---------------------|
/// | `compile_geometry_call` → `compile_geometry_call`  | 146,176 (~143 KiB)  |
/// | `compile_geometry_call` → `compile_expr…`          | 144,880 (~141 KiB)  |
/// | `compile_expr…` → `compile_expr…`                  |  95,984 (~94 KiB)   |
///
/// The red zone MUST exceed one level's consumption, or a single frame could
/// still overflow between two checks; 512 KiB is ~3.5× the worst measured level,
/// leaving margin for arms the measurement did not cover. It must equally stay
/// well below the smallest embedder stack (the 2 MiB tokio worker) so ordinary
/// shallow compiles never trigger growth at all — at 512 KiB the first ~10 debug
/// levels (many more in release) fit on the original stack, versus ~7 at the
/// 1 MiB this constant originally guessed.
///
/// Why not amortise the check (grow only every Nth level)? The red zone would
/// then have to cover N levels — which on a 2 MiB worker exceeds the whole stack
/// and would force a grow on *every* top-level compile. Growing only near
/// exhaustion is strictly cheaper.
pub(crate) const RECURSION_RED_ZONE: usize = 512 * 1024;

/// New stack segment size requested from `stacker::maybe_grow` when the red zone
/// is hit.
///
/// `stacker` `mmap`s a fresh segment per grow and unmaps it when that level
/// returns (it caches nothing), so this trades mapping size against the number
/// of grows. 16 MiB covers ~110 further debug levels, so even a descent all the
/// way to [`MAX_COMPILE_RECURSION_DEPTH`] needs only ~3 nested segments while
/// keeping any single mapping modest.
pub(crate) const RECURSION_STACK_GROWTH: usize = 16 * 1024 * 1024;

thread_local! {
    /// Live count of nested compiler-recursion guards on the current thread.
    /// Single-threaded per compile, so a plain `Cell` suffices (no atomics).
    static COMPILE_RECURSION_DEPTH: Cell<usize> = const { Cell::new(0) };

    /// Latch: set once the too-deep diagnostic has been reported for the
    /// current outermost recursion entry, cleared when the depth counter
    /// returns to 0 (see the module-level reporting contract).
    static TOO_DEEP_REPORTED: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that increments the thread-local compiler-recursion depth counter
/// on [`enter`](RecursionDepthGuard::enter) and decrements it on `Drop`.
///
/// Held for the whole body of each recursion entry point (see
/// [`with_recursion_guard`]) so the counter reflects the number of live
/// on-stack recursion frames — decrementing correctly on the normal return, on
/// the poison/`None` early-return, and on panic-unwind.
#[must_use = "the guard must be held in a binding for the whole recursive call; \
              dropping it immediately would defeat the depth cap"]
pub(crate) struct RecursionDepthGuard {
    // Private field so the guard can only be produced via `enter()`, which is
    // the sole site that increments the counter (keeping increment/decrement
    // balanced by construction).
    _private: (),
}

impl RecursionDepthGuard {
    /// Increment the thread-local depth counter and return a guard whose `Drop`
    /// will decrement it.
    pub(crate) fn enter() -> Self {
        COMPILE_RECURSION_DEPTH.with(|d| d.set(d.get() + 1));
        RecursionDepthGuard { _private: () }
    }

    /// The current thread-local recursion depth (number of live guards,
    /// including `self`). Compared against [`MAX_COMPILE_RECURSION_DEPTH`]
    /// immediately after `enter()` by [`with_recursion_guard`].
    pub(crate) fn depth(&self) -> usize {
        COMPILE_RECURSION_DEPTH.with(|d| d.get())
    }
}

impl Drop for RecursionDepthGuard {
    fn drop(&mut self) {
        // `saturating_sub` is defensive: enter/Drop are balanced by
        // construction, but never underflow even if that invariant is somehow
        // broken.
        let remaining = COMPILE_RECURSION_DEPTH.with(|d| {
            let n = d.get().saturating_sub(1);
            d.set(n);
            n
        });
        if remaining == 0 {
            // The outermost recursion entry has returned (normally or by
            // unwind): re-arm the report latch so the next top-level compile
            // reports its own too-deep error.
            TOO_DEEP_REPORTED.with(|reported| reported.set(false));
        }
    }
}

/// Run one level of compiler expression recursion under the shared depth cap
/// and on-demand stack growth.
///
/// This is the single implementation of the mechanism described in the module
/// docs, shared by `compile_expr_guarded_with_expected` and
/// `compile_geometry_call` so their combined on-stack depth is bounded
/// uniformly and the rationale lives in exactly one place.
///
/// * `body` runs the level's real work on a stack that has been grown if it was
///   within [`RECURSION_RED_ZONE`] of exhaustion. It receives `diagnostics`
///   back (rather than capturing it) so the two branches cannot both borrow it.
/// * `too_deep` supplies the caller's failure value once the depth exceeds
///   [`MAX_COMPILE_RECURSION_DEPTH`] — poison for the expression path, `None`
///   for the geometry path. The `E_EXPR_NESTING_TOO_DEEP` diagnostic is pushed
///   here, at most once per outermost recursion entry (module reporting
///   contract), so `too_deep` only has to name the value.
pub(crate) fn with_recursion_guard<'d, T>(
    span: SourceSpan,
    diagnostics: &'d mut Vec<Diagnostic>,
    too_deep: impl FnOnce() -> T,
    body: impl FnOnce(&'d mut Vec<Diagnostic>) -> T,
) -> T {
    let _depth_guard = RecursionDepthGuard::enter();
    if _depth_guard.depth() > MAX_COMPILE_RECURSION_DEPTH {
        if !TOO_DEEP_REPORTED.with(|reported| reported.replace(true)) {
            diagnostics.push(recursion_too_deep_diagnostic(span));
        }
        return too_deep();
    }
    // `_depth_guard` stays live across the grown-stack call (it is not
    // captured), so its RAII decrement fires when this level returns.
    stacker::maybe_grow(RECURSION_RED_ZONE, RECURSION_STACK_GROWTH, move || {
        body(diagnostics)
    })
}

/// Build the `E_EXPR_NESTING_TOO_DEEP` diagnostic emitted when compiler
/// expression recursion exceeds [`MAX_COMPILE_RECURSION_DEPTH`].
///
/// Follows the `TraitRefinementChainTooDeep` too-deep *shape*
/// (`trait_requirements.rs`): `Diagnostic::error(...)` + `.with_code(...)` +
/// one `.with_label(...)` anchored at the offending expression's span. Unlike
/// that older diagnostic, the message also carries the `E_EXPR_NESTING_TOO_DEEP`
/// mnemonic inline, which is the prevailing convention for newer `E_*`
/// diagnostics in this workspace (`E_MODULE_PATH_MISMATCH`, `E_PRIV_REDUNDANT`,
/// `E_OBJECTIVE_CONFLICT`, `E_DFM_OVERHANG`, …). The message names the cap, the
/// failure, and the `let`-binding remediation.
pub(crate) fn recursion_too_deep_diagnostic(span: SourceSpan) -> Diagnostic {
    Diagnostic::error(format!(
        "E_EXPR_NESTING_TOO_DEEP: expression nests too deeply (exceeded {} levels); \
         bind intermediate results with `let` to reduce nesting depth",
        MAX_COMPILE_RECURSION_DEPTH
    ))
    .with_code(DiagnosticCode::ExpressionNestingTooDeep)
    .with_label(DiagnosticLabel::new(span, "expression too deeply nested"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RAII guard's thread-local counter reflects the number of live
    /// guards: nested `enter()` calls increment (1, 2, 3…), dropping a guard
    /// decrements, and once every guard has dropped the counter is back at 0
    /// (so a fresh `enter()` reports depth 1 again).
    #[test]
    fn recursion_depth_guard_counts_live_guards() {
        // (a) nested enters increment 1, 2, 3
        let g1 = RecursionDepthGuard::enter();
        assert_eq!(g1.depth(), 1, "first enter -> depth 1");
        let g2 = RecursionDepthGuard::enter();
        assert_eq!(g2.depth(), 2, "second enter -> depth 2");
        let g3 = RecursionDepthGuard::enter();
        assert_eq!(g3.depth(), 3, "third enter -> depth 3");

        // (b) dropping a guard decrements the thread-local counter: after
        // dropping the innermost guard a fresh enter reports depth 3 again.
        drop(g3);
        let g3b = RecursionDepthGuard::enter();
        assert_eq!(g3b.depth(), 3, "re-enter after one drop -> depth 3 again");
        drop(g3b);
        drop(g2);
        drop(g1);

        // (c) after all guards drop, the counter has returned to 0, so a fresh
        // enter reports depth 1.
        let g = RecursionDepthGuard::enter();
        assert_eq!(
            g.depth(),
            1,
            "counter returned to 0 after all guards dropped"
        );
    }

    /// The counter is restored when a panic unwinds *through* live guards — the
    /// module doc's explicit panic-unwind claim. Without it, one panicking
    /// compile would permanently poison every later compile on that thread with
    /// a spurious `E_EXPR_NESTING_TOO_DEEP`.
    #[test]
    fn recursion_depth_guard_unwinds_to_zero_on_panic() {
        let unwound = std::panic::catch_unwind(|| {
            let _g1 = RecursionDepthGuard::enter();
            let _g2 = RecursionDepthGuard::enter();
            assert_eq!(_g2.depth(), 2, "two live guards before the panic");
            panic!("simulated mid-recursion panic (expected by this test)");
        });
        assert!(unwound.is_err(), "the closure must have panicked");

        let g = RecursionDepthGuard::enter();
        assert_eq!(
            g.depth(),
            1,
            "panic-unwind must run every guard's Drop, returning the counter to 0"
        );
    }

    /// `recursion_too_deep_diagnostic(span)` builds the machine-checkable half
    /// of the contract consumers depend on: `Severity::Error`, code
    /// `ExpressionNestingTooDeep`, and exactly one label anchored at the passed
    /// span. (The prose wording is deliberately NOT pinned here — the code, not
    /// the message text, is the stable contract.)
    #[test]
    fn recursion_too_deep_diagnostic_has_code_severity_and_label() {
        let span = reify_core::SourceSpan::new(7, 42);
        let diag = recursion_too_deep_diagnostic(span);

        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::ExpressionNestingTooDeep)
        );
        assert_eq!(diag.labels.len(), 1, "expected exactly one label");
        assert_eq!(
            diag.labels[0].span, span,
            "label should be anchored at the passed span"
        );
    }

    /// `with_recursion_guard` runs `body` below the cap and `too_deep` past it,
    /// and reports the too-deep diagnostic AT MOST ONCE per outermost entry —
    /// the module's reporting contract. Every over-deep level still gets the
    /// caller's failure value; only the diagnostic is latched.
    #[test]
    fn with_recursion_guard_reports_too_deep_once_per_outermost_entry() {
        let span = reify_core::SourceSpan::new(0, 1);
        let too_deep_count = |diags: &[Diagnostic]| {
            diags
                .iter()
                .filter(|d| d.code == Some(reify_core::DiagnosticCode::ExpressionNestingTooDeep))
                .count()
        };

        // Below the cap: `body` runs, nothing is reported.
        let mut diags: Vec<Diagnostic> = vec![];
        let ran = with_recursion_guard(span, &mut diags, || "too_deep", |_| "body");
        assert_eq!(ran, "body", "below the cap the body must run");
        assert_eq!(too_deep_count(&diags), 0);

        // Past the cap: three sibling entries all fail, one diagnostic total.
        let guards: Vec<RecursionDepthGuard> = (0..MAX_COMPILE_RECURSION_DEPTH)
            .map(|_| RecursionDepthGuard::enter())
            .collect();
        let mut diags: Vec<Diagnostic> = vec![];
        for _ in 0..3 {
            let ran = with_recursion_guard(span, &mut diags, || "too_deep", |_| "body");
            assert_eq!(ran, "too_deep", "past the cap every sibling must fail");
        }
        assert_eq!(
            too_deep_count(&diags),
            1,
            "wide over-deep nodes must report once, not once per sibling: {:?}",
            diags
        );

        // Dropping to depth 0 re-arms both the counter and the latch, so the
        // next outermost compile reports its own error.
        drop(guards);
        let guards: Vec<RecursionDepthGuard> = (0..MAX_COMPILE_RECURSION_DEPTH)
            .map(|_| RecursionDepthGuard::enter())
            .collect();
        let mut diags: Vec<Diagnostic> = vec![];
        let ran = with_recursion_guard(span, &mut diags, || "too_deep", |_| "body");
        assert_eq!(ran, "too_deep");
        assert_eq!(
            too_deep_count(&diags),
            1,
            "the latch must re-arm once the depth counter returns to 0"
        );
        drop(guards);
    }
}
