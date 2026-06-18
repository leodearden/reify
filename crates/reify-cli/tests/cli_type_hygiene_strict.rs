//! CLI integration gate for §10 row 12 — `reify check --strict` exit both ways
//! (type-hygiene λ, task #4495).
//!
//! # §10 row 12 contract
//!
//! `reify check --strict <file>` must exit non-zero when the file contains an
//! indeterminate constraint; `reify check <file>` (no `--strict`) must exit 0.
//!
//! The fixture `type_hygiene_strict.ri` exercises a hygiene-relevant indeterminate
//! constraint (β bare-zero coercion + `auto` mass param) that is OCCT-independent:
//! no geometry query is needed to arrive at `Indeterminate`.
//!
//! # Why a dedicated file for row 12
//!
//! Row 12 is a CLI process-exit behavior delivered by θ (task #4488) in
//! `reify-cli/src/main.rs`.  It cannot be exercised from a library test.
//! The repo precedent for this pattern is `cli_check.rs` (§`check_strict_*`)
//! and `cli_determinacy_gate.rs`.

mod common;

// ── §10 row 12a: --strict mode exits non-zero for indeterminate constraint ────

/// §10 row 12a: `reify check --strict type_hygiene_strict.ri` must exit non-zero
/// and write "Strict check failed" to stderr when the constraint is Indeterminate.
///
/// The fixture has `param mass : Mass = auto` + `constraint mass > 0` — the
/// `auto` default makes `mass` indeterminate at `reify check` time (pre-realization,
/// OCCT-independent), so `mass > 0` resolves Indeterminate → θ strict mode fails.
///
/// RED (step-3): fixture `type_hygiene_strict.ri` does not exist yet → `reify check`
/// cannot load the file → exits non-zero (file-not-found), but for the wrong reason;
/// the non-strict exit-0 assertion (row 12b) is what makes this step RED.
#[test]
fn check_strict_exits_failure_for_indeterminate_hygiene_constraint() {
    let path = common::fixture_path("type_hygiene_strict.ri");
    let (status, stdout, stderr) = common::run_with_args(&["check", "--strict", &path]);

    assert!(
        !status.success(),
        "§10 row 12a: `reify check --strict type_hygiene_strict.ri` should exit non-zero \
         (indeterminate mass > 0 constraint with auto default → Indeterminate → strict fail).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("Strict check failed") || stdout.contains("Strict check failed"),
        "§10 row 12a: strict failure must print 'Strict check failed' on stderr (or stdout).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // The indeterminate entity must be named so a reviewer can identify the constraint.
    assert!(
        stderr.contains("TypeHygieneStrict") || stdout.contains("TypeHygieneStrict"),
        "§10 row 12a: output must name the entity 'TypeHygieneStrict'.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

// ── §10 row 12b: without --strict, indeterminate constraint exits 0 ───────────

/// §10 row 12b: `reify check type_hygiene_strict.ri` (no `--strict`) must exit 0
/// when the only constraint is Indeterminate.
///
/// Indeterminate is not a failure in default mode (only in strict mode per θ).
///
/// RED (step-3): fixture does not exist → `reify check` cannot load the file →
/// exits non-zero → `status.success()` assertion FAILS.
/// GREEN (step-4): fixture is created → `reify check` exits 0 → assertion passes.
#[test]
fn check_without_strict_exits_zero_for_indeterminate_hygiene_constraint() {
    let path = common::fixture_path("type_hygiene_strict.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "§10 row 12b: `reify check type_hygiene_strict.ri` (no --strict) should exit 0 \
         when the only constraint is Indeterminate (Indeterminate is not a failure in \
         default mode — only strict mode fails on Indeterminate per θ #4488).\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // In default mode the summary must not say "Strict check failed".
    assert!(
        !stderr.contains("Strict check failed") && !stdout.contains("Strict check failed"),
        "§10 row 12b: 'Strict check failed' must NOT appear without --strict.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}
