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
fn check_with_multi_binding_value_exits_failure_with_specific_message() {
    // Alpha only activates the single-binding form via activate_purpose(name, entity);
    // multi-ref activation requires task γ's activate_purpose_with_bindings, so the
    // CLI must REJECT multi-binding values with a SPECIFIC error (not the generic
    // step-10 fallback) so users get an actionable signal.
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "fits_within=part:PartA,envelope:BoxB",
        &common::fixture_path("purpose_multi_param.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --purpose with multi-binding value should exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Distinctive wording that step-10's generic message does NOT contain.
    assert!(
        stderr.contains("multi-ref"),
        "stderr should contain the specific multi-ref rejection wording 'multi-ref', got: {stderr}"
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
