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
    CompilationScope, CompiledModule, CompiledSourceUnit, LOCAL_REFERENCE_ENTITY, LocalReference,
    ReferenceRead, ReferenceRoot, SourceStep, SourceUnitNames, SourceUnitSpec, TopologyTemplate,
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
        scope
            .sub_component_types
            .insert(sub.name.clone(), sub.structure_name.clone());
        if sub.is_collection {
            scope.collection_sub_names.insert(sub.name.clone());
        }
        if let Some(child) = registry.get(&sub.structure_name) {
            scope
                .sub_member_types
                .insert(sub.name.clone(), member_types(child));
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
    let expr = parse_expr(text);
    let mut diagnostics = Vec::new();
    let spec = SourceUnitSpec {
        names: SourceUnitNames::Own,
        expected_type: None,
    };
    let unit = reify_compiler::compile_source_unit(
        &expr,
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
