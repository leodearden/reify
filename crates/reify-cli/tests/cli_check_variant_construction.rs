//! End-to-end `reify check` gate for brace-form enum-variant construction
//! field-set/type checking (task δ #3942, step-11).
//!
//! `reify check` compiles + constraint-checks (no geometry eval), so the
//! compile-time field-set/type diagnostics emitted by `crate::variant_construct`
//! carry the user-observable signal: the three malformed fixtures exit non-zero
//! with the diagnostic surfaced on stderr; the well-formed fixture exits 0 and
//! reports "All constraints satisfied". The CLI surfaces diagnostic MESSAGE text
//! (not the typed `DiagnosticCode`), so these assertions match message
//! substrings — the typed-code assertions live in the compiler unit tests
//! (`reify-compiler/tests/variant_construction_check_tests.rs`).

mod common;

/// `Rect { width: 20mm }` omits declared field `height` -> VariantMissingField.
#[test]
fn check_variant_missing_field_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("variant_construct_missing_field.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for a missing-field construction.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("missing field"),
        "stderr should name the missing field, got: {stderr}"
    );
}

/// `Circle { diameter: 5mm }` supplies undeclared field `diameter` ->
/// VariantUnknownField.
#[test]
fn check_variant_unknown_field_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("variant_construct_unknown_field.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for an unknown-field construction.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("error:"),
        "stderr should contain 'error:', got: {stderr}"
    );
    assert!(
        stderr.contains("has no field"),
        "stderr should report the unknown field, got: {stderr}"
    );
}

/// `Circle { radius: "x" }` supplies a String for the Length-typed field ->
/// VariantPayloadType (field-set is correct, so the type check is isolated).
#[test]
fn check_variant_payload_type_exits_failure() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("variant_construct_payload_type.ri"),
    );

    assert!(
        !status.success(),
        "reify check should exit non-zero for a payload-type mismatch.\nstdout: {stdout}\nstderr: {stderr}"
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

/// `Rect { width: 20mm, height: 10mm }` is well-formed -> checks clean.
#[test]
fn check_variant_valid_exits_success() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("variant_construct_valid.ri"),
    );

    assert!(
        status.success(),
        "reify check should exit 0 for a well-formed construction.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied', got: {stdout}"
    );
}
