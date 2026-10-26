//! E_MODULE_PATH_MISMATCH names the declaration that fixes it, and that
//! declaration is accepted, on every CLI route into the module-header check
//! (task #7930).
//!
//! `eval` reaches the check through the CLI's own parse-and-compile path, and
//! `check` through the module DAG, so both are exercised.

use crate::common;

const SUBCOMMANDS: [&str; 2] = ["eval", "check"];
const FIXTURE: &str = "mod_decl_mismatch.ri";
const CORRECTIVE_DECLARATION: &str = "module mod_decl_mismatch";

#[test]
fn mismatch_error_names_the_corrective_declaration_on_every_cli_route() {
    let path = common::fixture_path(FIXTURE);
    for sc in SUBCOMMANDS {
        let (status, stdout, stderr) = common::run_subcommand(sc, &path);
        assert!(
            !status.success(),
            "reify {sc} on a mismatched module declaration should fail.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            stderr.contains(CORRECTIVE_DECLARATION),
            "reify {sc} should name the corrective declaration `{CORRECTIVE_DECLARATION}`.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
    }
}

#[test]
fn the_named_declaration_is_accepted_on_every_cli_route() {
    let original = std::fs::read_to_string(common::fixture_path(FIXTURE)).unwrap();
    let (_declared, body) = original
        .split_once('\n')
        .expect("fixture should have a body after its module line");
    let tmp = tempfile::tempdir().unwrap();
    let corrected = tmp.path().join(FIXTURE);
    std::fs::write(&corrected, format!("{CORRECTIVE_DECLARATION}\n{body}")).unwrap();
    let corrected = corrected.to_str().unwrap();

    for sc in SUBCOMMANDS {
        let (status, stdout, stderr) = common::run_subcommand(sc, corrected);
        assert!(
            status.success(),
            "reify {sc} should accept `{CORRECTIVE_DECLARATION}`.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            !stderr.contains("E_MODULE_PATH_MISMATCH"),
            "reify {sc} should raise no E_MODULE_PATH_MISMATCH for `{CORRECTIVE_DECLARATION}`.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
    }
}
