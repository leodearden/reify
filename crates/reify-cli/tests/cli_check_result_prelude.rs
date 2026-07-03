//! End-to-end `reify check` gate for the PRELUDE `Result<T, E>` enum — task
//! B-α #4035 (PRD docs/prds/v0_6/result-and-fallback.md §4.3/§8.B).
//!
//! `reify check` compiles + constraint-checks (no geometry eval), so the
//! compile-time construction-inference / pinned-annotation payload-type
//! diagnostics carry the user-observable signal for this task: the
//! well-formed fixture (`Ok { .. }` against the PRELUDE Result, with NO
//! inline `enum Result` declared) exits 0 and reports "All constraints
//! satisfied"; the pinned-mismatch fixture exits non-zero with the
//! type-param-aware `VariantPayloadType` diagnostic surfaced on stderr. The
//! CLI surfaces diagnostic MESSAGE text (not the typed `DiagnosticCode`), so
//! these assertions match message substrings — the typed-code assertions
//! live in the compiler tests (`reify-compiler/tests/result_prelude_enum_tests.rs`).
//! Mirrors `cli_check_variant_construction.rs` (task δ #3942, step-11).
//!
//! RED (before step-4 adds the two fixtures below): both fixture paths do
//! not exist yet, so `reify check` fails with a file-read error rather than
//! the expected success/diagnostic-message signal.

mod common;

/// `let r = Ok { value: 5mm }` against the PRELUDE Result (no inline `enum
/// Result` declared) -> task γ #4031's payload-driven inference binds
/// `T = Length` -> checks clean.
///
/// RED: `result_prelude_ok_clean.ri` does not exist yet.
#[test]
fn check_result_prelude_ok_clean_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("result_prelude_ok_clean.ri"),
    );

    assert!(
        status.success(),
        "reify check should exit 0 for Ok {{ value: 5mm }} against the PRELUDE Result.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
}

/// `param r : Result<Force, String> = Ok { value: 5mm }` pins `T = Force` via
/// the annotation, but the supplied payload is `Length` -> the
/// type-param-aware `VariantPayloadType` check (Error) fires.
///
/// RED: `result_prelude_pinned_mismatch.ri` does not exist yet.
#[test]
fn check_result_prelude_pinned_mismatch_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("result_prelude_pinned_mismatch.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for a pinned Result<Force, String> vs a Length payload.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("expects type"),
        "stderr should report the type mismatch, got: {stderr}"
    );
}
