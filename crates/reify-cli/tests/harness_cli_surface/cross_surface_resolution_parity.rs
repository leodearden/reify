//! Cross-surface resolution parity: PRD docs/prds/v0_6/resolution-unification.md η, boundaries
//! #2, #3 and #4. `reify check`, `reify eval`, the GUI's `EngineSession` and the LSP must each
//! report the canonical program compile's diagnostics, compared as (severity, message) multisets
//! of Errors and Warnings.
//!
//! The parity rule: `reify check` and `reify eval` report exactly the canonical Errors and
//! Warnings, and so do the GUI and the LSP when the canonical compile raises no Error. When it
//! raises one, the GUI, whose rejected load carries Errors alone, reports exactly the canonical
//! Errors, and the LSP, which evaluates through compile Errors, reports at least the canonical
//! Errors and Warnings.
//!
//! D-11: exit codes are never compared across commands; each command's own outcome is pinned on
//! its own.

use std::path::Path;
use std::process::{Command, Output, Stdio};

use reify_compiler::cfg::CfgSet;
use reify_compiler::program::{self, CompiledProgram};
use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, Severity};
use reify_gui::engine::EngineSession;
use reify_gui::types::GuiState;
use reify_lsp::diagnostics::{DiagnosticsResult, EvalState, compute_diagnostics_with_state};
use tempfile::TempDir;
use tower_lsp::lsp_types::{DiagnosticSeverity, Url};

const SEVERITIES: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Info];

/// One Error or Warning as a surface reported it. A row is a sorted `Vec` of these: a multiset,
/// so a surface that reports a diagnostic twice differs from one that reports it once.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Reported {
    severity: Severity,
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

    /// What the surface must report of the canonical row, given whether the canonical compile
    /// raised an Error.
    fn obligation(self, canonical_has_error: bool) -> (Scope, Comparison) {
        match (self, canonical_has_error) {
            // reify-gui's `reject_compile_errors` keeps a rejected compile's Errors alone.
            (Surface::Gui, true) => (Scope::ErrorsOnly, Comparison::Equals),
            // reify-lsp's `compute_diagnostics_with_state` evaluates through compile Errors, so it
            // may append eval-time diagnostics.
            (Surface::Lsp, true) => (Scope::ErrorsAndWarnings, Comparison::Contains),
            _ => (Scope::ErrorsAndWarnings, Comparison::Equals),
        }
    }
}

/// Which of a row's diagnostics a surface is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    ErrorsAndWarnings,
    ErrorsOnly,
}

impl Scope {
    fn select(self, row: &[Reported]) -> Vec<Reported> {
        row.iter()
            .filter(|r| self == Scope::ErrorsAndWarnings || r.severity == Severity::Error)
            .cloned()
            .collect()
    }
}

/// How a surface's selected diagnostics must relate to the canonical row's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Equals,
    Contains,
}

