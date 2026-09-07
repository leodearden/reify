use crate::common;

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

// --- B10 LEAF: W_UNDERDETERMINED on `reify check` (task κ #4019, PRD §3.6/§10.2) ---

/// `reify check` on a fixture with one unconstrained `auto` param emits
/// `W_UNDERDETERMINED` to stderr and exits 0 (warning-only).
///
/// Validates the full path: Engine::eval → detect_underdetermined →
/// Engine::check → report_eval_output → stderr.
///
/// Precedent for warning-only → exit 0: `check_absent_module_decl_exits_success_with_warning`.
#[test]
fn check_underdetermined_free_param_exits_success_with_warning() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("underdetermined_free_param.ri"),
    );

    assert!(
        status.success(),
        "reify check should exit 0 for a warning-only W_UNDERDETERMINED fixture.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("W_UNDERDETERMINED"),
        "stderr should contain 'W_UNDERDETERMINED'; got: {stderr}"
    );
    assert!(
        stderr.contains("FreeBar.gap"),
        "stderr should name the free param as 'FreeBar.gap' (the cell id); got: {stderr}"
    );
}

// --- appearance-substrate α: Color/Finish/Appearance/Visual stdlib module (task #4760) ---

#[test]
fn check_appearance_surface_exits_success_no_unresolved() {
    // CLI-path guard for task #4760 α — distinct purpose from the
    // examples_smoke compile harness (which exercises compile_with_stdlib
    // directly, not the `reify check` CLI binary or its summary output).
    //
    // What this test asserts: `reify check` exits 0 and the CLI emits its
    // "All constraints satisfied" success-summary for appearance_surface.ri.
    //
    // Scope note: "All constraints satisfied" is emitted when zero violated AND
    // zero indeterminate constraints exist — which also holds when no constraints
    // are evaluated at all.  Whether `reify check` evaluates the Appearance
    // metalness/roughness range constraints on nested struct defaults is not
    // confirmed here; a should-fail fixture with metalness > 1 would be needed
    // to verify constraint-evaluation liveness directly (out of scope for α).
    //
    // The negative-stderr assertions below are diagnostic aids: they narrow
    // the failure site when something regresses, even though status.success()
    // would already catch those error classes.
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::example_path("appearance_surface.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for appearance_surface.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "reify check should emit 'All constraints satisfied' (zero violated + zero indeterminate; \
         see scope note above — constraint-evaluation liveness is not confirmed by this assertion), \
         got: {stdout}"
    );
    // Diagnostic aids — also caught by status.success() above, but narrow the failure site:
    assert!(
        !stderr.contains("unresolved type"),
        "stderr must not contain 'unresolved type', got: {stderr}"
    );
    assert!(
        !stderr.contains("unresolved name"),
        "stderr must not contain 'unresolved name', got: {stderr}"
    );
}

