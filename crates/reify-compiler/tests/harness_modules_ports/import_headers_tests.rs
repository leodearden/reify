//! `reify_compiler::import_headers` (stdlib-namespace ε, task #5497) driven
//! through its public API over SYNTHETIC universes.
//!
//! Every universe module is compiled with no prelude, so a `std.fake.*` path
//! lands in name_env's Stdlib tier and a non-std path (`parts`) in its Import
//! tier. Each fixture is asserted parse-clean first, so a syntax slip in a
//! fixture cannot read as a tool failure.

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::import_headers::{ImportPlan, NameRole, NameUse, derive_std_imports};
use reify_core::ModulePath;

const SUBJECT_PATH: &str = "subject";

fn parse_clean(source: &str, path: &str) -> ParsedModule {
    let module_path = ModulePath::from_dotted(path).expect("valid dotted fixture path");
    let parsed = reify_syntax::parse(source, module_path);
    assert!(
        parsed.errors.is_empty(),
        "fixture `{path}` must parse cleanly; errors: {:?}\nsource:\n{source}",
        parsed.errors
    );
    parsed
}

fn compile_universe(modules: &[(&str, &str)]) -> Vec<CompiledModule> {
    modules
        .iter()
        .map(|(path, source)| reify_compiler::compile_with_prelude(&parse_clean(source, path), &[]))
        .collect()
}

fn derive(source: &str, universe: &[CompiledModule]) -> ImportPlan {
    let parsed = parse_clean(source, SUBJECT_PATH);
    let universe_refs: Vec<&CompiledModule> = universe.iter().collect();
    derive_std_imports(&parsed, &universe_refs)
}

fn dotted_modules(plan: &ImportPlan) -> Vec<String> {
    plan.modules().iter().map(ModulePath::to_dotted).collect()
}

fn names_needing(plan: &ImportPlan, module: &str) -> Vec<NameUse> {
    plan.required()
        .iter()
        .find(|required| required.module.to_dotted() == module)
        .unwrap_or_else(|| panic!("plan has no entry for {module}: {:?}", dotted_modules(plan)))
        .names
        .clone()
}

fn name_use(name: &str, role: NameRole) -> NameUse {
    NameUse {
        name: name.to_string(),
        role,
    }
}

const SHAPES: (&str, &str) = (
    "std.fake.shapes",
    "structure def Bracket { param w : Real = 1.0 }\n\
     structure def Gizmo { param h : Real = 2.0 }\n",
);

const MOUNTABLE: (&str, &str) = ("std.fake.traits", "trait Mountable {}\n");

// ─── t1: type positions ────────────────────────────────────────────────────

#[test]
fn type_positions_attribute_to_the_defining_std_module() {
    let universe = compile_universe(&[SHAPES]);
    let plan = derive(
        "structure def S {\n\
             param b : Bracket\n\
             param bs : List<Bracket>\n\
         }\n\
         fn pick(x: Bracket, y: Gizmo) -> Gizmo { y }\n\
         type Alias = Gizmo\n",
        &universe,
    );
    assert_eq!(dotted_modules(&plan), vec!["std.fake.shapes"]);
    assert_eq!(
        names_needing(&plan, "std.fake.shapes"),
        vec![
            name_use("Bracket", NameRole::Type),
            name_use("Gizmo", NameRole::Type)
        ]
    );
}

// ─── t2: silent trait references ───────────────────────────────────────────

#[test]
fn trait_refinements_bounds_and_type_param_bounds_need_the_trait_module() {
    let universe = compile_universe(&[MOUNTABLE]);
    for source in [
        "trait Holder : Mountable {}\n",
        "structure def S : Mountable { }\n",
        "fn g<T: Mountable>(x: T) -> T { x }\n",
    ] {
        let plan = derive(source, &universe);
        assert_eq!(
            dotted_modules(&plan),
            vec!["std.fake.traits"],
            "source:\n{source}"
        );
        assert_eq!(
            names_needing(&plan, "std.fake.traits"),
            vec![name_use("Mountable", NameRole::Trait)],
            "source:\n{source}"
        );
    }
}

// ─── t3: local shadowing ───────────────────────────────────────────────────

#[test]
fn a_local_definition_needs_no_import() {
    let universe = compile_universe(&[SHAPES]);
    let plan = derive(
        "structure def Bracket { param w : Real = 3.0 }\n\
         structure def S { param b : Bracket }\n",
        &universe,
    );
    assert!(plan.modules().is_empty(), "{:?}", dotted_modules(&plan));
}

// ─── t4: builtin type exclusion ────────────────────────────────────────────

#[test]
fn a_builtin_type_name_in_type_position_needs_no_import() {
    let universe = compile_universe(&[("std.fake.traits", "trait Solid {}\n")]);
    let plan = derive("structure def S { param g : Solid }\n", &universe);
    assert!(plan.modules().is_empty(), "{:?}", dotted_modules(&plan));
}

#[test]
fn a_builtin_type_name_in_trait_position_still_needs_the_trait_module() {
    let universe = compile_universe(&[("std.fake.traits", "trait Solid {}\n")]);
    let plan = derive("structure def P : Solid {}\n", &universe);
    assert_eq!(dotted_modules(&plan), vec!["std.fake.traits"]);
    assert_eq!(
        names_needing(&plan, "std.fake.traits"),
        vec![name_use("Solid", NameRole::Trait)]
    );
}

// ─── t5: tiering ───────────────────────────────────────────────────────────

#[test]
fn a_user_import_tier_definition_beats_a_std_one() {
    let universe = compile_universe(&[
        ("parts", "structure def Bracket { param w : Real = 1.0 }\n"),
        SHAPES,
    ]);
    let plan = derive("structure def S { param b : Bracket }\n", &universe);
    assert!(plan.modules().is_empty(), "{:?}", dotted_modules(&plan));
}

// ─── t6: determinism ───────────────────────────────────────────────────────

#[test]
fn modules_are_sorted_by_dotted_path_and_names_sorted_and_deduplicated() {
    let universe = compile_universe(&[
        (
            "std.fake.zeta",
            "structure def Zed { param z : Real = 1.0 }\n",
        ),
        (
            "std.fake.alpha",
            "structure def Beta { param b : Real = 1.0 }\n\
             structure def Alp { param a : Real = 1.0 }\n",
        ),
    ]);
    let plan = derive(
        "structure def S {\n\
             param z : Zed\n\
             param b1 : Beta\n\
             param a : Alp\n\
             param b2 : Beta\n\
         }\n",
        &universe,
    );
    assert_eq!(
        dotted_modules(&plan),
        vec!["std.fake.alpha", "std.fake.zeta"]
    );
    assert_eq!(
        names_needing(&plan, "std.fake.alpha"),
        vec![
            name_use("Alp", NameRole::Type),
            name_use("Beta", NameRole::Type)
        ]
    );
    assert_eq!(
        names_needing(&plan, "std.fake.zeta"),
        vec![name_use("Zed", NameRole::Type)]
    );
}
