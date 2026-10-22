//! Structural queries over the module's sub-component graph, shared by the
//! post-passes that must reason about which template can instantiate which.
//!
//! A leaf: it reads only `crate::types`, so every pass can depend on it
//! without the passes depending on each other.

use std::collections::HashSet;

use crate::types::TopologyTemplate;

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

/// Can an instance of `container` hold, at any depth, an instance of the
/// template named `target_name`?
///
/// Walks `sub_components` depth-first, resolving each `structure_name` through
/// [`names_same_structure`] against `all_templates` and visiting EVERY match,
/// since a locally monomorphised generic puts both `G` and `G$Arg` in the
/// module. Conservative wherever the graph is not fully known:
/// - a sub resolving to no local template but to an imported one cannot hold a
///   local template (an imported module cannot name this one), so it is
///   skipped;
/// - a sub resolving nowhere at all may hold anything, so it answers `true`.
///
/// A `seen` set keyed on template name makes a recursive structure terminate.
pub(super) fn may_contain(
    container: &TopologyTemplate,
    target_name: &str,
    all_templates: &[TopologyTemplate],
    imported_names: &[&str],
) -> bool {
    let mut seen: HashSet<&str> = HashSet::from([container.name.as_str()]);
    let mut frontier: Vec<&TopologyTemplate> = vec![container];

    while let Some(current) = frontier.pop() {
        for sub in &current.sub_components {
            if names_same_structure(target_name, &sub.structure_name) {
                return true;
            }
            let mut local = all_templates
                .iter()
                .filter(|t| names_same_structure(&t.name, &sub.structure_name))
                .peekable();
            if local.peek().is_none() {
                let imported = imported_names
                    .iter()
                    .any(|name| names_same_structure(name, &sub.structure_name));
                if imported {
                    continue;
                }
                return true;
            }
            for child in local {
                if seen.insert(child.name.as_str()) {
                    frontier.push(child);
                }
            }
        }
    }

    false
}
