//! `reify check`'s exit for the relate family, on both phases, under #5403's
//! general `Severity::Error` gate (docs/legibility/design-invariants.md INV-SF-2;
//! PRD docs/prds/v0_6/placement-relations-belt.md task ε / boundary B3).
//!
//! These pins add no relate-specific exit logic. The allowlist guard lives in
//! crates/reify-cli/src/main.rs `check_error_gate_tests`, because the allowlist
//! is private to the binary.

use crate::common;

/// The line `finish_check` prints when no constraint failed.
const CLEAN_SUMMARY: &str = "All constraints satisfied.";

/// Which `cmd_check` phase is expected to reject the fixture.
enum Phase {
    /// The compile-diagnostics early return, before any evaluation.
    Compile,
    /// The relate solve, after evaluation ran to `finish_check`.
    Engine,
}

/// Run `reify check` on `fixture` and assert it exits exactly 1, reports
/// `needle` on stderr, and was rejected in `phase`.
fn assert_check_rejects(fixture: &str, needle: &str, phase: Phase) {
    if matches!(phase, Phase::Engine) && !reify_kernel_occt::OCCT_AVAILABLE {
        // Without a registered kernel `build_with_geometry_output` skips the
        // relate solve, so no engine-phase relate diagnostic exists to gate on.
        eprintln!(
            "skipping engine-phase relate exit assertions for {fixture}: OCCT \
             unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path(fixture));

    assert_eq!(
        status.code(),
        Some(1),
        "a relate-family Severity::Error must make `reify check` exit exactly 1 \
         for {fixture}.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(needle),
        "the relate-family diagnostic `{needle}` must be reported for {fixture}, \
         or this test could pass for an unrelated reason.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    match phase {
        Phase::Compile => assert!(
            !stdout.contains(CLEAN_SUMMARY),
            "{fixture} must be rejected by the compile-diagnostics early return, \
             before evaluation prints a constraint summary.\n\
             stdout: {stdout}\nstderr: {stderr}"
        ),
        Phase::Engine => assert!(
            stdout.contains(CLEAN_SUMMARY),
            "{fixture} must evaluate to `finish_check` with no failed constraint, \
             so that only the INV-SF-2 Severity::Error gate can produce exit 1.\n\
             stdout: {stdout}\nstderr: {stderr}"
        ),
    }
}

#[test]
fn check_exits_nonzero_on_compile_phase_relate_operand_projection_error() {
    assert_check_rejects(
        "relate_operand_projection_compile.ri",
        "concentric: operand Real has no Axis projection",
        Phase::Compile,
    );
}

#[test]
fn check_exits_nonzero_on_compile_phase_relate_metric_unit_error() {
    assert_check_rejects(
        "relate_metric_unit_compile.ri",
        "angle: metric argument expects Angle",
        Phase::Compile,
    );
}

/// The Real operand reaches `concentric` through a generic `fn … -> Relation`
/// wrapper, so compile-time gradualism (`check_relation_arg_types` skips
/// `Type::TypeParam` operands) accepts it and the relate solve rejects the
/// member. The needle is the message, not the code, so a dedicated code (#7494)
/// does not break the pin.
#[test]
fn check_exits_nonzero_on_engine_phase_relate_operand_type_error_hidden_by_gradualism() {
    assert_check_rejects(
        "relate_wrapped_operand_type_engine.ri",
        "is not a direct call to a geometric relation",
        Phase::Engine,
    );
}

#[test]
fn check_exits_nonzero_on_engine_phase_static_relate_violation() {
    assert_check_rejects(
        "relate_static_violated_engine.ri",
        "not satisfied by the subs' fixed placements",
        Phase::Engine,
    );
}

/// This Error is CODE-LESS — the only relate shape a `MessageContains`
/// allowlist entry could swallow — so this end-to-end pin is the allowlist
/// guard for code-less relate Errors.
#[test]
fn check_exits_nonzero_on_engine_phase_relate_conflict() {
    assert_check_rejects(
        "relate_conflict_engine.ri",
        "conflicting relations on `pin`",
        Phase::Engine,
    );
}
