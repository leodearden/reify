//! End-to-end integration tests for the `fn modal_analysis` @optimized →
//! ComputeNode → trampoline pipeline (task ζ, docs/prds/v0_3/modal-analysis.md
//! §10).
//!
//! Steps:
//!   step-13/14 — trampoline registration + seam pin (always-run)
//!   step-15/16 — cantilever first-mode-frequency e2e (release-gated)
//!   step-17/18 — simply-supported first-mode + higher-mode band (release-gated)

use reify_eval::ComputeFn;
use reify_test_support::make_simple_engine;

// ── step-13: RED — trampoline registration + seam pin ────────────────────────
//
// Compile-time seam pin: coerce
//   `reify_eval::modal_ops::solve_modal_analysis_trampoline`
// to `ComputeFn`, pinning the cross-crate trampoline signature. Compile success
// is the signal (no runtime assertion). Paired with a runtime check that
// `register_compute_fns` installs the trampoline under "modal::free_vibration".
//
// RED until step-14 adds `solve_modal_analysis_trampoline` + its registration:
// the seam pin references a symbol that does not yet exist (compile-fail RED),
// mirroring buckling_smoke.rs's step-1 seam pin.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn = reify_eval::modal_ops::solve_modal_analysis_trampoline;
}

/// Step-13: `register_compute_fns` installs the modal trampoline under the key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, and asserts
/// `engine.compute_dispatch("modal::free_vibration").is_some()`.
///
/// Expected to fail (compile error) until step-14 creates the trampoline and
/// registers it.
#[test]
fn register_compute_fns_installs_modal_free_vibration() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("modal::free_vibration").is_some(),
        "register_compute_fns must install a trampoline under 'modal::free_vibration'"
    );
}
