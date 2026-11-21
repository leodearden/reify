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

/// Gates std.materials.appearance on the `appearance` flag.
const GATED_HEADER: &str =
    "#cfg(appearance)\nimport std.materials.appearance\nimport std.si_units\n\n";

/// Gates an entity import of `Appearance`, which binds neither of PLATE's `Color`/`Finish`.
const GATED_APPEARANCE_ENTITY_HEADER: &str =
    "#cfg(appearance)\nimport std.materials.appearance.Appearance\nimport std.si_units\n\n";

/// Gates an entity import of PLATE's `Finish` alone.
const GATED_FINISH_HEADER: &str =
    "#cfg(appearance)\nimport std.materials.appearance.Finish\nimport std.si_units\n\n";

/// What `fix` appends to a reported module that the file imports under a `#cfg` the active cfg
/// leaves unsatisfied.
const GATED_OFF_NOTE: &str = "; a #cfg-gated import of it is not in effect under the active cfg";

/// The stderr line a refused fix adds, naming the file's gated-off imports that its derived
/// imports do not bring in anyway.
const GATED_OFF_REFUSAL_NOTE: &str =
    "info: #cfg-gated imports not in effect under the active cfg: ";

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
fn fix_refuses_to_write_an_import_a_pending_cfg_would_gate() {
    let dir = tempdir().expect("tempdir");
    let source = format!("import std.materials.appearance #cfg(extra)\n\n{PLATE}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in ["std.si_units", "#cfg(extra)", "not writing"] {
        assert!(
            stderr.contains(expected),
            "{expected}\n{}",
            describe(&output)
        );
    }
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("added import"),
        "{}",
        describe(&output)
    );
    assert_eq!(
        fs::read_to_string(&file).expect("read fixture"),
        source,
        "{}",
        describe(&output)
    );
}

