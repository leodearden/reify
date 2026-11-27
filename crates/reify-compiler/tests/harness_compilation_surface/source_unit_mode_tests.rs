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

use reify_compiler::{CompilationScope, CompiledModule, TopologyTemplate};
use reify_core::{Diagnostic, Severity, ValueCellId};

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
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let module = reify_compiler::compile_with_stdlib(&parsed);
    let errors = errors_of(&module.diagnostics);
    assert!(errors.is_empty(), "source does not compile clean: {errors:?}\n{source}");
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
    for cell in own.value_cells.iter().filter(|c| !c.id.member.starts_with("__")) {
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
