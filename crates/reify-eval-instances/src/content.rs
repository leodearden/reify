//! The content this crate gives the core of a result, an input or a
//! description (contract 5.2): a hash of everything a reader can see of the
//! value (O7). Every content is std `Hash` fed to one xxh3 hasher.
//!
//! A member table's content is hashed field by field: `Member`, `DeclaredType`
//! and `MemberOrigin` are destructured with no `..` and matched with no `_`,
//! so a field or variant the second output adds is a compile error here
//! rather than a content that silently stops covering it.

use std::hash::{Hash, Hasher};

use reify_eval_core::Hash128;
use reify_eval_source::{DeclaredType, Member, MemberOrigin, MemberTable};
use xxhash_rust::xxh3::Xxh3;

/// The content of `value`: its std `Hash`, through xxh3.
pub(crate) fn content_of(value: &impl Hash) -> Hash128 {
    let mut hasher = Xxh3::new();
    value.hash(&mut hasher);
    Hash128::from_u128(hasher.digest128())
}

/// The content of a definition's member table.
pub(crate) fn member_table_content(table: &MemberTable) -> Hash128 {
    content_of(&Table(table))
}

/// A member table, hashed as its count, then each member in order.
struct Table<'t>(&'t MemberTable);

impl Hash for Table<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(self.0.len());
        for member in self.0 {
            hash_member(member, state);
        }
    }
}

fn hash_member<H: Hasher>(member: &Member, state: &mut H) {
    let Member {
        name,
        kind,
        declared_type,
        origin,
        guarded,
        branches,
    } = member;
    name.hash(state);
    kind.hash(state);
    hash_declared_type(declared_type, state);
    hash_origin(origin, state);
    guarded.hash(state);
    state.write_usize(branches.len());
    for branch in branches {
        Table(branch).hash(state);
    }
}

fn hash_declared_type<H: Hasher>(declared_type: &DeclaredType, state: &mut H) {
    match declared_type {
        DeclaredType::None => state.write_u8(0),
        DeclaredType::Value(ty) => {
            state.write_u8(1);
            ty.hash(state);
        }
        DeclaredType::Definition(definition) => {
            state.write_u8(2);
            definition.hash(state);
        }
        DeclaredType::Written(written) => {
            state.write_u8(3);
            written.hash(state);
        }
    }
}

fn hash_origin<H: Hasher>(origin: &MemberOrigin, state: &mut H) {
    match origin {
        MemberOrigin::Own => state.write_u8(0),
        MemberOrigin::Inherited(from) => {
            state.write_u8(1);
            from.hash(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_eval_source::MemberTable;

    use super::{content_of, member_table_content};
    use crate::member_kinds::MemberKinds;
    use crate::test_support::{compiled, in_entry};

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
        (
            "declared type",
            "structure def A { param t : Angle = 1deg }",
        ),
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
        let definitions = compiled(source);
        let definition = definitions.get(&in_entry("A"));
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

    #[test]
    fn a_table_of_member_kinds_has_a_content_of_its_rows() {
        assert_eq!(
            content_of(&MemberKinds::built()),
            content_of(&MemberKinds::built())
        );
        assert_ne!(
            content_of(&MemberKinds::built()),
            content_of(&MemberKinds::empty())
        );
    }
}