impl Comparison {
    fn holds(self, got: &[Reported], canonical: &[Reported]) -> bool {
        match self {
            Comparison::Equals => got == canonical,
            Comparison::Contains => contains_multiset(got, canonical),
        }
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

/// Every row's raw observation of one entry, for the parity rule and the scenarios' own
/// postconditions.
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

/// Observe `entry` on all five surfaces.
fn observe(entry: &Path) -> Observed {
    Observed {
        program: compile_canonically(entry),
        check: reify(&["check"], entry),
        eval: reify(&["eval"], entry),
        gui: load_in_gui(entry),
        lsp: diagnose_in_lsp(entry),
    }
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

/// Run the built `reify` on `entry` with a cache of its own, so no ambient cache reaches it.
fn reify(argv: &[&str], entry: &Path) -> Output {
    let cache_dir = reify_test_support::prefixed_tempdir("reify-resolution-parity-cache-");
    Command::new(reify_test_support::test_package::bin_exe("reify"))
        .args(argv)
        .arg(entry)
        .env("REIFY_CACHE_DIR", cache_dir.path())
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
    /// Every text the surface produced, with the severity it reported it at: each diagnostic, and
    /// for a command each stderr line, which has no severity unless it is a diagnostic.
    fn raw(&self, surface: Surface) -> Vec<(Option<Severity>, String)> {
        match surface {
            Surface::Program => self
                .program
                .entry()
                .diagnostics
                .iter()
                .map(|d| (Some(d.severity), d.message.clone()))
                .collect(),
            Surface::Check => from_cli_stderr(&self.check),
            Surface::Eval => from_cli_stderr(&self.eval),
            Surface::Gui => self
                .gui
                .state
                .compile_diagnostics
                .iter()
                .map(|d| (Some(from_gui_wire(&d.severity)), d.message.clone()))
                .collect(),
            Surface::Lsp => self
                .lsp
                .diagnostics
                .iter()
                .map(|d| (from_lsp(d.severity), d.message.clone()))
                .collect(),
        }
    }

    /// The surface's Errors and Warnings, sorted.
    fn row(&self, surface: Surface) -> Vec<Reported> {
        let mut row: Vec<Reported> = self
            .raw(surface)
            .into_iter()
            .filter_map(|(severity, message)| match severity {
                Some(severity @ (Severity::Error | Severity::Warning)) => {
                    Some(Reported { severity, message })
                }
                _ => None,
            })
            .collect();
        row.sort();
        row
    }

    /// The surfaces whose text mentions any of `needles`, whatever its severity.
    fn surfaces_mentioning(&self, needles: &[&str]) -> Vec<Surface> {
        Surface::ALL
            .into_iter()
            .filter(|&surface| {
                self.raw(surface)
                    .iter()
                    .any(|(_, text)| needles.iter().any(|needle| text.contains(needle)))
            })
            .collect()
    }
}

/// `reify check` and `reify eval` print each diagnostic as `{severity}: {message}`, `Severity`'s
/// `Display` before the message; any other stderr line is not a diagnostic.
fn from_cli_stderr(output: &Output) -> Vec<(Option<Severity>, String)> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(|line| {
            SEVERITIES
                .into_iter()
                .find_map(|severity| {
                    line.strip_prefix(&format!("{severity}: "))
                        .map(|message| (Some(severity), message.to_owned()))
                })
                .unwrap_or_else(|| (None, line.to_owned()))
        })
        .collect()
}

fn from_gui_wire(wire: &str) -> Severity {
    SEVERITIES
        .into_iter()
        .find(|severity| severity.as_wire_str() == wire)
        .unwrap_or_else(|| panic!("GuiState severity {wire:?} is not a wire severity"))
}

fn from_lsp(severity: Option<DiagnosticSeverity>) -> Option<Severity> {
    match severity {
        Some(DiagnosticSeverity::ERROR) => Some(Severity::Error),
        Some(DiagnosticSeverity::WARNING) => Some(Severity::Warning),
        Some(DiagnosticSeverity::INFORMATION) => Some(Severity::Info),
        _ => None,
    }
}

/// Assert the parity rule over `observed`, every surface's observation of `entry`.
fn assert_parity(entry: &Path, observed: &Observed) {
    let rows: Vec<(Surface, Vec<Reported>)> = Surface::ALL
        .iter()
        .map(|&surface| (surface, observed.row(surface)))
        .collect();
    let canonical = observed.row(Surface::Program);
    let canonical_has_error = canonical.iter().any(|r| r.severity == Severity::Error);
    let broken: Vec<String> = rows
        .iter()
        .filter_map(|(surface, row)| {
            let (scope, comparison) = surface.obligation(canonical_has_error);
            (!comparison.holds(&scope.select(row), &scope.select(&canonical)))
                .then(|| format!("{surface:?} must {comparison:?} the canonical {scope:?}"))
        })
        .collect();
    assert!(
        broken.is_empty(),
        "resolution parity broken for {}: {broken:?} (the Program row is the canonical \
         compile_program)\n{}",
        entry.display(),
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
                .map(|r| format!("    {:?}: {}", r.severity, r.message))
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
fn contains_multiset_counts_repeats_and_severity() {
    let error = |message: &str| Reported {
        severity: Severity::Error,
        message: message.to_owned(),
    };
    let warning_a = Reported {
        severity: Severity::Warning,
        message: "a".to_owned(),
    };
    assert!(contains_multiset(
        &[error("a"), error("a"), error("b")],
        &[error("a"), error("a")]
    ));
    assert!(!contains_multiset(&[error("a")], &[error("a"), error("a")]));
    assert!(!contains_multiset(&[error("b")], &[error("a")]));
    assert!(!contains_multiset(&[warning_a], &[error("a")]));
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
    assert_parity(&entry, &observed);

    let canonical = &observed.program.entry().diagnostics;
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

const HEALTHY_PARTS: (&str, &str) = (
    "parts.ri",
    "module parts\npub structure def Pulley {\n    param diameter : Length = 40mm\n}\n",
);

/// A module that fails to parse: anything that resolved it would report a parse error.
const UNPARSABLE_SOURCE: &str = "module unparsable\npub structure def Gear { param = }\n";

#[test]
fn broken_import_errors_reach_every_surface_attributed_to_the_import() {
    // (entry stem, broken import, the broken import's source — `None` writes no file)
    let shapes: &[(&str, &str, Option<&str>)] = &[
        ("missing_import", "nosuch", None),
        ("parse_error_import", "unparsable", Some(UNPARSABLE_SOURCE)),
        (
            "type_error_import",
            "ill_typed",
            Some("module ill_typed\npub structure def Gear { param q : Frobnicate = zorp() }\n"),
        ),
    ];
    for &(stem, broken, broken_source) in shapes {
        let entry_name = format!("{stem}.ri");
        let entry_source = format!(
            "module {stem}\nimport {broken}\nimport parts\nstructure def Rig {{ sub p = Pulley() }}\n"
        );
        let broken_name = format!("{broken}.ri");
        let mut files = vec![HEALTHY_PARTS, (entry_name.as_str(), entry_source.as_str())];
        if let Some(source) = broken_source {
            files.push((broken_name.as_str(), source));
        }
        let dir = project(&files);
        let broken_path = dir.path().join(&broken_name).display().to_string();

        let entry = dir.path().join(&entry_name);
        let observed = observe(&entry);
        assert_parity(&entry, &observed);

        let program = &observed.program;
        assert!(
            program
                .entry()
                .diagnostics
                .iter()
                .any(|d| d.severity == Severity::Error && d.message.contains(&broken_path)),
            "{stem}: the canonical compile must raise an Error attributed to {broken_path}; every \
             surface reports the canonical Errors (parity above), so it is on every surface\n{:#?}",
            program.entry().diagnostics
        );
        assert!(
            program
                .entry_declared_templates()
                .iter()
                .any(|t| t.name == "Rig"),
            "{stem}: the entry must still compile despite its broken import"
        );

        // D-11: each command against its own pinned outcome, never against another command's.
        let pinned_outcomes = [
            (
                "`reify check` exits 1",
                observed.check.status.code() == Some(1),
            ),
            (
                "`reify eval` exits 1",
                observed.eval.status.code() == Some(1),
            ),
            (
                "the GUI's load_file returns Err",
                observed.gui.load_error.is_some(),
            ),
        ];
        for (outcome, holds) in pinned_outcomes {
            assert!(
                holds,
                "{stem}: {outcome} (its own pinned outcome; G7 waiver error-severity-exits-nonzero)\n\
                 check: {:?}\neval: {:?}\ngui load error: {:?}",
                observed.check.status, observed.eval.status, observed.gui.load_error
            );
        }
    }
}

/// Boundary #4's project: a healthy import, and two imports only a wasm target follows — one that
/// fails to parse, one defining `WasmOnly`. No surface is given a cfg, so each takes the host's.
fn cfg_gated_project() -> TempDir {
    project(&[
        HEALTHY_PARTS,
        ("unparsable.ri", UNPARSABLE_SOURCE),
        (
            "wasm_parts.ri",
            "module wasm_parts\npub structure def WasmOnly {\n    param size : Length = 20mm\n}\n",
        ),
        (
            "gated_import.ri",
            "module gated_import\n\
             #cfg(target = \"wasm\")\nimport unparsable\n\
             #cfg(target = \"wasm\")\nimport wasm_parts\n\
             import parts\n\
             structure def Rig { sub p = Pulley() }\n",
        ),
        (
            "gated_ref.ri",
            "module gated_ref\n\
             #cfg(target = \"wasm\")\nimport wasm_parts\n\
             structure def Rig { sub w = WasmOnly() }\n",
        ),
    ])
}

#[test]
fn cfg_unsatisfied_import_is_inert_on_every_surface() {
    let dir = cfg_gated_project();
    let gated_paths: Vec<String> = ["unparsable.ri", "wasm_parts.ri"]
        .iter()
        .map(|name| dir.path().join(name).display().to_string())
        .collect();

    let entry = dir.path().join("gated_import.ri");
    let observed = observe(&entry);
    assert_parity(&entry, &observed);

    let program = &observed.program;
    assert!(
        program
            .entry()
            .diagnostics
            .iter()
            .all(|d| d.severity != Severity::Error),
        "precondition: the canonical compile raises no Error, so the parity above compared \
         Errors and Warnings\n{:#?}",
        program.entry().diagnostics
    );
    let mut needles: Vec<&str> = gated_paths.iter().map(String::as_str).collect();
    needles.extend([
        "imported module 'unparsable'",
        "imported module 'wasm_parts'",
    ]);
    let mentioning = observed.surfaces_mentioning(&needles);
    assert!(
        mentioning.is_empty(),
        "a gated-out import must be inert: no surface may report a diagnostic attributed to it; \
         {mentioning:?} did"
    );
    let import_keys: Vec<&str> = program
        .imports
        .iter()
        .map(|unit| unit.import_path.as_str())
        .collect();
    assert_eq!(
        import_keys,
        ["parts"],
        "the program's imports must be the followed `parts` alone"
    );
    let gated_sources: Vec<&str> = program
        .sources
        .iter()
        .map(|source| source.module_key.as_str())
        .filter(|key| ["unparsable", "wasm_parts"].contains(key))
        .collect();
    assert!(
        gated_sources.is_empty(),
        "the program's sources must name no gated module; found {gated_sources:?}"
    );
    assert!(
        program.entry().templates.iter().all(|t| t.name != "WasmOnly"),
        "the I5 merge must not take WasmOnly from a gated-out import"
    );
    let env = reify_eval::DefEnv::from_program(program);
    assert!(
        env.resolve_template("WasmOnly").is_none()
            && env.resolve_template_in("wasm_parts", "WasmOnly").is_none(),
        "the DefEnv must resolve WasmOnly neither through its precedence view nor through \
         `wasm_parts`"
    );
}

#[test]
fn cfg_unsatisfied_import_lends_the_entry_no_definitions() {
    let dir = cfg_gated_project();
    let entry = dir.path().join("gated_ref.ri");
    let parsed = reify_compiler::parse_with_stdlib(&read(&entry), ModulePath::single("gated_ref"));
    assert!(
        !program::entry_follows_imports(&parsed, &CfgSet::host_default()),
        "precondition: the entry follows no import, so the GUI and the LSP compile it single-file \
         while `reify check` and `reify eval` take the program compile, and the parity below spans \
         both routes"
    );

    let observed = observe(&entry);
    assert_parity(&entry, &observed);

    let program = &observed.program;
    assert!(
        program
            .entry()
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("WasmOnly")),
        "the canonical compile must reject Rig's WasmOnly, which only a gated-out import defines\n\
         {:#?}",
        program.entry().diagnostics
    );
    assert!(
        reify_eval::DefEnv::from_program(program)
            .resolve_template("WasmOnly")
            .is_none(),
        "the DefEnv must not resolve WasmOnly"
    );
}
