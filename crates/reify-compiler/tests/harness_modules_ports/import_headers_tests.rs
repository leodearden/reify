//! `reify_compiler::import_headers` (stdlib-namespace ε, task #5497) driven
//! through its public API over SYNTHETIC universes.
//!
//! Every universe module is compiled with no prelude, so a `std.fake.*` path
//! lands in name_env's Stdlib tier and a non-std path (`parts`) in its Import
//! tier. Each fixture is asserted parse-clean first, so a syntax slip in a
//! fixture cannot read as a tool failure.

use reify_ast::ParsedModule;
use reify_compiler::CompiledModule;
use reify_compiler::import_headers::{
    ImportPlan, NameRole, NameUse, RequiredImport, derive_std_imports, insert_imports,
    residual_diagnostics,
};
use reify_core::{DiagnosticCode, ModulePath, Severity};

const SUBJECT_PATH: &str = "subject";

fn parse_clean(source: &str, path: &str) -> ParsedModule {
    let module_path = ModulePath::from_dotted(path).expect("valid dotted fixture path");
    assert_parsed_clean(reify_syntax::parse(source, module_path), source)
}

fn assert_parsed_clean(parsed: ParsedModule, source: &str) -> ParsedModule {
    assert!(
        parsed.errors.is_empty(),
        "fixture `{}` must parse cleanly; errors: {:?}\nsource:\n{source}",
        parsed.path.to_dotted(),
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
    derive_parsed(&parse_clean(source, SUBJECT_PATH), universe)
}

fn derive_parsed(parsed: &ParsedModule, universe: &[CompiledModule]) -> ImportPlan {
    let universe_refs: Vec<&CompiledModule> = universe.iter().collect();
    derive_std_imports(parsed, &universe_refs)
}

fn dotted_modules(plan: &ImportPlan) -> Vec<String> {
    plan.modules().iter().map(ModulePath::to_dotted).collect()
}

/// An import-free module is missing every derived module, so `missing_from` yields the whole plan.
fn names_needing(plan: &ImportPlan, module: &str) -> Vec<NameUse> {
    let imports_nothing = parse_clean("structure def S {}\n", SUBJECT_PATH);
    let whole_plan = plan.missing_from(&imports_nothing);
    let yielded: Vec<String> = whole_plan
        .iter()
        .map(|required| required.module.to_dotted())
        .collect();
    assert_eq!(
        yielded,
        dotted_modules(plan),
        "missing_from(an import-free module) must yield the whole plan"
    );
    whole_plan
        .into_iter()
        .find(|required| required.module.to_dotted() == module)
        .unwrap_or_else(|| panic!("plan has no entry for {module}: {:?}", dotted_modules(plan)))
        .names
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

// ─── expression positions ──────────────────────────────────────────────────

const FNS: (&str, &str) = (
    "std.fake.fns",
    "pub fn twice(x: Real) -> Real { x * 2.0 }\n",
);

const ENUMS: (&str, &str) = (
    "std.fake.enums",
    "enum Hue { Red, Green }\n\
     enum Mark { Dot, Ring { radius: Real } }\n",
);

/// `mm` shares its name with a builtin-table unit; `furlong` is in no builtin
/// table (`reify_core::units::unit_symbol_to_si`).
const UNITS: (&str, &str) = (
    "std.fake.units",
    "pub unit mm : Length = 0.001\n\
     pub unit furlong : Length = 201.168\n",
);

const CDEFS: (&str, &str) = (
    "std.fake.cdefs",
    "pub constraint def positive {\n\
         param v : Real\n\
         v > 0.0\n\
     }\n",
);

fn assert_needs(plan: &ImportPlan, module: &str, expected: &[NameUse], source: &str) {
    assert_eq!(dotted_modules(plan), vec![module], "source:\n{source}");
    let names = names_needing(plan, module);
    for name_use in expected {
        assert!(
            names.contains(name_use),
            "{name_use:?} should need {module}; got {names:?}\nsource:\n{source}"
        );
    }
}

// ─── e1: callees ───────────────────────────────────────────────────────────

#[test]
fn callees_attribute_to_functions_structures_and_constraint_defs() {
    let universe = compile_universe(&[FNS, SHAPES, CDEFS]);
    let cases: &[(&str, &str, NameUse)] = &[
        (
            "structure def S { let a = twice(1.0) }\n",
            "std.fake.fns",
            name_use("twice", NameRole::Callee),
        ),
        (
            "structure def S { let b = Bracket() }\n",
            "std.fake.shapes",
            name_use("Bracket", NameRole::Callee),
        ),
        (
            "structure def S { sub s = Bracket() }\n",
            "std.fake.shapes",
            name_use("Bracket", NameRole::Callee),
        ),
        (
            "structure def S { constraint positive(v: 1.0) }\n",
            "std.fake.cdefs",
            name_use("positive", NameRole::Callee),
        ),
    ];
    for (source, module, expected) in cases {
        let plan = derive(source, &universe);
        assert_needs(&plan, module, std::slice::from_ref(expected), source);
    }
}

#[test]
fn a_local_function_needs_no_import_for_its_own_call() {
    let universe = compile_universe(&[FNS]);
    let plan = derive(
        "fn twice(x: Real) -> Real { x }\n\
         structure def S { let y = twice(1.0) }\n",
        &universe,
    );
    assert!(plan.modules().is_empty(), "{:?}", dotted_modules(&plan));
}

// ─── e2: enum accesses in both lowerings ───────────────────────────────────

#[test]
fn an_enum_access_needs_the_enum_module_in_both_lowerings() {
    let universe = compile_universe(&[ENUMS]);
    let source = "structure def S { let h = Hue.Red }\n";

    let as_member_access = parse_clean(source, SUBJECT_PATH);
    let plan = derive_parsed(&as_member_access, &universe);
    assert_needs(
        &plan,
        "std.fake.enums",
        &[name_use("Hue", NameRole::Value)],
        source,
    );

    let module_path = ModulePath::from_dotted(SUBJECT_PATH).expect("valid dotted fixture path");
    let as_enum_access = assert_parsed_clean(
        reify_syntax::parse_with_prelude_enums(source, module_path, &["Hue"]),
        source,
    );
    let plan = derive_parsed(&as_enum_access, &universe);
    assert_needs(
        &plan,
        "std.fake.enums",
        &[name_use("Hue", NameRole::Enum)],
        source,
    );
}

// ─── e3: variants ──────────────────────────────────────────────────────────

#[test]
fn match_patterns_and_variant_constructions_need_the_enum_module() {
    let universe = compile_universe(&[ENUMS]);
    let source = "structure def S {\n\
                      param n : Int = 1\n\
                      let k = match n { Red => 1, Green => 2 }\n\
                  }\n";
    let plan = derive(source, &universe);
    assert_needs(
        &plan,
        "std.fake.enums",
        &[
            name_use("Green", NameRole::Variant),
            name_use("Red", NameRole::Variant),
        ],
        source,
    );

    let source = "structure def S { let r = Ring { radius: 1.0 } }\n";
    let plan = derive(source, &universe);
    assert_needs(
        &plan,
        "std.fake.enums",
        &[name_use("Ring", NameRole::Callee)],
        source,
    );
}

// ─── e4: units ─────────────────────────────────────────────────────────────

#[test]
fn a_bare_non_builtin_unit_needs_the_unit_module() {
    let universe = compile_universe(&[UNITS]);
    let source = "structure def S { let d = 3furlong }\n";
    let plan = derive(source, &universe);
    assert_needs(
        &plan,
        "std.fake.units",
        &[name_use("furlong", NameRole::BareUnit)],
        source,
    );
}

#[test]
fn a_bare_builtin_table_unit_needs_no_import() {
    let universe = compile_universe(&[UNITS]);
    let plan = derive("structure def S { let d = 1mm }\n", &universe);
    assert!(plan.modules().is_empty(), "{:?}", dotted_modules(&plan));
}

#[test]
fn a_compound_unit_factor_resolves_through_the_registry_only() {
    let universe = compile_universe(&[UNITS]);
    let source = "structure def S { let v = 1mm/s }\n";
    let plan = derive(source, &universe);
    assert_needs(
        &plan,
        "std.fake.units",
        &[name_use("mm", NameRole::UnitFactor)],
        source,
    );
}

// ─── e5: binder exclusion ──────────────────────────────────────────────────

#[test]
fn a_module_binder_shadows_a_std_value_of_the_same_name() {
    let universe = compile_universe(&[FNS]);
    for source in [
        "structure def S {\n\
             let twice = 2.0\n\
             let y = twice\n\
         }\n",
        "structure def S {\n\
             param twice : Real = 1.0\n\
             let y = twice * 2.0\n\
         }\n",
    ] {
        let plan = derive(source, &universe);
        assert!(
            plan.modules().is_empty(),
            "{:?}\nsource:\n{source}",
            dotted_modules(&plan)
        );
    }
}

#[test]
fn a_module_binder_does_not_shadow_a_std_function_in_call_position() {
    let universe = compile_universe(&[FNS]);
    for source in [
        "structure def S {\n\
             param twice : Real = 1.0\n\
             let y = twice(3.0)\n\
         }\n",
        "fn apply(twice: (Real) -> Real) -> Real { twice(1.0) }\n",
    ] {
        let plan = derive(source, &universe);
        assert_needs(
            &plan,
            "std.fake.fns",
            &[name_use("twice", NameRole::Callee)],
            source,
        );
    }
}

#[test]
fn a_std_callee_named_like_a_value_param_leaves_a_residual_the_derived_import_clears() {
    let universe = compile_universe(&[FNS, SHAPES]);
    let universe_refs: Vec<&CompiledModule> = universe.iter().collect();
    let parsed = parse_clean(
        "structure def S {\n\
             param twice : Real = 1.0\n\
             let y = twice(3.0)\n\
         }\n",
        SUBJECT_PATH,
    );

    let residual = residual_diagnostics(&parsed, &[], &universe_refs);
    assert!(
        residual
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Warning
                && diagnostic.code == Some(DiagnosticCode::UnresolvedFunction)),
        "expected an UnresolvedFunction warning; got {residual:?}"
    );

    let plan = derive_parsed(&parsed, &universe);
    let residual = residual_diagnostics(&parsed, &plan.modules(), &universe_refs);
    assert!(residual.is_empty(), "{residual:?}");
}

// ─── e6: member names ──────────────────────────────────────────────────────

#[test]
fn member_names_never_attribute() {
    let universe = compile_universe(&[FNS, SHAPES]);
    let plan = derive(
        "structure def S {\n\
             let w = Bracket().w\n\
             let t = Bracket().twice\n\
         }\n",
        &universe,
    );
    assert_eq!(dotted_modules(&plan), vec!["std.fake.shapes"]);
    assert_eq!(
        names_needing(&plan, "std.fake.shapes"),
        vec![name_use("Bracket", NameRole::Callee)]
    );
}

// ─── e7: declared imports are not special-cased ────────────────────────────

#[test]
fn an_already_imported_module_is_still_derived() {
    let universe = compile_universe(&[FNS]);
    let source = "import std.fake.fns\n\
                  structure def S { let a = twice(1.0) }\n";
    let plan = derive(source, &universe);
    assert_needs(
        &plan,
        "std.fake.fns",
        &[name_use("twice", NameRole::Callee)],
        source,
    );
}

// ─── g1/g2: the residual-diagnostics guard ─────────────────────────────────

fn module_paths(dotted: &[&str]) -> Vec<ModulePath> {
    dotted
        .iter()
        .map(|path| ModulePath::from_dotted(path).expect("valid dotted module path"))
        .collect()
}

#[test]
fn a_missing_import_leaves_a_residual_that_the_derived_import_clears() {
    let universe = compile_universe(&[SHAPES, FNS]);
    let universe_refs: Vec<&CompiledModule> = universe.iter().collect();
    let parsed = parse_clean("structure def S { param b : Bracket }\n", SUBJECT_PATH);

    let residual = residual_diagnostics(&parsed, &[], &universe_refs);
    assert!(
        residual
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error
                && diagnostic.code == Some(DiagnosticCode::UnresolvedType)),
        "expected an UnresolvedType error; got {residual:?}"
    );

    let residual =
        residual_diagnostics(&parsed, &module_paths(&["std.fake.shapes"]), &universe_refs);
    assert!(residual.is_empty(), "{residual:?}");
}

