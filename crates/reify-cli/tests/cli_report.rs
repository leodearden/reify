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
