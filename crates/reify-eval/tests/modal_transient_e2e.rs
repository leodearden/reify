//! End-to-end integration tests for the `fn transient_response` /
//! `fn displacement_at` @optimized → ComputeNode → trampoline pipeline
//! (task ι, docs/prds/v0_3/modal-analysis.md §1 / §5.2 / §9.1).
//!
//! Steps:
//!   step-9/10  — trampoline registration + seam pin (always-run)
//!   step-17/18 — cantilever step-response decay-envelope e2e (release-gated)

use reify_eval::ComputeFn;
use reify_test_support::make_simple_engine;

// ── step-9: RED — trampoline registration + seam pin ──────────────────────────
//
// Compile-time seam pin: coerce both
//   `reify_eval::modal_ops::solve_transient_response_trampoline`
//   `reify_eval::modal_ops::displacement_at_trampoline`
// to `ComputeFn`, pinning the cross-crate trampoline signatures. Compile success
// is the signal (no runtime assertion). Paired with a runtime check that
// `register_compute_fns` installs both trampolines under their target keys.
//
// Mirrors modal_analysis_e2e.rs:82-103 (the modal::free_vibration seam pin).
//
// RED until step-10 adds the two trampolines + their registration: the seam pin
// references symbols that do not yet exist (compile-fail RED).

#[allow(dead_code)]
fn _seam_pin() {
    let _t: ComputeFn = reify_eval::modal_ops::solve_transient_response_trampoline;
    let _d: ComputeFn = reify_eval::modal_ops::displacement_at_trampoline;
}

/// Step-9: `register_compute_fns` installs both transient trampolines.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, and asserts
/// `engine.compute_dispatch("modal::transient_response").is_some()` AND
/// `engine.compute_dispatch("modal::displacement_at").is_some()`.
///
/// Expected to fail (compile error) until step-10 creates the trampolines and
/// registers them.
#[test]
fn register_compute_fns_installs_transient_trampolines() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine
            .compute_dispatch("modal::transient_response")
            .is_some(),
        "register_compute_fns must install a trampoline under 'modal::transient_response'"
    );
    assert!(
        engine.compute_dispatch("modal::displacement_at").is_some(),
        "register_compute_fns must install a trampoline under 'modal::displacement_at'"
    );
}
