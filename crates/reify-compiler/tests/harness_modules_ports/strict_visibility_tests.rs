//! Strict stdlib visibility (stdlib-namespace κ, task #5503; PRD
//! `docs/prds/v0_6/stdlib-namespace.md` NS-V1/NS-V2): a module sees builtins
//! plus the std modules it imports, and a std name it writes without importing
//! its module draws a coded fix-it Error naming the import that brings it in.
//!
//! The fix-it builder is driven over the REAL stdlib: each module is compiled
//! against exactly the std modules its imports make visible (each import and
//! what it re-exports through `pub import`), as the strict program compile
//! does. The program sections drive `program::compile_program` itself.

use reify_ast::{Declaration, ParsedModule};
use reify_compiler::CompiledModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::import_headers::std_import_fixits;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity};
use reify_ir::{CompiledExprKind, Satisfaction, Value};
use reify_test_support::mocks::MockConstraintChecker;

const SUBJECT: &str = "subject";

const GPA_BODY: &str = "structure def S {\n    param e : Pressure = 200GPa\n}\n";

fn linux() -> CfgSet {
    CfgSet {
        target: Some("linux".to_string()),
        ..CfgSet::default()
    }
}

fn parse_clean(source: &str, path: &str) -> ParsedModule {
    let parsed = reify_compiler::parse_with_stdlib(
        source,
        ModulePath::from_dotted(path).expect("valid dotted fixture path"),
    );
    assert!(
        parsed.errors.is_empty(),
        "fixture `{path}` must parse cleanly: {:?}\nsource:\n{source}",
        parsed.errors
    );
    parsed
}

/// The stdlib module `path`.
fn std_module(path: &str) -> &'static CompiledModule {
    load_stdlib()
        .iter()
        .find(|module| module.path.to_dotted() == path)
        .unwrap_or_else(|| panic!("no stdlib module {path}"))
}

/// `roots` followed, depth-first, by every std module they re-export, once each.
fn std_tier(roots: &[&str]) -> Vec<&'static CompiledModule> {
    fn visit(path: &str, tier: &mut Vec<&'static CompiledModule>) {
        let module = std_module(path);
        if tier.iter().any(|seen| std::ptr::eq(*seen, module)) {
            return;
        }
        tier.push(module);
        for import in module.imports.iter().filter(|import| import.is_pub) {
            visit(&import.path, tier);
        }
    }
    let mut tier = Vec::new();
    for root in roots {
        visit(root, &mut tier);
    }
    tier
}

/// The fix-its for `source`, compiled against the std tier of `std_imports`
/// plus `user_imports`, under `cfg`.
fn fixits_with(
    source: &str,
    std_imports: &[&str],
    user_imports: &[&CompiledModule],
    cfg: &CfgSet,
) -> Vec<Diagnostic> {
    let parsed = parse_clean(source, SUBJECT);
    let mut tier: Vec<&CompiledModule> = std_tier(std_imports);
    tier.extend(user_imports.iter().copied());
    let tier_owned: Vec<CompiledModule> = tier.iter().map(|module| (*module).clone()).collect();
    let local = reify_compiler::compile_with_prelude(&parsed, &tier_owned);
    std_import_fixits(&parsed, &local, &tier, cfg)
}

fn fixits(source: &str, std_imports: &[&str]) -> Vec<Diagnostic> {
    fixits_with(source, std_imports, &[], &linux())
}

#[track_caller]
fn assert_no_fixit(diagnostics: &[Diagnostic], context: &str) {
    assert!(
        diagnostics.is_empty(),
        "{context}: expected no fix-it: {diagnostics:#?}"
    );
}

fn only_fixit(diagnostics: &[Diagnostic]) -> &Diagnostic {
    assert_eq!(
        diagnostics.len(),
        1,
        "expected exactly one fix-it: {diagnostics:#?}"
    );
    let fixit = &diagnostics[0];
    assert_eq!(fixit.severity, Severity::Error, "{fixit:#?}");
    assert_eq!(
        fixit.code,
        Some(DiagnosticCode::StdNameNotImported),
        "{fixit:#?}"
    );
    fixit
}

