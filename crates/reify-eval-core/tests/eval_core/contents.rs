//! The content of a declaration, which the core supplies because it owns
//! the declaration's representation (contract 5.2): it follows the kind and
//! what each binding names, never the order the bindings were set in.

use reify_eval_core::{
    Binding, Declaration, InputKey, Kind, KindId, Link, Ref, Space, Step, StepKey,
};

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
            "a declarer step's space",
            base().bind(
                "1",
                Binding::Declarer(vec![Step::new(
                    Space::Named("toy.other"),
                    StepKey::Name("b".into()),
                )]),
            ),
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
    ];
    for (what, declaration) in changed {
        assert_ne!(declaration.content(), base().content(), "{what}");
    }
}

#[test]
fn a_chain_step_s_links_and_space_enter_a_declaration_s_content() {
    let declaring = |step: Step| Declaration::new(Cell::ID).bind("0", Binding::Declarer(vec![step]));
    let chain = |space: Space, links: &[Link]| declaring(Step::chain(space, links.iter().copied()));
    let call = Space::Named("toy.call");

    assert_eq!(
        chain(call, &[Link::Site(1), Link::Iteration(2)]).content(),
        chain(call, &[Link::Site(1), Link::Iteration(2)]).content()
    );
    let differing = [
        (
            "a site and an iteration",
            chain(call, &[Link::Site(1)]),
            chain(call, &[Link::Iteration(1)]),
        ),
        (
            "the order of the links",
            chain(call, &[Link::Site(1), Link::Site(2)]),
            chain(call, &[Link::Site(2), Link::Site(1)]),
        ),
        (
            "the space",
            chain(Space::Default, &[Link::Site(1)]),
            chain(call, &[Link::Site(1)]),
        ),
        (
            "a chain and a name",
            chain(Space::Default, &[Link::Site(1)]),
            declaring(Step::new(Space::Default, StepKey::Name("1".into()))),
        ),
    ];
    for (what, one, other) in differing {
        assert_ne!(one.content(), other.content(), "{what}");
    }
}
