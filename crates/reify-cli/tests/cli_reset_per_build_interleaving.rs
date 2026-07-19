//! CLI leaf-(d) characterization pin (task ι / #5069): the combined-constraint
//! `cmd_check` arm (`reify-cli/src/main.rs`) production-depends on the
//! build↔tessellate per-build-state reset ASYMMETRY that
//! `Engine::reset_per_build_state` must preserve.
//!
//! For a module carrying BOTH a geometric/DFM constraint kind (reads
//! `realization_handles`) AND `RepresentationWithin` (reads `achieved_repr_tol`),
//! `cmd_check` runs:
//!
//!   build(Step)              → populates `realization_handles`
//!   tessellate_realizations()→ populates `achieved_repr_tol`, PRESERVES handles
//!   check()                  → reads BOTH maps
//!
//! `measure_dfm_rules` skips any rule whose `subject_handle` is `None`, so if a
//! naive FULL-UNION reset made `tessellate_realizations()` clear
//! `realization_handles`, the DFM rule would silently degrade to skipped
//! (no `W_DFM_OVERHANG`) even though the box bottom face violates. This pin
//! asserts BOTH kinds still yield REAL (non-Indeterminate / non-skipped)
//! verdicts, which the refactor must keep true.
//!
//! Reuses the existing `dfm_with_repr_within.ri` fixture (the canonical
//! both-kinds subject; DFMRule Warning + RepresentationWithin Satisfied).
//! OCCT-gated via the runtime `reify_kernel_occt::OCCT_AVAILABLE` guard,
//! mirroring `cli_gdt_conformance.rs` / `cli_representation_within.rs`; skips
//! cleanly in stub-mode builds where `cfg(has_occt)` is not set.

mod common;

/// Leaf-(d): `reify check dfm_with_repr_within.ri` — under OCCT, BOTH the DFM
/// kind (`realization_handles`-backed) and the RepresentationWithin kind
/// (`achieved_repr_tol`-backed) yield real verdicts from the single combined
/// build→tessellate→check arm. A reset that cleared handles during tessellate
/// would drop the `W_DFM_OVERHANG` diagnostic (rule skipped on `None` handle).
#[test]
fn check_both_kinds_yield_real_verdicts_under_occt() {
    let path = common::fixture_path("dfm_with_repr_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    // Exit 0 in both modes: DFM Warning is non-fatal; RepresentationWithin is
    // Satisfied (OCCT) or Indeterminate (stub) — neither fails the check.
    assert!(
        status.success(),
        "combined DFM+RepresentationWithin module should exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // RepresentationWithin is never VIOLATED here (sphere chord deviation ≪ 1mm).
    assert!(
        !stdout.contains("VIOLATED"),
        "RepresentationWithin must be Satisfied, never VIOLATED.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op (no diagnostic),
        // tessellate skipped → RepresentationWithin Indeterminate. Both kinds
        // degrade gracefully (C1); the real-verdict assertions cannot run.
        assert!(
            !stderr.contains("W_DFM_OVERHANG"),
            "stub mode: no W_DFM_OVERHANG (C1 no-op).\nstderr: {stderr}"
        );
        eprintln!(
            "skipping leaf-(d) real-verdict assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // ── OCCT: BOTH kinds must yield REAL verdicts ─────────────────────────

    // DFM kind — REAL, non-skipped: the box bottom face (normal -Z, 90° past
    // the build plane) violates max_overhang_angle=0deg → exactly one
    // W_DFM_OVERHANG. This is the direct leaf-(d) signal: it can only fire if
    // `realization_handles` (populated by build) SURVIVED tessellate_realizations
    // so measure_dfm_rules found a non-None subject_handle. A full-union reset
    // would clear the handles → rule skipped → zero W_DFM_OVERHANG here.
    let dfm_count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        dfm_count, 1,
        "OCCT: exactly one W_DFM_OVERHANG expected (DFM real verdict — proves \
         realization_handles survived tessellate). got {dfm_count}.\nstderr: {stderr}"
    );

    // RepresentationWithin kind — REAL, non-Indeterminate: tessellate populated
    // achieved_repr_tol["Sphere#realization[0]"] and the 1mm bound is met →
    // Satisfied, so the summary is "All constraints satisfied." (NOT the
    // stub-mode "No constraints violated (1 indeterminate).").
    assert!(
        stdout.contains("All constraints satisfied"),
        "OCCT: RepresentationWithin must be a REAL Satisfied verdict → summary \
         'All constraints satisfied' (not Indeterminate).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.to_lowercase().contains("indeterminate"),
        "OCCT: no constraint may be Indeterminate — both kinds measured a real \
         verdict.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