#[test]
fn an_unimported_si_unit_gets_one_fixit_naming_its_module_and_the_prelude() {
    let diagnostics = fixits(GPA_BODY, &[]);
    let fixit = only_fixit(&diagnostics);
    for needle in ["GPa", "std.si_units", "std.prelude"] {
        assert!(
            fixit.message.contains(needle),
            "message must name `{needle}`: {}",
            fixit.message
        );
    }
    assert_eq!(fixit.candidates, vec!["std.si_units", "std.prelude"]);
    let label = fixit.labels.first().expect("a primary label");
    assert_eq!(
        &GPA_BODY[label.span.start as usize..label.span.end as usize],
        "200GPa",
        "the primary label marks the name's first written occurrence"
    );
}

#[test]
fn a_name_the_prelude_does_not_re_export_offers_only_its_module() {
    let source = "structure def S {\n    param m : ElasticMaterial = ABS_Plastic()\n}\n";
    let diagnostics = fixits(source, &[]);
    assert_eq!(
        diagnostics.len(),
        2,
        "one fix-it per name (ElasticMaterial, ABS_Plastic): {diagnostics:#?}"
    );
    for fixit in &diagnostics {
        assert_eq!(fixit.code, Some(DiagnosticCode::StdNameNotImported));
        assert_eq!(fixit.candidates, vec!["std.materials.fea"]);
        assert!(
            !fixit.message.contains("std.prelude"),
            "std.prelude re-exports no candidate: {}",
            fixit.message
        );
    }
}

#[test]
fn a_name_two_std_modules_define_lists_both_and_the_prelude() {
    let source =
        "structure def S {\n    let w : Length = unwrap_or(parse_length(\"2mm\"), 6mm)\n}\n";
    let diagnostics = fixits(source, &[]);
    let fixit = only_fixit(&diagnostics);
    assert_eq!(
        fixit.candidates,
        vec!["std.option_recovery", "std.result", "std.prelude"]
    );
    for needle in [
        "unwrap_or",
        "std.option_recovery",
        "std.result",
        "std.prelude",
    ] {
        assert!(
            fixit.message.contains(needle),
            "message must name `{needle}`: {}",
            fixit.message
        );
    }
}

#[test]
fn builtins_need_no_import() {
    let source = "structure def S {\n    param w : Length = 10mm\n    param m : Mass = 2kg\n}\n";
    assert_no_fixit(&fixits(source, &[]), "builtins");
}

#[test]
fn an_import_of_the_module_or_of_the_prelude_covers_the_name() {
    for import in ["std.si_units", "std.prelude"] {
        let source = format!("import {import}\n\n{GPA_BODY}");
        assert_no_fixit(&fixits(&source, &[import]), &format!("import {import}"));
    }
}

#[test]
fn a_name_the_module_declares_itself_needs_no_import() {
    let source = "structure def ABS_Plastic {\n    param x : Length = 1mm\n}\n\
                  structure def S {\n    let m = ABS_Plastic()\n}\n";
    assert_no_fixit(&fixits(source, &[]), "a local definition");
}

#[test]
fn a_name_a_user_import_provides_needs_no_import() {
    let parts = reify_compiler::compile_with_prelude(
        &parse_clean(
            "module parts\n\npub structure def ABS_Plastic {\n    param x : Length = 1mm\n}\n",
            "parts",
        ),
        &[],
    );
    let source = "import parts\n\nstructure def S {\n    let m = ABS_Plastic()\n}\n";
    assert_no_fixit(
        &fixits_with(source, &[], &[&parts], &linux()),
        "a user-import definition",
    );
}

/// `Part` is a std name (std.modal.analysis); a user import that should
/// define it but is absent from the compiled tier must not draw advice to
/// import the std module.
const RIG_USING_PART: &str = "structure def Rig {\n    sub p = Part()\n}\n";

#[test]
fn a_user_import_missing_from_the_tier_suppresses_std_attribution() {
    let source = format!("import parts\n\n{RIG_USING_PART}");
    assert_no_fixit(
        &fixits_with(&source, &[], &[], &linux()),
        "an absent user import",
    );
}

#[test]
fn a_missing_user_import_file_draws_no_std_fixit_in_a_program() {
    let program = compile_main(
        &format!("module main\n\nimport partz\n\n{RIG_USING_PART}"),
        &linux(),
    );
    assert!(
        std_name_fixits(program.entry()).is_empty(),
        "no std fix-it for a name the missing import may define: {:#?}",
        program.entry().diagnostics
    );
}

