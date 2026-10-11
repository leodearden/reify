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

use reify_ast::{Declaration, ParsedModule};
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
    /// followed imports); production `Engine`. See [`parse_project_entry`]
    /// for how the entry is parsed.
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
        ParityRow {
            // Boundary #2: std.ports.electrical declares
            // `enum SignalKind { Analog, Digital, PWM, Differential }`. The local
            // enum shadows it ENTIRELY, so a stdlib-only variant is unknown.
            name: "(f) enum, local SignalKind beats the stdlib's",
            setup: Setup::SingleFileWithStdlib,
            entry: "enum SignalKind { Foo, Bar }\n\nstructure def Main {\n    let read = SignalKind.PROBE\n}\n",
            winner_probe: "Foo",
            winner_value: || enum_value("SignalKind", "Foo"),
            loser_probe: "Analog",
            loser_error: "unknown variant",
        },
        ParityRow {
            name: "(g) enum, import tier beats the stdlib",
            setup: Setup::Project(&[("a", "pub enum SignalKind { Foo }\n")]),
            entry: "import a\n\nstructure def Main {\n    let read = SignalKind.PROBE\n}\n",
            winner_probe: "Foo",
            winner_value: || enum_value("SignalKind", "Foo"),
            loser_probe: "Analog",
            loser_error: "unknown variant",
        },
        ParityRow {
            name: "(h) enum, user-import peers: first import wins",
            setup: Setup::Project(&[
                ("a", "pub enum Pick { Left }\n"),
                ("b", "pub enum Pick { Right }\n"),
            ]),
            entry: "import a\nimport b\n\nstructure def Main {\n    let read = Pick.PROBE\n}\n",
            winner_probe: "Left",
            winner_value: || enum_value("Pick", "Left"),
            loser_probe: "Right",
            loser_error: "unknown variant",
        },
    ]
}

fn enum_value(type_name: &str, variant: &str) -> Value {
    Value::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        payload: vec![],
    }
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
        Setup::Project(imports) => Compiled {
            module: compile_project(row.name, &source, imports),
            engine: reify_test_support::make_engine(),
        },
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

/// Write `entry` as `main.ri` plus its imports into a tempdir and compile it
/// the way `reify check` does (prelude = stdlib ++ followed imports).
fn compile_project(row: &str, entry: &str, imports: &[(&str, &str)]) -> CompiledModule {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("main.ri"), entry).expect("write main.ri");
    for (name, import_source) in imports {
        fs::write(dir.join(format!("{name}.ri")), import_source).expect("write import");
    }
    let parsed = parse_project_entry(entry, imports);
    assert!(
        parsed.errors.is_empty(),
        "[{row}] entry parse errors: {:?}",
        parsed.errors
    );
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    compile_entry_with_stdlib_cfg(&parsed, &resolver, &CfgSet::default())
}

/// Parse a project entry knowing the enum names of the stdlib AND of its
/// imports. Production entry parsing (`parse_with_stdlib`) knows only the
/// stdlib's, so an imported enum's `Pick.Left` would not parse as an enum
/// access at all — a parser gap orthogonal to which definition binds, which is
/// what these rows probe.
fn parse_project_entry(source: &str, imports: &[(&str, &str)]) -> ParsedModule {
    let mut enum_names: Vec<&'static str> = reify_compiler::stdlib_loader::load_stdlib_context()
        .enum_names()
        .collect();
    for (name, import_source) in imports {
        let parsed = reify_syntax::parse(import_source, ModulePath::single(*name));
        enum_names.extend(parsed.declarations.iter().filter_map(|decl| match decl {
            Declaration::Enum(e) => Some(&*Box::leak(e.name.clone().into_boxed_str())),
            _ => None,
        }));
    }
    reify_syntax::parse_with_prelude_enums(source, ModulePath::single("main"), &enum_names)
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

/// The compile-side half of every row: the winner's probe compiles clean and
/// the loser's is rejected.
struct CompileSide<'a> {
    row: &'a str,
    winner_probe: &'a str,
    winner: &'a CompiledModule,
    loser_probe: &'a str,
    loser: &'a CompiledModule,
    loser_error: &'a str,
}

fn compile_side_failures(side: CompileSide<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let errors = errors_of(&side.winner.diagnostics);
    if !errors.is_empty() {
        failures.push(format!(
            "[{}] compile side: the winner's probe `{}` must compile clean, got {errors:?}",
            side.row, side.winner_probe
        ));
    }
    let loser_errors = errors_of(&side.loser.diagnostics);
    if !loser_errors.iter().any(|m| m.contains(side.loser_error)) {
        failures.push(format!(
            "[{}] compile side: the loser's probe `{}` must raise an Error containing {:?}, got {loser_errors:?}",
            side.row, side.loser_probe, side.loser_error
        ));
    }
    failures
}

