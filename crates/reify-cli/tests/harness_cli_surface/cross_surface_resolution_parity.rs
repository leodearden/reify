//! Cross-surface resolution parity: PRD docs/prds/v0_6/resolution-unification.md η, boundaries
//! #2, #3 and #4. `reify check`, `reify eval`, the GUI's `EngineSession` and the LSP must each
//! report the canonical program compile's diagnostics, compared as (severity, message) multisets.
//!
//! The parity rule: when the canonical compile raises no Error, every surface's Errors and
//! Warnings equal its Errors and Warnings. When it raises one, only Errors are compared: a surface
//! that stops at a compile Error reports exactly the canonical Errors, and the LSP, which
//! evaluates through compile Errors, reports at least those.
//!
//! D-11: exit codes are never compared across commands; each command's own outcome is pinned on
//! its own.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use reify_compiler::cfg::CfgSet;
use reify_compiler::program::{self, CompiledProgram};
use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_gui::engine::EngineSession;
use reify_gui::types::GuiState;
use reify_lsp::diagnostics::{DiagnosticsResult, EvalState, compute_diagnostics_with_state};
use tempfile::TempDir;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Error,
    Warning,
}

/// One diagnostic as a surface reported it. A row is a sorted `Vec` of these: a multiset, so a
/// surface that reports a diagnostic twice differs from one that reports it once.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Reported {
    level: Level,
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Program,
    Check,
    Eval,
    Gui,
    Lsp,
}

impl Surface {
    const ALL: [Surface; 5] = [
        Surface::Program,
        Surface::Check,
        Surface::Eval,
        Surface::Gui,
        Surface::Lsp,
    ];

    /// Whether the surface stops at a compile Error. The LSP evaluates through one by design
    /// (reify-lsp's `compute_diagnostics_with_state`), so it may append eval-time diagnostics.
    fn stops_at_compile_error(self) -> bool {
        self != Surface::Lsp
    }
}

/// A project of inline sources in a fresh temp dir.
fn project(files: &[(&str, &str)]) -> TempDir {
    let dir = reify_test_support::prefixed_tempdir("reify-resolution-parity-");
    for (name, source) in files {
        std::fs::write(dir.path().join(name), source)
            .unwrap_or_else(|e| panic!("write {name}: {e}"));
    }
    dir
}

/// Every row's raw observation of one entry, for the scenarios' own postconditions.
struct Observed {
    program: CompiledProgram,
    check: Output,
    eval: Output,
    gui: GuiObservation,
    lsp: DiagnosticsResult,
}

struct GuiObservation {
    /// `load_file`'s error, when it rejected the compile.
    load_error: Option<String>,
    /// The accepted load's state, or `build_gui_state()` after a rejected one.
    state: GuiState,
}

/// Observe `entry` on all five surfaces and assert the parity rule over them.
fn observe(entry: &Path) -> Observed {
    let observed = Observed {
        program: compile_canonically(entry),
        check: reify(&["check"], entry),
        eval: reify(&["eval"], entry),
        gui: load_in_gui(entry),
        lsp: diagnose_in_lsp(entry),
    };
    assert_parity(entry, &observed);
    observed
}

fn compile_canonically(entry: &Path) -> CompiledProgram {
    program::compile_program(
        entry,
        &read(entry),
        &program::entry_dir_resolver(entry),
        &CfgSet::host_default(),
        &SimpleConstraintChecker,
    )
}

fn reify(argv: &[&str], entry: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(argv)
        .arg(entry)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
}

fn load_in_gui(entry: &Path) -> GuiObservation {
    let mut session = EngineSession::with_registered_kernel(Box::new(SimpleConstraintChecker));
    match session.load_file(entry) {
        Ok(state) => GuiObservation {
            load_error: None,
            state,
        },
        Err(load_error) => GuiObservation {
            load_error: Some(load_error),
            state: session
                .build_gui_state()
                .expect("build_gui_state after a rejected load_file"),
        },
    }
}

