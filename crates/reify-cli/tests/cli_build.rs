mod common;

#[test]
fn build_parse_error_exits_failure() {
    let result = common::run_build("bracket_parse_error.ri");

    assert!(
        !result.status.success(),
        "reify build should exit non-zero for file with parse errors.\nstderr: {}",
        result.stderr
    );
    assert!(
        result.stderr.contains("Parse error"),
        "stderr should contain 'Parse error', got: {}",
        result.stderr
    );
    assert!(
        !result.output_path.exists(),
        "no output file should be written on parse error"
    );
}

#[test]
fn build_violating_bracket_exits_failure() {
    let result = common::run_build("bracket_violating.ri");

    assert!(
        !result.status.success(),
        "reify build should exit non-zero when constraints are violated.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Some constraints violated."),
        "stdout should contain summary message, got: {}",
        result.stdout
    );
    // Geometry file should still be written even when constraints are violated
    assert!(
        result.output_path.exists(),
        "geometry file should still be written even with constraint violations"
    );
}

#[test]
fn build_valid_bracket_exits_success() {
    let result = common::run_build("bracket.ri");

    assert!(
        result.status.success(),
        "reify build should exit 0 for valid bracket.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED' for valid bracket, got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "geometry file should be written on success"
    );
}

#[test]
fn build_compile_error_exits_failure() {
    let result = common::run_build("bracket_compile_error.ri");

    assert!(
        !result.status.success(),
        "reify build should exit non-zero for file with compiler errors.\nstderr: {}",
        result.stderr
    );
    assert!(
        result.stderr.contains("error:"),
        "stderr should contain 'error:', got: {}",
        result.stderr
    );
    assert!(
        !result.output_path.exists(),
        "no output file should be written on compile error"
    );
}

#[test]
fn build_indeterminate_constraint_exits_success() {
    let result = common::run_build("bracket_indeterminate.ri");

    assert!(
        result.status.success(),
        "reify build should exit 0 when constraints are indeterminate (not violated).\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("  OK "),
        "stdout should contain '  OK ' for the satisfied constraint (thickness > 2mm), got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED', got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("Some constraints violated"),
        "stdout should NOT contain violation summary, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("No constraints violated"),
        "stdout should contain 'No constraints violated', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("indeterminate"),
        "stdout should contain 'indeterminate', got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "geometry file should be written when constraints are only indeterminate"
    );
}

#[test]
fn build_violated_with_indeterminate_exits_failure() {
    let result = common::run_build("bracket_violated_with_indeterminate.ri");

    assert!(
        !result.status.success(),
        "reify build should exit non-zero when constraints are violated.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Some constraints violated."),
        "stdout should contain violation summary, got: {}",
        result.stdout
    );
    // Geometry file should still be written even with violations
    assert!(
        result.output_path.exists(),
        "geometry file should still be written even with constraint violations"
    );
}

#[test]
fn build_all_indeterminate_exits_success() {
    let result = common::run_build("bracket_all_indeterminate.ri");

    assert!(
        result.status.success(),
        "reify build should exit 0 when all constraints are indeterminate.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("INDETERMINATE"),
        "stdout should contain 'INDETERMINATE', got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("  OK "),
        "stdout should NOT contain '  OK ' (no satisfied constraints), got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("VIOLATED"),
        "stdout should NOT contain 'VIOLATED', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("No constraints violated"),
        "stdout should contain 'No constraints violated', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("indeterminate"),
        "stdout should contain 'indeterminate', got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "geometry file should be written when constraints are only indeterminate"
    );
}

/// step-3 (T7, RED) — CLI `reify build sub_placement_export.ri` must produce a
/// STEP file containing exactly **2** product solids (placed product children at
/// their composed world coordinates) and ZERO aux solids.
///
/// The fixture `sub_placement_export.ri` has 2 product subs + 1 aux sub; the
/// aux body must be absent from the exported STEP.  Fails on base because
/// `Engine::build` exports only `*step_handles.last()` — one un-placed solid —
/// not the two placed product bodies.
#[test]
fn build_sub_placement_export_has_two_product_solids() {
    let result = common::run_build("sub_placement_export.ri");

    assert!(
        result.status.success(),
        "reify build should exit 0 for sub_placement_export.ri.\nstdout: {}\nstderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {}",
        result.stdout
    );
    assert!(
        result.output_path.exists(),
        "geometry file should be written for sub_placement_export.ri"
    );

    // Read the exported STEP and count manifold solid B-Reps.
    let step_bytes = std::fs::read(&result.output_path).expect("failed to read exported STEP file");
    let step_str = String::from_utf8(step_bytes).expect("STEP output must be valid UTF-8");

    let solid_count = step_str.matches("MANIFOLD_SOLID_BREP(").count();
    assert_eq!(
        solid_count, 2,
        "exported STEP must contain exactly 2 product solids (aux excluded); \
         got {solid_count} MANIFOLD_SOLID_BREP entities.\n\
         (1 → old last-handle bug; 3 → aux not excluded)"
    );
}
