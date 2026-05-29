mod common;

#[test]
fn check_with_violated_purpose_exits_failure_with_summary() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=Bracket",
        &common::fixture_path("purpose_single_violated.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --purpose for a violated purpose should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain summary 'Some constraints violated.', got: {stdout}"
    );
}

#[test]
fn check_with_repeated_purpose_flag_activates_each_purpose() {
    // Repeatable per PRD §11 Open Q#4: each --purpose occurrence is one
    // name=binding-list activation. Both purposes' injected constraint ids
    // must appear in the report.
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=Bracket",
        "--purpose",
        "lightweight=Bracket",
        &common::fixture_path("purpose_two.ri"),
    ]);

    assert!(
        status.success(),
        "reify check with two satisfiable --purpose flags should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("purpose:mfg_ready@Bracket"),
        "stdout should contain first purpose's id 'purpose:mfg_ready@Bracket', got: {stdout}"
    );
    assert!(
        stdout.contains("purpose:lightweight@Bracket"),
        "stdout should contain second purpose's id 'purpose:lightweight@Bracket', got: {stdout}"
    );
}

/// B7 / RED (step-09a): activating a multi-param purpose with distinct per-param
/// bindings must SUCCEED and report the purpose-injected constraint as OK.
///
/// `PartA.length = 80mm`, `BoxB.length = 100mm`, constraint `part.length < envelope.length`.
/// With distinct binding (part→PartA, envelope→BoxB) → 80mm < 100mm → Satisfied.
///
/// RED because the CLI currently rejects any multi-binding (bindings.len()!=1).
/// Step-10 removes the rejection and routes multi-binding through
/// activate_purpose_with_bindings.
#[test]
fn check_with_multi_binding_value_activates_multi_param_purpose() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "fits_within=part:PartA,envelope:BoxB",
        &common::fixture_path("purpose_multi_param.ri"),
    ]);

    assert!(
        status.success(),
        "reify check --purpose with distinct multi-binding should exit 0 (constraint satisfied).\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Purpose-injected constraint must appear in the report with its entity prefix.
    assert!(
        stdout.contains("purpose:fits_within@"),
        "stdout should contain purpose-injected constraint id prefix 'purpose:fits_within@', got: {stdout}"
    );
    assert!(
        stdout.contains("OK") || stdout.contains("All constraints satisfied."),
        "stdout should indicate success, got: {stdout}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.', got: {stdout}"
    );
}

/// C2 / RED (step-09b): when a named-param single-binding is supplied for a
/// 2-param purpose and the other param is unbound, the CLI must exit non-zero
/// with a diagnostic naming the unbound param.
///
/// `--purpose fits_within=part:PartA` supplies only "part"; "envelope" is missing.
/// RED because after step-10 this routes through activate_purpose_with_bindings
/// (single named-param binding → multi-binding path), which returns C2 Err.
/// Currently the CLI falls through to activate_purpose (refuses quietly) and
/// gives a generic "could not activate" error that does not name "envelope".
#[test]
fn check_with_multi_binding_unbound_param_exits_failure() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "fits_within=part:PartA",
        &common::fixture_path("purpose_multi_param.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check with an unbound purpose param should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("envelope"),
        "stderr must name the unbound param 'envelope' (C2 diagnostic), got: {stderr}"
    );
}

/// C3 / RED (step-09c): when a binding names a param not declared by the
/// purpose, the CLI must exit non-zero with a diagnostic naming the unknown param.
///
/// `--purpose fits_within=part:PartA,bogus:BoxB` — "bogus" is not a param of
/// fits_within. RED because the CLI currently rejects any multi-binding with
/// "multi-ref not yet supported"; after step-10 it calls
/// activate_purpose_with_bindings which returns C3 Err naming "bogus".
#[test]
fn check_with_multi_binding_unknown_param_exits_failure() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "fits_within=part:PartA,bogus:BoxB",
        &common::fixture_path("purpose_multi_param.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check with an unknown binding param should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("bogus"),
        "stderr must name the unknown param 'bogus' (C3 diagnostic), got: {stderr}"
    );
}

#[test]
fn check_with_unknown_purpose_exits_failure_with_clear_error() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "does_not_exist=Bracket",
        &common::fixture_path("purpose_single_satisfiable.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --purpose with an unknown purpose name should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Error message must clearly attribute the failure to the unknown purpose:
    // either explicitly mention activation, or name the purpose itself.
    assert!(
        stderr.contains("could not activate purpose") || stderr.contains("does_not_exist"),
        "stderr should clearly attribute the failure to the unknown purpose, got: {stderr}"
    );
}

#[test]
fn check_with_satisfiable_purpose_succeeds_and_reports_purpose_constraint() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=Bracket",
        &common::fixture_path("purpose_single_satisfiable.ri"),
    ]);

    assert!(
        status.success(),
        "reify check --purpose for a satisfiable purpose should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("  OK "),
        "stdout should contain '  OK ' for the satisfied purpose constraint, got: {stdout}"
    );
    // Label-less injected constraints fall back to ConstraintNodeId Display
    // ("purpose:<name>@<entity>#constraint[i]"), so the prefix is a precise
    // signal that the PURPOSE's constraint reached the report.
    assert!(
        stdout.contains("purpose:mfg_ready@Bracket"),
        "stdout should contain the purpose-injected constraint id prefix 'purpose:mfg_ready@Bracket', got: {stdout}"
    );
}

/// Reviewer regression / RED (step-12): `reify check --purpose always_ok=Bracket`
/// against a ZERO-param purpose must NOT panic the CLI.
///
/// `always_ok=Bracket` parses to one bare binding (param None), so cmd_check
/// takes the bare-single branch → `engine.activate_purpose("always_ok", "Bracket")`,
/// which today panics inside the engine (index-out-of-bounds at `purpose.params[0]`,
/// engine_purposes.rs:138) and aborts the CLI. The RED→GREEN discriminator is
/// `!stderr.contains("panicked")`.
///
/// After GREEN (step-13) the shim refuses cleanly (purpose not active), so cmd_check
/// prints the existing "could not activate purpose 'always_ok'" error and returns
/// ExitCode::FAILURE via the is_purpose_active → FAILURE path. No CLI change is needed.
#[test]
fn check_with_single_binding_zero_param_purpose_does_not_panic() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "always_ok=Bracket",
        &common::fixture_path("purpose_zero_param.ri"),
    ]);

    // RED→GREEN discriminator: the engine must not panic / abort.
    assert!(
        !stderr.contains("panicked"),
        "reify check on a zero-param purpose must not panic.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // After GREEN: clean activation-failure exit via the is_purpose_active → FAILURE path.
    assert!(
        !status.success(),
        "reify check on a refused zero-param purpose should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("could not activate purpose"),
        "stderr should contain 'could not activate purpose' for the refused zero-param purpose, got: {stderr}"
    );
}
