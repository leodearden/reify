mod common;

#[test]
fn check_valid_bracket_exits_success() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket.ri"));

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
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket_violating.ri"));

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
    let (status, _stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket_parse_error.ri"));

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
    let (status, _stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket_compile_error.ri"));

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
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket_indeterminate.ri"));

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
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("bracket_violated_with_indeterminate.ri"),
    );

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
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("bracket_all_indeterminate.ri"),
    );

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
    let (status, _stdout, stderr) =
        common::run_subcommand("check", "nonexistent_file_that_does_not_exist.ri");

    assert!(
        !status.success(),
        "reify check should exit non-zero for missing file.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Error reading"),
        "stderr should contain error message about reading, got: {stderr}"
    );
}

// ── Task γ: module-path declaration enforcement (CLI, step-7) ──────

#[test]
fn check_mod_decl_mismatch_exits_failure_with_error_diagnostic() {
    // mod_decl_mismatch.ri: `module wrong.path.here` != stem "mod_decl_mismatch"
    let (status, _stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("mod_decl_mismatch.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for path mismatch.\nstdout: {_stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_MODULE_PATH_MISMATCH"),
        "stderr should contain 'E_MODULE_PATH_MISMATCH', got: {stderr}"
    );
}

#[test]
fn check_mod_decl_match_exits_success_no_path_diagnostic() {
    // mod_decl_match.ri: `module mod_decl_match` (correct)
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("mod_decl_match.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for correct module declaration.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
    assert!(
        !stderr.contains("E_MODULE_PATH_MISMATCH"),
        "stderr should not contain 'E_MODULE_PATH_MISMATCH', got: {stderr}"
    );
    assert!(
        !stderr.contains("W_MODULE_DECL_MISSING"),
        "stderr should not contain 'W_MODULE_DECL_MISSING', got: {stderr}"
    );
}

#[test]
fn check_absent_module_decl_exits_success_with_warning() {
    // bracket.ri has no module declaration → W_MODULE_DECL_MISSING warning, exit 0
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("bracket.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 when module declaration is absent (warning only).\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("W_MODULE_DECL_MISSING"),
        "stderr should contain 'W_MODULE_DECL_MISSING', got: {stderr}"
    );
}

// --- io-export α: std.io.formats occurrence surface (task 4284) ---

#[test]
fn check_io_formats_exits_success_no_unresolved() {
    // Guard for task 4284: examples/io_formats.ri exercises the new STEPOutput,
    // STLOutput, ThreeMFOutput, DisplayOutput, STEPInput occurrences plus
    // STEPVersion and DisplayStyle.  Must exit 0 with no unresolved-type or
    // unresolved-name:undef errors.
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::example_path("io_formats.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for io_formats.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // The five determined(subject) constraints on concrete box() geometry
    // (STLOutput, STEPOutput×3, ThreeMFOutput) should resolve to "All constraints satisfied".  We also accept the
    // "No constraints violated (N indeterminate)" message that reify check
    // prints when constraints resolve to SomeIndeterminate — exit code is still
    // 0 in that case and our primary contract is "exit 0, no unresolved errors".
    // This matches the pattern used in cli_integration_smoke.rs.
    assert!(
        stdout.contains("All constraints satisfied") || stdout.contains("No constraints violated"),
        "stdout should contain a success constraint message, got: {stdout}"
    );
    assert!(
        !stderr.contains("unresolved type"),
        "stderr must not contain 'unresolved type', got: {stderr}"
    );
    assert!(
        !stderr.contains("unresolved name: undef"),
        "stderr must not contain 'unresolved name: undef', got: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "stdout must not contain 'VIOLATED', got: {stdout}"
    );
}

// --- E_OBJECTIVE_CONFLICT CLI tests (task 4010, boundary B3) ---

/// B3 positive: a structure with conflicting objectives (`minimize mass` +
/// `maximize stiffness`) must exit non-zero and print `"E_OBJECTIVE_CONFLICT"`
/// to stderr.  This is the user-observable leaf signal for task 4010.
#[test]
fn check_objective_conflict_exits_failure_with_mnemonic() {
    let (status, _stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("objective_conflict.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for conflicting objectives.\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("E_OBJECTIVE_CONFLICT"),
        "stderr should contain 'E_OBJECTIVE_CONFLICT', got: {stderr}"
    );
}

/// B3 negative: a structure with same-sense objectives (`minimize mass` +
/// `minimize cost`) is NOT a conflict and must exit zero without the mnemonic.
#[test]
fn check_objective_no_conflict_exits_success_without_mnemonic() {
    let (status, _stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("objective_no_conflict.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for non-conflicting same-sense objectives.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("E_OBJECTIVE_CONFLICT"),
        "stderr should not contain 'E_OBJECTIVE_CONFLICT', got: {stderr}"
    );
}

// ── task 4488 θ: --strict flag (step-7 RED integration tests) ────────────────

/// (1) `check --strict bracket_indeterminate.ri` → failure + names the
/// indeterminate constraint on stderr; must NOT contain the legacy summary line.
#[test]
fn check_strict_indeterminate_exits_failure_naming_constraint() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--strict",
        &common::fixture_path("bracket_indeterminate.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --strict should exit non-zero when constraints are \
         indeterminate.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Strict check failed"),
        "stderr should contain 'Strict check failed' (strict detail goes to stderr), got stderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        stderr.contains("Bracket#constraint[1]"),
        "stderr should name 'Bracket#constraint[1]', got stderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        !stdout.contains("No constraints violated"),
        "stdout must NOT contain 'No constraints violated' in strict mode, got: {stdout}"
    );
}

/// (2) `check --strict bracket_all_indeterminate.ri` → failure + names BOTH
/// indeterminate constraints on stderr.
#[test]
fn check_strict_all_indeterminate_lists_all() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--strict",
        &common::fixture_path("bracket_all_indeterminate.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --strict should exit non-zero when all constraints are \
         indeterminate.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Bracket#constraint[0]"),
        "stderr should name 'Bracket#constraint[0]' (strict detail on stderr), got stderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        stderr.contains("Bracket#constraint[1]"),
        "stderr should name 'Bracket#constraint[1]' (strict detail on stderr), got stderr: {stderr}\nstdout: {stdout}"
    );
}

