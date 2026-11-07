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

/// Imports `Finish` alone of std.materials.appearance, leaving PLATE's `Color` uncovered.
const ENTITY_HEADER: &str = "import std.materials.appearance.Finish\nimport std.si_units\n\n";

/// `same`'s type param `Color` also hides Plate's `Color` from the module-wide import derivation,
/// leaving an unresolved type, while `MPa` still needs std.si_units: a header for the refused fix
/// to withhold. If the binders become scope-aware, re-point this at another fixture that leaves a
/// residual; a perfect derivation never refuses.
const TYPE_PARAM_HIDES_COLOR: &str = "fn same<Color>(x: Color) -> Color = x

structure def Plate {
    param tint : Color
    let stress = 2MPa
}
";

/// The `=` after `param w : Real` has no default expression, so this does not parse. The `MPa`
/// before it still needs std.si_units: a header for a fix that wrote despite the error to add.
const BROKEN: &str = "structure def Broken {
    let stress = 2MPa
    param w : Real =
}
";

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

fn write_ri(dir: &Path, name: &str, source: &str) -> String {
    let file = dir.join(name);
    fs::write(&file, source).expect("write fixture");
    file.to_str().expect("utf-8 temp path").to_string()
}

#[test]
fn check_reports_the_missing_imports_and_leaves_the_file_alone() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);

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
    let file = write_ri(dir.path(), "plate.ri", PLATE);

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
    let file = write_ri(dir.path(), "plate.ri", PLATE);

    let output = run_reify(&["fix", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Usage: reify fix --imports"),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
}

#[test]
fn fix_refuses_to_write_when_the_derived_imports_leave_a_diagnostic() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", TYPE_PARAM_HIDES_COLOR);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not writing"), "{}", describe(&output));
    assert!(stderr.contains(&file), "{}", describe(&output));
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("added import"),
        "{}",
        describe(&output)
    );
    assert_eq!(
        fs::read_to_string(&file).expect("read fixture"),
        TYPE_PARAM_HIDES_COLOR
    );
}

#[test]
fn a_parse_error_fails_the_file_and_leaves_it_alone() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "broken.ri", BROKEN);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Parse error"),
        "{}",
        describe(&output)
    );
    assert!(output.stdout.is_empty(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), BROKEN);
}

#[test]
fn check_fails_the_run_when_any_file_is_missing_imports() {
    let dir = tempdir().expect("tempdir");
    let plate = write_ri(dir.path(), "plate.ri", PLATE);
    let clean_source = format!("{HEADER}{PLATE}");
    let clean = write_ri(dir.path(), "clean.ri", &clean_source);

    let output = run_reify(&["fix", "--imports", "--check", &plate, &clean]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("std.materials.appearance"),
        "{}",
        describe(&output)
    );
    assert!(stdout.contains("std.si_units"), "{}", describe(&output));
    assert!(!stdout.contains(&clean), "{}", describe(&output));
    assert!(output.stderr.is_empty(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&plate).expect("read fixture"), PLATE);
    assert_eq!(
        fs::read_to_string(&clean).expect("read fixture"),
        clean_source
    );
}

#[test]
fn a_failing_file_does_not_stop_the_files_after_it_being_fixed() {
    let dir = tempdir().expect("tempdir");
    let broken = write_ri(dir.path(), "broken.ri", BROKEN);
    let plate = write_ri(dir.path(), "plate.ri", PLATE);

    let output = run_reify(&["fix", "--imports", &broken, &plate]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert_eq!(
        fs::read_to_string(&plate).expect("read fixed file"),
        format!("{HEADER}{PLATE}"),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&broken).expect("read fixture"), BROKEN);
}

#[test]
fn an_entity_import_covers_only_the_name_it_imports() {
    let dir = tempdir().expect("tempdir");
    let source = format!("{ENTITY_HEADER}{PLATE}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing import std.materials.appearance (Color)"),
        "{}",
        describe(&output)
    );
    assert!(!stdout.contains("Finish"), "{}", describe(&output));
    assert!(!stdout.contains("std.si_units"), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("added import std.materials.appearance (Color)"),
        "{}",
        describe(&output)
    );
    let fixed = fs::read_to_string(&file).expect("read fixed file");
    assert_eq!(
        fixed,
        format!(
            "import std.materials.appearance.Finish\nimport std.si_units\n\
             import std.materials.appearance\n\n{PLATE}"
        )
    );

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixed file"), fixed);

    let output = run_reify(&["check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
}
