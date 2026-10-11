//! D-2, N0 interim (stdlib-namespace §3.1 NS-P2): a user declaration that
//! shadows a stdlib definition of the SAME kind binds locally and draws exactly
//! one `StdlibNameShadowed` Warning, naming the kind, the name, the local
//! module and the shadowed std module. Boundary #2's Warning half.

use std::fs;

use reify_ast::{Declaration, ParsedModule};
use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::{ModuleResolver, compile_entry_with_stdlib_cfg};
use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity, SourceSpan};

const LOCAL_MODULE: &str = "acme.shop";

struct ShadowRow {
    kind: &'static str,
    name: &'static str,
    std_module: &'static str,
    source: &'static str,
    /// Which of the source's same-named declarations the Warning must point
    /// at, in source order.
    declaration_index: usize,
}

fn rows() -> Vec<ShadowRow> {
    vec![
        ShadowRow {
            // Boundary #2: the local variants resolve in the same compile.
            kind: "enum",
            name: "SignalKind",
            std_module: "std.ports.electrical",
            source: "enum SignalKind { Foo, Bar }\n\nstructure def Main {\n    let read = SignalKind.Foo\n}\n",
            declaration_index: 0,
        },
        ShadowRow {
            kind: "structure",
            name: "Color",
            std_module: "std.materials.appearance",
            source: "structure def Color {\n    param zz : Real = 3.0\n}\n",
            declaration_index: 0,
        },
        ShadowRow {
            kind: "trait",
            name: "Rigid",
            std_module: "std.structural.physical",
            source: "trait Rigid {\n    param k : Real\n}\n",
            declaration_index: 0,
        },
        ShadowRow {
            // Only the overload whose parameter types match std.dynamics'
            // `ramp_profile(Real, Real, Real, Real)` collides.
            kind: "function",
            name: "ramp_profile",
            std_module: "std.dynamics",
            source: "fn ramp_profile(joint: Real) -> Real { joint }\n\nfn ramp_profile(joint: Real, from: Real, to: Real, max_accel: Real) -> Real { joint }\n",
            declaration_index: 1,
        },
        ShadowRow {
            kind: "purpose",
            name: "design_review",
            std_module: "std.determinacy.purposes",
            source: "pub purpose design_review(subject : Structure) {\n    constraint forall p in subject.params: determined(p)\n}\n",
            declaration_index: 0,
        },
        ShadowRow {
            kind: "type alias",
            name: "ArealCostRate",
            std_module: "std.surface_finish",
            source: "type ArealCostRate = Length\n",
            declaration_index: 0,
        },
        ShadowRow {
            kind: "constraint def",
            name: "Conforms",
            std_module: "std.tolerancing",
            source: "constraint def Conforms {\n    param t : Length\n    t > 0mm\n}\n",
            declaration_index: 0,
        },
    ]
}

fn parse(source: &str) -> ParsedModule {
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        ModulePath::from_dotted(LOCAL_MODULE).expect("valid dotted path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    parsed
}

fn shadow_warnings(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StdlibNameShadowed))
        .collect()
}

