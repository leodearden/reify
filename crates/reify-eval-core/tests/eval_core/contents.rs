//! The content of a declaration, which the core supplies because it owns
//! the declaration's representation (contract 5.2): it follows the kind and
//! what each binding names, never the order the bindings were set in.

use reify_eval_core::{Binding, Declaration, InputKey, Kind, KindId, Ref, Step};

use crate::toy::{self, Cell};

fn base() -> Declaration {
    Declaration::new(Cell::ID)
        .bind("0", toy::input("a"))
        .bind("1", toy::sibling(&["b"]))
}

#[test]
fn a_declaration_s_content_follows_its_kind_and_bindings() {
    assert_eq!(base().content(), base().content());

    let reordered = Declaration::new(Cell::ID)
        .bind("1", toy::sibling(&["b"]))
        .bind("0", toy::input("a"));
    assert_eq!(reordered.content(), base().content());

    let changed = [
        (
            "the kind",
            Declaration::new(KindId("toy.other"))
                .bind("0", toy::input("a"))
                .bind("1", toy::sibling(&["b"])),
        ),
        (
            "a binding's name",
            Declaration::new(Cell::ID)
                .bind("2", toy::input("a"))
                .bind("1", toy::sibling(&["b"])),
        ),
        ("an input key", base().bind("0", toy::input("c"))),
        (
            "an input key's sort",
            base().bind("0", Binding::Input(InputKey::root("a"))),
        ),
        ("a declarer step", base().bind("1", toy::sibling(&["c"]))),
        (
            "a declarer step's sort",
            base().bind("1", Binding::Declarer(vec![Step::index(0)])),
        ),
        (
            "a passed ref",
            base().bind("1", Binding::Passed(Ref::binding("b"))),
        ),
        (
            "a passed ref's steps",
            base().bind(
                "1",
                Binding::Passed(Ref::binding("b").step(Step::named("x"))),
            ),
        ),
        (
            "an attached input",
            base().bind("1", Binding::Attached("b".into())),
        ),
    ];
    for (what, declaration) in changed {
        assert_ne!(declaration.content(), base().content(), "{what}");
    }
}