#[test]
fn a_diagnostic_the_full_universe_also_raises_is_not_residual() {
    let universe = compile_universe(&[SHAPES]);
    let universe_refs: Vec<&CompiledModule> = universe.iter().collect();
    let parsed = parse_clean("structure def S { param b : Nowhere }\n", SUBJECT_PATH);
    let residual = residual_diagnostics(&parsed, &[], &universe_refs);
    assert!(residual.is_empty(), "{residual:?}");
}

// ─── m1-m5: declared-import coverage by import form ────────────────────────

/// Calls `Bracket` and `Gizmo`, both of std.fake.shapes.
const BRACKET_AND_GIZMO: &str = "structure def S {\n\
                                     let b = Bracket()\n\
                                     let g = Gizmo()\n\
                                 }\n";

fn missing_for(header: &str, body: &str, universe: &[CompiledModule]) -> Vec<RequiredImport> {
    let parsed = parse_clean(&format!("{header}{body}"), SUBJECT_PATH);
    derive_parsed(&parsed, universe).missing_from(&parsed)
}

fn required(module: &str, names: Vec<NameUse>) -> RequiredImport {
    RequiredImport {
        module: ModulePath::from_dotted(module).expect("valid dotted module path"),
        names,
    }
}

#[test]
fn a_namespace_import_covers_every_name_of_its_module() {
    let universe = compile_universe(&[SHAPES]);
    for header in [
        "import std.fake.shapes\n",
        "import std.fake.shapes as sh\n",
        "import std.fake.shapes\nimport std.fake.shapes.Bracket\n",
        "import std.fake.shapes.Bracket\nimport std.fake.shapes\n",
    ] {
        assert_eq!(
            missing_for(header, BRACKET_AND_GIZMO, &universe),
            vec![],
            "header:\n{header}"
        );
    }
}