#[test]
fn fix_refusal_locates_the_residual_in_the_file() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", TYPE_PARAM_HIDES_COLOR);
    let offset = TYPE_PARAM_HIDES_COLOR
        .find("tint : Color")
        .expect("fixture declares tint")
        + "tint : ".len();
    let before = &TYPE_PARAM_HIDES_COLOR[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let col = before[line_start..].chars().count() + 1;

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let located = format!("{file}:{line}:{col}: error: ");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let residuals: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.contains("not writing"))
        .collect();
    let [residual] = residuals.as_slice() else {
        panic!("expected exactly one residual line\n{}", describe(&output));
    };
    assert!(
        residual.starts_with(&located) && residual.contains("Color"),
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
fn each_parse_error_line_leads_with_its_file_line_and_column_when_several_are_fixed() {
    let dir = tempdir().expect("tempdir");
    let broken = write_ri(dir.path(), "broken.ri", BROKEN);
    let plate = write_ri(dir.path(), "plate.ri", PLATE);

    let output = run_reify(&["fix", "--imports", &broken, &plate]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parse_errors: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("Parse error"))
        .collect();
    assert!(!parse_errors.is_empty(), "{}", describe(&output));
    let last_line = BROKEN.matches('\n').count() + 1;
    for reported in parse_errors {
        let located = reported
            .strip_prefix(&format!("{broken}:"))
            .unwrap_or_else(|| {
                panic!(
                    "{reported:?} must lead with its file\n{}",
                    describe(&output)
                )
            });
        let (line, col, message) = located
            .split_once(':')
            .and_then(|(line, rest)| {
                let (col, message) = rest.split_once(':')?;
                Some((
                    line.parse::<usize>().ok()?,
                    col.parse::<usize>().ok()?,
                    message,
                ))
            })
            .unwrap_or_else(|| {
                panic!(
                    "{reported:?} must give a line and column after its file\n{}",
                    describe(&output)
                )
            });
        assert!(
            (1..=last_line).contains(&line) && col >= 1,
            "{reported:?} must locate a position in the file\n{}",
            describe(&output)
        );
        let message = message
            .strip_prefix(" error: Parse error: ")
            .unwrap_or_else(|| {
                panic!(
                    "{reported:?} must carry the `error` severity and the parse-error label \
                     before its message\n{}",
                    describe(&output)
                )
            });
        assert!(!message.is_empty(), "{}", describe(&output));
    }
}

/// Whether `line` matches `^.+:\d+:\d+: (warning|error): `, the shape VS Code's strict `$gcc`
/// problem matcher accepts.
fn matches_gcc_problem_matcher(line: &str) -> bool {
    let is_number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    ["error", "warning"].iter().any(|severity| {
        line.match_indices(&format!(": {severity}: "))
            .any(|(at, _)| {
                line[..at]
                    .rsplit_once(':')
                    .and_then(|(rest, col)| Some((rest.rsplit_once(':')?, col)))
                    .is_some_and(|((path, line_no), col)| {
                        !path.is_empty() && is_number(line_no) && is_number(col)
                    })
            })
    })
}

#[test]
fn parse_error_and_residual_lines_both_match_a_strict_gcc_problem_matcher() {
    assert!(matches_gcc_problem_matcher("f.ri:3:20: error: m"));
    assert!(
        !matches_gcc_problem_matcher("f.ri:3:20: Parse error: m"),
        "a line without a severity word after its position must not match"
    );

    let dir = tempdir().expect("tempdir");
    let broken = write_ri(dir.path(), "broken.ri", BROKEN);
    let plate = write_ri(dir.path(), "plate.ri", TYPE_PARAM_HIDES_COLOR);

    let output = run_reify(&["fix", "--imports", &broken, &plate]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parse_errors: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with(&format!("{broken}:")))
        .collect();
    let residuals: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with(&format!("{plate}:")) && !line.contains("not writing"))
        .collect();
    assert!(
        !parse_errors.is_empty(),
        "expected parse-error lines\n{}",
        describe(&output)
    );
    assert!(
        !residuals.is_empty(),
        "expected residual lines\n{}",
        describe(&output)
    );
    for reported in parse_errors.iter().chain(&residuals) {
        assert!(
            matches_gcc_problem_matcher(reported),
            "{reported:?} must match the `$gcc` problem matcher\n{}",
            describe(&output)
        );
    }
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
    assert!(!stdout.contains(GATED_OFF_NOTE), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("added import std.materials.appearance (Color)"),
        "{}",
        describe(&output)
    );
    assert!(!stdout.contains(GATED_OFF_NOTE), "{}", describe(&output));
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

#[test]
fn a_cfg_gated_import_covers_only_under_a_cfg_that_satisfies_it() {
    let dir = tempdir().expect("tempdir");
    let source = format!("{GATED_HEADER}{PLATE}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "missing import std.materials.appearance (Color, Finish){GATED_OFF_NOTE}\n"
        )),
        "{}",
        describe(&output)
    );
    assert!(!stdout.contains("std.si_units"), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);

    let output = run_reify(&["fix", "--imports", "--check", "--cfg", "appearance", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(output.stdout.is_empty(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn fix_adds_an_ungated_import_beside_an_unsatisfied_gated_one() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", &format!("{GATED_HEADER}{PLATE}"));

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&format!(
            "added import std.materials.appearance (Color, Finish){GATED_OFF_NOTE}\n"
        )),
        "{}",
        describe(&output)
    );
    let fixed = fs::read_to_string(&file).expect("read fixed file");
    assert_eq!(
        fixed,
        format!(
            "#cfg(appearance)\nimport std.materials.appearance\nimport std.si_units\n\
             import std.materials.appearance\n\n{PLATE}"
        )
    );

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixed file"), fixed);

    let output = run_reify(&["check", &file]);
    assert!(output.status.success(), "{}", describe(&output));
}

#[test]
fn the_host_target_satisfies_a_target_gated_import_by_default() {
    let dir = tempdir().expect("tempdir");
    let source = format!(
        "#cfg(target = \"{}\")\nimport std.materials.appearance\nimport std.si_units\n\n{PLATE}",
        std::env::consts::OS
    );
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(output.status.success(), "{}", describe(&output));

    let output = run_reify(&[
        "fix",
        "--imports",
        "--check",
        "--cfg",
        "target=nowhere",
        &file,
    ]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("missing import std.materials.appearance"),
        "{}",
        describe(&output)
    );
    assert!(stdout.contains(GATED_OFF_NOTE), "{}", describe(&output));
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn a_gated_off_entity_import_that_binds_none_of_the_missing_names_adds_no_note() {
    let dir = tempdir().expect("tempdir");
    let source = format!("{GATED_APPEARANCE_ENTITY_HEADER}{PLATE}");
    let file = write_ri(dir.path(), "plate.ri", &source);
    let expected = format!("{file}: missing import std.materials.appearance (Color, Finish)\n");

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    let output = run_reify(&["fix", "--imports", "--check", "--cfg", "appearance", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);

    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn a_gated_off_entity_import_that_binds_a_missing_name_keeps_the_note() {
    let dir = tempdir().expect("tempdir");
    let source = format!("{GATED_FINISH_HEADER}{PLATE}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", "--check", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "{file}: missing import std.materials.appearance (Color, Finish){GATED_OFF_NOTE}\n"
        )
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn the_residual_guard_judges_only_the_imports_the_cfg_leaves_in_effect() {
    let dir = tempdir().expect("tempdir");
    let source =
        format!("#cfg(appearance)\nimport std.materials.appearance\n\n{TYPE_PARAM_HIDES_COLOR}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not writing"), "{}", describe(&output));
    let refusal_note = format!("{file}: {GATED_OFF_REFUSAL_NOTE}std.materials.appearance");
    assert!(
        stderr.lines().any(|line| line == refusal_note),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);

    let output = run_reify(&["fix", "--imports", "--cfg", "appearance", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(
        fs::read_to_string(&file).expect("read fixed file"),
        format!(
            "#cfg(appearance)\nimport std.materials.appearance\nimport std.si_units\n\n\
             {TYPE_PARAM_HIDES_COLOR}"
        )
    );
}

#[test]
fn the_refusal_note_names_each_gated_off_module_once_in_sorted_order() {
    let dir = tempdir().expect("tempdir");
    let source = format!(
        "#cfg(extra)\nimport std.tolerancing\n\n\
         #cfg(appearance)\nimport std.materials.appearance\n\n\
         #cfg(appearance)\nimport std.materials.appearance.Finish\n\n{TYPE_PARAM_HIDES_COLOR}"
    );
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let refusal_note =
        format!("{file}: {GATED_OFF_REFUSAL_NOTE}std.materials.appearance, std.tolerancing");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line == refusal_note),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn the_refusal_note_omits_a_gated_off_module_the_derived_imports_bring_in() {
    let dir = tempdir().expect("tempdir");
    let gated_import_the_fix_derives_anyway = "#cfg(extra)\nimport std.si_units\n\n";
    let source = format!("{gated_import_the_fix_derives_anyway}{TYPE_PARAM_HIDES_COLOR}");
    let file = write_ri(dir.path(), "plate.ri", &source);

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not writing"), "{}", describe(&output));
    assert!(
        !stderr.contains(GATED_OFF_REFUSAL_NOTE),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), source);
}

#[test]
fn a_fix_that_writes_adds_no_refusal_note_beside_a_gated_off_import() {
    let dir = tempdir().expect("tempdir");
    // Not a std module: a gated-off std module the fix does not derive is itself a residual.
    let gated_non_std_import = "#cfg(extra)\nimport vendor.parts\n\n";
    let file = write_ri(
        dir.path(),
        "plate.ri",
        &format!("{gated_non_std_import}{PLATE}"),
    );

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(GATED_OFF_REFUSAL_NOTE),
        "{}",
        describe(&output)
    );
}

#[test]
fn a_malformed_cfg_prints_the_usage_and_leaves_the_file_alone() {
    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);

    let output = run_reify(&["fix", "--imports", &file, "--cfg"]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--cfg requires a value"),
        "{}",
        describe(&output)
    );
    assert!(
        stderr.contains("Usage: reify fix --imports"),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
}

#[cfg(unix)]
fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[cfg(unix)]
#[test]
fn fix_replaces_the_file_so_a_reader_that_opened_it_first_still_reads_the_original() {
    use std::io::Read;

    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);
    let mut early_reader = fs::File::open(&file).expect("open fixture");

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    let mut seen_by_early_reader = String::new();
    early_reader
        .read_to_string(&mut seen_by_early_reader)
        .expect("read through the early handle");
    assert_eq!(seen_by_early_reader, PLATE, "{}", describe(&output));
    assert_eq!(
        fs::read_to_string(&file).expect("read fixed file"),
        format!("{HEADER}{PLATE}")
    );
    assert_eq!(entry_names(dir.path()), ["plate.ri"]);
}

#[cfg(unix)]
#[test]
fn fix_keeps_the_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).expect("chmod fixture");

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(output.status.success(), "{}", describe(&output));
    let mode = fs::metadata(&file)
        .expect("stat fixed file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640, "mode {mode:o}");
}

#[cfg(unix)]
#[test]
fn fix_leaves_a_read_only_file_alone() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).expect("chmod fixture");
    if fs::OpenOptions::new().write(true).open(&file).is_ok() {
        eprintln!("skipped: file permissions do not bind for this user");
        return;
    }

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(&format!("Error writing {file}: ")),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
    assert_eq!(entry_names(dir.path()), ["plate.ri"]);
}

