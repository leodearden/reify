//! The `reify` help surface: every dispatched subcommand answers `--help` / `-h`, and the
//! top-level list names every one of them and says how to get per-command help.

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
];

/// The four flags `cmd_check`'s flag walk accepts.
const CHECK_FLAGS: &[&str] = &["--strict", "--verbose", "--purpose", "--cfg"];

fn run_reify(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
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

/// The `Flags:` row of `help` that documents `flag`: the line that starts with it.
fn flag_row<'a>(help: &'a str, flag: &str) -> Option<&'a str> {
    help.lines()
        .map(str::trim_start)
        .find(|line| line.starts_with(flag))
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
