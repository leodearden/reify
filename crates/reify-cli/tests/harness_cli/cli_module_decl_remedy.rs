//! E_MODULE_PATH_MISMATCH names the declaration that fixes it, and that
//! declaration is accepted, on every CLI route into the module-header check
//! (task #7930).
//!
//! `eval` reaches the check through the CLI's own parse-and-compile path, and
//! `check` through the module DAG, so both are exercised.
//!
//! An imported file's header defect names that file at the entry (task #6953,
//! PRD resolution-unification ξ, boundary #16).

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

const ENTRY_IMPORTING_PARTS: &str =
    "module main\nimport parts\nstructure def Rig { param x : Length = 1mm }\n";
const PARTS_BODY: &str = "pub structure def Pulley { param d : Length = 1mm }\n";

#[test]
fn an_imported_files_header_defect_names_the_imported_file() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main.ri");
    let parts = tmp.path().join("parts.ri");
    std::fs::write(&main, ENTRY_IMPORTING_PARTS).unwrap();
    let main = main.to_str().unwrap();
    let parts_path = parts.display().to_string();

    std::fs::write(&parts, format!("module wrongname\n{PARTS_BODY}")).unwrap();
    let (status, stdout, stderr) = common::run_with_args(&["check", main]);
    assert!(
        !status.success(),
        "an imported file's wrong header is an Error, so check should fail.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    for needle in [
        "in imported module 'parts'",
        parts_path.as_str(),
        "E_MODULE_PATH_MISMATCH",
        "module parts",
    ] {
        assert!(
            stderr.contains(needle),
            "check should report {needle:?} for parts.ri's wrong header.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
    }

    std::fs::write(&parts, PARTS_BODY).unwrap();
    let (status, stdout, stderr) = common::run_with_args(&["check", main]);
    assert!(
        status.success(),
        "an imported file's missing header is only a Warning, so check should pass.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    for needle in [parts_path.as_str(), "W_MODULE_DECL_MISSING"] {
        assert!(
            stderr.contains(needle),
            "check should report {needle:?} for parts.ri's missing header.\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
    }
}
