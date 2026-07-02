//! Cross-crate re-export compatibility pins for task A (#4934): reify-eval
//! extracts the pure `ComputeFn` contract value types into the OCCT-free
//! `reify-compute-contract` foundation crate and re-exports every moved type
//! at its original public paths (INV-2 / BT-3).
//!
//! Each cluster below adds an IDENTITY assertion — a fn typed on
//! `reify_compute_contract::X` invoked with a value built via the
//! `reify_eval::` re-export path — which only compiles if the re-export is
//! the SAME type, not an accidental duplicate definition. A mere
//! `use`/existence check would miss that failure mode.
//!
//! This file grows with each extraction step; see `.task/plan.json` steps
//! 1/3/5. The pre-existing `tests/compute_dispatch_registry.rs::_seam_pin_api_surface`
//! is the INV-2 regression guard for the compute-dispatch cluster and must
//! stay green, unchanged, throughout this extraction.

// ── step-1: CancellationHandle ───────────────────────────────────────────

/// Identity seam: a fn parameter typed on
/// `reify_compute_contract::CancellationHandle` accepts a value constructed
/// via `reify_eval::CancellationHandle::new()`. This only compiles if the
/// `reify_eval` re-export is the SAME type as the compute-contract
/// definition, not a duplicate.
fn _cc_identity(_: reify_compute_contract::CancellationHandle) {}

#[test]
fn cancellation_handle_reexport_is_identity_not_duplicate() {
    _cc_identity(reify_eval::CancellationHandle::new());
}

#[test]
fn cancellation_handle_new_is_not_cancelled() {
    let h = reify_eval::CancellationHandle::new();
    assert!(!h.is_cancelled(), "a fresh handle must not be cancelled");
}

#[test]
fn cancellation_handle_cancel_transitions_false_to_true() {
    let h = reify_eval::CancellationHandle::new();
    assert!(!h.is_cancelled(), "must start false");
    h.cancel();
    assert!(
        h.is_cancelled(),
        "is_cancelled() must be true after cancel()"
    );
}

#[test]
fn cancellation_handle_clone_shares_flag() {
    let h = reify_eval::CancellationHandle::new();
    let clone = h.clone();
    clone.cancel();
    assert!(
        h.is_cancelled(),
        "cancelling a clone must be observed by the original handle \
         (they share the same underlying Arc<AtomicBool>)"
    );
}
