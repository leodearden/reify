//! `reify fix --imports` (stdlib-namespace ε, task #5497) driven through the
//! real binary on a file in a temporary directory.

use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::tempdir;

/// `Finish`/`Color` come from std.materials.appearance; `MPa` from
/// std.si_units, and it is in no builtin unit table.
const PLATE: &str = "structure def Plate {
    param finish : Finish = Finish.Matte
    param tint : Color = Color()
    let stress = 2MPa
}
";

const HEADER: &str = "import std.materials.appearance\nimport std.si_units\n\n";

fn run_reify(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
}

fn describe(output: &Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn write_plate(dir: &Path) -> String {
    let file = dir.join("plate.ri");
    fs::write(&file, PLATE).expect("write fixture");
    file.to_str().expect("utf-8 temp path").to_string()
}

#[test]
fn check_reports_the_missing_imports_and_leaves_the_file_alone() {
    let dir = tempdir().expect("tempdir");
    let file = write_plate(dir.path());

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("std.materials.appearance"),
        "{}",
        describe(&output)
    );
    assert!(stdout.contains("std.si_units"), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
}

#[test]
fn fix_writes_the_header_once_and_the_fixed_file_checks_clean() {
    let dir = tempdir().expect("tempdir");
    let file = write_plate(dir.path());

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("std.materials.appearance"),
        "{}",
        describe(&output)
    );
    assert!(stdout.contains("std.si_units"), "{}", describe(&output));
    let fixed = fs::read_to_string(&file).expect("read fixed file");
    assert_eq!(fixed, format!("{HEADER}{PLATE}"));

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixed file"), fixed);

    let output = run_reify(&["check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
}

#[test]
fn fix_without_a_fixer_flag_prints_its_usage() {
    let dir = tempdir().expect("tempdir");
    let file = write_plate(dir.path());

    let output = run_reify(&["fix", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Usage: reify fix --imports"),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
}