#[test]
fn an_entity_import_leaves_the_names_it_does_not_bind_missing() {
    let universe = compile_universe(&[SHAPES]);
    for header in [
        "import std.fake.shapes.Bracket\n",
        "import std.fake.shapes.{Bracket}\n",
    ] {
        assert_eq!(
            missing_for(header, BRACKET_AND_GIZMO, &universe),
            vec![required(
                "std.fake.shapes",
                vec![name_use("Gizmo", NameRole::Callee)]
            )],
            "header:\n{header}"
        );
    }
}

#[test]
fn entity_imports_binding_every_needed_name_cover_the_module() {
    let universe = compile_universe(&[SHAPES]);
    for header in [
        "import std.fake.shapes.{Bracket, Gizmo}\n",
        "import std.fake.shapes.Bracket\nimport std.fake.shapes.Gizmo\n",
    ] {
        assert_eq!(
            missing_for(header, BRACKET_AND_GIZMO, &universe),
            vec![],
            "header:\n{header}"
        );
    }

    let universe = compile_universe(&[ENUMS]);
    assert_eq!(
        missing_for(
            "import std.fake.enums.{Hue}\n",
            "structure def S { let h = Hue.Red }\n",
            &universe
        ),
        vec![]
    );
}

#[test]
fn an_aliased_entity_import_binds_its_alias_not_the_entity_name() {
    let universe = compile_universe(&[SHAPES]);
    assert_eq!(
        missing_for(
            "import std.fake.shapes.Bracket as B\n",
            BRACKET_AND_GIZMO,
            &universe
        ),
        vec![required(
            "std.fake.shapes",
            vec![
                name_use("Bracket", NameRole::Callee),
                name_use("Gizmo", NameRole::Callee)
            ]
        )]
    );
}

