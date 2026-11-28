//! The scope a definition's expressions compile in, built from the existing
//! compile's template of the definition and the templates its module sees.

use std::collections::{BTreeMap, HashMap};

use reify_compiler::{CompilationScope, TopologyTemplate};
use reify_core::Type;

use crate::module_context::ModuleContext;

/// What one definition's expressions resolve names against: the existing
/// compile's template of the definition and the registry built for it, held
/// together so that a scope is never built over another registry.
pub(crate) struct DefinitionNames<'t, 'm> {
    own: &'t TopologyTemplate,
    /// The module's constructor view, with the target each of `own`'s subs
    /// and match arms binds to in place of its name's entry. A read through a
    /// sub walks this registry, so it holds the sub's own target, of any
    /// kind; a call is lowered as a constructor only for a structure entry.
    registry: HashMap<String, &'m TopologyTemplate>,
}

impl<'t, 'm> DefinitionNames<'t, 'm> {
    /// The names of the definition `own`, declared in `module`.
    pub(crate) fn new(own: &'t TopologyTemplate, module: &ModuleContext<'m>) -> Self {
        let arms = own
            .match_arm_groups
            .iter()
            .flat_map(|group| &group.arms)
            .filter_map(|arm| match &arm.arm_type {
                Type::StructureRef(arm_def) => Some(arm_def.as_str()),
                _ => None,
            });
        let mut registry = module.templates().clone();
        for written in own
            .sub_components
            .iter()
            .map(|sub| sub.element_structure_name())
            .chain(arms)
        {
            if let Some(target) = module.sub_target(written) {
                registry.insert(written.to_string(), target.template);
            }
        }
        Self { own, registry }
    }

    /// The scope the definition's expressions compile in.
    pub(crate) fn scope(&self) -> CompilationScope<'_> {
        scope_over(self.own, &self.registry)
    }
}

/// The scope the expressions of `own` compile in, over `registry`.
fn scope_over<'u>(
    own: &'u TopologyTemplate,
    registry: &'u HashMap<String, &'u TopologyTemplate>,
) -> CompilationScope<'u> {
    let mut scope = CompilationScope::new(&own.name);
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
                Type::StructureRef(arm_def) => registry
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
fn member_types(child: &TopologyTemplate) -> BTreeMap<String, Type> {
    child
        .value_cells
        .iter()
        .filter(|c| !c.id.member.starts_with("__"))
        .map(|c| (c.id.member.clone(), c.cell_type.clone()))
        .collect()
}