#[test]
fn a_failed_user_import_draws_no_std_fixit_on_the_entry() {
    for parts_source in [
        "module parts\n\nimport std\n\npub structure def Part {\n    param q : Real = 1\n}\n",
        "module parts\n\nimport std.nonexistent\n\npub structure def Part {\n    param q : Real = 1\n}\n",
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("parts.ri"), parts_source).expect("write parts.ri");
        let program = compile_program(
            &dir.path().join("main.ri"),
            &format!("module main\n\nimport parts\n\n{RIG_USING_PART}"),
            &ModuleResolver::new(dir.path(), dir.path().join("stdlib")),
            &linux(),
            &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
        );
        assert!(
            std_name_fixits(program.entry()).is_empty(),
            "no std fix-it for `Part` when `parts` fails ({parts_source}): {:#?}",
            program.entry().diagnostics
        );
    }
}

#[test]
fn a_detached_entry_with_an_unfollowed_user_import_draws_no_std_fixit() {
    let parsed = parse_clean(&format!("import parts\n\n{RIG_USING_PART}"), SUBJECT);
    let program = reify_compiler::program::compile_detached_entry(
        &parsed,
        &linux(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );
    assert!(
        std_name_fixits(program.entry()).is_empty(),
        "no std fix-it for a name the unfollowed import may define: {:#?}",
        program.entry().diagnostics
    );
}

#[test]
fn a_gated_off_import_of_the_module_is_named_as_out_of_effect() {
    let source = format!("#cfg(target = \"wasm\")\nimport std.si_units\n\n{GPA_BODY}");
    let diagnostics = fixits(&source, &[]);
    let fixit = only_fixit(&diagnostics);
    assert!(
        fixit.message.contains("not in effect under the active cfg"),
        "the gated-off import is named: {}",
        fixit.message
    );
}

// ─── the strict program compile (boundaries #12, #14) ──────────────────────

fn compile_main(source: &str, cfg: &CfgSet) -> CompiledProgram {
    let dir = tempfile::tempdir().expect("tempdir");
    compile_program(
        &dir.path().join("main.ri"),
        source,
        &ModuleResolver::new(dir.path(), dir.path().join("stdlib")),
        cfg,
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    )
}

fn error_diagnostics(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn std_name_fixits(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StdNameNotImported))
        .collect()
}

const MAIN_GPA: &str = "module main\n\nstructure def S {\n    param e : Pressure = 200GPa\n}\n";

#[test]
fn an_entry_writing_an_unimported_std_unit_gets_the_fixit_error() {
    let program = compile_main(MAIN_GPA, &linux());
    let fixits = std_name_fixits(program.entry());
    assert_eq!(fixits.len(), 1, "{:#?}", program.entry().diagnostics);
    assert_eq!(fixits[0].severity, Severity::Error);
    for needle in ["std.si_units", "std.prelude"] {
        assert!(fixits[0].message.contains(needle), "{}", fixits[0].message);
    }
}

#[test]
fn importing_the_unit_module_or_the_prelude_compiles_the_entry_clean() {
    for import in ["std.si_units", "std.prelude"] {
        let source = format!(
            "module main\n\nimport {import}\n\nstructure def S {{\n    param e : Pressure = 200GPa\n}}\n"
        );
        let program = compile_main(&source, &linux());
        assert!(
            error_diagnostics(program.entry()).is_empty(),
            "import {import}: {:#?}",
            program.entry().diagnostics
        );
    }
}

/// The compile builds a unit's tier one whole module at a time, so an
/// entity-form std import makes the module's whole pub surface visible; the
/// fix-it must agree and stay silent on a name the compile resolved.
#[test]
fn an_entity_form_std_import_makes_the_whole_module_visible_to_the_fixit() {
    let sources = [
        "module main\n\nimport std.si_units.{MPa}\n\nstructure def S {\n    param e : Pressure = 200GPa\n}\n",
        "module main\n\nimport std.materials.fea.{ElasticMaterial}\n\nstructure def S {\n    param material : ElasticMaterial = ABS_Plastic()\n}\n",
    ];
    for source in sources {
        let program = compile_main(source, &linux());
        assert!(
            std_name_fixits(program.entry()).is_empty(),
            "{source}: {:#?}",
            program.entry().diagnostics
        );
        assert!(
            error_diagnostics(program.entry()).is_empty(),
            "{source}: {:#?}",
            program.entry().diagnostics
        );
    }
}

