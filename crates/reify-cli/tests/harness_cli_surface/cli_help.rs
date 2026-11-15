//! The `reify` help surface: every dispatched subcommand answers `--help` / `-h`, and the
//! top-level list names every one of them and says how to get per-command help. Each
//! `Flags:` table in that help keeps one layout, and `--cfg` reads the same wherever it is
//! documented.

use std::iter;
use std::process::{Command, Output, Stdio};

/// Every subcommand name the CLI must accept. Owned here, not read from the binary, so a
/// subcommand that drops out of the help table, and with it out of dispatch, is caught.
const DISPATCHED_SUBCOMMANDS: &[&str] = &[
    "check",
    "test",
    "build",
    "run",
    "eval",
    "report",
    "doc",
    "lsp",
    "gui",
    "gui-debug",
    "explain",
    "dev",
    "cache",
    "fix",
];

/// The four flags `cmd_check`'s flag walk accepts.
const CHECK_FLAGS: &[&str] = &["--strict", "--verbose", "--purpose", "--cfg"];

/// The subcommands whose help prints a `Flags:` table. A floor, not the full set: the layout
/// test reads every subcommand's table, and this only keeps it from passing on tables it
/// failed to find.
const COMMANDS_WITH_FLAG_TABLES: &[&str] = &["check", "build", "eval", "gui", "doc", "fix"];

/// The subcommands that accept `--cfg`. A floor for the same reason: the description guard
/// reads every subcommand's `--cfg` row, so a command that gains the flag joins it unasked.
const COMMANDS_TAKING_CFG: &[&str] = &["check", "build", "eval"];

/// The widest line a `Flags:` table may print, in characters.
const HELP_WIDTH: usize = 80;

fn run_reify(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
}

fn stdout_of(args: &[&str]) -> String {
    String::from_utf8_lossy(&run_reify(args).stdout).into_owned()
}

/// The first word of every indented row of `reify --help`'s command list: the commands it
/// lists, as opposed to words that only appear in another row's summary.
fn listed_commands(overview: &str) -> Vec<&str> {
    overview
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|row| row.split_whitespace().next())
        .collect()
}

/// One `Flags:` table of a command's help: its rows, and the lines that precede the first.
struct FlagTable<'a> {
    rows: Vec<FlagTableRow<'a>>,
    /// Lines that continue a row no row precedes: a table cannot open on one.
    orphan_lines: Vec<&'a str>,
}

/// One row of a `Flags:` table: the flag as typed, and a description that may run on over
/// `continuations`.
struct FlagTableRow<'a> {
    line: &'a str,
    synopsis: &'a str,
    /// The char index at which the description starts on `line`.
    description_column: usize,
    continuations: Vec<&'a str>,
}

impl<'a> FlagTable<'a> {
    /// Every line of the table, in the order it was printed.
    fn lines(&self) -> impl Iterator<Item = &'a str> {
        let rows = self
            .rows
            .iter()
            .flat_map(|row| iter::once(row.line).chain(row.continuations.iter().copied()));
        self.orphan_lines.iter().copied().chain(rows)
    }
}

impl<'a> FlagTableRow<'a> {
    /// The row `line` opens: two spaces, the synopsis, a gap of two or more spaces, the
    /// description. `None` for any other line, which continues the row above it.
    fn open(line: &'a str) -> Option<Self> {
        let body = line
            .strip_prefix("  ")
            .filter(|body| body.chars().next().is_some_and(|first| first != ' '))?;
        let (synopsis, after_synopsis) = body.split_once("  ").unwrap_or((body, ""));
        let description = after_synopsis.trim_start();
        Some(FlagTableRow {
            line,
            synopsis,
            description_column: line.chars().count() - description.chars().count(),
            continuations: Vec::new(),
        })
    }

    /// The description with its continuation lines joined, whitespace collapsed.
    fn description(&self) -> String {
        let first_line: String = self.line.chars().skip(self.description_column).collect();
        iter::once(first_line.as_str())
            .chain(self.continuations.iter().copied())
            .flat_map(str::split_whitespace)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The `Flags:` tables of `help`: after each line that is exactly `Flags:`, the lines up to
/// the next empty line or the end.
fn flag_tables(help: &str) -> Vec<FlagTable<'_>> {
    let mut tables = Vec::new();
    let mut lines = help.lines();
    while let Some(line) = lines.next() {
        if line != "Flags:" {
            continue;
        }
        let mut table = FlagTable {
            rows: Vec::new(),
            orphan_lines: Vec::new(),
        };
        for line in lines.by_ref().take_while(|line| !line.is_empty()) {
            if let Some(row) = FlagTableRow::open(line) {
                table.rows.push(row);
            } else if let Some(row) = table.rows.last_mut() {
                row.continuations.push(line);
            } else {
                table.orphan_lines.push(line);
            }
        }
        tables.push(table);
    }
    tables
}

/// The `Flags:` row of `help` that documents `flag`: the row whose synopsis starts with it,
/// as the line it opens on.
fn flag_row<'a>(help: &'a str, flag: &str) -> Option<&'a str> {
    flag_tables(help)
        .into_iter()
        .flat_map(|table| table.rows)
        .find(|row| row.synopsis.starts_with(flag))
        .map(|row| row.line.trim_start())
}