#[cfg(unix)]
#[test]
fn fix_refuses_a_file_with_other_hard_links() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);
    let other = dir.path().join("other.ri");
    fs::hard_link(&file, &other).expect("hard-link fixture");

    let output = run_reify(&["fix", "--imports", &file]);
    assert!(!output.status.success(), "{}", describe(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("Error writing {file}: ")) && stderr.contains("hard links"),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
    assert_eq!(
        fs::metadata(&file).expect("stat fixture").ino(),
        fs::metadata(&other).expect("stat other link").ino()
    );
    assert_eq!(entry_names(dir.path()), ["other.ri", "plate.ri"]);
}

#[cfg(unix)]
#[test]
fn a_replacement_that_cannot_be_made_leaves_the_file_alone() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().expect("tempdir");
    let file = write_ri(dir.path(), "plate.ri", PLATE);
    let set_dir_mode = |mode| {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode)).expect("chmod dir")
    };
    set_dir_mode(0o555);
    if fs::write(dir.path().join("probe"), "").is_ok() {
        set_dir_mode(0o700);
        eprintln!("skipped: directory permissions do not bind for this user");
        return;
    }

    let output = run_reify(&["fix", "--imports", &file]);
    set_dir_mode(0o700);
    assert!(!output.status.success(), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(&format!("Error writing {file}: ")),
        "{}",
        describe(&output)
    );
    assert_eq!(fs::read_to_string(&file).expect("read fixture"), PLATE);
    assert_eq!(entry_names(dir.path()), ["plate.ri"]);
}

#[cfg(unix)]
#[test]
fn fix_through_a_symlink_rewrites_the_target_and_keeps_the_link() {
    let dir = tempdir().expect("tempdir");
    let real_dir = dir.path().join("real");
    let link_dir = dir.path().join("link");
    fs::create_dir(&real_dir).expect("mkdir real");
    fs::create_dir(&link_dir).expect("mkdir link");
    let target = write_ri(&real_dir, "plate.ri", PLATE);
    let link = link_dir.join("plate.ri");
    std::os::unix::fs::symlink(&target, &link).expect("symlink fixture");
    let link = link.to_str().expect("utf-8 temp path").to_string();

    let output = run_reify(&["fix", "--imports", &link]);
    assert!(output.status.success(), "{}", describe(&output));
    assert!(
        fs::symlink_metadata(&link)
            .expect("lstat link")
            .file_type()
            .is_symlink(),
        "{}",
        describe(&output)
    );
    assert_eq!(
        fs::read_to_string(&target).expect("read fixed target"),
        format!("{HEADER}{PLATE}")
    );
    assert_eq!(entry_names(&real_dir), ["plate.ri"]);
    assert_eq!(entry_names(&link_dir), ["plate.ri"]);
}