#[test]
fn an_entity_import_of_an_enum_does_not_cover_its_variants_written_bare() {
    let universe = compile_universe(&[ENUMS]);
    assert_eq!(
        missing_for(
            "import std.fake.enums.{Hue}\n",
            "structure def S {\n\
                 param n : Int = 1\n\
                 let k = match n { Red => 1, Green => 2 }\n\
             }\n",
            &universe
        ),
        vec![required(
            "std.fake.enums",
            vec![
                name_use("Green", NameRole::Variant),
                name_use("Red", NameRole::Variant)
            ]
        )]
    );
}

// ─── h1-h5: inserting import lines ─────────────────────────────────────────

fn insert(source: &str, modules: &[&str]) -> String {
    let parsed = parse_clean(source, SUBJECT_PATH);
    insert_imports(source, &parsed, &module_paths(modules))
}

#[test]
fn new_imports_land_after_the_last_import_line() {
    assert_eq!(
        insert(
            "import std.fake.a\nimport std.fake.b\n\nstructure def S {}\n",
            &["std.fake.c", "std.fake.d"],
        ),
        "import std.fake.a\nimport std.fake.b\nimport std.fake.c\nimport std.fake.d\n\nstructure def S {}\n"
    );
}

#[test]
fn new_imports_follow_a_module_declaration_after_a_blank_line() {
    assert_eq!(
        insert("module a.b\n\nstructure def S {}\n", &["std.fake.c"]),
        "module a.b\n\nimport std.fake.c\n\nstructure def S {}\n"
    );
}

