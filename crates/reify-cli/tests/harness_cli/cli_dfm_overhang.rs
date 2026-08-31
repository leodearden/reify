//! CLI integration tests for `reify check` with a `DFMRule` module that
//! triggers an overhang violation (task 4600, process-dfm routing).
//!
//! ## OCCT-gated tests (step-3 RED / step-4 GREEN; amend)
//!
//! Two tests cover the routing and exit-code contract:
//!
//! ### Warning severity (`check_dfm_overhang_emits_one_w_dfm_overhang_under_occt`)
//! `reify check <dfm-file>` must emit exactly one `W_DFM_OVERHANG` diagnostic
//! on stderr (PRD §7.2) and exit 0 when OCCT is present.
//!
//! - Fixture `dfm_overhang.ri`: FDM Adding conformer + `DFMRule(Warning)`.
//!   No `constraint` declarations → Warning is non-fatal → exit 0.
//!
//! ### Error severity (`check_dfm_overhang_error_emits_e_dfm_overhang_under_occt`)
//! `reify check <dfm-file>` must emit exactly one `E_DFM_OVERHANG` diagnostic
//! on stderr and exit non-zero when OCCT is present.
//!
//! - Fixture `dfm_overhang_error.ri`: same geometry, `DFMSeverity.Error`.
//!   `cmd_check` escalates any code-less Error-severity diagnostic to FAILURE.
//!
//! Stub mode (no OCCT): `measure_dfm_rules` C1 no-op guard fires
//! (`default_kernel_name` is None → no kernel → no handles → no diagnostic).
//! Both tests exit 0 and emit nothing (C1 graceful degradation).
//!
//! These mirror `crates/reify-cli/tests/harness_cli/cli_gdt_conformance.rs` and
//! `cli_representation_within.rs` (the sibling routing-change harnesses).

use crate::common;