#[test]
fn builtin_dimensions_and_units_need_no_import_in_a_program() {
    let program = compile_main(
        "module main\n\nstructure def S {\n    param w : Length = 10mm\n}\n",
        &linux(),
    );
    assert!(
        error_diagnostics(program.entry()).is_empty(),
        "{:#?}",
        program.entry().diagnostics
    );
    assert!(std_name_fixits(program.entry()).is_empty());
}

#[test]
fn a_gated_unit_import_counts_only_under_the_cfg_that_satisfies_it() {
    let source = "module main\n\n#cfg(target = \"wasm\")\nimport std.si_units\n\n\
                  structure def S {\n    param e : Pressure = 200GPa\n}\n";
    let under_linux = compile_main(source, &linux());
    assert_eq!(
        std_name_fixits(under_linux.entry()).len(),
        1,
        "{:#?}",
        under_linux.entry().diagnostics
    );
    let wasm = CfgSet {
        target: Some("wasm".to_string()),
        ..CfgSet::default()
    };
    let under_wasm = compile_main(source, &wasm);
    assert!(
        error_diagnostics(under_wasm.entry()).is_empty(),
        "{:#?}",
        under_wasm.entry().diagnostics
    );
}

#[test]
fn a_healthy_prelude_program_carries_nothing_re_homed_from_a_std_module() {
    let source = "module main\n\nimport std.prelude\n\nstructure def S {\n    \
                  param e : Pressure = 200GPa\n    \
                  let w : Length = unwrap_or(parse_length(\"2mm\"), 6mm)\n}\n";
    let program = compile_main(source, &linux());
    assert!(
        error_diagnostics(program.entry()).is_empty(),
        "{:#?}",
        program.entry().diagnostics
    );
    let std_keys: Vec<&str> = program
        .imports
        .iter()
        .map(|unit| unit.import_path.as_str())
        .filter(|key| key.starts_with("std."))
        .collect();
    assert!(
        std_keys.contains(&"std.prelude") && std_keys.contains(&"std.si_units"),
        "the import tier lists the prelude and what it re-exports: {std_keys:?}"
    );
    for key in std_keys {
        assert_eq!(
            program.rehomed_from(key).count(),
            0,
            "nothing is re-homed from {key}"
        );
    }
    for diagnostic in &program.entry().diagnostics {
        assert!(
            !diagnostic.message.contains("in imported module 'std."),
            "{diagnostic:#?}"
        );
    }
}

// ─── bare `import std` is refused (§10 Q3) ─────────────────────────────────

fn bare_std_refusals(module: &CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::BareStdImport))
        .collect()
}

#[track_caller]
fn only_bare_std_refusal(program: &CompiledProgram) -> &Diagnostic {
    let refusals = bare_std_refusals(program.entry());
    assert_eq!(
        refusals.len(),
        1,
        "exactly one bare-std refusal on the entry: {:#?}",
        program.entry().diagnostics
    );
    let refusal = refusals[0];
    assert_eq!(refusal.severity, Severity::Error, "{refusal:#?}");
    assert!(
        refusal.message.contains("import std.prelude"),
        "the refusal points at the prelude facade: {}",
        refusal.message
    );
    assert_eq!(refusal.candidates, vec!["std.prelude"]);
    refusal
}

#[test]
fn an_entry_importing_bare_std_is_refused_and_still_compiles_its_own_declarations() {
    let program = compile_main(
        "module main\n\nimport std\n\nstructure def S {\n    param w : Length = 10mm\n}\n",
        &linux(),
    );
    only_bare_std_refusal(&program);
    assert!(
        program.entry().templates.iter().any(|t| t.name == "S"),
        "nothing short-circuits: the entry's own `S` still compiles (I2)"
    );
}

#[test]
fn an_import_unit_importing_bare_std_is_refused_on_the_entry_naming_the_units_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let parts = dir.path().join("parts.ri");
    std::fs::write(
        &parts,
        "module parts\n\nimport std\n\npub structure def Part {\n    param q : Real = 1\n}\n",
    )
    .expect("write parts.ri");
    let program = compile_program(
        &dir.path().join("main.ri"),
        "module main\n\nimport parts\n\nstructure def Rig {\n    sub p = Part()\n}\n",
        &ModuleResolver::new(dir.path(), dir.path().join("stdlib")),
        &linux(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );
    let refusal = only_bare_std_refusal(&program);
    let parts_file = parts.display().to_string();
    assert!(
        refusal.message.contains(&parts_file),
        "the refusal names the importing unit's file {parts_file}: {}",
        refusal.message
    );
}