#[test]
fn new_imports_follow_the_leading_module_pragmas_after_a_blank_line() {
    assert_eq!(
        insert("#no_prelude\n\nstructure def S {}\n", &["std.fake.c"]),
        "#no_prelude\n\nimport std.fake.c\n\nstructure def S {}\n"
    );
}

#[test]
fn new_imports_follow_the_leading_comment_block_up_to_its_last_blank_line() {
    assert_eq!(
        insert(
            "// Header line one\n// Header line two\n\n// Doc for S\nstructure def S {}\n",
            &["std.fake.c"],
        ),
        "// Header line one\n// Header line two\n\nimport std.fake.c\n\n// Doc for S\nstructure def S {}\n"
    );
}

#[test]
fn a_comment_hugging_the_first_declaration_keeps_the_imports_at_offset_zero() {
    assert_eq!(
        insert("// Doc for S\nstructure def S {}\n", &["std.fake.c"]),
        "import std.fake.c\n\n// Doc for S\nstructure def S {}\n"
    );
    assert_eq!(
        insert("structure def S {}\n", &["std.fake.c", "std.fake.d"]),
        "import std.fake.c\nimport std.fake.d\n\nstructure def S {}\n"
    );
}

#[test]
fn inserting_is_idempotent() {
    let source = "import std.fake.c\n\nstructure def S {}\n";
    assert_eq!(insert(source, &["std.fake.c"]), source);

    let once = insert(source, &["std.fake.c", "std.fake.d"]);
    assert_eq!(
        once,
        "import std.fake.c\nimport std.fake.d\n\nstructure def S {}\n"
    );
    assert_eq!(insert(&once, &["std.fake.c", "std.fake.d"]), once);
}

#[test]
fn a_module_repeated_in_modules_is_imported_once() {
    assert_eq!(
        insert("structure def S {}\n", &["std.fake.c", "std.fake.c"]),
        "import std.fake.c\n\nstructure def S {}\n"
    );
}

#[test]
fn an_entity_import_does_not_stop_its_module_being_imported_whole() {
    assert_eq!(
        insert(
            "import std.fake.c.Thing\n\nstructure def S {}\n",
            &["std.fake.c"]
        ),
        "import std.fake.c.Thing\nimport std.fake.c\n\nstructure def S {}\n"
    );
}

#[test]
fn an_aliased_module_import_counts_as_imported_whole() {
    let source = "import std.fake.c as c\n\nstructure def S {}\n";
    assert_eq!(insert(source, &["std.fake.c"]), source);
}

// ─── r1: inserting what is missing satisfies missing_from ──────────────────

#[test]
fn inserting_the_missing_imports_leaves_nothing_missing() {
    let universe = compile_universe(&[SHAPES]);
    let source = format!("import std.fake.shapes.Bracket\n\n{BRACKET_AND_GIZMO}");
    let parsed = parse_clean(&source, SUBJECT_PATH);
    let missing = derive_parsed(&parsed, &universe).missing_from(&parsed);
    assert!(
        !missing.is_empty(),
        "fixture must start out missing imports"
    );

    let modules: Vec<ModulePath> = missing
        .iter()
        .map(|required| required.module.clone())
        .collect();
    let fixed = insert_imports(&source, &parsed, &modules);
    let reparsed = parse_clean(&fixed, SUBJECT_PATH);
    assert_eq!(
        derive_parsed(&reparsed, &universe).missing_from(&reparsed),
        vec![],
        "fixed source:\n{fixed}"
    );
}
