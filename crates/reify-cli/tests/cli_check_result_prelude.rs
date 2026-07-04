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
/// The declared-type assertions below pin the CONCRETE substituted/supplied
/// types, not just the generic "expects type" phrase, so a regression that
/// pins the wrong type param (or substitutes the wrong arg) but still
/// happens to emit *some* "expects type" message would be caught. Note
/// `Type::Scalar`'s `Display` (`crates/reify-core/src/ty.rs`) renders the raw
/// SI dimension exponents rather than the dimension's `canonical_name()`, so
/// the pinned `Force` reads as `Scalar[m·kg·s^-2]` and the supplied `Length`
/// payload as `Scalar[m]` — confirmed against the live `reify check` output,
/// not the literal words "Force"/"Length".
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
        stderr.contains("expects type Scalar[m\u{b7}kg\u{b7}s^-2]"),
        "stderr should report the PINNED type (Force's dimension, substituted for T), got: {stderr}"
    );
    assert!(
        stderr.contains("got Scalar[m]"),
        "stderr should report the SUPPLIED payload's type (Length's dimension, from 5mm), got: {stderr}"
    );
}
