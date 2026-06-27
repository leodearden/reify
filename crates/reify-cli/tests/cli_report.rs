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

// ─── step-17/18: a collection-only design surfaces its warning, not a lie ─────

/// A design whose ONLY lifecycle item is a *collection* Buy sub
/// (`sub bolts : List<Bolt>`) rolls up to zero lines / waste / provenance but a
/// NON-empty `report.warnings` (build_bom_report flags the un-rolled-up
/// collection — a v1 limitation — precisely to make the under-count visible).
///
/// The CLI must NOT print the friendly "no BOM line items" message here: that
/// message actively lies (there IS a Buy sub, just a collection one) and — worse
/// — skips `report.render()`, the ONLY sink for `report.warnings` (the stderr
/// loop prints eval diagnostics, not report warnings), silently dropping the
/// warning and re-introducing the exact under-count it exists to prevent. A
/// collection-only design is not an error, so the run still exits 0.
#[test]
fn report_bom_collection_only_design_surfaces_warning() {
    let (status, stdout, stderr) =
        common::run_with_args(&["report", "--bom", &common::fixture_path("collection_bom.ri")]);

    assert!(
        status.success(),
        "a collection-only design is not an error and must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    // The under-count is VISIBLE on stdout: the skipped collection sub is named
    // under a Warnings-section marker (render() is the only sink for warnings).
    assert!(
        stdout.contains("bolts")
            && (stdout.contains("Warnings")
                || stdout.contains("not rolled up")
                || stdout.contains("v1 limitation")),
        "stdout must surface the collection-skip warning naming `bolts`, got: {stdout}"
    );
    // …and must NOT print the friendly empty-BOM message, which would both lie
    // (there IS a Buy sub) and silently drop the warning.
    assert!(
        !stdout.contains("no BOM line items"),
        "a collection-only design must not print the friendly empty-BOM message, got: {stdout}"
    );
}

// ─── amend(#4292): kernel-free eval is sufficient for a geometry-bearing BOM ───

/// `cmd_report` always uses the kernel-free eval path (`Engine::new(None) +
/// eval()`), unlike `cmd_eval`, which routes geometry-bearing modules through
/// `with_registered_kernel + build()`. For a design that BOTH realizes geometry
/// (a `box(...)` op) AND carries a BOM line item, plain eval must still populate
/// the cost cells and emit NO `Severity::Error` diagnostic — so the run exits 0
/// and renders a real BOM.
///
/// This locks the kernel-free-eval-is-sufficient contract: a future regression
/// where plain eval errors on an unrealized solid would non-zero-exit an
/// otherwise-valid BOM silently, and this test would catch it.
#[test]
fn report_bom_geometry_bearing_design_renders_on_kernel_free_path() {
    let (status, stdout, stderr) =
        common::run_with_args(&["report", "--bom", &common::fixture_path("geometry_bom.ri")]);

    assert!(
        status.success(),
        "a geometry-bearing BOM must exit 0 on the kernel-free eval path.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Bill of Materials"),
        "stdout should render the BOM header for a geometry-bearing design, got: {stdout}"
    );
    assert!(
        stdout.contains("BRACKET-X1"),
        "stdout should list the Bracket part number, got: {stdout}"
    );
    assert!(
        stdout.contains("Total: 12.00 USD"),
        "stdout should carry the 12.00 USD grand total (4.00 × 3), got: {stdout}"
    );
    // The friendly empty-message path must NOT trigger — there IS a BOM line, so
    // a real report (not the empty message) must reach stdout.
    assert!(
        !stdout.contains("no BOM line items"),
        "a geometry-bearing design with a Buy sub must render a real BOM, got: {stdout}"
    );
}
