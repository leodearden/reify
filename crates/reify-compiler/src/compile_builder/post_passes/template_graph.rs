//! Structural queries over the module's sub-component graph, shared by the
//! post-passes that must reason about which template can instantiate which.
//!
//! A leaf: it reads only `crate::types`, so every pass can depend on it
//! without the passes depending on each other.

use std::collections::HashSet;

use crate::types::{CompiledModule, TopologyTemplate};

/// Does `template_name` denote the structure `structure_name`, allowing for
/// monomorphisation?
///
/// A generic structure is compiled once per instantiation under the mangled
/// name `Generic$Arg` (`types::mangle_monomorph_name`), while the sub
/// declaration that instantiated it still records the *unmangled*
/// `structure_name`. Comparing the two by equality alone would miss the link.
pub(super) fn names_same_structure(template_name: &str, structure_name: &str) -> bool {
    template_name == structure_name
        || template_name
            .strip_prefix(structure_name)
            .is_some_and(|rest| rest.starts_with('$'))
}

/// Every template name the prelude declares, flattened once per pass.
///
/// Deliberately UNFILTERED by visibility, mirroring the registry
/// `phase_auto_type_param_resolution` monomorphises from: a private imported
/// generic reaches `ctx.templates` as a clone too, so filtering here would let
/// a clone of an imported template pass as a local one.
pub(super) fn imported_template_names<'p>(prelude: &[&'p CompiledModule]) -> Vec<&'p str> {
    prelude
        .iter()
        .flat_map(|m| m.templates.iter().map(|t| t.name.as_str()))
        .collect()
}

/// Where one sub-component edge leads, as [`search_sub_components`] resolves it.
pub(super) enum SubTarget<'t> {
    /// A local template the sub may instantiate. Reported once per walk: the
    /// first time any edge reaches it.
    Local(&'t TopologyTemplate),
    /// The sub's `structure_name`, which no local template matches.
    Unresolved(&'t str),
}

/// What [`search_sub_components`] does after a visit.
pub(super) enum Visit {
    /// Walk on into this local template's own subs.
    Descend,
    /// Walk on, but not below this target.
    Prune,
    /// Stop: the search has its answer.
    Found,
}

/// Depth-first search of the templates an instance of `root` can hold, at any
/// depth, `root` itself included if it holds itself; `true` iff `visit`
/// answers [`Visit::Found`] for some target.
///
/// Each sub resolves through [`names_same_structure`] against `all_templates`,
/// and EVERY match is visited, since a locally monomorphised generic puts both
/// `G` and `G$Arg` in the module and stopping at the first would let the other
/// go unseen. A sub matching no local template is reported as
/// [`SubTarget::Unresolved`], so each caller states what an unknown subtree
/// may hold. A `seen` set keyed on template name makes a recursive structure
/// terminate without relying on recursion detection having run first.
pub(super) fn search_sub_components<'t>(
    root: &'t TopologyTemplate,
    all_templates: &'t [TopologyTemplate],
    mut visit: impl FnMut(SubTarget<'t>) -> Visit,
) -> bool {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut frontier: Vec<&TopologyTemplate> = vec![root];

    while let Some(current) = frontier.pop() {
        for sub in &current.sub_components {
            let mut local = all_templates
                .iter()
                .filter(|t| names_same_structure(&t.name, &sub.structure_name))
                .peekable();
            if local.peek().is_none() {
                if let Visit::Found = visit(SubTarget::Unresolved(&sub.structure_name)) {
                    return true;
                }
                continue;
            }
            for child in local {
                if !seen.insert(child.name.as_str()) {
                    continue;
                }
                match visit(SubTarget::Local(child)) {
                    Visit::Found => return true,
                    Visit::Descend => frontier.push(child),
                    Visit::Prune => {}
                }
            }
        }
    }

    false
}

/// Can an instance of `container` hold, at any depth, an instance of the local
/// template named `target_name`?
///
/// A sub resolving to no local template but to an imported one cannot hold a
/// local template (an imported module cannot name this one), so it is pruned;
/// a sub resolving nowhere at all may hold anything, so it answers `true`.
pub(super) fn may_contain(
    container: &TopologyTemplate,
    target_name: &str,
    all_templates: &[TopologyTemplate],
    imported_names: &[&str],
) -> bool {
    search_sub_components(container, all_templates, |target| match target {
        SubTarget::Local(child) if child.name == target_name => Visit::Found,
        SubTarget::Local(_) => Visit::Descend,
        SubTarget::Unresolved(structure_name)
            if imported_names
                .iter()
                .any(|name| names_same_structure(name, structure_name)) =>
        {
            Visit::Prune
        }
        SubTarget::Unresolved(_) => Visit::Found,
    })
}
