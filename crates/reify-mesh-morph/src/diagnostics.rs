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