fn diagnose_in_lsp(entry: &Path) -> DiagnosticsResult {
    let uri = Url::from_file_path(entry).expect("entry path is absolute");
    compute_diagnostics_with_state(&mut EvalState::new(), &read(entry), &uri)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

impl Observed {
    fn row(&self, surface: Surface) -> Vec<Reported> {
        let mut row: Vec<Reported> = match surface {
            Surface::Program => self
                .program
                .entry
                .diagnostics
                .iter()
                .filter_map(|d| from_core(d.severity, &d.message))
                .collect(),
            Surface::Check => from_cli_stderr(&self.check),
            Surface::Eval => from_cli_stderr(&self.eval),
            Surface::Gui => self
                .gui
                .state
                .compile_diagnostics
                .iter()
                .filter_map(|d| from_gui_wire(&d.severity, &d.message))
                .collect(),
            Surface::Lsp => self
                .lsp
                .diagnostics
                .iter()
                .filter_map(|d| from_lsp(d.severity, &d.message))
                .collect(),
        };
        row.sort();
        row
    }
}

fn reported(level: Level, message: &str) -> Reported {
    Reported {
        level,
        message: message.to_owned(),
    }
}

fn from_core(severity: Severity, message: &str) -> Option<Reported> {
    match severity {
        Severity::Error => Some(reported(Level::Error, message)),
        Severity::Warning => Some(reported(Level::Warning, message)),
        Severity::Info => None,
    }
}

/// `reify check` and `reify eval` print each diagnostic as `{severity}: {message}`; every other
/// stderr line is not a diagnostic.
fn from_cli_stderr(output: &Output) -> Vec<Reported> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter_map(|line| {
            if let Some(message) = line.strip_prefix("error: ") {
                Some(reported(Level::Error, message))
            } else {
                line.strip_prefix("warning: ")
                    .map(|message| reported(Level::Warning, message))
            }
        })
        .collect()
}

fn from_gui_wire(severity: &str, message: &str) -> Option<Reported> {
    match severity {
        "Error" => Some(reported(Level::Error, message)),
        "Warning" => Some(reported(Level::Warning, message)),
        "Info" => None,
        other => panic!("GuiState severity {other:?} is not a wire severity"),
    }
}

fn from_lsp(severity: Option<DiagnosticSeverity>, message: &str) -> Option<Reported> {
    match severity {
        Some(DiagnosticSeverity::ERROR) => Some(reported(Level::Error, message)),
        Some(DiagnosticSeverity::WARNING) => Some(reported(Level::Warning, message)),
        _ => None,
    }
}

fn assert_parity(entry: &Path, observed: &Observed) {
    let rows: Vec<(Surface, Vec<Reported>)> = Surface::ALL
        .iter()
        .map(|&surface| (surface, observed.row(surface)))
        .collect();
    let canonical = observed.row(Surface::Program);
    let error_scope = canonical.iter().any(|r| r.level == Level::Error);
    let in_scope = |row: &[Reported]| -> Vec<Reported> {
        row.iter()
            .filter(|r| !error_scope || r.level == Level::Error)
            .cloned()
            .collect()
    };
    let expected = in_scope(&canonical);
    let divergent: Vec<Surface> = rows
        .iter()
        .filter(|(surface, row)| {
            let got = in_scope(row);
            if error_scope && !surface.stops_at_compile_error() {
                !contains_multiset(&got, &expected)
            } else {
                got != expected
            }
        })
        .map(|(surface, _)| *surface)
        .collect();
    assert!(
        divergent.is_empty(),
        "resolution parity broken for {} on {divergent:?} (scope: {}; the Program row is the \
         canonical compile_program)\n{}",
        entry.display(),
        if error_scope {
            "Errors only — the canonical compile raised an Error"
        } else {
            "Errors and Warnings"
        },
        table(&rows)
    );
}

