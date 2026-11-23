//! Import-header derivation (stdlib-namespace ε, task #5497) over the REAL
//! stdlib: every module's derived imports must compile it with nothing the
//! full stdlib would not also report, the headed stdlib must still build,
//! and re-deriving on the headed sources must find nothing missing.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use reify_ast::ParsedModule;
use reify_compiler::cfg::CfgSet;
use reify_compiler::import_headers::{
    ImportPlan, cfg_satisfied_imports, derive_std_imports, insert_imports,
    stdlib_universe_excluding,
};
use reify_compiler::stdlib_loader::{build_stdlib_modules, stdlib_sources};
use reify_core::ModulePath;

struct DerivedModule {
    name: &'static str,
    source: String,
    parsed: ParsedModule,
    plan: ImportPlan<'static>,
}

fn module_path(name: &str) -> ModulePath {
    ModulePath::from_dotted(name).expect("stdlib module names are dotted paths")
}

fn derive_for(name: &str, source: &str) -> (ParsedModule, ImportPlan<'static>) {
    let path = module_path(name);
    let parsed = reify_compiler::parse_with_stdlib(source, path.clone());
    assert!(
        parsed.errors.is_empty(),
        "{name} must parse cleanly: {:?}",
        parsed.errors
    );
    let plan = derive_std_imports(&parsed, &stdlib_universe_excluding(&path));
    (parsed, plan)
}

fn derived_stdlib() -> &'static [DerivedModule] {
    static DERIVED: OnceLock<Vec<DerivedModule>> = OnceLock::new();
    DERIVED.get_or_init(|| {
        stdlib_sources()
            .into_iter()
            .map(|(name, source)| {
                let (parsed, plan) = derive_for(name, &source);
                DerivedModule {
                    name,
                    source,
                    parsed,
                    plan,
                }
            })
            .collect()
    })
}

fn headed_source(module: &DerivedModule) -> String {
    insert_imports(
        &module.source,
        &module.parsed,
        &CfgSet::host_default(),
        &module.plan.modules(),
    )
    .unwrap_or_else(|defect| panic!("insert_imports refused {}: {defect}", module.name))
}

fn plan_of(name: &str) -> Vec<String> {
    derived_stdlib()
        .iter()
        .find(|module| module.name == name)
        .unwrap_or_else(|| panic!("no stdlib module named {name}"))
        .plan
        .modules()
        .iter()
        .map(ModulePath::to_dotted)
        .collect()
}

// ─── s1: the derived imports leave no residual diagnostic ──────────────────

#[test]
fn every_stdlib_module_compiles_against_its_derived_imports_alone() {
    let offenders: Vec<String> = derived_stdlib()
        .iter()
        .filter_map(|module| {
            let residual = module
                .plan
                .residual_diagnostics(&module.parsed, &module.plan.modules());
            (!residual.is_empty()).then(|| {
                let lines: Vec<String> = residual
                    .iter()
                    .map(|d| format!("    {:?} {:?}: {}", d.severity, d.code, d.message))
                    .collect();
                format!("{}:\n{}", module.name, lines.join("\n"))
            })
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "derived imports leave residual diagnostics:\n{}",
        offenders.join("\n")
    );
}

// ─── s2: the headed stdlib builds ──────────────────────────────────────────

#[test]
fn the_stdlib_with_every_derived_header_inserted_still_builds() {
    let headed: Vec<(&str, String)> = derived_stdlib()
        .iter()
        .map(|module| (module.name, headed_source(module)))
        .collect();
    let sources: Vec<(&str, &str)> = headed
        .iter()
        .map(|(name, source)| (*name, source.as_str()))
        .collect();
    let built = build_stdlib_modules(&sources);
    assert_eq!(built.len(), sources.len());
}

// ─── s3: re-deriving on headed sources finds nothing missing ───────────────

#[test]
fn re_deriving_on_the_headed_stdlib_finds_nothing_missing() {
    let unstable: Vec<String> = derived_stdlib()
        .iter()
        .filter_map(|module| {
            let headed = headed_source(module);
            let (parsed, plan) = derive_for(module.name, &headed);
            let missing: Vec<String> = plan
                .missing_from(&parsed, &CfgSet::host_default())
                .iter()
                .map(|required| required.module.to_dotted())
                .collect();
            (!missing.is_empty()).then(|| format!("{}: {missing:?}", module.name))
        })
        .collect();
    assert!(
        unstable.is_empty(),
        "headed sources still miss imports:\n{}",
        unstable.join("\n")
    );
}

// ─── s4: exemplar pins ─────────────────────────────────────────────────────

#[test]
fn fdm_as_printed_needs_exactly_its_three_providers() {
    assert_eq!(
        plan_of("std.fdm.as_printed"),
        vec!["std.constitutive", "std.fdm", "std.fdm.correlations"]
    );
}

#[test]
fn a_trait_refinement_the_compiler_never_diagnoses_is_still_derived() {
    assert!(plan_of("std.ports.mechanical").contains(&"std.ports".to_string()));
}

#[test]
fn a_builtin_geometry_type_does_not_import_the_geometry_traits() {
    let plan = plan_of("std.structural.physical");
    assert!(
        plan.contains(&"std.materials.mechanical".to_string()),
        "{plan:?}"
    );
    assert!(
        !plan.contains(&"std.geometry.traits".to_string()),
        "{plan:?}"
    );
}

#[test]
fn registry_only_units_import_the_si_units() {
    let plan = plan_of("std.materials.fea");
    assert!(plan.contains(&"std.si_units".to_string()), "{plan:?}");
}

// ─── s5: the committed headers are the derived ones ────────────────────────

fn dotted_set(paths: &[ModulePath]) -> BTreeSet<String> {
    paths.iter().map(ModulePath::to_dotted).collect()
}

#[test]
fn every_stdlib_module_declares_exactly_its_derived_imports() {
    let offenders: Vec<String> = derived_stdlib()
        .iter()
        .filter_map(|module| {
            let declared = dotted_set(&cfg_satisfied_imports(
                &module.parsed,
                &CfgSet::host_default(),
            ));
            let derived = dotted_set(&module.plan.modules());
            let missing: Vec<&String> = derived.difference(&declared).collect();
            let extra: Vec<&String> = declared.difference(&derived).collect();
            (!missing.is_empty() || !extra.is_empty())
                .then(|| format!("{}: missing {missing:?}, extra {extra:?}", module.name))
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "stdlib import headers differ from the derived imports:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_stdlib_module_declares_its_registered_module_path() {
    let offenders: Vec<String> = derived_stdlib()
        .iter()
        .filter(|module| module.parsed.declared_module_path != Some(module_path(module.name)))
        .map(|module| {
            format!(
                "{}: declares {:?}",
                module.name,
                module
                    .parsed
                    .declared_module_path
                    .as_ref()
                    .map(ModulePath::to_dotted)
            )
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "stdlib modules not declaring their registered module path:\n{}",
        offenders.join("\n")
    );
}
