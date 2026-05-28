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
