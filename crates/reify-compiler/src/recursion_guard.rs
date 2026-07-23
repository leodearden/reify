//! Bounded compiler expression-recursion depth (task #5337).
//!
//! The compiler's expression recursion (`compile_expr_guarded_with_expected`
//! in `expr.rs`) and its geometry counterpart (`compile_geometry_call` in
//! `geometry.rs`) are mutually recursive and, on deeply-nested input (e.g.
//! `difference(difference(…), …)` boolean geometry), can descend arbitrarily
//! deep. On a small-stack embedder thread — notably the GUI's 2 MB tokio
//! worker — that overflows the guard page and SIGSEGVs the whole process, an
//! *uncatchable* hardware fault that no `catch_unwind` can intercept.
//!
//! This module provides the two mechanisms that convert that crash into a
//! clean, bounded outcome, shared by BOTH recursion entry points:
//!
//! 1. **On-demand stack growth** via [`RECURSION_RED_ZONE`] /
//!    [`RECURSION_STACK_GROWTH`] passed to `stacker::maybe_grow`, so realistic
//!    (and moderately deep) input compiles successfully even on a 2 MB stack.
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

use std::cell::Cell;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan};

/// Maximum combined recursion depth across the two compiler expression
/// recursion entry points before the cap fires.
///
/// Rationale: realistic designs nest at most a few dozen levels
/// (`designs/litter_tray/bottom_deck_split.ri` ≈ 9 boolean levels), and even
/// the 8 MB CLI main thread only fits ~50 fat debug frames today — so any
/// input that currently compiles is far below 256. The cap therefore never
/// regresses currently-working input, *rescues* the ~50–256 range (previously
/// a CLI stack overflow) via on-demand stack growth, and cleanly *rejects*
/// anything beyond with a diagnostic. Peak transient stack at the cap ≈ 256 ×
/// frame ≈ a few MB, then the cap bails — bounded.
pub(crate) const MAX_COMPILE_RECURSION_DEPTH: usize = 256;

/// `stacker::maybe_grow` red-zone: if fewer than this many bytes of stack
/// remain on entry, grow before recursing.
///
/// Must exceed the largest single debug frame of
/// `compile_expr_guarded_with_expected` (a huge `match` — tens to low-hundreds
/// of KB) so the stack grows *before* one frame can overflow. `maybe_grow` is
/// called at every recursion level, so growth triggers whenever the remaining
/// stack drops below this bound. Empirically validated by the 2 MB-thread
/// regression test; bump if that test still overflows.
pub(crate) const RECURSION_RED_ZONE: usize = 1024 * 1024;

/// New stack segment size requested from `stacker::maybe_grow` when the red
/// zone is hit. 16 MiB comfortably holds the remaining descent to
/// [`MAX_COMPILE_RECURSION_DEPTH`] before the cap fires.
pub(crate) const RECURSION_STACK_GROWTH: usize = 16 * 1024 * 1024;

thread_local! {
    /// Live count of nested compiler-recursion guards on the current thread.
    /// Single-threaded per compile, so a plain `Cell` suffices (no atomics).
    static COMPILE_RECURSION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// RAII guard that increments the thread-local compiler-recursion depth counter
/// on [`enter`](RecursionDepthGuard::enter) and decrements it on `Drop`.
///
/// Held (as `let _depth_guard = RecursionDepthGuard::enter();`) for the whole
/// body of each recursion entry point so the counter reflects the number of
/// live on-stack recursion frames — decrementing correctly on the normal
/// return, on the poison/`None` early-return, and on panic-unwind.
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
    /// including `self`). Callers compare this against
    /// [`MAX_COMPILE_RECURSION_DEPTH`] immediately after `enter()`.
    pub(crate) fn depth(&self) -> usize {
        COMPILE_RECURSION_DEPTH.with(|d| d.get())
    }
}

impl Drop for RecursionDepthGuard {
    fn drop(&mut self) {
        // `saturating_sub` is defensive: enter/Drop are balanced by
        // construction, but never underflow even if that invariant is somehow
        // broken.
        COMPILE_RECURSION_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Build the `E_EXPR_NESTING_TOO_DEEP` diagnostic emitted when compiler
/// expression recursion exceeds [`MAX_COMPILE_RECURSION_DEPTH`].
///
/// Mirrors the `TraitRefinementChainTooDeep` too-deep pattern
/// (`trait_requirements.rs`): `Diagnostic::error(...)` + `.with_code(...)` +
/// one `.with_label(...)` anchored at the offending expression's span. The
/// message names the cap, the failure, and the `let`-binding remediation.
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

    /// (d) The depth cap is present and carries generous headroom over
    /// realistic nesting (> 128).
    #[test]
    fn max_compile_recursion_depth_has_headroom() {
        assert!(
            MAX_COMPILE_RECURSION_DEPTH > 128,
            "MAX_COMPILE_RECURSION_DEPTH must exceed realistic-nesting headroom (128), got {}",
            MAX_COMPILE_RECURSION_DEPTH
        );
    }

    /// `recursion_too_deep_diagnostic(span)` builds the canonical
    /// `E_EXPR_NESTING_TOO_DEEP` error: `Severity::Error`, code
    /// `ExpressionNestingTooDeep`, a message carrying the mnemonic + the
    /// "nests too deeply" phrasing + the `let` remediation hint, and exactly
    /// one label anchored at the passed span.
    #[test]
    fn recursion_too_deep_diagnostic_has_code_message_and_label() {
        let span = reify_core::SourceSpan::new(7, 42);
        let diag = recursion_too_deep_diagnostic(span);

        assert_eq!(diag.severity, reify_core::Severity::Error);
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::ExpressionNestingTooDeep)
        );
        assert!(
            diag.message.contains("E_EXPR_NESTING_TOO_DEEP"),
            "message should carry the mnemonic, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("nests too deeply"),
            "message should describe the failure, got: {}",
            diag.message
        );
        assert!(
            diag.message.contains("let"),
            "message should hint the `let` remediation, got: {}",
            diag.message
        );
        assert_eq!(diag.labels.len(), 1, "expected exactly one label");
        assert_eq!(
            diag.labels[0].span, span,
            "label should be anchored at the passed span"
        );
    }
}