/// Every way `row` breaks parity, or an empty list.
fn parity_failures(row: &ParityRow) -> Vec<String> {
    let winner = compile_row(row, row.winner_probe);
    let loser = compile_row(row, row.loser_probe);
    let mut failures = compile_side_failures(CompileSide {
        row: row.name,
        winner_probe: row.winner_probe,
        winner: &winner.module,
        loser_probe: row.loser_probe,
        loser: &loser.module,
        loser_error: row.loser_error,
    });

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

/// The user-visible boundary #2 symptom, pinned directly: a local enum named
/// like a stdlib enum must not have its own variants rejected.
#[test]
fn local_enum_variants_resolve_despite_a_same_named_stdlib_enum() {
    let compiled = reify_test_support::compile_source_with_stdlib(
        "enum SignalKind { Foo, Bar }\n\nstructure def Main {\n    let foo = SignalKind.Foo\n    let bar = SignalKind.Bar\n}\n",
    );
    let errors = errors_of(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "the local SignalKind must bind, not std.ports.electrical's; got {errors:?}"
    );
}

// ─── NS-P3: the remaining kinds, compile side ──────────────────────────────

/// A compile-side-only row for a kind the evaluator never looks up by name.
/// Always a project (prelude = stdlib ++ imports); `PROBE` as in [`ParityRow`].
struct KindRow {
    name: &'static str,
    imports: &'static [(&'static str, &'static str)],
    entry: &'static str,
    winner_probe: &'static str,
    loser_probe: &'static str,
    loser_error: &'static str,
}

fn kind_rows() -> Vec<KindRow> {
    vec![
        KindRow {
            name: "trait, user-import peers: first import wins",
            imports: &[
                ("a", "pub trait Shape {\n    param ax : Real\n}\n"),
                ("b", "pub trait Shape {\n    param bx : Real\n}\n"),
            ],
            entry: "import a\nimport b\n\nstructure def S : Shape {\n    param PROBE : Real = 1.0\n}\n",
            winner_probe: "ax",
            loser_probe: "bx",
            loser_error: "missing required member 'ax'",
        },
        KindRow {
            // std.units declares `pub type Velocity = Length / Time`.
            name: "type alias, import tier beats the stdlib",
            imports: &[("a", "pub type Velocity = Length\n")],
            entry: "import a\n\nstructure def Main {\n    param v : Velocity = PROBE\n}\n",
            winner_probe: "3mm",
            loser_probe: "3mm / 1s",
            loser_error: "declared `Scalar[m]`",
        },
        KindRow {
            // std.dynamics declares `ramp_profile(Real, Real, Real, Real) ->
            // MotionTrajectory`; a's has the SAME param types, returns Real.
            name: "function, import tier beats the stdlib",
            imports: &[(
                "a",
                "pub fn ramp_profile(joint: Real, from: Real, to: Real, max_accel: Real) -> Real { joint }\n",
            )],
            entry: "import a\n\nstructure def Main {\n    param x : PROBE = ramp_profile(1.0, 2.0, 3.0, 4.0)\n}\n",
            winner_probe: "Real",
            loser_probe: "MotionTrajectory",
            loser_error: "evaluates to `Real`",
        },
        KindRow {
            // std.tolerancing declares `pub constraint def Conforms` with a
            // required `tolerance` param; a's takes only `t`.
            name: "constraint def, import tier beats the stdlib",
            imports: &[(
                "a",
                "pub constraint def Conforms {\n    param t : Length\n    t > 0mm\n}\n",
            )],
            entry: "import a\n\nstructure def Main {\n    param t : Length = 5mm\n    constraint Conforms(PROBE)\n}\n",
            winner_probe: "t: t",
            loser_probe: "tolerance: t",
            loser_error: "unknown argument 'tolerance'",
        },
    ]
}

fn kind_row_failures(row: &KindRow) -> Vec<String> {
    let compile =
        |probe: &str| compile_project(row.name, &row.entry.replace("PROBE", probe), row.imports);
    compile_side_failures(CompileSide {
        row: row.name,
        winner_probe: row.winner_probe,
        winner: &compile(row.winner_probe),
        loser_probe: row.loser_probe,
        loser: &compile(row.loser_probe),
        loser_error: row.loser_error,
    })
}

#[test]
fn every_kind_binds_the_import_tier_first_wins_on_the_compile_side() {
    let failures: Vec<String> = kind_rows().iter().flat_map(kind_row_failures).collect();
    assert!(
        failures.is_empty(),
        "NS-P3 broken:\n{}",
        failures.join("\n")
    );
}

/// Purposes have no reference site to probe: the observable is which
/// definition the entry's merged `compiled_purposes` carries. a's
/// `design_review` takes TWO params; std.determinacy.purposes' takes one.
#[test]
fn purposes_bind_the_import_tier_over_the_stdlib() {
    let module = compile_project(
        "purpose, import tier beats the stdlib",
        "import a\n\nstructure def Main {\n    param t : Length = 5mm\n}\n",
        &[(
            "a",
            "pub purpose design_review(subject : Structure, witness : Structure) {\n    constraint forall p in subject.params: determined(p)\n}\n",
        )],
    );
    let errors = errors_of(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "the project must compile clean, got {errors:?}"
    );
    let merged: Vec<usize> = module
        .compiled_purposes
        .iter()
        .filter(|p| p.name == "design_review")
        .map(|p| p.params.len())
        .collect();
    assert_eq!(
        merged,
        vec![2],
        "exactly one design_review is merged, and it is import a's two-param one"
    );
}
