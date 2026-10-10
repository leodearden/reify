//! NS-P4 parity (stdlib-namespace §3.1, boundary #1a): for every fixture, the
//! compile side and the eval side bind the SAME definition.
//!
//! One table, one driver. Each row names how its entry is compiled and which
//! prelude the `Engine` evaluates against; its entry carries a `PROBE`
//! placeholder that the driver fills twice:
//! - with the winner's probe: the compile must be Error-free and evaluating
//!   `Main.read` must yield the winner's value (so a clean compile cannot pass
//!   over an `Undef` eval binding);
//! - with the loser's probe: the compile must reject it, proving the compile
//!   side did not bind the loser.

use std::fs;

use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleResolver, compile_entry_with_stdlib_cfg};
use reify_core::{Diagnostic, ModulePath, Severity};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// How a row's entry is compiled, and what its `Engine` prelude is.
enum Setup {
    /// One file compiled with the real stdlib prelude; production `Engine`.
    SingleFileWithStdlib,
    /// A tempdir project: `main.ri` plus `(module name, source)` siblings,
    /// compiled through `compile_entry_with_stdlib_cfg` (prelude = stdlib ++
    /// followed imports); production `Engine`.
    Project(&'static [(&'static str, &'static str)]),
    /// `(dotted module path, source)` modules each compiled standalone and
    /// used, in this order, as BOTH the compile prelude and the `Engine`
    /// prelude.
    SyntheticPrelude(&'static [(&'static str, &'static str)]),
}

struct ParityRow {
    name: &'static str,
    setup: Setup,
    /// Entry source; `PROBE` is replaced by the probe under test, and the
    /// probed value is read back from cell `Main.read`.
    entry: &'static str,
    winner_probe: &'static str,
    winner_value: fn() -> Value,
    loser_probe: &'static str,
    /// Substring of the Error the compile side raises for the loser's probe.
    loser_error: &'static str,
}

const READ_WIDGET: &str = "structure def Main {\n    sub w = Widget()\n    let read = w.PROBE\n}\n";

fn rows() -> Vec<ParityRow> {
    vec![
        ParityRow {
            name: "(a) template, user-import peers: first import wins",
            setup: Setup::Project(&[
                (
                    "b",
                    "pub structure def Widget {\n    param from_b : Real = 2.0\n}\n",
                ),
                (
                    "c",
                    "pub structure def Widget {\n    param from_c : Real = 5.0\n}\n",
                ),
            ]),
            entry: "import b\nimport c\n\nstructure def Main {\n    sub w = Widget()\n    let read = w.PROBE\n}\n",
            winner_probe: "from_b",
            winner_value: || Value::Real(2.0),
            loser_probe: "from_c",
            loser_error: "has no member",
        },
        ParityRow {
            name: "(b) template, std-tier peers: first std module wins",
            setup: Setup::SyntheticPrelude(&[
                (
                    "std.p1",
                    "pub structure def Widget {\n    param from_p1 : Real = 2.0\n}\n",
                ),
                (
                    "std.p2",
                    "pub structure def Widget {\n    param from_p2 : Real = 5.0\n}\n",
                ),
            ]),
            entry: READ_WIDGET,
            winner_probe: "from_p1",
            winner_value: || Value::Real(2.0),
            loser_probe: "from_p2",
            loser_error: "has no member",
        },
        ParityRow {
            name: "(c) template, import tier beats the stdlib",
            setup: Setup::Project(&[(
                "a",
                "pub structure def Color {\n    param zz : Real = 3.0\n}\n",
            )]),
            entry: "import a\n\nstructure def Main {\n    sub col = Color()\n    let read = col.PROBE\n}\n",
            winner_probe: "zz",
            winner_value: || Value::Real(3.0),
            loser_probe: "r",
            loser_error: "has no member",
        },
        ParityRow {
            name: "(d) template, local beats the stdlib",
            setup: Setup::SingleFileWithStdlib,
            entry: "structure def Color {\n    param zz : Real = 3.0\n}\n\nstructure def Main {\n    sub col = Color()\n    let read = col.PROBE\n}\n",
            winner_probe: "zz",
            winner_value: || Value::Real(3.0),
            loser_probe: "r",
            loser_error: "has no member",
        },
        ParityRow {
            // The std module comes FIRST in the slice, so only a tiered rule
            // binds `b` — on the eval side as well as the compile side.
            name: "(e) template, mixed prelude: import tier beats an earlier std module",
            setup: Setup::SyntheticPrelude(&[
                (
                    "std.p1",
                    "pub structure def Widget {\n    param from_std : Real = 7.0\n}\n",
                ),
                (
                    "b",
                    "pub structure def Widget {\n    param from_b : Real = 2.0\n}\n",
                ),
            ]),
            entry: READ_WIDGET,
            winner_probe: "from_b",
            winner_value: || Value::Real(2.0),
            loser_probe: "from_std",
            loser_error: "has no member",
        },
    ]
}

/// A compiled entry plus the `Engine` that evaluates it under the row's prelude.
struct Compiled {
    module: CompiledModule,
    engine: Engine,
}

fn compile_row(row: &ParityRow, probe: &str) -> Compiled {
    let source = row.entry.replace("PROBE", probe);
    match row.setup {
        Setup::SingleFileWithStdlib => Compiled {
            module: reify_test_support::compile_source_with_stdlib(&source),
            engine: reify_test_support::make_engine(),
        },
        Setup::Project(imports) => {
            let tmp = tempfile::tempdir().expect("tempdir");
            let dir = tmp.path();
            fs::write(dir.join("main.ri"), &source).expect("write main.ri");
            for (name, import_source) in imports {
                fs::write(dir.join(format!("{name}.ri")), import_source).expect("write import");
            }
            let parsed = reify_compiler::parse_with_stdlib(&source, ModulePath::single("main"));
            assert!(
                parsed.errors.is_empty(),
                "[{}] entry parse errors: {:?}",
                row.name,
                parsed.errors
            );
            let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
            Compiled {
                module: compile_entry_with_stdlib_cfg(&parsed, &resolver, &CfgSet::default()),
                engine: reify_test_support::make_engine(),
            }
        }
        Setup::SyntheticPrelude(modules) => {
            let prelude = synthetic_prelude(row.name, modules);
            let parsed = reify_syntax::parse(&source, ModulePath::single("main"));
            assert!(
                parsed.errors.is_empty(),
                "[{}] entry parse errors: {:?}",
                row.name,
                parsed.errors
            );
            Compiled {
                module: reify_compiler::compile_with_prelude(&parsed, prelude),
                engine: Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, prelude),
            }
        }
    }
}

/// Compile each module standalone under its own path. Leaked because
/// `Engine::with_prelude` takes a `'static` prelude.
fn synthetic_prelude(row: &str, modules: &[(&str, &str)]) -> &'static [CompiledModule] {
    let compiled: Vec<CompiledModule> = modules
        .iter()
        .map(|(dotted, source)| {
            let path = ModulePath::from_dotted(dotted).expect("valid dotted path");
            let parsed = reify_syntax::parse(source, path.clone());
            assert!(
                parsed.errors.is_empty(),
                "[{row}] {dotted} parse errors: {:?}",
                parsed.errors
            );
            let mut module = reify_compiler::compile(&parsed);
            let errors = errors_of(&module.diagnostics);
            assert!(
                errors.is_empty(),
                "[{row}] {dotted} compile errors: {errors:?}"
            );
            module.path = path;
            module
        })
        .collect();
    Box::leak(compiled.into_boxed_slice())
}

