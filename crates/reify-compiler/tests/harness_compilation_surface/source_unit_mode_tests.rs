//! The expression compiler seen from another crate (instances-on-the-core γ₃,
//! task #10639).
//!
//! Every test here reaches `reify_compiler` through its public surface only,
//! which is the evidence that a crate other than the compiler can compile an
//! expression on a definition's scope. The two helpers below are what the
//! second output's lowering does from its own crate: build the scope a
//! definition's expressions compile in from the definition's
//! `TopologyTemplate`, and parse one expression on its own.
//!
//! Each behaviour is compiled in both modes: the existing `compile_expr`, which
//! mints today's absolute references, and the source-unit mode, which mints
//! references local to the expression.

use std::collections::HashMap;

use reify_compiler::{
    CompilationScope, CompiledModule, CompiledSourceUnit, InstanceRequest, LOCAL_REFERENCE_ENTITY,
    LocalReference, ReferenceRead, ReferenceRoot, RequestArgument, SourceStep, SourceUnitNames,
    SourceUnitSpec, StructuralQuery, TopologyTemplate,
};
use reify_core::{Diagnostic, DiagnosticCode, Severity, Type, ValueCellId};
use reify_ir::CompiledExprKind;

/// Parse `text` as one expression, by wrapping it in a throwaway definition.
///
/// Probes never enter the source the definition scopes are built from, so a
/// form the existing compile rejects cannot poison those templates.
fn parse_expr(text: &str) -> reify_ast::Expr {
    let source = format!("structure def ProbeHolder {{ let probe = {text} }}");
    let parsed = reify_syntax::parse(&source, reify_core::ModulePath::single("probe"));
    assert!(
        parsed.errors.is_empty(),
        "probe does not parse: {:?}\n{source}",
        parsed.errors
    );
    let probe = parsed.declarations.iter().find_map(|decl| match decl {
        reify_ast::Declaration::Structure(def) => def.members.iter().find_map(|m| match m {
            reify_ast::MemberDecl::Let(l) if l.name == "probe" => Some(l.value.clone()),
            _ => None,
        }),
        _ => None,
    });
    probe.unwrap_or_else(|| panic!("no `probe` let in parsed source:\n{source}"))
}

/// Compile `source` with the standard library, failing on any error.
fn compile_clean(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("unit_mode"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let module = reify_compiler::compile_with_stdlib(&parsed);
    let errors = errors_of(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "source does not compile clean: {errors:?}\n{source}"
    );
    module
}

/// The module's templates by name: the registry a definition scope reads.
fn template_registry(module: &CompiledModule) -> HashMap<String, &TopologyTemplate> {
    module
        .templates
        .iter()
        .map(|t| (t.name.clone(), t))
        .collect()
}

fn template<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no template named {name}"))
}

/// The scope the expressions of definition `name` compile in, built from its
/// template and the registry of the module's templates.
fn definition_scope<'a>(
    module: &'a CompiledModule,
    registry: &'a HashMap<String, &'a TopologyTemplate>,
    name: &str,
) -> CompilationScope<'a> {
    let own = template(module, name);
    let mut scope = CompilationScope::new(name);
    scope.is_entity_scope = true;
    scope.set_template_registry(registry);
    for cell in own
        .value_cells
        .iter()
        .filter(|c| !c.id.member.starts_with("__"))
    {
        scope.register(&cell.id.member, cell.cell_type.clone());
    }
    for group in &own.guarded_groups {
        for cell in group.member_decls() {
            scope.register_guarded(
                &cell.id.member,
                cell.cell_type.clone(),
                group.guard_value_cell.clone(),
            );
        }
    }
    for sub in &own.sub_components {
        let element = sub.element_structure_name().to_string();
        scope
            .sub_component_types
            .insert(sub.name.clone(), element.clone());
        if sub.is_collection {
            scope.collection_sub_names.insert(sub.name.clone());
        }
        if !sub.keyed_members.is_empty() {
            scope.keyed_sub_keys.insert(
                sub.name.clone(),
                sub.keyed_members.iter().map(|k| k.0.clone()).collect(),
            );
        }
        if let Some(child) = registry.get(&element) {
            scope
                .sub_member_types
                .insert(sub.name.clone(), member_types(child));
            scope
                .sub_structure_traits
                .insert(element, child.trait_bounds.clone());
        }
    }
    scope
        .geometry_realization_names
        .extend(own.realizations.iter().filter_map(|r| r.name.clone()));
    scope
        .port_names
        .extend(own.ports.iter().map(|p| p.name.clone()));
    for group in &own.match_arm_groups {
        let arms = group
            .arms
            .iter()
            .filter_map(|arm| match &arm.arm_type {
                reify_core::Type::StructureRef(arm_def) => registry
                    .get(arm_def)
                    .map(|child| (arm_def.clone(), member_types(child))),
                _ => None,
            })
            .collect();
        scope.register_match_arm_group(&group.name, group.clone());
        scope
            .match_arm_group_arm_member_types
            .insert(group.name.clone(), arms);
    }
    scope
}

/// A child template's member types, as the compiler records a sub's members.
fn member_types(child: &TopologyTemplate) -> std::collections::BTreeMap<String, reify_core::Type> {
    child
        .value_cells
        .iter()
        .filter(|c| !c.id.member.starts_with("__"))
        .map(|c| (c.id.member.clone(), c.cell_type.clone()))
        .collect()
}

