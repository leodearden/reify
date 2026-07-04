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
/// This CLI-layer test only needs to confirm the pinned-vs-supplied signal
/// is surfaced end-to-end; the exact `Type::Scalar` rendering is a separate
/// concern owned by the compiler-level `VariantPayloadType` assertions in
/// `result_prelude_enum_tests.rs`. So rather than pin the exact raw
/// SI-exponent Display string (`Scalar[m\u{b7}kg\u{b7}s^-2]` — exponent
/// ordering + middot separator, which the doc comment on `Type::Scalar`'s
/// `Display` impl notes could someday switch to `canonical_name()`), the
/// assertions below use the stable, behavior-bearing substrings "expects
/// type" / "got" plus the `kg` mass-dimension marker that Force's dimension
/// carries and Length's does not — and pin that marker to the correct SIDE
/// of the message (before "got" for the PINNED Force type, absent after
/// "got" for the SUPPLIED Length type) so a regression that pins the wrong
/// type param, or substitutes the wrong arg, is still caught even though the
/// exact exponent formatting is no longer asserted here.
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
        "stderr should report the type mismatch (expects), got: {stderr}"
    );
    let got_index = stderr.find("got").unwrap_or_else(|| {
        panic!("stderr should report the supplied type (got), got: {stderr}")
    });
    let kg_index = stderr.find("kg").unwrap_or_else(|| {
        panic!(
            "stderr should carry the 'kg' mass-dimension marker for the PINNED Force type, got: {stderr}"
        )
    });
    assert!(
        kg_index < got_index,
        "the 'kg' mass-dimension marker should appear in the PINNED (expects) segment, \
         before 'got' -- proving T was substituted with Force, not Length, got: {stderr}"
    );
    assert!(
        !stderr[got_index..].contains("kg"),
        "the SUPPLIED payload's type (Length) segment should NOT carry a mass-dimension \
         marker ('kg'), got: {stderr}"
    );
}
