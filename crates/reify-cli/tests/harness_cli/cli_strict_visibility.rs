//! Strict stdlib visibility through the production CLI (stdlib-namespace κ
//! #5503, boundary #12): a std name written without its import makes
//! `reify check` fail with the NS-V2 fix-it, its import makes the file clean,
//! and a builtin needs no import. Inputs: the `strict_*.ri` fixtures under
//! `tests/fixtures/`.

use crate::common;

#[test]
fn check_fails_on_an_unimported_std_unit_naming_its_module_and_the_prelude() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        &common::fixture_path("strict_gpa_no_import.ri"),
    ]);
    assert!(
        !status.success(),
        "check must exit non-zero.\nstdout: {stdout}\nstderr: {stderr}"
    );
    for needle in ["GPa", "std.si_units", "std.prelude"] {
        assert!(
            stderr.contains(needle),
            "stderr must name `{needle}`.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn check_passes_once_the_std_unit_module_is_imported() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        &common::fixture_path("strict_gpa_with_si_units.ri"),
    ]);
    assert!(
        status.success(),
        "check must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

#[test]
fn check_passes_on_builtins_with_no_import_and_no_fixit() {
    let (status, stdout, stderr) = common::run_with_args(&[
        "check",
        &common::fixture_path("strict_length_negative_control.ri"),
    ]);
    assert!(
        status.success(),
        "check must exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("std.") && !stderr.contains("std."),
        "a builtin draws no std fix-it.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
