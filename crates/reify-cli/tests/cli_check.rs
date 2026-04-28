mod common;

#[test]
fn check_valid_bracket_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for valid bracket.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
    assert!(
        !stderr.contains("Unknown command"),
        "stderr should not contain 'Unknown command', got: {stderr}"
    );
}

#[test]
fn check_violating_bracket_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket_violating.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for violating bracket.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated"),
        "stdout should contain 'Some constraints violated', got: {stdout}"
    );
}

#[test]
fn check_parse_error_exits_failure() {
    let (status, _stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket_parse_error.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for file with parse errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Parse error"),
        "stderr should contain 'Parse error', got: {stderr}"
    );
}

#[test]
fn check_compile_error_exits_failure() {
    let (status, _stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket_compile_error.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for file with compiler errors.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
}

#[test]
fn check_indeterminate_constraint_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket_indeterminate.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 when constraints are indeterminate (not violated).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("  OK "),
        "stdout should contain '  OK ' for the satisfied constraint (thickness > 2mm), got: {stdout}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
    assert!(
        !stderr.contains("INDETERMINATE"),
        "INDETERMINATE should appear on stdout, not stderr, got stderr: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr should not contain 'error:' for a successful check, got: {stderr}"
    );
    // INDETERMINATE is non-violating by design (auto params not yet resolved),
    // so the summary still reads "No constraints violated".
    assert!(
        stdout.contains("No constraints violated"),
        "stdout should contain 'No constraints violated', got: {stdout}"
    );
    assert!(
        stdout.contains("indeterminate"),
        "stdout should contain 'indeterminate', got: {stdout}"
    );
}

#[test]
fn check_violated_with_indeterminate_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path(
        "bracket_violated_with_indeterminate.ri",
    ));

    assert!(
        !status.success(),
        "reify check should exit non-zero when constraints are violated.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
    assert!(
        stdout.contains("Some constraints violated."),
        "stdout should contain violation summary, got: {stdout}"
    );
    // Negative assertions: the fixture has zero satisfied constraints
    // (thickness=1mm violates thickness>2mm, tolerance=auto makes tolerance>0.1mm indeterminate).
    assert!(
        !stdout.contains("  OK "),
        "stdout should NOT contain '  OK ' (no satisfied constraints in fixture), got: {stdout}"
    );
    assert!(
        !stdout.contains("All constraints satisfied"),
        "stdout should NOT contain 'All constraints satisfied' when violations exist, got: {stdout}"
    );
    assert!(
        !stderr.contains("panic"),
        "stderr should not contain 'panic', got: {stderr}"
    );
}

#[test]
fn check_all_indeterminate_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand("check", &common::fixture_path("bracket_all_indeterminate.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 when all constraints are indeterminate.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {stdout}"
    );
    assert!(
        !stdout.contains("  OK "),
        "stdout should NOT contain '  OK ' (no satisfied constraints), got: {stdout}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED', got: {stdout}"
    );
    assert!(
        stdout.contains("No constraints violated"),
        "stdout should contain 'No constraints violated', got: {stdout}"
    );
    assert!(
        stdout.contains("indeterminate"),
        "stdout should contain 'indeterminate', got: {stdout}"
    );
}

#[test]
fn check_drivebelt_trait_bounds_resolves_stdlib_enums() {
    // Regression guard for task 2525: `examples/drivebelt_trait_bounds.ri` references
    // stdlib enums (`CorrosionClass.C5`, `BiocompatibilityClass.USP_Class_VI`) WITHOUT
    // inline redeclarations. The CLI's `parse_and_compile` must use prelude-aware parsing
    // so the parser disambiguates these as `EnumAccess` (not `MemberAccess`), letting
    // `compile_with_stdlib` resolve them against the stdlib `PreludeContext`.
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::example_path("drivebelt_trait_bounds.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for drivebelt_trait_bounds.ri (stdlib enum refs).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
}

#[test]
fn check_nonexistent_file_exits_failure() {
    let (status, _stdout, stderr) = common::run_subcommand("check", "nonexistent_file_that_does_not_exist.ri");

    assert!(
        !status.success(),
        "reify check should exit non-zero for missing file.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain error message about reading, got: {stderr}"
    );
}