fn errors_of(module: &CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

/// The spans of the top-level declarations named `name`, in source order.
fn declaration_spans(parsed: &ParsedModule, name: &str) -> Vec<SourceSpan> {
    parsed
        .declarations
        .iter()
        .filter_map(|decl| match decl {
            Declaration::Structure(d) => Some((&d.name, d.span)),
            Declaration::Occurrence(d) => Some((&d.name, d.span)),
            Declaration::Enum(d) => Some((&d.name, d.span)),
            Declaration::Function(d) => Some((&d.name, d.span)),
            Declaration::Trait(d) => Some((&d.name, d.span)),
            Declaration::Purpose(d) => Some((&d.name, d.span)),
            Declaration::Constraint(d) => Some((&d.name, d.span)),
            Declaration::TypeAlias(d) => Some((&d.name, d.span)),
            _ => None,
        })
        .filter(|(decl_name, _)| *decl_name == name)
        .map(|(_, span)| span)
        .collect()
}

fn row_failures(row: &ShadowRow) -> Vec<String> {
    let label = format!("[{} {}]", row.kind, row.name);
    let parsed = parse(row.source);
    let module = reify_compiler::compile_with_stdlib(&parsed);
    let mut failures = Vec::new();

    let errors = errors_of(&module);
    if !errors.is_empty() {
        failures.push(format!(
            "{label} the local definition must bind cleanly, got {errors:?}"
        ));
    }

    let warnings = shadow_warnings(&module);
    let [warning] = warnings.as_slice() else {
        failures.push(format!(
            "{label} expected exactly one StdlibNameShadowed, got {:?}",
            warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
        ));
        return failures;
    };
    if warning.severity != Severity::Warning {
        failures.push(format!(
            "{label} severity must be Warning, got {}",
            warning.severity
        ));
    }
    for needle in [row.kind, row.name, row.std_module, LOCAL_MODULE] {
        if !warning.message.contains(needle) {
            failures.push(format!(
                "{label} message must name {needle:?}: {:?}",
                warning.message
            ));
        }
    }
    let expected_span = declaration_spans(&parsed, row.name)
        .get(row.declaration_index)
        .copied();
    match warning.labels.first() {
        Some(primary) if Some(primary.span) == expected_span => {}
        other => failures.push(format!(
            "{label} the primary label must sit on the local declaration {expected_span:?}, got {other:?}"
        )),
    }
    if !warning.labels.iter().skip(1).any(|l| l.span.is_prelude()) {
        failures.push(format!(
            "{label} a secondary label must point into the prelude, got {:?}",
            warning.labels
        ));
    }
    failures
}

#[test]
fn a_local_definition_shadowing_the_stdlib_warns_once_for_every_kind() {
    let failures: Vec<String> = rows().iter().flat_map(row_failures).collect();
    assert!(
        failures.is_empty(),
        "D-2 Warning form broken:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_shadowed_name_warns_once_however_often_it_is_referenced() {
    let module = reify_compiler::compile_with_stdlib(&parse(
        "structure def Color {\n    param zz : Real = 3.0\n}\n\nstructure def Main {\n    sub a = Color()\n    sub b = Color()\n    sub c = Color()\n    param d : Color\n    let read = a.zz + b.zz + c.zz\n}\n",
    ));
    assert_eq!(errors_of(&module), Vec::<&str>::new());
    assert_eq!(
        shadow_warnings(&module).len(),
        1,
        "one Warning per shadowed (kind, name), got {:?}",
        module.diagnostics
    );
}

// ─── Negative rows: no StdlibNameShadowed ───────────────────────────────────

/// The only shadowing definition comes from a user import (the import tier):
/// entry-vs-import and import-vs-import dispositions are resolution-unification
/// D-6's, not this lint's.
#[test]
fn an_import_shadowing_the_stdlib_does_not_warn_in_the_entry() {
    let entry =
        "import a\n\nstructure def Main {\n    sub col = Color()\n    let read = col.zz\n}\n";
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("main.ri"), entry).expect("write main.ri");
    fs::write(
        dir.join("a.ri"),
        "pub structure def Color {\n    param zz : Real = 3.0\n}\n",
    )
    .expect("write a.ri");
    let parsed = reify_compiler::parse_with_stdlib(entry, ModulePath::single("main"));
    let resolver = ModuleResolver::new(dir, dir.join("stdlib"));
    let module = compile_entry_with_stdlib_cfg(&parsed, &resolver, &CfgSet::default());
    assert_eq!(errors_of(&module), Vec::<&str>::new());
    assert!(
        shadow_warnings(&module).is_empty(),
        "an import-tier winner is not a local shadowing: {:?}",
        shadow_warnings(&module)
    );
}

#[test]
fn a_no_prelude_module_does_not_warn() {
    let module = reify_compiler::compile_with_stdlib(&parse(
        "#no_prelude\nenum SignalKind { Foo, Bar }\n\nstructure def Main {\n    let read = SignalKind.Foo\n}\n",
    ));
    assert_eq!(errors_of(&module), Vec::<&str>::new());
    assert!(shadow_warnings(&module).is_empty());
}

#[test]
fn a_non_colliding_name_does_not_warn() {
    let module = reify_compiler::compile_with_stdlib(&parse(
        "enum Gadget { Foo }\n\nstructure def Main {\n    let read = Gadget.Foo\n}\n",
    ));
    assert_eq!(errors_of(&module), Vec::<&str>::new());
    assert!(shadow_warnings(&module).is_empty());
}

/// Units keep their own duplicate-declaration Error until resolution-unification
/// θ #5522 moves them onto the shared rule.
#[test]
fn a_unit_duplicating_a_stdlib_unit_keeps_its_error_and_does_not_warn() {
    let module = reify_compiler::compile_with_stdlib(&parse("unit thou : Length = 0.0000254\n"));
    assert!(
        errors_of(&module)
            .iter()
            .any(|m| m.contains("duplicate unit declaration 'thou'")),
        "the duplicate-unit Error must still fire, got {:?}",
        module.diagnostics
    );
    assert!(shadow_warnings(&module).is_empty());
}
