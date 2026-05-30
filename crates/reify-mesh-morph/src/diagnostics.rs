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

use crate::eligibility::Reason;
use crate::quality::QualityVerdict;

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

// ── Outcome taxonomy + counter routing ────────────────────────────────────────

/// The seven mutually-exclusive per-tick outcomes the mesh-morph engine can
/// produce. One variant per [`DiagnosticSnapshot`] counter bucket; selected
/// inside the recorders from the existing `&QualityVerdict` / `&Reason` the
/// engine already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MorphOutcome {
    /// Existing mesh deformed in place.
    Morphed,
    /// Remeshed after a quality hard-fail (element inversion).
    RemeshedQualityHardFail,
    /// Remeshed after a quality soft-fail (metric threshold breach).
    RemeshedQualitySoftFail,
    /// Ineligible: Stage-A structural change.
    IneligibleStructuralChange,
    /// Ineligible: Stage-B bijection failure.
    IneligibleBijectionFailure,
    /// Ineligible: persistent-naming-layer error.
    IneligibleNamingError,
    /// Morph panicked (caught at the engine boundary).
    Panicked,
}

// Exhaustive no-wildcard variant compile-fence (crate convention). Adding or
// renaming a `MorphOutcome` variant without updating `counter` below — and the
// snapshot/format surfaces — is a compile error.
const _: fn() = || {
    fn _assert_exhaustive(outcome: MorphOutcome) {
        match outcome {
            MorphOutcome::Morphed
            | MorphOutcome::RemeshedQualityHardFail
            | MorphOutcome::RemeshedQualitySoftFail
            | MorphOutcome::IneligibleStructuralChange
            | MorphOutcome::IneligibleBijectionFailure
            | MorphOutcome::IneligibleNamingError
            | MorphOutcome::Panicked => {}
        }
    }
};

/// Map an outcome to its process-global counter. The single source of truth for
/// bucket selection; the recorders route through this.
fn counter(outcome: MorphOutcome) -> &'static AtomicU64 {
    match outcome {
        MorphOutcome::Morphed => &COUNTERS.morphed,
        MorphOutcome::RemeshedQualityHardFail => &COUNTERS.remeshed_quality_hard_fail,
        MorphOutcome::RemeshedQualitySoftFail => &COUNTERS.remeshed_quality_soft_fail,
        MorphOutcome::IneligibleStructuralChange => &COUNTERS.ineligible_structural_change,
        MorphOutcome::IneligibleBijectionFailure => &COUNTERS.ineligible_bijection_failure,
        MorphOutcome::IneligibleNamingError => &COUNTERS.ineligible_naming_error,
        MorphOutcome::Panicked => &COUNTERS.panicked,
    }
}

// ── Recorders ─────────────────────────────────────────────────────────────────
//
// Each recorder couples a counter increment with its policy-level `tracing`
// event (the logging policy is added in a later step). Bucket selection lives
// here so the engine call site forwards the `&QualityVerdict` / `&Reason` it
// already holds.

/// Record a successful morph.
pub fn record_morphed() {
    counter(MorphOutcome::Morphed).fetch_add(1, Ordering::Relaxed);
}

/// Record a quality-driven remesh fallback, bucketed by hard vs soft fail.
///
/// [`QualityVerdict::Pass`] is not a remesh trigger — the engine only calls this
/// on a fail verdict — so it is a no-op here. The `debug_assert!` makes that
/// contract loud in debug builds at no release-build cost.
pub fn record_quality_remesh(verdict: &QualityVerdict) {
    let outcome = match verdict {
        QualityVerdict::HardFail(_) => MorphOutcome::RemeshedQualityHardFail,
        QualityVerdict::SoftFail(_) => MorphOutcome::RemeshedQualitySoftFail,
        QualityVerdict::Pass => {
            debug_assert!(
                false,
                "record_quality_remesh called with QualityVerdict::Pass (not a remesh trigger)"
            );
            return;
        }
    };
    counter(outcome).fetch_add(1, Ordering::Relaxed);
}

/// Record an ineligible edit, bucketed by reject category.
pub fn record_ineligible(reason: &Reason) {
    let outcome = match reason {
        Reason::StructuralChange => MorphOutcome::IneligibleStructuralChange,
        Reason::BijectionFailure(_) => MorphOutcome::IneligibleBijectionFailure,
        Reason::NamingLayerError { .. } => MorphOutcome::IneligibleNamingError,
    };
    counter(outcome).fetch_add(1, Ordering::Relaxed);
}

