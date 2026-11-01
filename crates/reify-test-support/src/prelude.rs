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

#[cfg(test)]
mod tests {
    use super::member_type;
    use reify_core::{DimensionVector, Type};

    /// Two differently-typed members of one template: the lookup selects by member.
    #[test]
    fn member_type_returns_each_members_declared_type() {
        assert_eq!(
            member_type("Layer", "z"),
            Type::Scalar {
                dimension: DimensionVector::LENGTH
            }
        );
        assert_eq!(member_type("Layer", "index"), Type::Int);
    }

    #[test]
    #[should_panic(expected = "prelude has no template NoSuchTemplate")]
    fn member_type_panics_naming_an_unknown_template() {
        member_type("NoSuchTemplate", "z");
    }

    /// The panic lists the template's real members, not just the missing name.
    #[test]
    fn member_type_panics_listing_members_for_an_unknown_member() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            member_type("Layer", "no_such_member")
        }));
        let payload = match result {
            Ok(ty) => panic!("expected member_type to panic on a missing member, got: {ty:?}"),
            Err(payload) => payload,
        };
        let message = reify_core::panic_payload_to_string(payload.as_ref());
        assert!(
            message.contains("prelude template Layer has no member no_such_member; have:"),
            "panic does not name the template and missing member; got: {message}"
        );
        assert!(
            message.contains("\"z\"") && message.contains("\"index\""),
            "panic does not list Layer's real members; got: {message}"
        );
    }
}