fn errors_of(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Compile `text` with today's `compile_expr` on `scope`.
fn compile_existing(
    text: &str,
    scope: &CompilationScope,
    module: &CompiledModule,
) -> (reify_ir::CompiledExpr, Vec<Diagnostic>) {
    let expr = parse_expr(text);
    let mut diagnostics = Vec::new();
    let compiled = reify_compiler::compile_expr(
        &expr,
        scope,
        &module.enum_defs,
        &module.functions,
        &mut diagnostics,
    );
    (compiled, diagnostics)
}

const PLATE_AND_ASM: &str = "
structure def Plate { param t : Real = 3  pub let w = t * 2 }
structure def Asm { param k : Real = 1  sub p : Plate }
";

#[test]
fn compile_expr_is_callable_from_another_crate() {
    let module = compile_clean(PLATE_AND_ASM);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");

    let (compiled, diagnostics) = compile_existing("self.p.t + k", &scope, &module);

    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let mut refs = compiled.collect_value_refs();
    refs.sort_by(|a, b| (&a.entity, &a.member).cmp(&(&b.entity, &b.member)));
    assert_eq!(
        refs,
        vec![ValueCellId::new("Asm", "k"), ValueCellId::new("Asm.p", "t")]
    );
}

/// Compile `text` in the source-unit mode, written in the definition `scope`
/// describes.
fn compile_unit(
    text: &str,
    scope: &CompilationScope,
    module: &CompiledModule,
) -> (CompiledSourceUnit, Vec<Diagnostic>) {
    compile_unit_named(&parse_expr(text), SourceUnitNames::Own, scope, module)
}

/// Compile `expr` in the source-unit mode, its names resolved as `names` says.
fn compile_unit_named(
    expr: &reify_ast::Expr,
    names: SourceUnitNames,
    scope: &CompilationScope,
    module: &CompiledModule,
) -> (CompiledSourceUnit, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let spec = SourceUnitSpec {
        names,
        expected_type: None,
    };
    let unit = reify_compiler::compile_source_unit(
        expr,
        scope,
        &spec,
        &module.enum_defs,
        &module.functions,
        &mut diagnostics,
    );
    (unit, diagnostics)
}

fn reference_of<'u>(unit: &'u CompiledSourceUnit, id: &ValueCellId) -> &'u LocalReference {
    unit.references
        .iter()
        .find(|r| &r.id == id)
        .unwrap_or_else(|| panic!("{id:?} is not in the table {:?}", unit.references))
}

/// The reference a unit whose whole tree is one read reads, with the tree's type.
fn sole_reference(unit: &CompiledSourceUnit) -> (&LocalReference, &Type) {
    match &unit.expr.kind {
        CompiledExprKind::ValueRef(id) => (reference_of(unit, id), &unit.expr.result_type),
        other => panic!("expected a single read, got {other:?}"),
    }
}

fn members(names: &[&str]) -> Vec<SourceStep> {
    names
        .iter()
        .map(|n| SourceStep::Member((*n).to_string()))
        .collect()
}

fn own_value(steps: Vec<SourceStep>) -> (ReferenceRoot, Vec<SourceStep>, ReferenceRead) {
    (ReferenceRoot::Own, steps, ReferenceRead::Value)
}

fn shape(reference: &LocalReference) -> (ReferenceRoot, Vec<SourceStep>, ReferenceRead) {
    (
        reference.root.clone(),
        reference.steps.clone(),
        reference.read.clone(),
    )
}

fn has_error_code(diagnostics: &[Diagnostic], code: DiagnosticCode) -> bool {
    errors_of(diagnostics).iter().any(|d| d.code == Some(code))
}

/// Every `ValueRef` node of the tree, binder reads included (which
/// `collect_value_refs`, a list of free reads, leaves out).
fn every_value_ref(tree: &reify_ir::CompiledExpr) -> Vec<ValueCellId> {
    let mut ids = Vec::new();
    tree.walk(&mut |node| {
        if let CompiledExprKind::ValueRef(id) = &node.kind {
            ids.push(id.clone());
        }
    });
    ids
}

fn quantifier_variable(tree: &reify_ir::CompiledExpr) -> ValueCellId {
    match &tree.kind {
        CompiledExprKind::Quantifier { variable_id, .. } => variable_id.clone(),
        other => panic!("expected a quantifier, got {other:?}"),
    }
}

const BOLTS_PLATE_ASM: &str = "
structure def Bolt {
    param length : Real = 5
}
structure def Plate {
    param t : Real = 3
}
structure def Asm {
    param k : Real = 1
    sub p : Plate
    sub bolts : List<Bolt>
    constraint bolts.count == 2
}
";

#[test]
fn bare_names_and_self_are_local_references() {
    let module = compile_clean(BOLTS_PLATE_ASM);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");

    let (unit, diagnostics) = compile_unit("k + k", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let refs = unit.expr.collect_value_refs();
    assert!(
        refs.iter().all(|id| id.entity == LOCAL_REFERENCE_ENTITY),
        "{refs:?}"
    );
    assert_eq!(unit.references.len(), 1, "{:?}", unit.references);
    assert_eq!(shape(&unit.references[0]), own_value(members(&["k"])));

    let (unit, _) = compile_unit("self", &scope, &module);
    let (reference, ty) = sole_reference(&unit);
    assert_eq!(shape(reference), own_value(vec![]));
    assert_eq!(ty, &Type::StructureRef("Asm".into()));

    let (unit, _) = compile_unit("p", &scope, &module);
    let (reference, ty) = sole_reference(&unit);
    assert_eq!(shape(reference), own_value(members(&["p"])));
    assert_eq!(ty, &Type::StructureRef("Plate".into()));

    let (unit, _) = compile_unit("bolts", &scope, &module);
    let (reference, ty) = sole_reference(&unit);
    assert_eq!(shape(reference), own_value(members(&["bolts"])));
    assert_eq!(ty, &Type::List(Box::new(Type::StructureRef("Bolt".into()))));

    let (unit, diagnostics) = compile_unit("forall x in [1, 2, 3]: x < k", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let variable = quantifier_variable(&unit.expr);
    assert!(!variable.entity.contains("Asm"), "{variable:?}");
    let binder_reads: Vec<_> = every_value_ref(&unit.expr)
        .into_iter()
        .filter(|id| id.entity != LOCAL_REFERENCE_ENTITY)
        .collect();
    assert!(!binder_reads.is_empty());
    assert!(
        binder_reads.iter().all(|id| !id.entity.contains("Asm")),
        "{binder_reads:?}"
    );
    assert_eq!(unit.references.len(), 1, "{:?}", unit.references);
    assert_eq!(shape(&unit.references[0]), own_value(members(&["k"])));

    let (_, diagnostics) = compile_unit("nosuch + 1", &scope, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::UnresolvedName),
        "{diagnostics:?}"
    );

    // The existing compile, on the same scope.
    let (tree, _) = compile_existing("k + k", &scope, &module);
    assert!(
        tree.collect_value_refs()
            .iter()
            .all(|id| id == &ValueCellId::new("Asm", "k"))
    );
    let (tree, _) = compile_existing("bolts", &scope, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm", "__list_bolts__length")),
        "{tree:?}"
    );
    assert_eq!(
        tree.result_type,
        Type::List(Box::new(Type::dimensionless_scalar()))
    );
    let (tree, _) = compile_existing("forall x in [1, 2, 3]: x < k", &scope, &module);
    assert!(quantifier_variable(&tree).entity.contains("Asm"));
    let (_, diagnostics) = compile_existing("nosuch + 1", &scope, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::UnresolvedName),
        "{diagnostics:?}"
    );
}