fn errors_of(diagnostics: &[Diagnostic]) -> Vec<&str> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

/// Every way `row` breaks parity, or an empty list.
fn parity_failures(row: &ParityRow) -> Vec<String> {
    let mut failures = Vec::new();

    let winner = compile_row(row, row.winner_probe);
    let errors = errors_of(&winner.module.diagnostics);
    if !errors.is_empty() {
        failures.push(format!(
            "[{}] compile side: reading the winner's `{}` must compile clean, got {errors:?}",
            row.name, row.winner_probe
        ));
    }

    let loser = compile_row(row, row.loser_probe);
    let loser_errors = errors_of(&loser.module.diagnostics);
    if !loser_errors.iter().any(|m| m.contains(row.loser_error)) {
        failures.push(format!(
            "[{}] compile side: reading the loser's `{}` must raise an Error containing {:?}, got {loser_errors:?}",
            row.name, row.loser_probe, row.loser_error
        ));
    }

    let Compiled { module, mut engine } = winner;
    let result = engine.eval(&module);
    let eval_errors = errors_of(&result.diagnostics);
    if !eval_errors.is_empty() {
        failures.push(format!("[{}] eval side: errors {eval_errors:?}", row.name));
    }
    match result
        .values
        .get(&reify_core::ValueCellId::new("Main", "read"))
    {
        Some(value) if *value == (row.winner_value)() => {}
        other => failures.push(format!(
            "[{}] eval side: Main.read must be the winner's {:?}, got {other:?}",
            row.name,
            (row.winner_value)()
        )),
    }
    failures
}

#[test]
fn compile_and_eval_bind_the_same_definition_for_every_fixture() {
    let failures: Vec<String> = rows().iter().flat_map(parity_failures).collect();
    assert!(
        failures.is_empty(),
        "NS-P4 parity broken:\n{}",
        failures.join("\n")
    );
}
