//! Read-only introspection of the compiled stdlib prelude.

use reify_core::Type;

/// The DECLARED type of `<template>.<member>`, read straight off the compiled
/// prelude (`load_stdlib` -> `find_template` -> `value_cells`), so a test can
/// assert an exact `Type` equality rather than guess how a diagnostic renderer
/// spells a type.
///
/// Panics, naming the template (and, for a missing member, the members it
/// does have), when either lookup fails.
pub fn member_type(template_name: &str, member: &str) -> Type {
    let prelude = reify_compiler::stdlib_loader::load_stdlib();
    let tmpl = prelude
        .iter()
        .find_map(|m| reify_compiler::find_template(&m.templates, template_name))
        .unwrap_or_else(|| panic!("prelude has no template {template_name}"));
    tmpl.value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| {
            let members: Vec<&str> = tmpl
                .value_cells
                .iter()
                .map(|c| c.id.member.as_str())
                .collect();
            panic!("prelude template {template_name} has no member {member}; have: {members:?}")
        })
        .cell_type
        .clone()
}