#[test]
fn check_appearance_violated_exits_failure() {
    // Negative liveness test for task #4760 α: an Appearance with metalness = 1.5
    // (outside the 0..1 range) must cause `reify check` to exit non-zero and report
    // a VIOLATED constraint.  Pairs with check_appearance_surface_exits_success_no_unresolved
    // to cover both branches of the metalness range constraint.
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("appearance_violated.ri"));

    assert!(
        !status.success(),
        "reify check should exit non-zero for appearance_violated.ri (metalness=1.5 violates 0..1).\
         \nstdout: {stdout}\nstderr: {stderr}"
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
fn check_geometry_module_resolves_geometry_query_constraints() {
    // Task 5748 / PRD docs/prds/v0_6/check-diagnostic-truthfulness.md leaf β, D1.
    //
    // `reify check` used to route a geometry-bearing module through the
    // lightweight `Engine::new(None) + check()` path whenever it carried none of
    // {geometric Conforms, RepresentationWithin, DFMRule}.  Geometry-query value
    // cells (`centroid`, `moment_of_inertia`, …) are populated only by
    // `run_post_processes`/`post_process_geometry_queries` on the
    // `with_registered_kernel + build()` path, so they stayed `undef` and every
    // constraint reading one degraded to INDETERMINATE.
    //
    // Measured pre-change baseline for this fixture:
    //     stdout: "  OK BoltFlange#constraint[0]"
    //             "  INDETERMINATE BoltFlange#constraint[1]"
    //             "  OK BoltFlange#constraint[2]" / "[3]"
    //             "No constraints violated (1 indeterminate)."
    //     stderr: "error: `centroid` could not be resolved: …"
    //             "error: `moment_of_inertia` could not be resolved: …"
    //             "warning: constraint BoltFlange#constraint[1] indeterminate: \
    //                       undefined inputs: BoltFlange.moi_principal"
    //     exit 0
    //
    // `reify eval` on the same fixture (already geometry-routed via
    // `module_has_geometry`, main.rs cmd_eval) resolves both cells — so D1's
    // routing change is exactly what flips constraint[1] INDETERMINATE → OK.
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::example_path("m5_geometry_flange.ri"));

    assert!(
        status.success(),
        "reify check should exit 0 for m5_geometry_flange.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Kernel-DEPENDENT below this line, unlike the exit-code assertion
        // above: resolving `centroid`/`moment_of_inertia` needs the realization
        // loop to actually produce a solid, which a stub build cannot do.  The
        // cells stay `undef`, constraint[1] degrades to INDETERMINATE, and the
        // command still exits 0 (indeterminate is not a failure — see
        // `check_indeterminate_constraint_exits_success`), so only the content
        // assertions have to be skipped.  Same C1 convention as the three
        // sibling task-5748 tests in this file.
        eprintln!(
            "skipping geometry-query resolution assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    assert!(
        stdout.contains("OK BoltFlange#constraint[1]"),
        "constraint[1] reads a geometry-query cell (moment_of_inertia); with geometry routing it \
         must resolve to OK.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("INDETERMINATE BoltFlange#constraint[1]"),
        "constraint[1] must no longer be INDETERMINATE.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("`centroid` could not be resolved"),
        "the centroid geometry query must resolve on the build() path.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("`moment_of_inertia` could not be resolved"),
        "the moment_of_inertia geometry query must resolve on the build() path.\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("indeterminate: undefined inputs: BoltFlange.moi_principal"),
        "moi_principal derives from a now-resolvable geometry query, so its constraint must no \
         longer report undefined inputs.\nstderr: {stderr}"
    );
}

/// Task 5748 / PRD `check-diagnostic-truthfulness.md` leaf β, D2.
///
/// `cmd_check`'s kernel-backed arm calls `build()` for its handle-population
/// side effect and used to throw the `BuildResult` away (`let _ = …`).  That
/// silently swallowed every realization-only diagnostic — `compile_geometry_op`
/// gating errors and kernel-dispatch failures that `check()` alone never
/// produces — so a module whose geometry cannot compile AT ALL still reported
/// "All constraints satisfied." under `check`.
///
/// Measured pre-change baseline for `fixtures/mirror_bare_origin.ri`:
///     stdout: "  OK MirrorBareOrigin#constraint[0]" / "All constraints satisfied."
///     stderr: EMPTY
///     exit 0
///
/// WHY THE FIXTURE MOVED (task 5662).  It used to carry the 7-arg scalar form
/// `mirror(arm, 0, 0, 0, 1, 0, 0)`, which now short-circuits `cmd_check` before
/// `build()` and would have destroyed every assertion below, including the dedup
/// pin this test exists for.  It moved to the decoded-value form
/// `mirror(arm, plane_yz(0))` and is still deliberately BARE — only the route
/// changed, not the mistake.  The argument for why that route is the durable one
/// is stated once, on the `mirror` / `circular_pattern` arms of
/// `builtin_arg_slots` in `crates/reify-compiler/src/builtin_signatures.rs`.
/// The exit-gate half is pinned by
/// `check_rejects_bare_scalar_mirror_origin_before_reaching_build` below.
///
/// Baseline RE-MEASURED at task 5662 on the retargeted fixture.  `reify check`
/// is unchanged in shape (exit 0, both stdout lines above).  `reify eval` on the
/// same file reports, on stderr:
///     error: mirror: ox argument expects Length, got Int; …     [TWICE]
///     error: mirror: oy/oz argument expects Length, got Real; … [TWICE]
///     error: failed to compile geometry operation: mirror: missing or
///            non-Length argument 'ox' for mirror                [TWICE]
///     error: failed to compile geometry operation: unresolvable GeomRef::Step(1) …
///     exit 1
/// The internal duplication — the whole reason for the dedup pin — is unchanged
/// by the retarget; only the message gained the `mirror: ` builtin prefix that
/// the decoded-value route carries, and oy/oz read `Real` rather than `Int`
/// because only the offset argument is the bare literal.
///
/// The `matches(...).count() == 1` assertion is the load-bearing one: it pins
/// D2's ACCUMULATING dedup. `build()` emits that error twice for a single call
/// site, so a merge that deduped only against `check()`'s original list (which
/// is empty here) would print it twice on `check`'s stderr.
#[test]
fn check_surfaces_geometry_compile_error_from_discarded_build() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("mirror_bare_origin.ri"));

    // Mode-independent: this leaf fixes diagnostic COLLECTION, not the exit
    // gate — that is the PRD's β/γ split.  Task 5403 (γ) lands the general
    // `Severity::Error` exit gate and is the leaf that flips this assertion to
    // `!status.success()`; γ's implementer finds it by grepping #5403 in the
    // test tree.
    assert!(
        status.success(),
        "leaf β fixes diagnostic collection only — the exit gate stays as-is until \
         #5403 (γ) lands the Severity::Error gate.\nstdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // The `compile_geometry_op` argument validation that produces these
        // diagnostics is itself kernel-independent
        // (`geometry_ops::required_length_arg`'s `LengthArg::Invalid` arm —
        // no `OCCT_AVAILABLE` guard anywhere on it), but whether the
        // realization loop is reached at all under a stub build is NOT measured
        // here, so follow the C1 convention used by
        // `cli_dfm_overhang.rs::check_dfm_plus_repr_within_combined_arm` and
        // skip the content assertions rather than guess.
        eprintln!(
            "skipping build-diagnostic merge assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let needle = "failed to compile geometry operation: mirror: missing or non-Length argument 'ox' for mirror";
    assert!(
        stderr.contains(needle),
        "the geometry-compile error `build()` produces must reach `check`'s stderr \
         (D2: every build()-only diagnostic appears at least once).\nstderr: {stderr}"
    );
    let occurrences = stderr.matches(needle).count();
    assert_eq!(
        occurrences,
        1,
        "D2's dedup accumulates: `build()` emits this error TWICE for one call site, \
         so it must collapse to exactly one line on `check`'s stderr — got {occurrences}.\n\
         stderr: {stderr}"
    );
    assert!(
        stderr.contains("mirror: ox argument expects Length, got Int"),
        "the companion argument-type warning `build()` produces must reach `check`'s \
         stderr too.\nstderr: {stderr}"
    );
}

/// Task 5662 — the CLI-seam LOCK on this task's headline user-visible change:
/// `reify check` on a bare 7-arg scalar `mirror` origin now EXITS 1, where it
/// exited 0 before.
///
/// This is emergent behaviour, owned by no single layer: task 5662 added the
/// ox/oy/oz LENGTH slots in `crates/reify-compiler/src/builtin_signatures.rs`,
/// and `cmd_check` turns any compile `Severity::Error` into `ExitCode::FAILURE`
/// with a short-circuit BEFORE constraint checking and before `build()`.  The
/// compiler-side tests pin the DIAGNOSTIC; nothing pinned the EXIT GATE, yet the
/// short-circuit is precisely why the sibling test's fixture had to move to the
/// decoded-value route (see the `mirror` arm of `builtin_signatures.rs`).  A
/// regression in that short-circuit would silently invalidate that retarget
/// while every compiler-side test stayed green — hence this test.
///
/// A temp module rather than a `tests/fixtures/` file on purpose: this source is
/// deliberately UNCOMPILABLE, and every fixture in that directory is fair game
/// for the corpus walkers under `crates/**/*.ri`.
///
/// Baseline MEASURED at task 5662 against a binary built from this branch, on
/// the source below:
///     stdout: EMPTY — no verdict line at all
///     stderr: error: mirror: ox argument expects Length, got Int; pass a
///                    dimensioned length such as `5mm`
///             error: mirror: oy … (same)
///             error: mirror: oz … (same)
///     exit 1
/// Note what is ABSENT: `failed to compile geometry operation: …`.  Compilation
/// aborts on the slot errors, so realization is never reached on this route —
/// that diagnostic survives only on the decoded-value route the sibling test
/// above exercises.
///
/// NOT an exit-gate flip: this asserts FAILURE on a COMPILE `Severity::Error`,
/// which `cmd_check` has always produced.  The two `status.success()` assertions
/// above are about BUILD-only diagnostics and stay as they are until #5403 (leaf
/// γ) lands the general Severity::Error gate.
#[test]
fn check_rejects_bare_scalar_mirror_origin_before_reaching_build() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    // The stem must match the `module` declaration, or `check` reports
    // E_MODULE_PATH_MISMATCH and we would be measuring that instead.
    let path = dir.path().join("mirror_bare_scalar_origin.ri");
    std::fs::write(
        &path,
        r#"module mirror_bare_scalar_origin

structure def MirrorBareScalarOrigin {
    param arm_len   : Length = 40mm
    param arm_width : Length = 8mm

    let arm = box(arm_len, arm_width, arm_width)

    // WRONG on purpose — bare-Int origin triple (should be `0mm, 0mm, 0mm`).
    // The normal `1, 0, 0` is correctly bare: it is a dimensionless direction.
    let reflected = mirror(arm, 0, 0, 0, 1, 0, 0)

    param geometry : Solid = union(arm, reflected)

    constraint arm_len > arm_width
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["check", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "a compile-layer ArgTypeMismatch must make `reify check` exit non-zero — \
         this is the short-circuit that forced the decoded-value retarget of \
         `mirror_bare_origin.ri`.\nstdout: {stdout}\nstderr: {stderr}"
    );

    for component in ["ox", "oy", "oz"] {
        let needle = format!("mirror: {component} argument expects Length, got Int");
        assert!(
            stderr.contains(&needle),
            "every component of the origin triple gets its own diagnostic; \
             `{needle}` is missing.\nstderr: {stderr}"
        );
    }

    assert!(
        !stdout.contains("All constraints satisfied"),
        "the short-circuit happens BEFORE constraint checking, so no verdict line \
         may be printed — a green verdict beside these errors is the exact \
         falsehood this gate closes.\nstdout: {stdout}"
    );
}

/// Task 5748 / PRD `check-diagnostic-truthfulness.md` leaf β, D2 — regression LOCK.
///
/// Green before AND after the D2 wiring.  D2 merges `build()`'s DIAGNOSTICS
/// into the reported set; it must never let `build()`'s stale
/// `constraint_results` reach the verdict lines.  `build_with_geometry_output`
/// calls `self.check(module)` internally as its first step, BEFORE
/// `realization_handles` / `achieved_repr_tol` are populated, so substituting
/// its copy would silently degrade RepresentationWithin / DFM verdicts.
///
/// `fixtures/dfm_with_repr_within.ri` is the right lock because it carries BOTH
/// a DFMRule and a RepresentationWithin, so it already exercises the full
/// `build()` → `tessellate_realizations()` → authoritative `check()` sequence
/// that D2 threads a second value through.
///
/// Measured pre-change baseline:
///     stdout: "  OK SphereCheck#constraint[0]" / "All constraints satisfied."
///     stderr: "warning: constraint expression has type Sphere, expected Bool"
///             "warning: W_MODULE_DECL_MISSING: …"
///             "warning: W_DFM_OVERHANG: face dips below the build plane — …"
///     exit 0
///
/// # The contradiction lock (review fix)
///
/// Note what is NOT in that baseline: any line claiming
/// `SphereCheck#constraint[0]` is indeterminate.  `check()` resolved that
/// constraint to `Satisfied`, so `check()` cannot have emitted a
/// `ConstraintIndeterminate` for it; and at the base commit `build()`'s result
/// was discarded wholesale, so build's own copy of that claim had no route to
/// stderr.  D2 opened one — and D2's merge is ONE-DIRECTIONAL by construction:
/// build()'s internal task-4229 retain (in
/// `engine_build::build_with_geometry_output`) drops only
/// the warnings BUILD itself upgraded, and `merge_post_build_verdicts`
/// (main.rs) retains only over check()'s OWN list, for the entries IT upgraded.
/// Nothing in either pass knows about the later, authoritative `check()`, which
/// is why the CLI-side `drop_falsified_indeterminate_diagnostics` filter is
/// required: stdout reporting `OK SphereCheck#constraint[0]` +
/// `All constraints satisfied.` while stderr calls that same constraint
/// indeterminate is a self-contradiction, and truthfulness is the whole point
/// of PRD `check-diagnostic-truthfulness.md`.
#[test]
fn check_constraint_results_come_from_authoritative_check_not_build() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("dfm_with_repr_within.ri"));

    assert!(
        status.success(),
        "DFM Warning + Satisfied RepresentationWithin stays exit 0.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("VIOLATED"),
        "RepresentationWithin must stay Satisfied — a stale build() verdict would \
         degrade it.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("INDETERMINATE"),
        "the verdict must stay definite — build()'s copy is computed before \
         achieved_repr_tol is populated and would read Indeterminate.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("OK SphereCheck#constraint[0]"),
        "the authoritative check() verdict line must be unchanged from the \
         pre-change baseline.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "the summary line must be unchanged from the pre-change baseline.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // The contradiction lock.  Deliberately placed BEFORE the OCCT_AVAILABLE
    // early-return: the leak reproduces regardless of whether the DFM rule
    // actually fired, so gating it on the kernel would hide the defect in
    // stub-mode builds.
    assert!(
        !stderr.contains("SphereCheck#constraint[0] indeterminate"),
        "stdout reports `OK SphereCheck#constraint[0]` and `All constraints satisfied.`, \
         so a stderr line calling that same constraint indeterminate is a \
         self-contradiction — and it is absent from this test's own recorded \
         pre-change baseline.  build()'s stale claim must be dropped against the \
         AUTHORITATIVE verdict list before the D2 merge.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping DFM double-print assertion: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // The other half of D2: "no diagnostic that would appear in check()'s own
    // diagnostics today is ever printed twice".  W_DFM_OVERHANG is produced by
    // `measure_dfm_rules`, which runs inside the authoritative check() AND
    // inside build()'s own internal check() — so it is present in BOTH lists
    // and is exactly the entry a naive concatenation would double.
    let dfm_count = stderr.matches("W_DFM_OVERHANG").count();
    assert_eq!(
        dfm_count,
        1,
        "W_DFM_OVERHANG appears in both check()'s and build()'s diagnostics; the \
         structural-equality merge must collapse it to one line — got {dfm_count}.\n\
         stderr: {stderr}"
    );
}

/// Task 5748 / PRD `check-diagnostic-truthfulness.md` leaf β, D1 item 2 + D2 —
/// the `--purpose` twin of `check_surfaces_geometry_compile_error_from_discarded_build`.
///
/// Sub-path (c) takes an unconditional `Engine::new(checker, None).eval(...)`
/// branch, so a geometry-bearing module under `--purpose` realizes nothing: the
/// `compile_geometry_op` diagnostic is never even PRODUCED, let alone reported.
/// D1 item 2 gives this branch the same `module_has_geometry` build()-vs-eval()
/// choice `cmd_eval` already has.
///
/// Measured pre-change baseline for `fixtures/mirror_bare_origin_purpose.ri`:
///     stdout: "  OK purpose:mfg_ready@MirrorBareOriginPurpose#constraint[0]"
///             "All constraints satisfied."
///     stderr: EMPTY
///     exit 0
///
/// WHY THE FIXTURE MOVED (task 5662): identical to its non-purpose twin — see
/// `check_surfaces_geometry_compile_error_from_discarded_build`, and through it
/// the `mirror` arm of `builtin_arg_slots` in
/// `crates/reify-compiler/src/builtin_signatures.rs`.
///
/// Baseline RE-MEASURED at task 5662 on the retargeted fixture, `reify check
/// --purpose mfg_ready=MirrorBareOriginPurpose`: exit 0, both stdout lines
/// above unchanged, and on stderr the ox/oy/oz argument-type triple twice plus
/// `failed to compile geometry operation: mirror: missing or non-Length
/// argument 'ox' for mirror` exactly ONCE — the dedup pin below is unaffected.
///
/// The stdout assertions are the load-bearing half of D1 item 2: they prove the
/// purpose activation + `check_constraints_with_values` path is unaffected by
/// swapping `EvalResult` for `BuildResult` as the `.values` source (PRD
/// Contract, `--purpose` branch: that call is agnostic to which result type
/// produced `.values`).
#[test]
fn check_purpose_surfaces_geometry_compile_error() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=MirrorBareOriginPurpose",
        &common::fixture_path("mirror_bare_origin_purpose.ri"),
    ]);

    // Unchanged by this leaf — see the sibling test: β fixes collection, γ
    // (#5403) lands the Severity::Error exit gate that flips this.
    assert!(
        status.success(),
        "leaf β fixes diagnostic collection only — the exit gate stays as-is until \
         #5403 (γ) lands the Severity::Error gate.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The purpose path itself must be untouched by the build()-vs-eval() swap.
    assert!(
        stdout.contains("purpose:mfg_ready@"),
        "the purpose-injected constraint id prefix must still be reported — \
         check_constraints_with_values is agnostic to which result produced .values.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied."),
        "the purpose constraint (subject.width > 0mm, default 80mm) stays satisfied.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping --purpose build-diagnostic assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let needle = "failed to compile geometry operation: mirror: missing or non-Length argument 'ox' for mirror";
    assert!(
        stderr.contains(needle),
        "with geometry routing, the --purpose branch realizes geometry and must \
         report the compile error it produces.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let occurrences = stderr.matches(needle).count();
    assert_eq!(
        occurrences,
        1,
        "D2's dedup applies on this branch too: build() emits this error TWICE for \
         one call site, so it must collapse to exactly one line — got {occurrences}.\n\
         stderr: {stderr}"
    );
}

/// Task 5748 / PRD `check-diagnostic-truthfulness.md` leaf β — the `--purpose`
/// twin of the contradiction lock in
/// `check_constraint_results_come_from_authoritative_check_not_build`.
///
/// SELF-MAINTAINING encoding of the invariant rather than a hard-coded needle:
/// every definite verdict line `check` prints on stdout (`  OK <id>` /
/// `  VIOLATED <id>`) is read back out and its subject is required to be absent
/// from stderr's indeterminacy claims.  Whatever constraints the fixture grows,
/// the invariant travels with it.  The `<id>` token is exactly the right needle
/// because `report_constraint_results` prints `constraint_display_label(entry)`
/// — the same label-preferring string the checker embeds in
/// `constraint {…} indeterminate: …`
/// (`reify_constraints::SimpleConstraintChecker::check`).
///
/// STATED HONESTLY: unlike its sub-path (b) sibling this is expected GREEN both
/// before and after the fix — a regression LOCK, not a RED.  Two targeted probes
/// while planning failed to reproduce the leak on sub-path (c), and the plan
/// records that rather than asserting a defect it did not measure.  The
/// mechanism explaining the asymmetry: here `build()`'s internal task-4229
/// recheck and the CLI's `check_constraints_with_values(&values)` run against
/// the SAME post-realization value map (and build's own retain already dropped
/// what it upgraded), so the two lists normally agree — whereas on sub-path (b)
/// `engine_constraints::Engine::check` opens with a fresh `self.eval(module)`
/// and is a genuinely independent, stronger
/// authority.  A residual divergence path does exist on (c) (the UnifiedDag
/// auto-constraint `declined` set, inside
/// `engine_build::build_with_geometry_output`'s
/// `engine_fixpoint::BuildScheduler::UnifiedDag` arm), so the symmetric filter
/// ships as defence-in-depth and this test is what stops the two sub-paths
/// drifting.
#[test]
fn check_purpose_does_not_contradict_definite_verdicts() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        "--purpose",
        "mfg_ready=MirrorBareOriginPurpose",
        &common::fixture_path("mirror_bare_origin_purpose.ri"),
    ]);

    // Unchanged by this leaf — γ (#5403) lands the Severity::Error exit gate.
    assert!(
        status.success(),
        "leaf β fixes diagnostic collection only — the exit gate stays as-is until \
         #5403 (γ) lands the Severity::Error gate.\nstdout: {stdout}\nstderr: {stderr}"
    );

    let definite: Vec<&str> = stdout
        .lines()
        .filter_map(|line| {
            let t = line.trim();
            t.strip_prefix("OK ")
                .or_else(|| t.strip_prefix("VIOLATED "))
        })
        .map(str::trim)
        .collect();
    assert!(
        !definite.is_empty(),
        "the fixture must report at least one definite verdict, else this lock \
         is vacuous.\nstdout: {stdout}\nstderr: {stderr}"
    );

    for id in &definite {
        let claim = format!("constraint {id} indeterminate");
        assert!(
            !stderr.contains(&claim),
            "stdout reports a DEFINITE verdict for `{id}`, so a stderr line \
             claiming it is indeterminate is a self-contradiction — sub-path (c) \
             must filter build()'s stale claims against the authoritative \
             `check_constraints_with_values` verdicts exactly as sub-path (b) \
             does.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

/// esc-5748-6 regression lock: `reify check` must not print EXPORT-ONLY errors.
///
/// D2 merges the realization's diagnostics into `check`'s reported set. While
/// `cmd_check` obtained those via `engine.build(...)`, the merge also picked up
/// the trailing Phase-B PRODUCT EXPORT walk's diagnostics — `"all realized
/// bodies are aux; no product geometry to export"`, `"export error: …"`,
/// `"compound assembly error: …"` (reify-eval `engine_build.rs`). Those were
/// invisible before this task only because the whole `BuildResult` was
/// discarded.
///
/// `reify check` writes no artifact, so "cannot export" is not a fact about the
/// design — it is a FALSE error, and precisely the class of untruthful output
/// PRD `check-diagnostic-truthfulness.md` exists to remove. It is also a
/// forward landmine: leaf γ (#5403) replaces the two ad-hoc escalations with a
/// general `Severity::Error` gate over this same merged set, at which point a
/// leaked export error makes `reify check` EXIT 1 on a perfectly valid design.
///
/// Fix: `cmd_check` calls `Engine::realize_for_check` (realization with the
/// Phase-B export disabled) instead of `Engine::build`. Both `cmd_check`
/// sub-paths use it — the kernel-backed arm and the `--purpose` arm.
///
/// Measured on `fixtures/aux_only_geometry.ri` (geometry-bearing, but every
/// realized body is `aux`, so the export walk finds zero product bodies):
///   - `reify build <f> -o out.step` → `error: all realized bodies are aux; no
///     product geometry to export`, exit 1. CORRECT: build was asked to write.
///   - `reify check <f>` BEFORE the fix → that same `error:` line on stderr,
///     printed directly alongside "All constraints satisfied.", exit 0.
///   - `reify check <f>` AFTER the fix → stdout "  OK AuxOnly#constraint[0]" /
///     "All constraints satisfied.", no export error, exit 0.
#[test]
fn check_does_not_surface_export_only_diagnostics() {
    let (status, stdout, stderr) =
        common::run_subcommand("check", &common::fixture_path("aux_only_geometry.ri"));

    assert!(
        status.success(),
        "the aux-only fixture's constraints are trivially satisfied — `check` \
         must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Whether the realization loop (and hence the export walk that would
        // leak these lines) is reached at all under a stub build is not
        // measured here; follow the same C1 convention as the sibling tests
        // above and skip the content assertions rather than guess.
        eprintln!(
            "skipping export-only-diagnostic assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // The load-bearing assertion. This exact line is what `build -o` prints for
    // this fixture, so its absence here is the whole contract.
    assert!(
        !stderr.contains("no product geometry to export"),
        "`reify check` writes no artifact, so an export-only diagnostic is a \
         FALSE error — `cmd_check` must realize via `realize_for_check` (Phase-B \
         export disabled), never `build()`. Leaf γ (#5403) turns this leak into \
         a false EXIT 1.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // The other two export-only producers on the same walk.
    for leaked in ["export error:", "compound assembly error:"] {
        assert!(
            !stderr.contains(leaked),
            "`{leaked}` is emitted only by the Phase-B product-export walk, \
             which `reify check` must never run.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }

    // Guard against the test passing for the wrong reason: if the fixture ever
    // stops being geometry-bearing, `check` would take the lightweight arm and
    // the assertions above would hold vacuously.
    assert!(
        stdout.contains("AuxOnly#constraint[0]"),
        "fixture must still report its constraint, else this lock is \
         vacuous.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Task 5748 / PRD `check-diagnostic-truthfulness.md` leaf β, D1 — the
/// EXIT-CODE leg of the routing change.
///
/// D1's most user-visible consequence is not a printed line.  Routing a
/// geometry-bearing module through the realization lets
/// `merge_post_build_verdicts` upgrade a constraint from `Indeterminate` to
/// `Violated`, and that verdict flows through `report_constraint_results` →
/// `finish_check` into the process exit code: a `reify check` that exited 0
/// now exits 1.  Nothing pinned that direction end to end.  The sibling D1
/// test (`check_geometry_module_resolves_geometry_query_constraints`) uses a
/// fixture that resolves to Satisfied, so it asserts `status.success()` and
/// covers only Indeterminate → OK; `merge_post_build_verdicts_tests::
/// adopts_a_definite_violated_verdict` stops at the `satisfaction` field and
/// never reaches the CLI's exit mapping.
///
/// `fixtures/geometry_query_violated.ri` is a `: Rigid` body whose only
/// explicit constraint reads the geometry-derived `mass` cell
/// (`volume(geometry) * material.density`, stdlib `Physical`) and is FALSE
/// once that cell resolves: a 100 mm steel cube masses 7.85 kg against a
/// `mass < 1kg` bound.
///
/// Mechanism, not a re-measured baseline for this fixture: before D1 a module
/// carrying none of {geometric Conforms, RepresentationWithin, DFMRule} took
/// the lightweight `Engine::new(None) + check()` path, where nothing runs
/// `run_post_processes`/`post_process_geometry_queries`, so `mass` stayed
/// `undef` and the constraint degraded to INDETERMINATE at exit 0 — the same
/// degradation the sibling test measured for the flange's `moi_principal`.
///
/// A future refactor that drops the upgrade, or narrows it to `Satisfied`,
/// fails HERE rather than silently returning `reify check` to reporting a
/// constraint it cannot evaluate as if it were fine.
#[test]
fn check_geometry_module_upgrades_indeterminate_to_violated_and_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("geometry_query_violated.ri"),
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        // Kernel-DEPENDENT in BOTH directions here, unlike the sibling D1 test
        // whose fixture exits 0 either way: resolving `mass` needs the
        // realization loop to produce a solid, which a stub build cannot do, so
        // the constraint stays INDETERMINATE and `check` exits 0 (C1 — a
        // missing kernel degrades to indeterminate, never to a false
        // violation).  Same C1 convention as the sibling task-5748 tests: skip
        // the content assertions rather than guess at the stub-mode wording.
        assert!(
            stdout.contains("OverweightBlock#constraint[2]"),
            "the fixture must still report its mass constraint, else this lock \
             is vacuous.\nstdout: {stdout}\nstderr: {stderr}"
        );
        eprintln!(
            "skipping verdict-upgrade assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    assert!(
        !status.success(),
        "the mass constraint is FALSE once the geometry query resolves, so \
         `reify check` must exit non-zero — this is D1's exit-code \
         consequence.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("VIOLATED OverweightBlock#constraint[2]"),
        "the upgraded verdict must be REPORTED as violated, not merely counted \
         in the exit code.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("INDETERMINATE OverweightBlock#constraint[2]"),
        "the constraint must no longer degrade to INDETERMINATE.\nstdout: \
         {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Some constraints violated"),
        "the summary line must agree with the per-constraint verdict.\nstdout: \
         {stdout}\nstderr: {stderr}"
    );
    // The other half of D2's no-self-contradiction property, on the direction
    // that moves the exit code: stdout says VIOLATED, so no surviving stderr
    // line may still claim the same constraint is indeterminate.
    assert!(
        !stderr.contains("OverweightBlock#constraint[2] indeterminate"),
        "stdout reports VIOLATED, so a surviving `… indeterminate` line for the \
         same constraint would be a self-contradiction \
         (`drop_falsified_indeterminate_diagnostics`).\nstderr: {stderr}"
    );
}