/// PRD §10 C4: the source-unit mode is off unless the second output sets it.
#[test]
fn scenario_c4() {
    let source = "
structure def Bolt {
    param length : Real = 5
}
structure def Plate {
    param t : Real = 3
}
structure def Asm {
    param k : Real = 1
    sub p : Plate
    sub bolts : List<Bolt>
    constraint bolts.count == 2
    sub s : Plate { t = k }
    let c = Plate()
    let a = self.p.t
    let n = bolts.count
    let q = self.children
    let b = Plate(t: auto)
    let r = present(self.p)
}
";
    let module = compile_clean(source);

    let mut local_reads = Vec::new();
    let mut note_local_reads = |tree: &reify_ir::CompiledExpr| {
        tree.walk(&mut |node| match &node.kind {
            CompiledExprKind::ValueRef(id)
            | CompiledExprKind::DeterminacyPredicate { cell: id, .. }
                if id.entity == LOCAL_REFERENCE_ENTITY =>
            {
                local_reads.push(id.clone());
            }
            _ => {}
        });
    };
    for template in &module.templates {
        for cell in &template.value_cells {
            if let Some(default) = &cell.default_expr {
                note_local_reads(default);
            }
        }
        for constraint in &template.constraints {
            note_local_reads(&constraint.expr);
        }
        for sub in &template.sub_components {
            for (_, argument) in &sub.args {
                note_local_reads(argument);
            }
        }
    }
    assert!(local_reads.is_empty(), "{local_reads:?}");

    let asm = template(&module, "Asm");
    let c = asm
        .value_cells
        .iter()
        .find(|cell| cell.id.member == "c")
        .and_then(|cell| cell.default_expr.as_ref())
        .expect("`c` has a default");
    match &c.kind {
        CompiledExprKind::StructureInstanceCtor { defaults, .. } => {
            assert!(defaults.iter().any(|(name, _)| name == "t"), "{defaults:?}")
        }
        other => panic!("expected a constructor, got {other:?}"),
    }

    assert!(
        module
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedFunction)),
        "{:?}",
        module.diagnostics
    );

    let mut fresh = CompilationScope::new("Asm");
    fresh.register("k", Type::dimensionless_scalar());
    fresh.is_entity_scope = true;
    let (tree, _) = compile_existing("k", &fresh, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm", "k")),
        "{tree:?}"
    );
}

const PATHS_SOURCE: &str = r#"
trait Rated {
    param rating : Real
}
structure def Shim {
    param r : Real = 1
}
structure def Plate : Rated {
    param t : Real = 3
    param rating : Real = 7
    pub let w = t * 2
    sub q : Shim
}
structure def Vent {
    param w : Real = 2
}
structure def Asm {
    param k : Real = 1
    sub p : Plate
    sub vents : List<Vent>
    constraint vents.count == 3
    sub ks : Keyed<Vent> { "a" => { w = 4 } }
    let d = Plate(t: 2)
}
"#;

fn steps(items: &[SourceStep]) -> Vec<SourceStep> {
    items.to_vec()
}

fn member(name: &str) -> SourceStep {
    SourceStep::Member(name.to_string())
}

