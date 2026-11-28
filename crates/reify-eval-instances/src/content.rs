#[cfg(test)]
mod tests {
    use std::path::Path;

    use reify_core::{DefinitionName, PathName};
    use reify_eval_source::{MemberTable, compile_entry};

    use super::{content_of, member_table_content};

    const BASE: &str = "structure def A { param t : Length = 1mm }";

    /// Each source differs from `BASE` in one thing a reader of A's member
    /// table can see, named beside it.
    const VARIANTS: [(&str, &str); 7] = [
        ("name", "structure def A { param u : Length = 1mm }"),
        (
            "kind: a sub",
            "structure def B {}\nstructure def A { sub t : B }",
        ),
        (
            "kind: a geometry member",
            "structure def A { let t = box(1mm, 1mm, 1mm) }",
        ),
        ("declared type", "structure def A { param t : Angle = 1deg }"),
        (
            "trailing guard",
            "structure def A { param t : Length = 1mm where true }",
        ),
        (
            "origin",
            "trait T { param t : Length = 1mm }\nstructure def A : T {}",
        ),
        (
            "branches",
            "structure def A { where true { param t : Length = 1mm } }",
        ),
    ];

    /// The member table of definition `A` in `source`.
    fn members_of_a(source: &str) -> MemberTable {
        let output = compile_entry(Path::new("/nonexistent/content.ri"), source)
            .unwrap_or_else(|refusal| panic!("the second output lowers {source:?}: {refusal:?}"));
        let a = DefinitionName::new(None, PathName::new("A").expect("an identifier"));
        let definition = output.definitions.get(&a);
        let definition = definition.unwrap_or_else(|| panic!("{source:?} defines A"));
        definition.members.clone()
    }

    #[test]
    fn a_member_table_compiled_twice_has_one_content() {
        for (_, source) in VARIANTS.iter().chain([&("base", BASE)]) {
            assert_eq!(
                member_table_content(&members_of_a(source)),
                member_table_content(&members_of_a(source)),
                "{source:?}"
            );
        }
    }

    #[test]
    fn a_member_table_differing_in_one_thing_has_another_content() {
        let base = member_table_content(&members_of_a(BASE));
        for (what, source) in VARIANTS {
            assert_ne!(
                member_table_content(&members_of_a(source)),
                base,
                "a table differing in its {what} has another content: {source:?}"
            );
        }
    }

    #[test]
    fn equal_values_have_one_content_and_different_values_another() {
        assert_eq!(content_of(&(1_u32, "a")), content_of(&(1_u32, "a")));
        assert_ne!(content_of(&(1_u32, "a")), content_of(&(2_u32, "a")));
    }
}
