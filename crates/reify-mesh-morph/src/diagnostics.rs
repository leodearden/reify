//! Session-level diagnostic counters + verbose-logging policy for the
//! mesh-morph engine (PRD `docs/prds/v0_3/mesh-morphing.md` task #11).
//!
//! This module is the standalone diagnostic-counter + failure-mode-logging
//! infrastructure: a set of process-global lock-free counters, per-outcome
//! recorder functions that couple a counter increment with its policy-level
//! `tracing` event, a `snapshot()` accessor for the downstream debug RPC, and
//! a `format_summary()` renderer for the `--verbose` exit line.
//!
//! Engine call-site wiring is deferred (see the `// G-allow:` markers on the
//! recorder functions); the events fire from the engine integration in
//! `reify-eval`'s `engine_build.rs` (PRD task #10).

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

// ── Snapshot DTO ──────────────────────────────────────────────────────────────

/// Point-in-time snapshot of the process-global mesh-morph diagnostic counters.
///
/// The six per-outcome buckets named in PRD task #11 plus an additive
/// `panicked` bucket (the logging policy mandates "Morph panics → error +
/// diagnostic, plus counter"). Field names match the counter names exactly (no
/// `#[serde(rename_all)]`) so the downstream debug-RPC consumer (task #2949)
/// can deserialize this DTO by name. Derives mirror `stats::MorphStats`.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticSnapshot {
    /// Successful morphs (existing mesh deformed in place).
    pub morphed: u64,
    /// Remeshed because the quality check hard-failed (element inversion).
    pub remeshed_quality_hard_fail: u64,
    /// Remeshed because the quality check soft-failed (metric threshold breach).
    pub remeshed_quality_soft_fail: u64,
    /// Ineligible: Stage-A structural change.
    pub ineligible_structural_change: u64,
    /// Ineligible: Stage-B bijection failure.
    pub ineligible_bijection_failure: u64,
    /// Ineligible: persistent-naming-layer error.
    pub ineligible_naming_error: u64,
    /// Morph panicked (caught at the engine boundary).
    pub panicked: u64,
}

// ── Process-global counters ───────────────────────────────────────────────────

/// Process-global lock-free counters, one per [`MorphOutcome`] bucket.
///
/// `AtomicU64::new(0)` is `const`, so a plain `static COUNTERS` suffices — no
/// `OnceLock`/`Mutex` wrapper (simpler and lock-free, unlike `stats.rs`). The
/// counters are mutually independent with no cross-counter invariant, so
/// `Ordering::Relaxed` is sufficient for both `fetch_add` and `load`.
struct Counters {
    morphed: AtomicU64,
    remeshed_quality_hard_fail: AtomicU64,
    remeshed_quality_soft_fail: AtomicU64,
    ineligible_structural_change: AtomicU64,
    ineligible_bijection_failure: AtomicU64,
    ineligible_naming_error: AtomicU64,
    panicked: AtomicU64,
}

static COUNTERS: Counters = Counters {
    morphed: AtomicU64::new(0),
    remeshed_quality_hard_fail: AtomicU64::new(0),
    remeshed_quality_soft_fail: AtomicU64::new(0),
    ineligible_structural_change: AtomicU64::new(0),
    ineligible_bijection_failure: AtomicU64::new(0),
    ineligible_naming_error: AtomicU64::new(0),
    panicked: AtomicU64::new(0),
};

/// Return a point-in-time snapshot of the process-global diagnostic counters.
///
/// Reached via the `diagnostics::` module path; deliberately NOT bare-re-exported
/// from the crate root, to avoid colliding with the existing `stats::snapshot`
/// re-export.
pub fn snapshot() -> DiagnosticSnapshot {
    DiagnosticSnapshot {
        morphed: COUNTERS.morphed.load(Ordering::Relaxed),
        remeshed_quality_hard_fail: COUNTERS.remeshed_quality_hard_fail.load(Ordering::Relaxed),
        remeshed_quality_soft_fail: COUNTERS.remeshed_quality_soft_fail.load(Ordering::Relaxed),
        ineligible_structural_change: COUNTERS
            .ineligible_structural_change
            .load(Ordering::Relaxed),
        ineligible_bijection_failure: COUNTERS
            .ineligible_bijection_failure
            .load(Ordering::Relaxed),
        ineligible_naming_error: COUNTERS.ineligible_naming_error.load(Ordering::Relaxed),
        panicked: COUNTERS.panicked.load(Ordering::Relaxed),
    }
}

/// Reset all counters to zero.
///
/// Available in same-crate `#[cfg(test)]` context, and also when the crate is
/// compiled with `features = ["testing"]` — enabling cross-crate test isolation
/// (e.g. from task #2949's debug-RPC tests). Mirrors `stats::reset_for_test`.
#[cfg(any(test, feature = "testing"))]
pub fn reset_for_test() {
    COUNTERS.morphed.store(0, Ordering::Relaxed);
    COUNTERS
        .remeshed_quality_hard_fail
        .store(0, Ordering::Relaxed);
    COUNTERS
        .remeshed_quality_soft_fail
        .store(0, Ordering::Relaxed);
    COUNTERS
        .ineligible_structural_change
        .store(0, Ordering::Relaxed);
    COUNTERS
        .ineligible_bijection_failure
        .store(0, Ordering::Relaxed);
    COUNTERS.ineligible_naming_error.store(0, Ordering::Relaxed);
    COUNTERS.panicked.store(0, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize parallel test access to the process-global diagnostic
    /// counters. Each test acquires this before resetting state so tests don't
    /// interfere with each other regardless of execution order. Mirrors the
    /// `TEST_LOCK` discipline in `stats.rs`.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Run `f` under `TEST_LOCK` with freshly-reset counters.
    fn with_locked_state<F: FnOnce()>(f: F) {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_for_test();
        f();
    }

    // ── Step-1: snapshot foundation ───────────────────────────────────────────

    #[test]
    fn snapshot_returns_all_zeros_after_reset() {
        with_locked_state(|| {
            let s = snapshot();
            assert_eq!(s.morphed, 0, "morphed should be 0 after reset");
            assert_eq!(
                s.remeshed_quality_hard_fail, 0,
                "remeshed_quality_hard_fail should be 0 after reset"
            );
            assert_eq!(
                s.remeshed_quality_soft_fail, 0,
                "remeshed_quality_soft_fail should be 0 after reset"
            );
            assert_eq!(
                s.ineligible_structural_change, 0,
                "ineligible_structural_change should be 0 after reset"
            );
            assert_eq!(
                s.ineligible_bijection_failure, 0,
                "ineligible_bijection_failure should be 0 after reset"
            );
            assert_eq!(
                s.ineligible_naming_error, 0,
                "ineligible_naming_error should be 0 after reset"
            );
            assert_eq!(s.panicked, 0, "panicked should be 0 after reset");
        });
    }
}
