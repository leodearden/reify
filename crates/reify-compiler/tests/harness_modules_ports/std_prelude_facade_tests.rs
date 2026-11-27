//! The `std.prelude` facade (task #5502; PRD `docs/prds/v0_6/stdlib-namespace.md`
//! ι, D-4, boundary #11): one `import std.prelude` brings the curated core,
//! because the facade `pub import`s each core module and declares nothing of
//! its own.

use std::fs;

use reify_compiler::CompiledModule;
use reify_compiler::module_dag::{ModuleResolver, compile_project};
use reify_compiler::stdlib_loader;
use reify_core::{DiagnosticCode, Severity};

/// The curated core, in declaration order: the facade's public contract.
const CURATED_CORE: [&str; 6] = [
    "std.units",
    "std.si_units",
    "std.option_recovery",
    "std.result",
    "std.determinacy.purposes",
    "std.io",
];

fn std_prelude() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|module| module.path.to_dotted() == "std.prelude")
        .expect("std.prelude is a registered stdlib module")
}

#[test]
fn std_prelude_is_a_facade_over_the_curated_core() {
    let facade = std_prelude();

    assert!(
        facade.imports.iter().all(|import| import.is_pub),
        "every std.prelude import must be a `pub import`; got: {:?}",
        facade.imports
    );
    let imported: Vec<&str> = facade.imports.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(imported, CURATED_CORE, "std.prelude's curated core");

    assert!(
        facade.templates.is_empty(),
        "std.prelude declares no templates"
    );
    assert!(
        facade.functions.is_empty(),
        "std.prelude declares no functions"
    );
    assert!(facade.enum_defs.is_empty(), "std.prelude declares no enums");
    assert!(
        facade.trait_defs.is_empty(),
        "std.prelude declares no traits"
    );
    assert!(facade.units.is_empty(), "std.prelude declares no units");
    assert!(
        facade.type_aliases.is_empty(),
        "std.prelude declares no type aliases"
    );
    assert!(
        facade.constraint_defs.is_empty(),
        "std.prelude declares no constraint defs"
    );
    assert!(facade.fields.is_empty(), "std.prelude declares no fields");
    assert!(
        facade
            .compiled_purposes
            .iter()
            .all(|p| p.declaration_span.is_prelude()),
        "std.prelude may carry only merged purpose copies; got own: {:?}",
        facade
            .compiled_purposes
            .iter()
            .filter(|p| !p.declaration_span.is_prelude())
            .map(|p| &p.name)
            .collect::<Vec<_>>()
    );

    let errors: Vec<_> = facade
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "std.prelude has errors: {errors:?}");
}

// ─── boundary #11 in the strict (compile_project) regime ───────────────────
//
// compile_project compiles the entry against its import tier only, with every
// `std.*` served by the embedded stdlib (stdlib_root does not exist). `Length`
// and `mm` are builtins there too, so `unwrap_or` and `simulation_ready` are
// the names that discriminate.

const PART_BODY: &str = "structure def Part {\n    param width : Length = 80mm\n    let bore : Length = unwrap_or(parse_length(\"garbage\"), 6mm)\n}\n";

/// Compile `main.ri` (`imports` followed by [`PART_BODY`]) through
/// `compile_project` and return the entry module.
fn compile_strict(imports: &str) -> CompiledModule {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let entry = dir.join("main.ri");
    fs::write(&entry, format!("module main\n{imports}{PART_BODY}")).unwrap();
    let resolver = ModuleResolver::new(dir, dir.join("no_stdlib"));
    let mut modules = compile_project(&entry, &resolver)
        .unwrap_or_else(|diagnostics| panic!("compile_project failed: {diagnostics:?}"));
    modules
        .pop()
        .expect("compile_project returns the entry last")
}

fn purpose_names(module: &CompiledModule) -> Vec<&str> {
    module
        .compiled_purposes
        .iter()
        .map(|p| p.name.as_str())
        .collect()
}

#[test]
fn a_file_whose_only_import_is_std_prelude_uses_mm_length_unwrap_or_and_simulation_ready() {
    let entry = compile_strict("import std.prelude\n");

    let errors: Vec<_> = entry
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "the entry has errors: {errors:?}");
    let unwrap_or_mentions: Vec<_> = entry
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unwrap_or"))
        .collect();
    assert!(
        unwrap_or_mentions.is_empty(),
        "unwrap_or must resolve through std.prelude: {unwrap_or_mentions:?}"
    );

    let purposes = purpose_names(&entry);
    for expected in ["simulation_ready", "design_review"] {
        assert!(
            purposes.contains(&expected),
            "{expected} must reach the entry through std.prelude; got {purposes:?}"
        );
    }
}

#[test]
fn without_std_prelude_the_strict_compile_resolves_neither_unwrap_or_nor_simulation_ready() {
    let entry = compile_strict("");

    assert!(
        entry.diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::UnresolvedFunction)
                && d.severity == Severity::Warning
                && d.message.contains("unwrap_or")
        }),
        "without an import, unwrap_or must not resolve; diagnostics: {:?}",
        entry.diagnostics
    );
    assert!(
        entry
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error),
        "without an import the entry must carry an Error, the counterpart of the \
         positive test's no-Error check; diagnostics: {:?}",
        entry.diagnostics
    );
    assert!(
        !purpose_names(&entry).contains(&"simulation_ready"),
        "without an import, simulation_ready must not reach the entry"
    );
}
