//! The P1 `DefEnv` skeleton over a `CompiledProgram` (PRD
//! docs/prds/v0_6/resolution-unification.md §3.2, D-3; task #5516).
//!
//! Resolution order is entry > imports (declaration order) > stdlib, keyed
//! internally by `(module_key, name)`. In P1 the flat view must agree with
//! today's eval lookup (`find_template_in_scope`, which binds by
//! `reify_compiler::name_env::resolve` over the entry's merged template list
//! and the stdlib); (g) pins that parity.

use std::fs;
use std::path::Path;

use reify_compiler::cfg::CfgSet;
use reify_compiler::module_dag::ModuleResolver;
use reify_compiler::program::{CompiledProgram, compile_program};
use reify_compiler::stdlib_loader::load_stdlib;
use reify_compiler::{CompiledModule, TopologyTemplate};
use reify_eval::DefEnv;
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

/// A local always-indeterminate checker (same contract as the compiler's stub).
struct Indeterminate;

impl ConstraintChecker for Indeterminate {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
                indeterminate_reason: None,
            })
            .collect()
    }
}

/// A stdlib-only template name (std.io), resolved through the stdlib scope.
const STDLIB_ONLY: &str = "STLOutput";
/// A stdlib template name the entry also declares, to test shadowing.
const SHADOWED: &str = "DisplayStyle";

fn stdlib_module_declaring(name: &str) -> &'static CompiledModule {
    load_stdlib()
        .iter()
        .find(|m| m.templates.iter().any(|t| t.name == name))
        .unwrap_or_else(|| panic!("precondition: some stdlib module declares `{name}`"))
}

fn template_in<'m>(module: &'m CompiledModule, name: &str) -> &'m TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("`{name}` in module {:?}", module.path))
}

fn compile_fixture(dir: &Path) -> CompiledProgram {
    fs::write(
        dir.join("parts.ri"),
        "module parts\n\
         pub structure def Pulley {\n    param r : Real = 1\n}\n\
         structure def Hidden {\n    param h : Real = 0\n}\n",
    )
    .unwrap();
    let main_src = format!(
        "module main\nimport parts\n\
         structure def Rig {{\n    sub p = Pulley()\n}}\n\
         structure def {SHADOWED} {{\n    param marker : Real = 7\n}}\n"
    );
    let entry_path = dir.join("main.ri");
    fs::write(&entry_path, &main_src).unwrap();
    compile_program(
        &entry_path,
        &main_src,
        &ModuleResolver::new(dir, dir.join("stdlib")),
        &CfgSet::default(),
        &Indeterminate,
    )
}

fn same(found: Option<&TopologyTemplate>, expected: &TopologyTemplate) -> bool {
    found.is_some_and(|t| std::ptr::eq(t, expected))
}

#[test]
fn def_env_resolves_with_entry_then_imports_then_stdlib_precedence() {
    let stdlib_only_home = stdlib_module_declaring(STDLIB_ONLY);
    let shadowed_home = stdlib_module_declaring(SHADOWED);
    let dir = tempfile::tempdir().unwrap();
    let program = compile_fixture(dir.path());
    let env = DefEnv::from_program(&program);

    // (a) entry-declared and merged-import templates come from the merged list.
    for name in ["Rig", "Pulley"] {
        assert!(
            same(
                env.resolve_template(name),
                template_in(&program.entry, name)
            ),
            "`{name}` must resolve to the entry's merged template"
        );
    }

    // (b) a stdlib-only name resolves to the stdlib module's template.
    assert!(same(
        env.resolve_template(STDLIB_ONLY),
        template_in(stdlib_only_home, STDLIB_ONLY)
    ));

    // (c) the entry's declaration shadows the stdlib one.
    let entry_shadow = template_in(&program.entry, SHADOWED);
    assert!(!std::ptr::eq(
        entry_shadow,
        template_in(shadowed_home, SHADOWED)
    ));
    assert!(same(env.resolve_template(SHADOWED), entry_shadow));

    // (d) unknown and non-pub names do not resolve.
    assert!(env.resolve_template("NoSuchThing").is_none());
    assert!(
        env.resolve_template("Hidden").is_none(),
        "a non-pub import template is neither merged nor visible"
    );

    // (e) qualified lookup is keyed by (module_key, name), pub surface only.
    let parts_unit = &program.imports[0];
    assert_eq!(parts_unit.import_path, "parts");
    assert!(same(
        env.resolve_template_in("parts", "Pulley"),
        template_in(&parts_unit.module, "Pulley")
    ));
    assert!(env.resolve_template_in("parts", "Hidden").is_none());
    assert!(same(
        env.resolve_template_in("main", "Rig"),
        template_in(&program.entry, "Rig")
    ));
    let stdlib_key = stdlib_only_home.path.0.join(".");
    assert!(same(
        env.resolve_template_in(&stdlib_key, STDLIB_ONLY),
        template_in(stdlib_only_home, STDLIB_ONLY)
    ));
    assert!(env.resolve_template_in("nope", "Rig").is_none());

    // (f) the declared module is the entry itself.
    assert!(std::ptr::eq(env.declared(), &program.entry));
}

/// (g) In P1 the flat view equals today's eval rule: `name_env::resolve` over
/// the entry's (merged) template list and the stdlib, as eval's
/// `find_template_in_scope` calls it.
#[test]
fn def_env_flat_view_matches_find_template_in_scope_rule() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile_fixture(dir.path());
    let env = DefEnv::from_program(&program);

    let reference = |name: &str| -> Option<&TopologyTemplate> {
        reify_compiler::name_env::resolve(&program.entry.templates, load_stdlib(), name)
    };

    let names = program
        .entry
        .templates
        .iter()
        .chain(load_stdlib().iter().flat_map(|m| m.templates.iter()))
        .map(|t| t.name.as_str());
    let mut checked = 0;
    for name in names {
        let expected = reference(name).expect("every enumerated name resolves");
        assert!(
            same(env.resolve_template(name), expected),
            "flat-view parity broken for `{name}`"
        );
        checked += 1;
    }
    assert!(
        checked > program.entry.templates.len(),
        "stdlib names were enumerated"
    );
}
