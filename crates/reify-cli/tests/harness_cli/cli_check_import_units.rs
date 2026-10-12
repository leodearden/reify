//! `reify check` over an entry whose imports are themselves compiled as
//! program units (PRD docs/prds/v0_6/resolution-unification.md §3.1, task #5516).
//!
//! An imported module sees the full stdlib (#19), exactly like the entry.

use crate::common;

use std::fs;
use std::path::Path;

/// An imported module that uses stdlib units, traits and constructors.
const PARTS_WITH_STDLIB_NAMES: &str = "module parts\n\
pub structure def Bracket {\n\
\x20   param w : Length = 10mm\n\
\x20   param material : ElasticMaterial = ABS_Plastic()\n\
}\n";

fn write_project(dir: &Path, main_src: &str) -> String {
    fs::write(dir.join("parts.ri"), PARTS_WITH_STDLIB_NAMES).unwrap();
    fs::write(dir.join("main.ri"), main_src).unwrap();
    dir.join("main.ri").to_str().unwrap().to_string()
}

/// An import unit that uses stdlib names checks cleanly, and the entry's
/// constraint over the imported sub's Length param is satisfied.
///
/// This is a guard against the aggregation-without-seeding intermediate state
/// (import diagnostics surfaced while import units still compile without the
/// stdlib), which would make this exit non-zero. It is NOT the pre-state
/// discriminator: before import units were stdlib-seeded their errors were
/// swallowed, so this also exited 0 — the compiler-level
/// `compile_program_import_units_see_the_stdlib` test is the one that tells the
/// two states apart.
#[test]
fn check_import_unit_using_stdlib_names_is_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let main = write_project(
        tmp.path(),
        "module main\nimport parts\nstructure def Rig {\n    sub b = Bracket()\n    constraint b.w > 5mm\n}\n",
    );

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