/// OCCT-gated: `reify check fixtures/dfm_overhang.ri` on a DFMRule(Warning)
/// module whose box subject has a 90° downward face (violating `max_overhang_angle=0deg`)
/// exits 0 (Warning is non-fatal) and emits exactly one `W_DFM_OVERHANG` on
/// stderr when OCCT is available.
///
/// Stub-mode (no OCCT): the same command exits 0 — `measure_dfm_rules`' C1
/// no-op guard fires (no kernel → no handles → no diagnostic, never a false
/// positive W_DFM_OVERHANG).
///
/// RED until step-4 wires `has_dfm_rule` into `cmd_check`'s kernel-backed
/// `build(ExportFormat::Step)`-before-`check` arm, causing `realization_handles`
/// to be populated and `measure_dfm_rules` to fire.
#[test]
fn check_dfm_overhang_emits_one_w_dfm_overhang_under_occt() {
    let path = common::fixture_path("dfm_overhang.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no diagnostic.
        // Must exit 0 and must NOT emit W_DFM_OVERHANG.
        assert!(
            status.success(),
            "stub mode: reify check dfm_overhang.ri should exit 0 \
             (DFMRule Warning is Indeterminate without OCCT — C1 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("W_DFM_OVERHANG"),
            "stub mode: stderr must not contain 'W_DFM_OVERHANG' \
             (C1: no kernel → measure_dfm_rules no-op → no diagnostic).\n\
             stderr: {stderr}"
        );
        eprintln!(
            "skipping W_DFM_OVERHANG assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the build-before-check → measure_dfm_rules pipeline must
    // fire.  The box bottom face (normal -Z) dips 90° past the build plane;
    // max_overhang_angle=0deg means any downward-pointing face violates.
    // Warning severity → non-fatal → exit 0.  Exactly one W_DFM_OVERHANG on
    // stderr (validated by the passing engine test
    // overhang_warning_rule_emits_w_dfm_overhang in process_dfm_measure.rs).
    assert!(
        status.success(),
        "OCCT mode: reify check dfm_overhang.ri should exit 0 \
         (DFMRule Warning severity → non-fatal → exit 0).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        count,
        1,
        "OCCT mode: stderr must carry exactly one 'W_DFM_OVERHANG' diagnostic \
         (one box bottom face violates the 0-deg overhang limit).\n\
         got {count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_OVERHANG"),
        "OCCT mode: stderr must NOT contain 'E_DFM_OVERHANG' — severity is \
         Warning, not Error.\nstderr: {stderr}"
    );
}

/// OCCT-gated: `reify check fixtures/dfm_with_repr_within.ri` on a module
/// combining a DFMRule(Warning) with a RepresentationWithin assertion.
///
/// This exercises the single kernel-backed arm in `cmd_check` with TWO
/// kernel-dependent constraint kinds present simultaneously:
///
/// - `has_dfm_rule = true`  → `engine.build(ExportFormat::Step)` must fire
///   to populate `realization_handles` for `measure_dfm_rules`.
/// - `has_representation_within = true` → `set_capture_repr_tol(true)` +
///   `tessellate_realizations()` must fire to populate `achieved_repr_tol`.
///
/// Both side effects touch DISJOINT engine maps, so the combined module gets
/// correct verdicts for both kinds in one `check()` call.
///
/// Under OCCT: exactly one `W_DFM_OVERHANG` on stderr (box bottom face dips
/// 90° with max_overhang_angle=0deg); RepresentationWithin Satisfied — both the
/// real 1mm assertion and the fixture's drift canary; no `VIOLATED` on stdout;
/// exit 0.  (Measured margin, drift canary and retuning procedure: see the
/// `dfm_with_repr_within.ri` header — the single site carrying those numbers.)
///
/// Stub mode (no OCCT): exit 0, no `W_DFM_OVERHANG`, no `VIOLATED` (C1).
#[test]
fn check_dfm_plus_repr_within_combined_arm() {
    let path = common::fixture_path("dfm_with_repr_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    // Both OCCT and stub: must exit 0.
    // DFM Warning + RepresentationWithin (Satisfied or Indeterminate) are non-fatal.
    assert!(
        status.success(),
        "combined DFM+RepresentationWithin module should exit 0 \
         (DFM Warning + Satisfied Repr → non-fatal).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // RepresentationWithin must be Satisfied (not VIOLATED) in both modes.
    assert!(
        !stdout.contains("VIOLATED"),
        "RepresentationWithin should be Satisfied (not VIOLATED) in both OCCT \
         and stub mode.\nstdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no DFM diagnostic;
        // tessellate skipped → RepresentationWithin Indeterminate.
        assert!(
            !stderr.contains("W_DFM_OVERHANG"),
            "stub mode: must not emit W_DFM_OVERHANG (C1 no-op).\nstderr: {stderr}"
        );
        eprintln!(
            "skipping DFM+RepresentationWithin combined OCCT assertions: \
             OCCT unavailable (stub-mode build)"
        );
        return;
    }

    // OCCT: build() + tessellate() sequence must both fire in the combined arm.

    // DFM: box bottom face (normal -Z, 90°) violates max_overhang_angle=0deg →
    // exactly one W_DFM_OVERHANG (Warning severity → non-fatal).
    let dfm_count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        dfm_count,
        1,
        "OCCT mode: exactly one W_DFM_OVERHANG expected from the DFMRule.\n\
         got {dfm_count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_DFM_OVERHANG"),
        "OCCT mode: no E_DFM_OVERHANG (severity is Warning, not Error).\n\
         stderr: {stderr}"
    );

    // RepresentationWithin kind — REAL, non-Indeterminate. The `!VIOLATED` check
    // above is satisfied by an *Indeterminate* verdict too, so on its own it would
    // pass straight through a hole where tessellate_realizations never populated
    // `achieved_repr_tol`. Pin the positive verdict as well (#5976).
    //
    // One assert suffices for both halves: `finish_check` prints the "All
    // constraints satisfied." summary only for ConstraintOutcome::AllSatisfied,
    // which `report_constraint_results` returns only when indeterminate_count == 0
    // — so this also rules out any per-entry `INDETERMINATE <label>` line. (The
    // sibling consumer of this fixture, cli_reset_per_build_interleaving.rs, holds
    // the same pin for its own leaf-(d) reason; each is load-bearing where it is.)
    assert!(
        stdout.contains("All constraints satisfied"),
        "OCCT: RepresentationWithin must be a REAL Satisfied verdict, not \
         Indeterminate — an Indeterminate here means tessellate_realizations did \
         not populate achieved_repr_tol.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// OCCT-gated: `reify check fixtures/dfm_overhang_error.ri` on a DFMRule(Error)
/// module whose box subject has a 90° downward face (violating `max_overhang_angle=0deg`)
/// exits non-zero (Error severity → fatal → non-zero exit) and emits exactly one
/// `E_DFM_OVERHANG` on stderr when OCCT is available.
///
/// Stub-mode (no OCCT): the same command exits 0 — `measure_dfm_rules`' C1
/// no-op guard fires (no kernel → no handles → no diagnostic, never a false
/// positive E_DFM_OVERHANG).
#[test]
fn check_dfm_overhang_error_emits_e_dfm_overhang_under_occt() {
    let path = common::fixture_path("dfm_overhang_error.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Stub mode: no kernel → measure_dfm_rules C1 no-op → no diagnostic.
        // Must exit 0 and must NOT emit E_DFM_OVERHANG.
        assert!(
            status.success(),
            "stub mode: reify check dfm_overhang_error.ri should exit 0 \
             (DFMRule Error is Indeterminate without OCCT — C1 graceful degradation).\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_DFM_OVERHANG"),
            "stub mode: stderr must not contain 'E_DFM_OVERHANG' \
             (C1: no kernel → measure_dfm_rules no-op → no diagnostic).\n\
             stderr: {stderr}"
        );
        eprintln!(
            "skipping E_DFM_OVERHANG assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // OCCT available: the build-before-check → measure_dfm_rules pipeline must
    // fire.  DFMSeverity.Error → E_DFM_OVERHANG diagnostic → cmd_check escalates
    // any code-less Error-severity diagnostic to FAILURE.
    assert!(
        !status.success(),
        "OCCT mode: reify check dfm_overhang_error.ri should exit non-zero \
         (DFMSeverity.Error → cmd_check escalates to FAILURE).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let count = stderr.matches("E_DFM_OVERHANG").count();
    assert_eq!(
        count,
        1,
        "OCCT mode: stderr must carry exactly one 'E_DFM_OVERHANG' diagnostic \
         (one box bottom face violates the 0-deg overhang limit).\n\
         got {count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("W_DFM_OVERHANG"),
        "OCCT mode: stderr must NOT contain 'W_DFM_OVERHANG' — severity is \
         Error, not Warning.\nstderr: {stderr}"
    );
}

/// OCCT-gated CONTRACT GUARD (task 6667): `reify check` output must be
/// UNCHANGED by the surface-attribution emissions.
///
/// Task 6667 makes a non-measuring surface (the GUI, `reify build`, `reify
/// eval`) say WHY a geometric `Conforms` is Indeterminate and why a declared
/// `DFMRule` produced nothing, reusing task 6169 (ζ)'s `Severity::Info` +
/// `ConstraintIndeterminate` channel.
///
/// ## What keeps this surface unchanged (CORRECTED — the original claim was false)
///
/// This doc comment originally argued that `reify check` runs
/// `realize_for_check` (`main.rs:919`) BEFORE `check()` reaches either pass, so
/// both `realization_handles.is_empty()` gates are false here. THAT ORDERING
/// CLAIM IS FALSE, and this test is what proved it: `realize_for_check`
/// delegates to `build_with_geometry_output`, which calls `self.check(module)`
/// BEFORE its realization loop runs. `realization_handles` is empty at that
/// nested check, both gates legitimately opened, and `cmd_check` folded the
/// resulting `BuildResult` diagnostics straight through — so `reify check`
/// measured the fixture's DFM rule AND told the user to run `reify check`
/// because nothing was measured. This test was committed RED on that finding
/// and escalated as esc-6667-2.
///
/// What keeps the output unchanged NOW is the engine-side build-phase marker
/// the steward authorized: `Engine::check_nested_in_build` is set immediately
/// before and cleared immediately after each of `engine_build.rs`'s three
/// `self.check(module)` calls, and both gates read it as an extra conjunct, so
/// NO check nested inside a realization can attribute — whatever its handle
/// map says. `cmd_check`'s AUTHORITATIVE `engine.check(&compiled)`
/// (`main.rs:944`) runs OUTSIDE any build with the marker clear, so on an OCCT
/// build it finds `realization_handles` populated and attributes nothing. This
/// test passes UNCHANGED under that fix; it was never weakened.
///
/// This is the direct mirror of ζ's own "`reify check` OUTPUT MUST BE
/// UNCHANGED" contract (`cli_representation_within.rs`'s
/// `check_surface_verdict_is_unchanged_for_build_surface_fixture`), and it is
/// the ONE assertion that would catch a gate keyed on the wrong engine field —
/// e.g. `achieved_repr_tol.is_empty()`, which is genuinely empty on a DFM-only
/// `reify check` and would therefore fire here.
///
/// ## This test's GD&T half is VACUOUS — do not read it as coverage
///
/// The fixture `dfm_with_repr_within.ri` contains ZERO `Conforms` (measured:
/// `grep -c` returns 0), so `!stderr.contains("does not measure geometric
/// conformance")` passes here even if the GD&T leak persists. The GD&T half is
/// covered instead by the kernel-free engine-level pins in
/// `crates/reify-eval/tests/harness_engine/unmeasured_surface_attribution.rs`
/// (`realize_for_check_emits_no_surface_attribution_for_either_kind`, which was
/// RED on exactly that half, and `the_build_phase_marker_is_cleared_on_the_way_out`).
/// Deleting either of those silently removes this contract's GD&T coverage.
///
/// The fixture's own expectations are re-asserted alongside, so a regression in
/// the MEASUREMENT path (not just the attribution) is caught here too: silently
/// losing the `W_DFM_OVERHANG` would otherwise satisfy the absence assertions
/// perfectly.
///
/// ## Stub mode: a DELIBERATE, ACCEPTED change — RE-DERIVED post-fix
///
/// A stub-mode `reify check` is still expected to gain an `info:` line naming
/// the kernel as the remedy, and that remains intended: a stub-mode run
/// genuinely cannot measure, saying so is the whole point, and it has direct
/// ζ precedent (ζ's own arm 1 changed stub-mode `reify check` output the same
/// way). But the MECHANISM stated here before the fix was wrong, and is
/// re-derived rather than carried over:
///
///  * NOT via `realize_for_check`. That path can no longer attribute at all.
///    MEASURED, not inferred: `the_build_phase_marker_is_cleared_on_the_way_out`
///    drives `realize_for_check` on an engine that realizes NOTHING and still
///    observes zero attributions, then observes two from a standalone `check()`
///    on the same engine. Stub mode's "nothing realized" is exactly that state,
///    so whatever a stub binary prints, `realize_for_check` is not its source.
///  * Via the AUTHORITATIVE `engine.check(&compiled)` (`main.rs:944`), which
///    runs outside any build with the marker clear. A stub binary reaches the
///    same kernel-backed arm this one does — the branch at `main.rs:860` keys
///    on MODULE CONTENT (`has_dfm_rule` / `has_geometric_conforms` / …), not on
///    OCCT — and `reify-kernel-manifold`'s `inventory::submit!` is
///    unconditional, so a stub binary still has `default_kernel_name ==
///    Some("manifold")` and `measure_dfm_rules` does not take its
///    `default_kernel_name.is_none()` early return (established in
///    `cli_representation_within.rs`'s `build_surface_reports_attributable_indeterminate`).
///
/// NOT MEASURED FROM THIS HOST, and deliberately not asserted either way:
/// whether that authoritative check finds `realization_handles` EMPTY in a real
/// stub binary — i.e. whether the manifold kernel realizes the fixture's
/// `subject` — which is now the sole remaining determinant of whether the
/// `info:` line appears. It cannot be measured here: `has_occt` is set purely by
/// `reify_build_utils::find(NativeDep::Occt)` auto-detection, `reify-kernel-occt`
/// exposes no feature flag or opt-out env var for it, and a bogus
/// `OCCT_INCLUDE_DIR` is taken VERBATIM (`find_dir_with_override`) rather than
/// falling back to stub — so no stub-mode build is producible on a host with
/// system OCCT installed. Stating an outcome we cannot observe is precisely the
/// confidently-wrong-claim failure this section exists to undo.
///
/// The test is OCCT-gated rather than asserting a stub-mode line because the
/// contract being pinned is about the MEASURING surface.
///
/// Audited while here (task 6667 step-7): every stderr assertion in this file
/// and in `cli_gdt_legality.rs` is `contains(...)` / `!contains(...)` /
/// `matches(<specific needle>).count()`. None asserts an exact stderr body or a
/// line count, and none uses a needle the new message contains, so all of them
/// survive an added `info:` line unmodified. No widening was needed.
#[test]
fn check_surface_emits_no_unmeasured_attribution_for_dfm_or_gdt() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping check_surface_emits_no_unmeasured_attribution_for_dfm_or_gdt: \
             OCCT unavailable (stub-mode build). Without a kernel nothing realizes, \
             so the task-6667 gates DO fire and an `info:` line is expected — a \
             deliberate change with direct ζ precedent, documented in this test's \
             doc comment rather than asserted."
        );
        return;
    }

    let path = common::fixture_path("dfm_with_repr_within.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "`reify check` on the combined fixture must still exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // ── The task-6667 tokens must be ABSENT on the measuring surface ─────────
    assert!(
        !stderr.contains("does not measure DFM rules"),
        "the DFM attribution gate must be closed on this surface: the \
         build-phase marker closes it for the `check()` nested inside \
         `realize_for_check`, and the authoritative `check()` (main.rs:944) \
         runs with `realization_handles` populated. Its firing here means \
         either the marker leaked or the gate is keyed on the wrong engine \
         field.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("does not measure geometric conformance"),
        "same for the GD&T attribution gate — the build-phase marker closes it \
         for the `check()` nested inside `realize_for_check`, and the \
         authoritative `check()` runs with handles populated. NOTE: this half \
         is VACUOUS on this fixture (zero `Conforms`); the real GD&T coverage \
         is in reify-eval's unmeasured_surface_attribution.rs.\nstderr: {stderr}"
    );

    // ── The fixture's existing expectations still hold ───────────────────────
    //
    // Without these, a regression that silently stopped measuring DFM rules
    // altogether would satisfy the absence assertions above perfectly.
    let dfm_count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        dfm_count, 1,
        "OCCT mode: exactly one W_DFM_OVERHANG must still be measured — the \
         absence assertions above are only meaningful if the measurement path \
         is still alive.\ngot {dfm_count} occurrences.\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "the RepresentationWithin assertions must still be Satisfied.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