/// What is wrong with the layout of `command`'s `Flags:` table: a line wider than
/// `HELP_WIDTH`, a row whose description starts in another column than the table's, a
/// continuation not indented to its row's description column, or one with no row above it.
fn layout_violations(command: &str, table: &FlagTable<'_>) -> Vec<String> {
    let violation = |problem: String| format!("`reify {command} --help`: {problem}");
    let mut violations = Vec::new();
    for line in table.lines() {
        let width = line.chars().count();
        if width > HELP_WIDTH {
            violations.push(violation(format!(
                "{width} columns, over the limit of {HELP_WIDTH}: {line:?}"
            )));
        }
    }
    for line in &table.orphan_lines {
        violations.push(violation(format!(
            "continuation line with no row above it: {line:?}"
        )));
    }
    let table_column = table.rows.first().map_or(0, |row| row.description_column);
    for row in &table.rows {
        if row.description_column != table_column {
            violations.push(violation(format!(
                "description starts in column {}, not the table's column {table_column}: {:?}",
                row.description_column, row.line
            )));
        }
        for line in &row.continuations {
            let indent = line.chars().take_while(|&c| c == ' ').count();
            if indent != row.description_column {
                violations.push(violation(format!(
                    "continuation indented {indent} columns, not to its row's description \
                     column {}: {line:?}",
                    row.description_column
                )));
            }
        }
    }
    violations
}

#[test]
fn every_subcommand_answers_help_on_stdout_and_exits_zero() {
    let mut failures = Vec::new();
    for name in DISPATCHED_SUBCOMMANDS {
        for flag in ["--help", "-h"] {
            let output = run_reify(&[name, flag]);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !output.status.success()
                || !stdout.starts_with("Usage: reify ")
                || stderr.contains("unknown flag")
            {
                failures.push(format!(
                    "`reify {name} {flag}`: status {:?}\n  stdout: {stdout}\n  stderr: {stderr}",
                    output.status.code()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "every subcommand must print its usage on stdout and exit 0 for --help / -h:\n{}",
        failures.join("\n")
    );
}

#[test]
fn check_help_documents_every_flag_check_accepts() {
    let output = run_reify(&["check", "--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "`reify check --help` must exit 0, got {:?}; stderr: {stderr}",
        output.status.code()
    );
    let undocumented: Vec<&str> = CHECK_FLAGS
        .iter()
        .copied()
        .filter(|flag| flag_row(&stdout, flag).is_none())
        .collect();
    assert!(
        undocumented.is_empty(),
        "`reify check --help` must give {undocumented:?} a row of their own, got: {stdout}"
    );
    let strict_row = flag_row(&stdout, "--strict").unwrap_or_default();
    assert!(
        strict_row.to_lowercase().contains("indeterminate"),
        "`reify check --help`'s `--strict` row must say it fails on an indeterminate \
         constraint, got: {strict_row:?}"
    );
}

#[test]
fn eval_and_build_help_document_the_cfg_flag() {
    for cmd in ["eval", "build"] {
        let output = run_reify(&[cmd, "--help"]);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "`reify {cmd} --help` must exit 0, got {:?}",
            output.status.code()
        );
        assert!(
            flag_row(&stdout, "--cfg").is_some(),
            "`reify {cmd} --help` must give `--cfg` a row of its own, got: {stdout}"
        );
    }
}

#[test]
fn top_level_help_lists_every_dispatched_subcommand() {
    let output = run_reify(&["--help"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "`reify --help` must exit 0");
    let listed = listed_commands(&stdout);
    let missing: Vec<&str> = DISPATCHED_SUBCOMMANDS
        .iter()
        .copied()
        .filter(|name| !listed.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "`reify --help` must give {missing:?} a row of their own, got: {stdout}"
    );
    assert!(
        stdout.contains("reify <command> --help"),
        "`reify --help` must point at per-command help (`reify <command> --help`), got: {stdout}"
    );
}

#[test]
fn every_flags_table_fits_80_columns_with_one_description_column() {
    let mut commands_with_a_table = Vec::new();
    let mut violations = Vec::new();
    for name in DISPATCHED_SUBCOMMANDS {
        let help = stdout_of(&[name, "--help"]);
        let tables = flag_tables(&help);
        if !tables.is_empty() {
            commands_with_a_table.push(*name);
        }
        for table in &tables {
            violations.extend(layout_violations(name, table));
        }
    }
    let tableless: Vec<&str> = COMMANDS_WITH_FLAG_TABLES
        .iter()
        .copied()
        .filter(|name| !commands_with_a_table.contains(name))
        .collect();
    assert!(
        tableless.is_empty(),
        "`reify <cmd> --help` must print a `Flags:` table for {tableless:?}; tables were found \
         for {commands_with_a_table:?}"
    );
    assert!(
        violations.is_empty(),
        "every `Flags:` table must fit {HELP_WIDTH} columns, start every description in one \
         column and indent each continuation line to it:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_cfg_row_gives_the_same_description() {
    let mut cfg_descriptions: Vec<(&str, String)> = Vec::new();
    for name in DISPATCHED_SUBCOMMANDS {
        let help = stdout_of(&[name, "--help"]);
        for table in flag_tables(&help) {
            for row in table
                .rows
                .iter()
                .filter(|row| row.synopsis.starts_with("--cfg"))
            {
                cfg_descriptions.push((name, row.description()));
            }
        }
    }
    let documenting: Vec<&str> = cfg_descriptions.iter().map(|(name, _)| *name).collect();
    let undocumented: Vec<&str> = COMMANDS_TAKING_CFG
        .iter()
        .copied()
        .filter(|name| !documenting.contains(name))
        .collect();
    assert!(
        undocumented.is_empty(),
        "`reify <cmd> --help` must give `--cfg` a row of its own for {undocumented:?}; rows were \
         found for {documenting:?}"
    );
    let listing = cfg_descriptions
        .iter()
        .map(|(name, description)| format!("  reify {name}: {description}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        cfg_descriptions
            .windows(2)
            .all(|pair| pair[0].1 == pair[1].1),
        "every command's `--cfg` row must give the same description, got:\n{listing}"
    );
}
