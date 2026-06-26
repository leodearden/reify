// Integration tests for `reify report --bom <file>` — the BOM / cost / waste /
// provenance eval-rollup CLI (io-lifecycle-bom-cost #4292, boundary α).
//
// These lock the user-observable binary surface: the report renders to stdout,
// diagnostics go to stderr, and exit codes reflect compile/eval errors. The
// rendered strings mirror the eval-layer lock in
// `crates/reify-eval/tests/bom_report_eval.rs` (the ε example row).

mod common;

/// `reify report --bom <examples/bom_lifecycle.ri>` renders the full BOM /
/// cost / waste / provenance report to stdout and exits 0 — the same strings
/// the eval-layer render test locks, now through the binary (α).
#[test]
fn report_bom_renders_full_lifecycle_report() {
    let (status, stdout, stderr) =
        common::run_with_args(&["report", "--bom", &common::example_path("bom_lifecycle.ri")]);

    assert!(
        status.success(),
        "reify report --bom should exit 0 for bom_lifecycle.ri.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Bill of Materials"),
        "stdout should contain the BOM header, got: {stdout}"
    );
    assert!(
        stdout.contains("BOLT-M6-20"),
        "stdout should list the Bolt part number, got: {stdout}"
    );
    assert!(
        stdout.contains("PLATE-A36-6"),
        "stdout should list the Plate part number, got: {stdout}"
    );
    assert!(
        stdout.contains("Total: 11.00 USD"),
        "stdout should carry the 11.00 USD grand total, got: {stdout}"
    );
    assert!(
        stdout.contains("Offcut"),
        "stdout should name the discard reason Offcut, got: {stdout}"
    );
    assert!(
        stdout.contains("incoming.step"),
        "stdout should name the imported provenance source, got: {stdout}"
    );
    assert!(
        !stderr.contains("Unknown command"),
        "stderr should not contain 'Unknown command' (report must be a real subcommand), got: {stderr}"
    );
}

// ─── step-15: empty-BOM, missing-flag, and compile-error edge cases ──────────

/// An empty-BOM design (no Buy/Discard/Input subs) still exits 0, but prints a
/// friendly "no BOM line items" message rather than an empty skeleton.
#[test]
fn report_bom_empty_design_prints_friendly_message() {
    let (status, stdout, stderr) =
        common::run_with_args(&["report", "--bom", &common::fixture_path("no_bom.ri")]);

    assert!(
        status.success(),
        "an empty-BOM design should still exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("no BOM line items"),
        "stdout should carry a friendly empty-BOM message, got: {stdout}"
    );
}

/// `reify report <file>` WITHOUT `--bom` is a usage error: non-zero exit with a
/// message naming the flag on stderr.
#[test]
fn report_without_bom_flag_is_usage_error() {
    let (status, _stdout, stderr) =
        common::run_with_args(&["report", &common::fixture_path("no_bom.ri")]);

    assert!(
        !status.success(),
        "`report` without --bom must exit non-zero, stderr: {stderr}"
    );
    assert!(
        stderr.contains("--bom"),
        "stderr should carry a usage/error message naming --bom, got: {stderr}"
    );
}

/// A compile-error design propagates to a non-zero exit (no panic, no success).
#[test]
fn report_bom_compile_error_exits_nonzero() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "report",
        "--bom",
        &common::fixture_path("bracket_compile_error.ri"),
    ]);

    assert!(
        !status.success(),
        "a compile error must exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // The stdout contract: a failed run renders NO report. A regression that
    // printed a (partial/garbage) BOM alongside the non-zero exit must fail.
    assert!(
        !stdout.contains("Bill of Materials"),
        "a compile error must not render a BOM to stdout, got: {stdout}"
    );
}

// ─── amend(#4292): arg-rejection paths are user-facing contract behavior ──────

/// An unknown flag is rejected before any file is read: non-zero exit with the
/// unknown-flag message on stderr (and no rendered report on stdout).
#[test]
fn report_unknown_flag_is_rejected() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "report",
        "--bom",
        &common::example_path("bom_lifecycle.ri"),
        "--nope",
    ]);

    assert!(
        !status.success(),
        "an unknown flag must exit non-zero, stderr: {stderr}"
    );
    assert!(
        stderr.contains("unknown flag"),
        "stderr should name the unknown flag, got: {stderr}"
    );
    assert!(
        !stdout.contains("Bill of Materials"),
        "a rejected arg must not render a BOM to stdout, got: {stdout}"
    );
}

/// A second positional path is a usage error: `report --bom a.ri b.ri` exits
/// non-zero with the extra-positional message on stderr.
#[test]
fn report_extra_positional_is_rejected() {
    let (status, _stdout, stderr) = common::run_with_args(&["report", "--bom", "a.ri", "b.ri"]);

    assert!(
        !status.success(),
        "a second positional must exit non-zero, stderr: {stderr}"
    );
    assert!(
        stderr.contains("extra positional"),
        "stderr should name the extra positional argument, got: {stderr}"
    );
}