/// (3) `check --strict bracket.ri` (all satisfied) → success; strict must not
/// break the happy path.
#[test]
fn check_strict_all_satisfied_still_exits_success() {
    let (status, stdout, stderr) =
        common::run_with_args(&["check", "--strict", &common::fixture_path("bracket.ri")]);

    assert!(
        status.success(),
        "reify check --strict should exit 0 when all constraints are satisfied.\
         \nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.', got: {stdout}"
    );
}

/// (4) `check bracket_indeterminate.ri` (no flag) → success + byte-identical
/// legacy line; explicit opt-in guard.
#[test]
fn check_indeterminate_without_strict_unchanged() {
    let (status, stdout, stderr) =
        common::run_with_args(&["check", &common::fixture_path("bracket_indeterminate.ri")]);

    assert!(
        status.success(),
        "reify check (no --strict) should exit 0 for indeterminate constraints.\
         \nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("No constraints violated (1 indeterminate)."),
        "stdout should contain the exact legacy summary 'No constraints violated \
         (1 indeterminate).', got: {stdout}"
    );
    assert!(
        !stdout.contains("Strict check failed"),
        "stdout must NOT contain 'Strict check failed' without --strict, got: {stdout}"
    );
}

/// (5) `check --strict --purpose mfg_ready=Bracket bracket_purpose_indeterminate.ri`
/// → failure + strict detail on stderr naming the purpose-injected indeterminate
/// constraint. Guards the wiring of `strict` into the `--purpose` branch against
/// future regressions (both paths share `finish_check` but the wiring is distinct).
#[test]
fn check_strict_purpose_indeterminate_exits_failure() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--strict",
        "--purpose",
        "mfg_ready=Bracket",
        &common::fixture_path("bracket_purpose_indeterminate.ri"),
    ]);

    assert!(
        !status.success(),
        "reify check --strict --purpose should exit non-zero when the purpose-injected \
         constraint is indeterminate.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Strict check failed"),
        "stderr should contain 'Strict check failed' for strict purpose-injected \
         indeterminate, got stderr: {stderr}\nstdout: {stdout}"
    );
    assert!(
        !stdout.contains("No constraints violated"),
        "stdout must NOT contain 'No constraints violated' in strict mode, got: {stdout}"
    );
}

// ── end task 4488 θ step-7 ───────────────────────────────────────────────────
