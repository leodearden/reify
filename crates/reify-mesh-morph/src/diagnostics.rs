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

use std::fmt::Write as _;
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
// event. Bucket selection lives here so the engine call site forwards the
// `&QualityVerdict` / `&Reason` it already holds. The default event target is
// this module path (`reify_mesh_morph::diagnostics`).

/// Record a successful morph.
// G-allow: mesh-morph engine call-site wiring deferred — events fire from the engine integration in reify-eval engine_build.rs (PRD docs/prds/v0_3/mesh-morphing.md task #10, engine-wire task #3429); snapshot consumer is debug-RPC task #2949
pub fn record_morphed() {
    tracing::trace!("mesh morph: morphed");
    counter(MorphOutcome::Morphed).fetch_add(1, Ordering::Relaxed);
}

/// Record a quality-driven remesh fallback, bucketed by hard vs soft fail.
///
/// [`QualityVerdict::Pass`] is not a remesh trigger — the engine only calls this
/// on a fail verdict — so it is a no-op here. The `debug_assert!` makes that
/// contract loud in debug builds at no release-build cost.
// G-allow: mesh-morph engine call-site wiring deferred — events fire from the engine integration in reify-eval engine_build.rs (PRD docs/prds/v0_3/mesh-morphing.md task #10, engine-wire task #3429); snapshot consumer is debug-RPC task #2949
pub fn record_quality_remesh(verdict: &QualityVerdict) {
    let outcome = match verdict {
        QualityVerdict::HardFail(details) => {
            tracing::info!(
                kind = "hard",
                ?details,
                "mesh morph: quality fail, remesh fallback"
            );
            MorphOutcome::RemeshedQualityHardFail
        }
        QualityVerdict::SoftFail(details) => {
            tracing::info!(
                kind = "soft",
                ?details,
                "mesh morph: quality fail, remesh fallback"
            );
            MorphOutcome::RemeshedQualitySoftFail
        }
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
// G-allow: mesh-morph engine call-site wiring deferred — events fire from the engine integration in reify-eval engine_build.rs (PRD docs/prds/v0_3/mesh-morphing.md task #10, engine-wire task #3429); snapshot consumer is debug-RPC task #2949
pub fn record_ineligible(reason: &Reason) {
    tracing::trace!(reason = ?reason, "mesh morph: ineligible edit");
    let outcome = match reason {
        Reason::StructuralChange => MorphOutcome::IneligibleStructuralChange,
        Reason::BijectionFailure(_) => MorphOutcome::IneligibleBijectionFailure,
        Reason::NamingLayerError { .. } => MorphOutcome::IneligibleNamingError,
    };
    counter(outcome).fetch_add(1, Ordering::Relaxed);
}

/// Record a caught morph panic; `detail` is surfaced in the ERROR log message.
// G-allow: mesh-morph engine call-site wiring deferred — events fire from the engine integration in reify-eval engine_build.rs (PRD docs/prds/v0_3/mesh-morphing.md task #10, engine-wire task #3429); snapshot consumer is debug-RPC task #2949
pub fn record_panicked(detail: &str) {
    tracing::error!("mesh morph panicked: {detail}");
    counter(MorphOutcome::Panicked).fetch_add(1, Ordering::Relaxed);
}

// ── Verbose summary ───────────────────────────────────────────────────────────

/// Render the one-line `--verbose` exit summary for a snapshot.
///
/// Format: `mesh updates: {m} morphed, {r} remeshed, {i} ineligible`, where `r`
/// aggregates the two remesh buckets and `i` the three ineligible buckets. When
/// `i > 0`, a parenthetical lists only the non-zero ineligible sub-categories in
/// the fixed order structural, bijection, naming. When `panicked > 0`, a
/// `, {p} panicked` suffix is appended. Matches the PRD example exactly:
/// `mesh updates: 47 morphed, 4 remeshed, 2 ineligible (1 structural, 1 bijection)`.
pub fn format_summary(snap: &DiagnosticSnapshot) -> String {
    let remeshed = snap.remeshed_quality_hard_fail + snap.remeshed_quality_soft_fail;
    let ineligible = snap.ineligible_structural_change
        + snap.ineligible_bijection_failure
        + snap.ineligible_naming_error;

    let mut out = format!(
        "mesh updates: {} morphed, {} remeshed, {} ineligible",
        snap.morphed, remeshed, ineligible
    );

    if ineligible > 0 {
        let mut parts: Vec<String> = Vec::new();
        if snap.ineligible_structural_change > 0 {
            parts.push(format!("{} structural", snap.ineligible_structural_change));
        }
        if snap.ineligible_bijection_failure > 0 {
            parts.push(format!("{} bijection", snap.ineligible_bijection_failure));
        }
        if snap.ineligible_naming_error > 0 {
            parts.push(format!("{} naming", snap.ineligible_naming_error));
        }
        // `ineligible > 0` ⇒ at least one sub-count is non-zero ⇒ `parts` is
        // non-empty, so the parenthetical is never rendered empty.
        let _ = write!(out, " ({})", parts.join(", "));
    }

    if snap.panicked > 0 {
        let _ = write!(out, ", {} panicked", snap.panicked);
    }

    out
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
    use reify_test_support::{CapturingSubscriberBuilder, prime_tracing_callsite_cache};
    use std::sync::Mutex;
    use tracing::Level;

    /// The default event target for `tracing` events emitted from this module —
    /// matches the recorders' module path so `target_prefix` filtering admits
    /// exactly the events under test.
    const TARGET: &str = "reify_mesh_morph::diagnostics";

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

    // ── Step-5: logging policy ────────────────────────────────────────────────
    //
    // Each recorder emits exactly one tracing event at its policy level,
    // targeted at this module path. `prime_tracing_callsite_cache()` is
    // mandatory before the count assertions: without it a sibling parallel test
    // thread can poison the per-callsite Interest cache to `never`, silently
    // bypassing `with_default` and capturing nothing.

    #[test]
    fn record_morphed_logs_at_trace() {
        with_locked_state(|| {
            prime_tracing_callsite_cache();
            let (subscriber, capture) = CapturingSubscriberBuilder::new(Level::TRACE)
                .target_prefix(TARGET)
                .build();
            tracing::subscriber::with_default(subscriber, || {
                record_morphed();
            });
            assert_eq!(
                capture.count(),
                1,
                "record_morphed must emit exactly one TRACE event"
            );
        });
    }

    #[test]
    fn record_ineligible_logs_at_trace() {
        with_locked_state(|| {
            prime_tracing_callsite_cache();
            let (subscriber, capture) = CapturingSubscriberBuilder::new(Level::TRACE)
                .target_prefix(TARGET)
                .build();
            tracing::subscriber::with_default(subscriber, || {
                record_ineligible(&Reason::StructuralChange);
            });
            assert_eq!(
                capture.count(),
                1,
                "record_ineligible must emit exactly one TRACE event"
            );
        });
    }

    #[test]
    fn record_quality_remesh_logs_at_info_mentioning_remesh() {
        with_locked_state(|| {
            prime_tracing_callsite_cache();
            let (subscriber, capture) = CapturingSubscriberBuilder::new(Level::INFO)
                .target_prefix(TARGET)
                .build();
            tracing::subscriber::with_default(subscriber, || {
                record_quality_remesh(&hard_fail());
            });
            // INFO is load-bearing: it answers "why was that slider tick slow?".
            assert_eq!(
                capture.count(),
                1,
                "record_quality_remesh must emit exactly one INFO event"
            );
            let msgs = capture.messages();
            assert!(
                msgs[0].contains("remesh"),
                "INFO message must mention 'remesh', got: {:?}",
                msgs[0]
            );
        });
    }

    #[test]
    fn record_panicked_logs_at_error_with_detail() {
        with_locked_state(|| {
            prime_tracing_callsite_cache();
            let (subscriber, capture) = CapturingSubscriberBuilder::new(Level::ERROR)
                .target_prefix(TARGET)
                .build();
            tracing::subscriber::with_default(subscriber, || {
                record_panicked("kaboom-detail");
            });
            assert_eq!(
                capture.count(),
                1,
                "record_panicked must emit exactly one ERROR event"
            );
            let msgs = capture.messages();
            assert!(
                msgs[0].contains("kaboom-detail"),
                "ERROR message must contain the passed detail, got: {:?}",
                msgs[0]
            );
        });
    }

    // ── Step-7: format_summary ────────────────────────────────────────────────
    //
    // Pure function over a `DiagnosticSnapshot` — no globals, no lock needed.

    #[test]
    fn format_summary_all_zero_has_no_parenthetical_or_panicked_suffix() {
        let snap = DiagnosticSnapshot::default();
        assert_eq!(
            format_summary(&snap),
            "mesh updates: 0 morphed, 0 remeshed, 0 ineligible"
        );
    }

    #[test]
    fn format_summary_matches_prd_example() {
        // remeshed aggregates hard+soft (3+1=4); the ineligible parenthetical
        // lists only the non-zero sub-categories (naming omitted at 0).
        let snap = DiagnosticSnapshot {
            morphed: 47,
            remeshed_quality_hard_fail: 3,
            remeshed_quality_soft_fail: 1,
            ineligible_structural_change: 1,
            ineligible_bijection_failure: 1,
            ..Default::default()
        };
        assert_eq!(
            format_summary(&snap),
            "mesh updates: 47 morphed, 4 remeshed, 2 ineligible (1 structural, 1 bijection)"
        );
    }

    #[test]
    fn format_summary_includes_naming_when_nonzero() {
        let snap = DiagnosticSnapshot {
            morphed: 47,
            remeshed_quality_hard_fail: 3,
            remeshed_quality_soft_fail: 1,
            ineligible_structural_change: 1,
            ineligible_bijection_failure: 1,
            ineligible_naming_error: 2,
            ..Default::default()
        };
        let summary = format_summary(&snap);
        assert!(
            summary.contains("2 naming"),
            "naming sub-category must appear when non-zero, got: {summary:?}"
        );
        assert_eq!(
            summary,
            "mesh updates: 47 morphed, 4 remeshed, 4 ineligible \
             (1 structural, 1 bijection, 2 naming)"
        );
    }

    #[test]
    fn format_summary_appends_panicked_suffix_when_nonzero() {
        let snap = DiagnosticSnapshot {
            morphed: 47,
            remeshed_quality_hard_fail: 3,
            remeshed_quality_soft_fail: 1,
            ineligible_structural_change: 1,
            ineligible_bijection_failure: 1,
            panicked: 5,
            ..Default::default()
        };
        let summary = format_summary(&snap);
        assert!(
            summary.ends_with(", 5 panicked"),
            "panicked suffix must be appended when panics > 0, got: {summary:?}"
        );
        assert_eq!(
            summary,
            "mesh updates: 47 morphed, 4 remeshed, 2 ineligible \
             (1 structural, 1 bijection), 5 panicked"
        );
    }
}
