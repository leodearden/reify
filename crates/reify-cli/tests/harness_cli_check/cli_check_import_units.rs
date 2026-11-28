//! `reify check` over an entry whose imports are themselves compiled as
//! program units (PRD docs/prds/v0_6/resolution-unification.md §3.1, task #5516).
//!
//! An imported module sees builtins plus what it imports, exactly like the
//! entry (stdlib-namespace NS-V1, κ #5503, which inverts RU #19).

use crate::common;

use std::fs;
use std::path::Path;

/// An imported module that uses std.materials.fea's `ElasticMaterial` and
/// `ABS_Plastic`, and the builtin `Length`/`mm`, importing nothing.
const PARTS_WITH_STDLIB_NAMES: &str = "module parts\n\
pub structure def Bracket {\n\
\x20   param w : Length = 10mm\n\
\x20   param material : ElasticMaterial = ABS_Plastic()\n\
}\n";

const MAIN_USING_PARTS: &str = "module main\nimport parts\nstructure def Rig {\n    sub b = Bracket()\n    constraint b.w > 5mm\n}\n";

fn write_project(dir: &Path, parts_src: &str) -> String {
    fs::write(dir.join("parts.ri"), parts_src).unwrap();
    fs::write(dir.join("main.ri"), MAIN_USING_PARTS).unwrap();
    dir.join("main.ri").to_str().unwrap().to_string()
}

/// An import unit that writes std names without importing their module fails
/// `reify check`, with the NS-V2 fix-it re-homed from the unit's file.
#[test]
fn check_import_unit_using_unimported_std_names_fails_with_the_fixit() {
    let tmp = tempfile::tempdir().unwrap();
    let main = write_project(tmp.path(), PARTS_WITH_STDLIB_NAMES);

    let (status, stdout, stderr) = common::run_with_args(&["check", &main]);

    assert!(
        !status.success(),
        "reify check must exit non-zero; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.lines().any(|l| l.starts_with("error:")
            && l.contains("parts.ri")
            && l.contains("ElasticMaterial")
            && l.contains("import std.materials.fea")),
        "expected an `error:` line re-homed from parts.ri carrying the fix-it; stderr: {stderr}"
    );
}

/// With the std import declared, the import unit checks cleanly, and the
/// entry's constraint over the imported sub's Length param is satisfied.
#[test]
fn check_import_unit_importing_its_std_module_is_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let parts = PARTS_WITH_STDLIB_NAMES.replacen(
        "module parts\n",
        "module parts\nimport std.materials.fea\n",
        1,
    );
    let main = write_project(tmp.path(), &parts);

    let (status, stdout, stderr) = common::run_with_args(&["check", &main]);

    assert!(
        status.success(),
        "reify check must exit 0; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("All constraints satisfied"),
        "expected 'All constraints satisfied' on stdout; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("error:"),
        "stderr must carry no error; stderr: {stderr}"
    );
}

/// An import that compiles but carries an Error (PRD boundary #3, third shape)
/// fails `reify check` — the unchanged "any compile Error, exit non-zero"
/// policy — and the error line names the imported file and the cause.
#[test]
fn check_import_unit_error_fails_and_names_the_imported_file() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("parts.ri"),
        "module parts\npub structure def Part {\n    param q : Frobnicate = zorp()\n}\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("main.ri"),
        "module main\nimport parts\nstructure def Rig {\n    sub p = Part()\n}\n",
    )
    .unwrap();
    let main = tmp.path().join("main.ri");

    let (status, stdout, stderr) = common::run_with_args(&["check", main.to_str().unwrap()]);

    assert!(
        !status.success(),
        "reify check must exit non-zero; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.starts_with("error:") && l.contains("parts.ri") && l.contains("Frobnicate")),
        "expected an `error:` line naming parts.ri and Frobnicate; stderr: {stderr}"
    );
}

/// An import that fails to parse fails `reify check`, naming the broken file.
#[test]
fn check_import_parse_error_fails_and_names_the_broken_file() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("broken.ri"),
        "module broken\npub structure def B { param = }\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("main.ri"),
        "module main\nimport broken\nstructure def Rig {\n    param r : Real = 1\n}\n",
    )
    .unwrap();
    let main = tmp.path().join("main.ri");

    let (status, stdout, stderr) = common::run_with_args(&["check", main.to_str().unwrap()]);

    assert!(
        !status.success(),
        "reify check must exit non-zero; stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.starts_with("error:") && l.contains("broken.ri")),
        "expected an `error:` line naming broken.ri; stderr: {stderr}"
    );
}
