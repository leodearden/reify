//! CLI integration tests for `reify check` with a `RepresentationWithin`
//! assertion (Determinacy γ, task-4199).
//!
//! ## OCCT-gated test (step-9 RED / step-10 GREEN)
//!
//! `check_representation_within_violated_under_occt` exercises the full
//! headline signal: `reify check examples/representation_within.ri` must exit
//! non-zero (FAILURE) and print "VIOLATED" when OCCT is present, because the
//! coarse sphere (50 mm deflection) produces a sampled facet-chord deviation
//! far above the `1um` bound declared in `CurvedBallCheck`.
//!
//! Without OCCT the same command exits 0 — the assertion is `Indeterminate`
//! when tessellation cannot run (C1 graceful degradation).
//!
//! These tests are RED until step-10 adds `module_has_representation_within`
//! to `cmd_check` and routes it through the kernel-backed
//! `set_capture_repr_tol(true)` → `tessellate_realizations` → `check` path.
//!
//! ## C2 guard (always GREEN)
//!
//! `check_non_representation_within_module_is_unaffected` verifies that a
//! plain module (no `RepresentationWithin` constraints) is byte-for-byte
//! unaffected by the new routing: it must still exit 0 on the
//! `Engine::new(None)+check()` path.

mod common;

/// OCCT-gated: `reify check examples/representation_within.ri` on a coarse
/// sphere (`#precision(50mm)`) with a tight `RepresentationWithin(subject, 1um)`
/// assertion exits non-zero (FAILURE) and prints "VIOLATED" when OCCT is
/// available.
///
/// Stub-mode (no OCCT): the same command exits 0 — the assertion is
/// `Indeterminate` when realization cannot run (C1 graceful degradation →
/// empty `achieved_repr_tol` map → never a false Violated).
///
/// RED: currently `cmd_check` routes all no-purpose modules through
/// `Engine::new(None)+check()` (no kernel, no tessellation), so the map stays
/// empty and the assertion is `Indeterminate` even under OCCT.  GREEN after
/// step-10 adds the `module_has_representation_within` routing.
#[test]
fn check_representation_within_violated_under_occt() {
    let path = common::example_path("representation_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no tessellation → map stays empty → Indeterminate → exit 0.
        // Must NOT be non-zero and must NOT print "VIOLATED".
        assert!(
            status.success(),
            "stub mode: reify check representation_within.ri should exit 0 \
             (RepresentationWithin is Indeterminate without OCCT — C1 graceful \
             degradation).\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stdout.contains("VIOLATED"),
            "stub mode: stdout must not contain 'VIOLATED' \
             (Indeterminate, not Violated).\nstdout: {stdout}"
        );
        eprintln!(
            "skipping VIOLATED assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the full tessellate → check pipeline must fire.
    // CurvedBall at #precision(50mm) ≈ 0.32 m chord deviation >> 1um (1e-6 m).
    assert!(
        !status.success(),
        "OCCT mode: reify check representation_within.ri should exit non-zero \
         (coarse sphere deviation >> 1um bound → Violated → FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "OCCT mode: stdout must contain 'VIOLATED' \
         (RepresentationWithin assertion fires: sampled deviation >> 1um bound).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

/// C2 guard: a module with no `RepresentationWithin` constraints must not be
/// affected by the new routing in `cmd_check`.
///
/// Uses `crates/reify-cli/tests/fixtures/bracket.ri` — a plain numeric module
/// with satisfied constraints and no geometry.  The no-purpose path must still
/// route through `Engine::new(None)+check()` (unchanged) and exit 0.
///
/// This test is GREEN immediately and must remain GREEN after step-10.
#[test]
fn check_non_representation_within_module_is_unaffected() {
    // bracket.ri has no RepresentationWithin constraints → existing path.
    let path = common::fixture_path("bracket.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "C2: reify check bracket.ri should exit 0 — no RepresentationWithin \
         constraints → existing Engine::new(None)+check() path unchanged.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "C2: stdout should contain 'All constraints satisfied' for bracket.ri.\n\
         stdout: {stdout}"
    );
}