/// Whether `got` holds every member of `expected`, counting repeats.
fn contains_multiset(got: &[Reported], expected: &[Reported]) -> bool {
    let mut unmatched = got.to_vec();
    expected.iter().all(
        |wanted| match unmatched.iter().position(|candidate| candidate == wanted) {
            Some(index) => {
                unmatched.remove(index);
                true
            }
            None => false,
        },
    )
}

fn table(rows: &[(Surface, Vec<Reported>)]) -> String {
    rows.iter()
        .map(|(surface, row)| {
            let lines: Vec<String> = row
                .iter()
                .map(|r| format!("    {:?}: {}", r.level, r.message))
                .collect();
            format!("  {surface:?} ({}):\n{}", row.len(), lines.join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn importing_entry_reports_one_diagnostic_set_on_every_surface() {
    let dir = project(&[
        (
            "rig.ri",
            "module rig\n\
             import parts\n\
             import spares\n\
             structure def Rig {\n    sub p = Pulley()\n    constraint p.diameter > 10mm\n}\n",
        ),
        (
            "parts.ri",
            "module parts\n\
             pub structure def Pulley {\n    param diameter : Length = 40mm\n    \
             param width : Length = 10mm\n}\n",
        ),
        (
            "spares.ri",
            "module spares\n\
             pub structure def Pulley {\n    param diameter : Length = 5mm\n}\n",
        ),
    ]);
    let entry = dir.path().join("rig.ri");

    let observed = observe(&entry);

    let canonical = &observed.program.entry.diagnostics;
    assert!(
        canonical.iter().all(|d| d.severity != Severity::Error)
            && canonical.iter().any(|d| {
                d.severity == Severity::Warning
                    && d.message.contains("parts")
                    && d.message.contains("spares")
            }),
        "precondition: the canonical compile raises no Error and warns that both `parts` and \
         `spares` declare Pulley (I6), so the parity above is not vacuous\n{canonical:#?}"
    );

    let check_stdout = stdout(&observed.check);
    assert!(
        check_stdout.contains("OK Rig#constraint[0]"),
        "check must resolve Pulley first-wins to parts' 40 mm, satisfying the constraint\n\
         {check_stdout}"
    );
    let eval_stdout = stdout(&observed.eval);
    assert!(
        eval_stdout.contains("Rig.p.diameter = 0.04 m"),
        "eval must resolve Pulley first-wins to parts' 40 mm\n{eval_stdout}"
    );
    let gui = &observed.gui.state;
    let gui_verdict = gui
        .constraints
        .iter()
        .find(|c| c.node_id == "Rig#constraint[0]")
        .map(|c| c.status.as_str());
    let gui_pulley_diameter = gui
        .values
        .iter()
        .find(|v| v.cell_id == "Pulley.diameter")
        .and_then(|v| v.si_value);
    assert!(
        observed.gui.load_error.is_none()
            && gui_verdict == Some("satisfied")
            && gui_pulley_diameter.is_some_and(|metres| (metres - 0.04).abs() < 1e-12),
        "the GUI must load, merge parts' Pulley first-wins (Pulley.diameter 40 mm) and satisfy \
         Rig#constraint[0] with it; load error {:?}, Rig#constraint[0] {gui_verdict:?}, \
         Pulley.diameter {gui_pulley_diameter:?} m\nconstraints {:#?}\nvalues {:#?}",
        observed.gui.load_error,
        gui.constraints,
        gui.values
    );
    let lsp_verdicts: Vec<_> = observed
        .lsp
        .check_result
        .as_ref()
        .map(|check| {
            check
                .constraint_results
                .iter()
                .map(|c| c.satisfaction)
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !lsp_verdicts.is_empty()
            && lsp_verdicts
                .iter()
                .all(|s| *s == reify_ir::Satisfaction::Satisfied),
        "the LSP must evaluate Rig with Pulley first-wins to parts' 40 mm, satisfying every \
         constraint; verdicts {lsp_verdicts:?}"
    );
}