#[test]
fn a_cfg_gated_bare_std_import_is_inert() {
    let program = compile_main(
        "module main\n\n#cfg(target = \"wasm\")\nimport std\n\n\
         structure def S {\n    param w : Length = 10mm\n}\n",
        &linux(),
    );
    assert!(
        bare_std_refusals(program.entry()).is_empty(),
        "a gated-out `import std` never reaches the gate (I3): {:#?}",
        program.entry().diagnostics
    );
    assert!(
        error_diagnostics(program.entry()).is_empty(),
        "{:#?}",
        program.entry().diagnostics
    );
}

// ─── boundary #2, Error form: a declared std import shadowed locally (D-2) ──

const SHADOWS_SIGNAL_KIND: &str = "import std.ports.electrical\n\n\
                                   enum SignalKind { Foo, Bar }\n\n\
                                   structure def Main {\n    let read = SignalKind.Foo\n}\n";

fn shadowings(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::StdlibNameShadowed))
        .collect()
}

#[test]
fn an_entry_shadowing_a_std_name_it_imports_binds_locally_with_an_error() {
    let source = format!("module main\n\n{SHADOWS_SIGNAL_KIND}");
    let program = compile_main(&source, &linux());
    let entry = program.entry();

    let shadows = shadowings(&entry.diagnostics);
    let [shadow] = shadows.as_slice() else {
        panic!("exactly one shadowing: {:#?}", entry.diagnostics);
    };
    assert_eq!(shadow.severity, Severity::Error, "{shadow:#?}");
    let local_enum = parse_clean(&source, "main")
        .declarations
        .iter()
        .find_map(|decl| match decl {
            Declaration::Enum(e) if e.name == "SignalKind" => Some(e.span),
            _ => None,
        })
        .expect("the local enum declaration");
    assert_eq!(
        shadow.labels.first().map(|label| label.span),
        Some(local_enum),
        "labelled at the local declaration: {shadow:#?}"
    );
    assert_eq!(
        error_diagnostics(entry).len(),
        1,
        "the shadowing is the only Error: {:#?}",
        entry.diagnostics
    );

    let read = entry
        .templates
        .iter()
        .find(|t| t.name == "Main")
        .and_then(|t| t.value_cells.iter().find(|cell| cell.id.member == "read"))
        .and_then(|cell| cell.default_expr.as_ref())
        .expect("Main.read has a compiled default");
    assert!(
        matches!(
            &read.kind,
            CompiledExprKind::Literal(Value::Enum { type_name, variant, .. })
                if type_name == "SignalKind" && variant == "Foo"
        ),
        "the local enum wins: {read:?}"
    );
}

#[test]
fn an_import_unit_shadowing_a_std_name_it_imports_surfaces_the_error_rehomed() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("parts.ri"),
        format!("module parts\n\n{SHADOWS_SIGNAL_KIND}"),
    )
    .expect("write parts.ri");
    let program = compile_program(
        &dir.path().join("main.ri"),
        "module main\n\nimport parts\n\nstructure def Rig {\n    param q : Real = 1\n}\n",
        &ModuleResolver::new(dir.path(), dir.path().join("stdlib")),
        &linux(),
        &MockConstraintChecker::new().with_default(Satisfaction::Indeterminate),
    );

    let rehomed: Vec<&Diagnostic> = program
        .rehomed_from("parts")
        .filter(|d| d.code == Some(DiagnosticCode::StdlibNameShadowed))
        .collect();
    let [shadow] = rehomed.as_slice() else {
        panic!(
            "the unit's shadowing is re-homed onto the entry once: {:#?}",
            program.entry().diagnostics
        );
    };
    assert_eq!(shadow.severity, Severity::Error, "{shadow:#?}");
    assert!(shadow.message.contains("SignalKind"), "{}", shadow.message);
    assert!(
        shadowings(&program.entry_own_diagnostics().cloned().collect::<Vec<_>>()).is_empty(),
        "the entry itself shadows nothing"
    );
}