#[test]
fn member_reads_through_subs_and_cells_are_references() {
    let module = compile_clean(PATHS_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");
    let real = Type::dimensionless_scalar();

    let expect_reference = |text: &str, expected: Vec<SourceStep>, ty: &Type| {
        let (unit, diagnostics) = compile_unit(text, &scope, &module);
        assert!(
            errors_of(&diagnostics).is_empty(),
            "{text}: {diagnostics:?}"
        );
        let (reference, actual) = sole_reference(&unit);
        assert_eq!(shape(reference), own_value(expected), "{text}");
        assert_eq!(actual, ty, "{text}");
        reference.id.clone()
    };

    let through_self = expect_reference("self.p.t", members(&["p", "t"]), &real);
    let bare = expect_reference("p.t", members(&["p", "t"]), &real);
    assert_eq!(through_self, bare);
    expect_reference("self.p.q.r", members(&["p", "q", "r"]), &real);
    expect_reference(
        "vents[1].w",
        steps(&[member("vents"), SourceStep::Index(1), member("w")]),
        &real,
    );
    expect_reference(
        "vents[1]",
        steps(&[member("vents"), SourceStep::Index(1)]),
        &Type::StructureRef("Vent".into()),
    );
    expect_reference(
        r#"ks["a"].w"#,
        steps(&[member("ks"), SourceStep::Key("a".into()), member("w")]),
        &real,
    );
    expect_reference("d.w", members(&["d", "w"]), &real);
    expect_reference("d.t", members(&["d", "t"]), &real);
    expect_reference("p.(Rated::rating)", members(&["p", "rating"]), &real);

    let (unit, diagnostics) = compile_unit("vents.count", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert_eq!(unit.expr.result_type, Type::Int);
    assert!(!matches!(unit.expr.kind, CompiledExprKind::ValueRef(_)));
    assert_eq!(unit.references.len(), 1, "{:?}", unit.references);
    assert_eq!(shape(&unit.references[0]), own_value(members(&["vents"])));

    for text in ["self.p.nosuch", "d.nosuch"] {
        let (_, diagnostics) = compile_unit(text, &scope, &module);
        assert!(
            has_error_code(&diagnostics, DiagnosticCode::StructureMemberNotFound),
            "{text}: {diagnostics:?}"
        );
    }

    let (unit, diagnostics) = compile_unit("self.origin", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert!(
        unit.references
            .iter()
            .all(|r| shape(r) == own_value(vec![])),
        "{:?}",
        unit.references
    );

    // The existing compile, on the same scope.
    let (tree, _) = compile_existing("self.p.t", &scope, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm.p", "t")),
        "{tree:?}"
    );
    for text in ["p.t", "d.w"] {
        let (tree, _) = compile_existing(text, &scope, &module);
        assert!(
            matches!(tree.kind, CompiledExprKind::IndexAccess { .. }),
            "{text}: {tree:?}"
        );
    }
    let (tree, _) = compile_existing("vents.count", &scope, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm", "__count_vents")),
        "{tree:?}"
    );
    let (tree, _) = compile_existing("self.p.q.r", &scope, &module);
    assert!(
        every_value_ref(&tree)
            .iter()
            .all(|id| id.entity != LOCAL_REFERENCE_ENTITY),
        "{tree:?}"
    );
}

#[test]
fn determinacy_tests_take_a_path_through_a_sub() {
    let module = compile_clean(PATHS_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");

    for test in ["determined", "constrained", "partially_determined"] {
        let text = format!("{test}(p.t)");
        let (unit, diagnostics) = compile_unit(&text, &scope, &module);
        assert!(
            errors_of(&diagnostics).is_empty(),
            "{text}: {diagnostics:?}"
        );
        let CompiledExprKind::DeterminacyPredicate { cell, .. } = &unit.expr.kind else {
            panic!("{text}: expected a determinacy test, got {:?}", unit.expr);
        };
        assert_eq!(
            shape(reference_of(&unit, cell)),
            own_value(members(&["p", "t"])),
            "{text}"
        );

        let (_, diagnostics) = compile_existing(&text, &scope, &module);
        assert!(
            errors_of(&diagnostics).iter().any(|d| d
                .message
                .contains("argument must be a direct cell reference")),
            "{text}: {diagnostics:?}"
        );
    }
}

const MAY_BE_ABSENT_SOURCE: &str = "
enum HeadKind { Hex, Round }
structure def Fan {
    param flow : Real = 6
}
structure def Plate {
    param thick : Bool = false
    where thick { param extra : Real = 1 }
}
structure def HexHead {
    param across : Real = 8
    param hex_only : Real = 1
    param size : Real = 1
}
structure def RoundHead {
    param across : Real = 9
    param round_only : Real = 2
    param size : Int = 2
}
structure def Bolt {
    param kind : HeadKind = HeadKind.Hex
    match kind {
        Hex => sub head : HexHead,
        Round => sub head : RoundHead
    }
}
structure def Asm {
    param needs_fan : Bool = false
    sub p : Plate
    sub b : Bolt
}
";

/// Leo's ruling of 2026-11-26 (PRD D13 items 5, 7 and 19): a read of a member
/// that may be absent compiles wherever it is written.
#[test]
fn member_reads_that_may_be_absent_are_references() {
    let module = compile_clean(MAY_BE_ABSENT_SOURCE);
    let registry = template_registry(&module);
    let scope = may_be_absent_scope(&module, &registry);
    let real = Type::dimensionless_scalar();

    let real_or_int = Type::Union(vec![real.clone(), Type::Int]);
    for (text, path, expected) in [
        ("self.p.extra", &["p", "extra"][..], &real),
        ("self.fan.flow", &["fan", "flow"], &real),
        // A member only one arm's definition declares, in either arm.
        ("b.head.hex_only", &["b", "head", "hex_only"], &real),
        ("b.head.round_only", &["b", "head", "round_only"], &real),
        // Both arms declare it: the type they agree on, or a union.
        ("b.head.across", &["b", "head", "across"], &real),
        ("b.head.size", &["b", "head", "size"], &real_or_int),
    ] {
        let (unit, diagnostics) = compile_unit(text, &scope, &module);
        assert!(
            errors_of(&diagnostics).is_empty(),
            "{text}: {diagnostics:?}"
        );
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(shape(reference), own_value(members(path)), "{text}");
        assert_eq!(ty, expected, "{text}");
    }
    for text in ["b.head.nosuch", "self.p.nosuch"] {
        let (_, diagnostics) = compile_unit(text, &scope, &module);
        assert!(
            has_error_code(&diagnostics, DiagnosticCode::StructureMemberNotFound),
            "{text}: {diagnostics:?}"
        );
    }

    // The existing compile, on the same scope (outcomes measured when this
    // test was written).
    for (text, message) in [
        ("self.p.extra", "unknown member 'extra' on sub 'p'"),
        (
            "b.head.hex_only",
            "member access not yet supported: .hex_only",
        ),
    ] {
        let (tree, diagnostics) = compile_existing(text, &scope, &module);
        assert_eq!(tree.result_type, Type::Error, "{text}");
        assert!(
            errors_of(&diagnostics).iter().any(|d| d.message == message),
            "{text}: {diagnostics:?}"
        );
    }
    let (tree, diagnostics) = compile_existing("self.fan.flow", &scope, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm.fan", "flow")),
        "{tree:?}"
    );
    assert_eq!(tree.result_type, Type::Error);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");

    // The same members read in the declaring definition's own scope, where
    // `head` names the arms' cluster.
    let bolt = definition_scope(&module, &registry, "Bolt");
    let either_head = Type::Union(vec![
        Type::StructureRef("HexHead".into()),
        Type::StructureRef("RoundHead".into()),
    ]);
    for (text, path, expected) in [
        ("self.head.hex_only", &["head", "hex_only"][..], &real),
        ("head.round_only", &["head", "round_only"], &real),
        ("head.size", &["head", "size"], &real_or_int),
        ("self.head", &["head"], &either_head),
    ] {
        let (unit, diagnostics) = compile_unit(text, &bolt, &module);
        assert!(
            errors_of(&diagnostics).is_empty(),
            "Bolt, {text}: {diagnostics:?}"
        );
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(shape(reference), own_value(members(path)), "Bolt, {text}");
        assert_eq!(ty, expected, "Bolt, {text}");
    }
    let (_, diagnostics) = compile_unit("head.nosuch", &bolt, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::StructureMemberNotFound),
        "Bolt, head.nosuch: {diagnostics:?}"
    );
    // The existing compile, on Bolt's scope (outcomes measured when these
    // cases were written).
    for (text, message) in [
        (
            "self.head.hex_only",
            "field 'hex_only' is not present in match-arm types: RoundHead",
        ),
        (
            "self.head.size",
            "field 'size' has divergent types across match-arm types: HexHead: Real, RoundHead: Int",
        ),
    ] {
        let (tree, diagnostics) = compile_existing(text, &bolt, &module);
        assert_eq!(tree.result_type, Type::Error, "Bolt, {text}");
        assert!(
            errors_of(&diagnostics).iter().any(|d| d.message == message),
            "Bolt, {text}: {diagnostics:?}"
        );
    }
}

const CTOR_SOURCE: &str = "
structure def Plate {
    param t : Real = 3
    param u : Real = 4
    param g : Real = auto
    pub let w = t * 2
}
structure def Pair {
    param a : Plate
    param n : Real = 1
}
structure def Asm {
    param k : Real = 1
}
";

fn request(place: Vec<usize>, definition: &str, parameters: &[&str]) -> InstanceRequest {
    InstanceRequest {
        place,
        definition: definition.to_string(),
        arguments: parameters
            .iter()
            .map(|parameter| RequestArgument {
                parameter: (*parameter).to_string(),
                unknown: None,
            })
            .collect(),
    }
}

/// The constructor node a unit's tree is, with its argument names.
fn ctor_of(tree: &reify_ir::CompiledExpr) -> (&str, Vec<&str>, &reify_ir::CompiledExpr) {
    match &tree.kind {
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            ..
        } => (
            type_name.as_str(),
            ordered_args.iter().map(|(name, _)| name.as_str()).collect(),
            tree,
        ),
        other => panic!("expected a constructor, got {other:?}"),
    }
}

fn callee_internals(tree: &reify_ir::CompiledExpr) -> (Vec<&str>, Vec<&str>, usize) {
    match &tree.kind {
        CompiledExprKind::StructureInstanceCtor {
            defaults,
            lets,
            geometry_lets,
            ..
        } => (
            defaults.iter().map(|(name, _)| name.as_str()).collect(),
            lets.iter().map(|(name, _)| name.as_str()).collect(),
            geometry_lets.len(),
        ),
        other => panic!("expected a constructor, got {other:?}"),
    }
}

/// PRD §5.2 item 7 and §5.5 "Making an instance from an expression": in the
/// source-unit mode a constructor call is a request to make an instance, and
/// nothing of the callee is copied into the caller.
#[test]
fn constructor_call_is_an_instance_request() {
    let module = compile_clean(CTOR_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");

    let (unit, diagnostics) = compile_unit("Plate(t: k / 2)", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let (definition, arguments, ctor) = ctor_of(&unit.expr);
    assert_eq!((definition, arguments), ("Plate", vec!["t"]));
    assert_eq!(callee_internals(ctor), (vec![], vec![], 0));
    let reads = unit.expr.collect_value_refs();
    assert!(!reads.is_empty());
    for id in &reads {
        assert_eq!(shape(reference_of(&unit, id)), own_value(members(&["k"])));
    }
    assert_eq!(
        unit.instance_requests,
        vec![request(vec![], "Plate", &["t"])]
    );

    let (unit, diagnostics) = compile_unit("Pair(a: Plate(t: 1))", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert_eq!(
        unit.instance_requests,
        vec![
            request(vec![], "Pair", &["a"]),
            request(vec![0], "Plate", &["t"]),
        ]
    );

    // Positional arguments fill the declared parameters in declaration order,
    // the `auto` parameter included (PRD 5.6 item 7).
    let (unit, diagnostics) = compile_unit("Plate(1, 2, 5)", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert_eq!(ctor_of(&unit.expr).1, vec!["t", "u", "g"]);
    assert_eq!(
        unit.instance_requests,
        vec![request(vec![], "Plate", &["t", "u", "g"])]
    );

    // The request inside the projection (child 0) inside the sum (child 0).
    let (unit, diagnostics) = compile_unit("Plate(t: 1).t + 1", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert_eq!(
        unit.instance_requests,
        vec![request(vec![0, 0], "Plate", &["t"])]
    );

    // The existing compile copies the callee's defaults and lets.
    let (tree, _) = compile_existing("Plate(t: k / 2)", &scope, &module);
    assert_eq!(callee_internals(&tree), (vec!["u"], vec!["w"], 0));
    let (tree, _) = compile_existing("Plate(1, 2, 5)", &scope, &module);
    assert_eq!(ctor_of(&tree).1, vec!["t", "u", "__arg2"]);
}

const AUTO_ARGUMENT_SOURCE: &str = "
structure def Plate {
    param t : Length = 3mm
    param u : Real = 4
}
structure def Asm {
    param k : Real = 1
}
fn f(x: Real) -> Real { x }
";

/// The constructor's argument for `parameter`.
fn argument_of<'t>(
    tree: &'t reify_ir::CompiledExpr,
    parameter: &str,
) -> &'t reify_ir::CompiledExpr {
    match &tree.kind {
        CompiledExprKind::StructureInstanceCtor { ordered_args, .. } => ordered_args
            .iter()
            .find(|(name, _)| name == parameter)
            .map(|(_, argument)| argument)
            .unwrap_or_else(|| panic!("no argument {parameter} in {ordered_args:?}")),
        other => panic!("expected a constructor, got {other:?}"),
    }
}

fn declared_type(module: &CompiledModule, definition: &str, member: &str) -> Type {
    template(module, definition)
        .value_cells
        .iter()
        .find(|cell| cell.id.member == member)
        .map(|cell| cell.cell_type.clone())
        .unwrap_or_else(|| panic!("{definition} declares no {member}"))
}

/// PRD §5.6 item 1 and §5.7 row "`auto` as a constructor argument": an `auto`
/// written as an argument is an unknown of the instance that wrote it, passed
/// to the child as an ordinary argument.
#[test]
fn auto_constructor_argument_is_an_unknown_reference() {
    let module = compile_clean(AUTO_ARGUMENT_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");
    let unknown_of = |parameter: &str| ReferenceRead::ArgumentUnknown {
        parameter: parameter.to_string(),
    };

    let (unit, diagnostics) = compile_unit("Plate(t: auto)", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let argument = argument_of(&unit.expr, "t");
    let CompiledExprKind::ValueRef(id) = &argument.kind else {
        panic!("expected a reference, got {argument:?}");
    };
    assert_eq!(
        shape(reference_of(&unit, id)),
        (ReferenceRoot::Own, vec![], unknown_of("t"))
    );
    assert_eq!(argument.result_type, declared_type(&module, "Plate", "t"));
    assert_eq!(
        unit.instance_requests,
        vec![InstanceRequest {
            place: vec![],
            definition: "Plate".into(),
            arguments: vec![RequestArgument {
                parameter: "t".into(),
                unknown: Some(id.clone()),
            }],
        }]
    );

    // Each `auto` written is its own unknown.
    let (unit, diagnostics) = compile_unit("Plate(t: auto).u + Plate(t: auto).u", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let unknowns: Vec<_> = unit
        .instance_requests
        .iter()
        .map(|request| request.arguments[0].unknown.clone())
        .collect();
    assert_eq!(unknowns.len(), 2, "{:?}", unit.instance_requests);
    assert!(unknowns.iter().all(Option::is_some), "{unknowns:?}");
    assert_ne!(unknowns[0], unknowns[1]);

    let (unit, diagnostics) = compile_unit("Plate(u: auto)", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert_eq!(
        argument_of(&unit.expr, "u").result_type,
        Type::dimensionless_scalar()
    );

    // A function is not a constructor, in either mode.
    let (_, diagnostics) = compile_unit("f(x: auto)", &scope, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::AutoNotAtBindingSite),
        "{diagnostics:?}"
    );
    let (_, diagnostics) = compile_existing("f(x: auto)", &scope, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::AutoNotAtBindingSite),
        "{diagnostics:?}"
    );

    // The existing compile passes an undefined literal.
    let (tree, _) = compile_existing("Plate(t: auto)", &scope, &module);
    let argument = argument_of(&tree, "t");
    assert!(
        matches!(
            &argument.kind,
            CompiledExprKind::Literal(reify_ir::Value::Undef)
        ),
        "{argument:?}"
    );
    assert_eq!(argument.result_type, Type::dimensionless_scalar());
}

/// The scope of `Asm` in the may-be-absent fixture. The second output declares
/// a guarded sub that today's compile does not elaborate (task 8414), as the
/// member table will, so `fan` is added by hand.
fn may_be_absent_scope<'a>(
    module: &'a CompiledModule,
    registry: &'a HashMap<String, &'a TopologyTemplate>,
) -> CompilationScope<'a> {
    let mut scope = definition_scope(module, registry, "Asm");
    scope.sub_component_types.insert("fan".into(), "Fan".into());
    scope
}

/// PRD §5.5 "The tests of state": `present(x)` compiles to a read whose outcome
/// the evaluator's caller tests, not to a new node.
#[test]
fn present_resolves_to_a_presence_read() {
    let module = compile_clean(MAY_BE_ABSENT_SOURCE);
    let registry = template_registry(&module);
    let scope = may_be_absent_scope(&module, &registry);
    let presence = |path: &[&str]| (ReferenceRoot::Own, members(path), ReferenceRead::Presence);

    let mut ids = Vec::new();
    for (text, path) in [
        ("present(self.p)", &["p"][..]),
        ("present(p)", &["p"]),
        ("present(self.fan)", &["fan"]),
        ("present(self.p.extra)", &["p", "extra"]),
    ] {
        let (unit, diagnostics) = compile_unit(text, &scope, &module);
        assert!(diagnostics.is_empty(), "{text}: {diagnostics:?}");
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(shape(reference), presence(path), "{text}");
        assert_eq!(ty, &Type::Bool, "{text}");
        ids.push(reference.id.clone());
    }

    // A presence read and a value read of the same path are two entries.
    let (unit, diagnostics) = compile_unit(
        "if present(self.p) then self.p else self.p",
        &scope,
        &module,
    );
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let mut shapes: Vec<_> = unit.references.iter().map(shape).collect();
    shapes.sort_by_key(|(_, _, read)| read == &ReferenceRead::Value);
    assert_eq!(shapes, vec![presence(&["p"]), own_value(members(&["p"]))]);

    // A user's function named `present` shadows the test.
    let shadowing = compile_clean(&format!(
        "{MAY_BE_ABSENT_SOURCE}\nfn present(x: Bool) -> Bool {{ x }}\n"
    ));
    let shadowing_registry = template_registry(&shadowing);
    let shadowing_scope = may_be_absent_scope(&shadowing, &shadowing_registry);
    let (unit, diagnostics) = compile_unit("present(needs_fan)", &shadowing_scope, &shadowing);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(
        matches!(&unit.expr.kind, CompiledExprKind::UserFunctionCall { function_name, .. } if function_name == "present"),
        "{:?}",
        unit.expr
    );

    // An argument that is not a path keeps the existing handling (measured
    // when this test was written: an unresolved function, in both modes).
    let (unit, new_diagnostics) = compile_unit("present(1 + 2)", &scope, &module);
    let (tree, old_diagnostics) = compile_existing("present(1 + 2)", &scope, &module);
    assert_eq!(unit.expr.content_hash, tree.content_hash);
    assert!(unit.references.is_empty(), "{:?}", unit.references);
    for diagnostics in [&new_diagnostics, &old_diagnostics] {
        assert!(
            diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::UnresolvedFunction)),
            "{diagnostics:?}"
        );
    }

    // The existing compile does not know the test.
    let (_, diagnostics) = compile_existing("present(self.p)", &scope, &module);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::UnresolvedFunction)),
        "{diagnostics:?}"
    );
}