/// Record a caught morph panic. `detail` is surfaced in the error log (added in
/// a later step).
pub fn record_panicked(detail: &str) {
    let _ = detail;
    counter(MorphOutcome::Panicked).fetch_add(1, Ordering::Relaxed);
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
    use crate::eligibility::Reason;
    use crate::quality::QualityVerdict;
    use crate::types::{InversionDetails, SoftFailDetails};
    use crate::{BijectionFailure, NamingLayerErrorReason, SubShapeKind};
    use std::sync::Mutex;

    /// A `QualityVerdict::HardFail` fixture for the remesh-bucket tests.
    fn hard_fail() -> QualityVerdict {
        QualityVerdict::HardFail(InversionDetails {
            element_index: 0,
            jacobian: -1.0,
        })
    }

    /// A `QualityVerdict::SoftFail` fixture (all detail fields `None`).
    fn soft_fail() -> QualityVerdict {
        QualityVerdict::SoftFail(SoftFailDetails {
            min_scaled_jacobian: None,
            pct_below_025: None,
            max_aspect_ratio_factor: None,
            degenerate_morphed_element: None,
        })
    }

    /// A `Reason::BijectionFailure` fixture for the bijection-bucket tests.
    fn bijection_failure() -> Reason {
        Reason::BijectionFailure(BijectionFailure::CountMismatch {
            kind: SubShapeKind::Face,
            old_count: 1,
            new_count: 2,
        })
    }

    /// A `Reason::NamingLayerError` fixture for the naming-bucket tests.
    fn naming_error() -> Reason {
        Reason::NamingLayerError {
            kind: SubShapeKind::Face,
            reason: NamingLayerErrorReason::Imported,
        }
    }

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

    // ── Step-3: counter routing ───────────────────────────────────────────────
    //
    // Each recorder must increment ONLY its bucket and leave the others 0. The
    // whole-snapshot `assert_eq!` against a `{ field: 1, ..Default::default() }`
    // literal pins both the increment and the zero-elsewhere invariant at once.

    #[test]
    fn record_morphed_increments_only_morphed() {
        with_locked_state(|| {
            record_morphed();
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    morphed: 1,
                    ..Default::default()
                }
            );
            // Repeated calls accumulate.
            record_morphed();
            record_morphed();
            assert_eq!(snapshot().morphed, 3, "repeated calls must accumulate");
        });
    }

    #[test]
    fn record_quality_remesh_hard_fail_increments_only_hard_bucket() {
        with_locked_state(|| {
            record_quality_remesh(&hard_fail());
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    remeshed_quality_hard_fail: 1,
                    ..Default::default()
                }
            );
            record_quality_remesh(&hard_fail());
            assert_eq!(
                snapshot().remeshed_quality_hard_fail,
                2,
                "repeated calls must accumulate"
            );
        });
    }

    #[test]
    fn record_quality_remesh_soft_fail_increments_only_soft_bucket() {
        with_locked_state(|| {
            record_quality_remesh(&soft_fail());
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    remeshed_quality_soft_fail: 1,
                    ..Default::default()
                }
            );
        });
    }

    #[test]
    fn record_ineligible_structural_change_increments_only_structural_bucket() {
        with_locked_state(|| {
            record_ineligible(&Reason::StructuralChange);
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    ineligible_structural_change: 1,
                    ..Default::default()
                }
            );
        });
    }

    #[test]
    fn record_ineligible_bijection_failure_increments_only_bijection_bucket() {
        with_locked_state(|| {
            record_ineligible(&bijection_failure());
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    ineligible_bijection_failure: 1,
                    ..Default::default()
                }
            );
        });
    }

    #[test]
    fn record_ineligible_naming_error_increments_only_naming_bucket() {
        with_locked_state(|| {
            record_ineligible(&naming_error());
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    ineligible_naming_error: 1,
                    ..Default::default()
                }
            );
        });
    }

    #[test]
    fn record_panicked_increments_only_panicked() {
        with_locked_state(|| {
            record_panicked("x");
            assert_eq!(
                snapshot(),
                DiagnosticSnapshot {
                    panicked: 1,
                    ..Default::default()
                }
            );
            record_panicked("y");
            assert_eq!(snapshot().panicked, 2, "repeated calls must accumulate");
        });
    }
}