const STRUCTURAL_QUERY_SOURCE: &str = "
structure def Plate {
    param t : Real = 3
}
structure def Asm {
    sub p : Plate
    sub r : Plate
}
structure def Odd {
    param members : Real = 2
}
";

/// PRD §5.4 last paragraph and §5.7 row "`self.children`, `self.members`,
/// `self.descendants`, and a bare collection, are the values of present
/// members": a structural query is a read of the instance a path reaches.
#[test]
fn structural_queries_are_typed_as_values() {
    let module = compile_clean(STRUCTURAL_QUERY_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");
    let instances = Type::List(Box::new(Type::StructureRef("Structure".into())));

    for (text, path, query) in [
        ("self.children", &[][..], StructuralQuery::Children),
        ("self.members", &[], StructuralQuery::Members),
        ("self.descendants", &[], StructuralQuery::Descendants),
        ("self.p.children", &["p"], StructuralQuery::Children),
    ] {
        let (unit, diagnostics) = compile_unit(text, &scope, &module);
        assert!(diagnostics.is_empty(), "{text}: {diagnostics:?}");
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(
            shape(reference),
            (
                ReferenceRoot::Own,
                members(path),
                ReferenceRead::Query(query)
            ),
            "{text}"
        );
        assert_eq!(ty, &instances, "{text}");
    }

    // A further hop types by the query's list.
    let (unit, diagnostics) = compile_unit("self.children.count", &scope, &module);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(unit.expr.result_type, Type::Int);
    let CompiledExprKind::MethodCall {
        object: receiver,
        method,
        ..
    } = &unit.expr.kind
    else {
        panic!("expected a count, got {:?}", unit.expr);
    };
    assert_eq!(method, "count");
    let CompiledExprKind::ValueRef(id) = &receiver.kind else {
        panic!("expected a reference, got {receiver:?}");
    };
    assert_eq!(
        shape(reference_of(&unit, id)),
        (
            ReferenceRoot::Own,
            vec![],
            ReferenceRead::Query(StructuralQuery::Children)
        )
    );
    assert_eq!(unit.references.len(), 1, "{:?}", unit.references);

    // A member a definition declares shadows the query of that name.
    let odd = definition_scope(&module, &registry, "Odd");
    let (unit, diagnostics) = compile_unit("self.members", &odd, &module);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let (reference, ty) = sole_reference(&unit);
    assert_eq!(shape(reference), own_value(members(&["members"])));
    assert_eq!(ty, &Type::dimensionless_scalar());

    // The existing compile: a method call on the definition's own reference.
    let (tree, _) = compile_existing("self.children", &scope, &module);
    let CompiledExprKind::MethodCall {
        object: receiver,
        method,
        ..
    } = &tree.kind
    else {
        panic!("expected a method call, got {tree:?}");
    };
    assert_eq!(method, "children");
    assert!(
        matches!(&receiver.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm", "__self")),
        "{receiver:?}"
    );
    assert_eq!(tree.result_type, instances);
}

const GEOMETRY_OF_A_VALUE_SOURCE: &str = "
structure def Tool {
    param r : Real = 2
    let body = cylinder(r * 1mm, 10mm)
}
structure def Asm {
    sub held : Tool
    let t = Tool(r: 3)
    let tools = [Tool(r: 1), Tool(r: 2)]
}
";

/// The nodes of the tree that `matches` picks.
fn nodes_where(
    tree: &reify_ir::CompiledExpr,
    matches: impl Fn(&reify_ir::CompiledExpr) -> bool,
) -> Vec<reify_ir::CompiledExpr> {
    let mut found = Vec::new();
    tree.walk(&mut |node| {
        if matches(node) {
            found.push(node.clone());
        }
    });
    found
}

/// PRD §5.7 row "`tool.body` on a value": a geometry member of a value is
/// read by opening the value, so it is a reference, not a field.
#[test]
fn geometry_member_of_a_value_is_a_reference() {
    let module = compile_clean(GEOMETRY_OF_A_VALUE_SOURCE);
    let registry = template_registry(&module);
    let scope = definition_scope(&module, &registry, "Asm");

    for (text, path) in [
        ("t.body", &["t", "body"][..]),
        ("self.held.body", &["held", "body"]),
    ] {
        let (unit, diagnostics) = compile_unit(text, &scope, &module);
        assert!(
            errors_of(&diagnostics).is_empty(),
            "{text}: {diagnostics:?}"
        );
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(shape(reference), own_value(members(path)), "{text}");
        assert_eq!(ty, &Type::Geometry, "{text}");
    }

    // Through a binder that holds an element of a list of values (a
    // quantifier's: a lambda's parameter is untyped here), the read opens the
    // value the binder holds.
    let (unit, diagnostics) = compile_unit(
        "forall x in tools: volume(x.body) == volume(x.body)",
        &scope,
        &module,
    );
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let binder = quantifier_variable(&unit.expr);
    let opened: Vec<_> = unit
        .references
        .iter()
        .filter(|r| r.root != ReferenceRoot::Own)
        .collect();
    assert_eq!(
        opened.iter().map(|r| shape(r)).collect::<Vec<_>>(),
        vec![(
            ReferenceRoot::Binder { id: binder },
            members(&["body"]),
            ReferenceRead::Value
        )]
    );
    let opened_id = opened[0].id.clone();
    let reads = nodes_where(
        &unit.expr,
        |node| matches!(&node.kind, CompiledExprKind::ValueRef(id) if id == &opened_id),
    );
    assert_eq!(reads.len(), 2, "{:?}", unit.expr);
    assert!(
        reads.iter().all(|read| read.result_type == Type::Geometry),
        "{reads:?}"
    );

    // A parameter is a field the value carries: today's projection.
    let (unit, diagnostics) = compile_unit("forall x in tools: x.r > 0", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let binder = quantifier_variable(&unit.expr);
    let projections = nodes_where(&unit.expr, |node| {
        matches!(&node.kind, CompiledExprKind::IndexAccess { object, .. }
            if matches!(&object.kind, CompiledExprKind::ValueRef(id) if id == &binder))
    });
    assert_eq!(projections.len(), 1, "{:?}", unit.expr);
    assert!(
        unit.references
            .iter()
            .all(|r| shape(r) == own_value(members(&["tools"]))),
        "{:?}",
        unit.references
    );

    // A name the value's definition does not declare.
    let (_, diagnostics) = compile_unit("forall x in tools: x.nosuch > 0", &scope, &module);
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::StructureMemberNotFound),
        "{diagnostics:?}"
    );

    // The existing compile, on the same scope (outcomes measured when this
    // test was written): a field projection of the cell, and the sub's
    // member stamped with the definition's name.
    let (tree, diagnostics) = compile_existing("t.body", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    let CompiledExprKind::IndexAccess { object, .. } = &tree.kind else {
        panic!("expected a field projection, got {tree:?}");
    };
    assert!(
        matches!(&object.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm", "t")),
        "{object:?}"
    );
    assert_eq!(tree.result_type, Type::Geometry);
    let (tree, diagnostics) = compile_existing("self.held.body", &scope, &module);
    assert!(errors_of(&diagnostics).is_empty(), "{diagnostics:?}");
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Asm.held", "body")),
        "{tree:?}"
    );
}

const BRACES_SOURCE: &str = "
trait Rated {
    param rating : Real
}
structure def Shaft : Rated {
    param len : Real = 5
    param rating : Real = 2
}
structure def Motor {
    param d : Real = 10
    param lock : Bool = false
    where lock { param torque : Real = 1 }
    sub shaft : Shaft
}
structure def Plate : Rated {
    param t : Real = 3
    param rating : Real = 7
}
structure def Rig {
    param d : Real = 99
    param w : Real = 3
    param t : Real = 8
    param tight : Bool = false
    let half = w / 2
    sub m : Motor {
        d = w where lock
        lock = true where tight
    }
    sub p = Plate(t: t / 2)
}
";

/// The `sub` named `sub` of definition `definition` in the parsed source.
fn parsed_sub<'p>(
    parsed: &'p reify_ast::ParsedModule,
    definition: &str,
    sub: &str,
) -> &'p reify_ast::SubDecl {
    parsed
        .declarations
        .iter()
        .find_map(|decl| match decl {
            reify_ast::Declaration::Structure(def) if def.name == definition => {
                def.members.iter().find_map(|m| match m {
                    reify_ast::MemberDecl::Sub(s) if s.name == sub => Some(s),
                    _ => None,
                })
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no sub {sub} in {definition}"))
}

/// PRD §5.4 "What declares what", D13 items 14 and 19, D10(g): in a sub's
/// braces a name is first a `let` of the same braces, then a member of the
/// sub's definition whether present or not, then a member of the definition
/// that wrote the braces; `self` is the writer.
#[test]
fn braces_names_resolve_against_two_scopes() {
    let module = compile_clean(BRACES_SOURCE);
    let registry = template_registry(&module);
    let motor = definition_scope(&module, &registry, "Motor");
    let rig = definition_scope(&module, &registry, "Rig");
    let real = Type::dimensionless_scalar();
    let lets = std::collections::BTreeMap::from([("h".to_string(), real.clone())]);
    let braces = SourceUnitNames::Braces {
        lets: &lets,
        writer: &rig,
    };
    let in_braces = |expr: &reify_ast::Expr| compile_unit_named(expr, braces, &motor, &module);
    let read = |root: ReferenceRoot, path: &[&str]| (root, members(path), ReferenceRead::Value);

    for (text, expected, ty) in [
        // The sub's own member comes first, although the writer declares `d` too.
        ("d", read(ReferenceRoot::Own, &["d"]), &real),
        ("h", read(ReferenceRoot::BracesLet, &["h"]), &real),
        // A guarded member, whether present or not.
        ("torque", read(ReferenceRoot::Own, &["torque"]), &real),
        (
            "shaft.len",
            read(ReferenceRoot::Own, &["shaft", "len"]),
            &real,
        ),
        ("w", read(ReferenceRoot::Writer, &["w"]), &real),
        ("half", read(ReferenceRoot::Writer, &["half"]), &real),
        (
            "self",
            read(ReferenceRoot::Writer, &[]),
            &Type::StructureRef("Rig".into()),
        ),
        ("self.d", read(ReferenceRoot::Writer, &["d"]), &real),
        // A trait-qualified read resolves its sub by the same two scopes.
        (
            "shaft.(Rated::rating)",
            read(ReferenceRoot::Own, &["shaft", "rating"]),
            &real,
        ),
        (
            "p.(Rated::rating)",
            read(ReferenceRoot::Writer, &["p", "rating"]),
            &real,
        ),
    ] {
        let (unit, diagnostics) = in_braces(&parse_expr(text));
        assert!(diagnostics.is_empty(), "{text}: {diagnostics:?}");
        let (reference, actual) = sole_reference(&unit);
        assert_eq!(shape(reference), expected, "{text}");
        assert_eq!(actual, ty, "{text}");
    }
    let (_, diagnostics) = in_braces(&parse_expr("nosuch"));
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::UnresolvedName),
        "{diagnostics:?}"
    );
    let (_, diagnostics) = in_braces(&parse_expr("m.(Rated::rating)"));
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::TraitNotImplemented),
        "{diagnostics:?}"
    );
    let (_, diagnostics) = in_braces(&parse_expr("nosuch.(Rated::rating)"));
    assert!(
        has_error_code(&diagnostics, DiagnosticCode::UnresolvedName),
        "{diagnostics:?}"
    );

    // A guard's condition and an assignment's right side get both scopes too.
    let parsed = reify_syntax::parse(BRACES_SOURCE, reify_core::ModulePath::single("unit_mode"));
    let m = parsed_sub(&parsed, "Rig", "m");
    let mut guards = Vec::new();
    for (name, value, guard) in m.guarded_spec_param_overrides() {
        let guard = guard.unwrap_or_else(|| panic!("`{name}` has no guard"));
        let (unit, diagnostics) = in_braces(&guard.condition);
        assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
        let (reference, ty) = sole_reference(&unit);
        assert_eq!(ty, &Type::Bool, "{name}");
        guards.push((name, shape(reference)));
        if name == "d" {
            let (unit, diagnostics) = in_braces(value);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let (reference, _) = sole_reference(&unit);
            assert_eq!(shape(reference), read(ReferenceRoot::Writer, &["w"]));
        }
    }
    assert_eq!(
        guards,
        vec![
            ("d", read(ReferenceRoot::Own, &["lock"])),
            ("lock", read(ReferenceRoot::Writer, &["tight"])),
        ]
    );

    // The call form of a sub needs no mode of its own: its argument is
    // written in, and read against, the writer (D10(g)).
    let p = parsed_sub(&parsed, "Rig", "p");
    let (_, argument) = &p.args[0];
    let (unit, diagnostics) = compile_unit_named(argument, SourceUnitNames::Own, &rig, &module);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let shapes: Vec<_> = unit.references.iter().map(shape).collect();
    assert_eq!(shapes, vec![own_value(members(&["t"]))]);

    // The existing compile reads the braces' right side on the writer's scope.
    let (tree, _) = compile_existing("d", &rig, &module);
    assert!(
        matches!(&tree.kind, CompiledExprKind::ValueRef(id) if id == &ValueCellId::new("Rig", "d")),
        "{tree:?}"
    );
}
