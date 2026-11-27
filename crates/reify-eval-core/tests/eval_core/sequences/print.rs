//! Prints a case as a complete Rust test file, every value written as a
//! call of design.rs's constructors or an enum variant, so a failing case
//! found by the random run can be pasted in as a named test.

use reify_eval_core::{Limit, Link, Space, Step, StepKey};

use super::case::{Case, HostEvent, Then};
use super::design::{
    Bind, Decl, Key, ModelPath, Site, ToyKind, Val, call, cell, collection, guard, measure, picker,
    scope, shape,
};
use crate::toy;

/// What the printed test asserts of its case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expectation {
    /// Check 1 reports the faulty cell, and the honest kinds pass it.
    CaughtWithFaultyCell,
    /// The honest kinds pass it: a regression for a fault fixed.
    PassesWithToyKinds,
}

/// The test file named `name` holding `case`, shrunk from seed `seed`,
/// asserting `expectation`. The header and `use` lines are in rustfmt's
/// form; the test carries `#[rustfmt::skip]`, one input or event a line.
pub fn print_test(case: &Case, seed: u64, name: &str, expectation: Expectation) -> String {
    let mut text = format!(
        "//! A case shrunk from seed {seed} of the random sequences and printed by\n\
         //! sequences::print. To regenerate it, run the test that printed it and paste\n\
         //! the text its failure message shows.\n\n\
         use super::case::*;\n\
         use super::design::*;\n\n\
         #[rustfmt::skip]\n\
         #[test]\n\
         fn {name}() {{\n    \
         let case = Case {{\n        \
         inputs: inputs([\n"
    );
    for (key, value) in &case.inputs {
        text += &format!("            ({}, {}),\n", key_text(key), val_text(value));
    }
    text += "        ]),\n        events: vec![\n";
    for event in &case.events {
        text += &format!("            {},\n", event_text(event));
    }
    text += "        ],\n    };\n";
    text += match expectation {
        Expectation::CaughtWithFaultyCell => "    assert!(super::faulty::caught(&case));\n",
        Expectation::PassesWithToyKinds => {
            "    if let Err(mismatch) = super::session::check(&case, &super::session::toy_core) \
             { panic!(\"{mismatch}\") }\n"
        }
    };
    text + "}\n"
}

fn list(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(", ")
}

/// A step as the call that rebuilds it, space and all: design.rs's
/// constructors make default-space names and indices and a call's child
/// steps, so any other chain, or a step in a named space, prints as the
/// core's own constructor.
fn step_text(step: &Step) -> String {
    if let Some(text) = call_step_text(step) {
        return text;
    }
    match (step.space(), step.key()) {
        (Space::Default, StepKey::Name(name)) => format!("step({:?})", &**name),
        (Space::Default, StepKey::Index(index)) => format!("index({index})"),
        (space, StepKey::Chain(links)) => format!(
            "reify_eval_core::Step::chain({}, [{}])",
            space_text(space),
            list(links.iter().map(link_text))
        ),
        (Space::Named(space), key) => format!(
            "reify_eval_core::Step::new(reify_eval_core::Space::Named({space:?}), {})",
            step_key_text(key)
        ),
    }
}

/// A step `call_step` makes, as that call: in the call space, site links,
/// then at most one iteration link.
fn call_step_text(step: &Step) -> Option<String> {
    let StepKey::Chain(links) = step.key() else {
        return None;
    };
    if step.space() != toy::CALL_SPACE {
        return None;
    }
    let (sites, each) = match links.split_last() {
        Some((Link::Iteration(index), sites)) => (sites, Some(*index)),
        _ => (&links[..], None),
    };
    let sites: Vec<u64> = sites
        .iter()
        .map(|link| match link {
            Link::Site(site) => Some(*site),
            Link::Iteration(_) => None,
        })
        .collect::<Option<_>>()?;
    let each = each.map_or_else(|| String::from("None"), |index| format!("Some({index})"));
    Some(format!("call_step(&{sites:?}, {each})"))
}

fn step_key_text(key: &StepKey) -> String {
    match key {
        StepKey::Name(name) => format!("reify_eval_core::StepKey::Name({:?}.into())", &**name),
        StepKey::Index(index) => format!("reify_eval_core::StepKey::Index({index})"),
        StepKey::Chain(links) => format!(
            "reify_eval_core::StepKey::Chain([{}].into())",
            list(links.iter().map(link_text))
        ),
    }
}

fn space_text(space: Space) -> String {
    match space {
        Space::Default => String::from("reify_eval_core::Space::Default"),
        Space::Named(name) => format!("reify_eval_core::Space::Named({name:?})"),
    }
}

fn link_text(link: &Link) -> String {
    match link {
        Link::Site(site) => format!("reify_eval_core::Link::Site({site})"),
        Link::Iteration(index) => format!("reify_eval_core::Link::Iteration({index})"),
    }
}

fn steps_text(steps: &[Step]) -> String {
    format!("[{}]", list(steps.iter().map(step_text)))
}

fn path_text(path: &ModelPath) -> String {
    format!("path({:?}, {})", path.root, steps_text(&path.steps))
}

fn key_text(key: &Key) -> String {
    match key {
        Key::Root(name) => format!("root({name:?})"),
        Key::Named(name) => format!("named({name:?})"),
        Key::Limit(Limit::Depth) => String::from("depth_limit()"),
        Key::Limit(Limit::ChildrenPerNode) => String::from("children_limit()"),
    }
}

fn bind_text(bind: &Bind) -> String {
    match bind {
        Bind::Input(name) => format!("input({name:?})"),
        Bind::Declarer(place) => format!("declarer({})", steps_text(place)),
        Bind::Passed(r) => format!("passed({:?}, {})", r.binding, steps_text(&r.steps)),
        Bind::Constant(value) => format!("constant({})", val_text(value)),
    }
}

fn kind_text(kind: ToyKind) -> String {
    format!("ToyKind::{kind:?}")
}

/// A declaration as the constructor that makes it, where one does, else
/// as its kind and bindings.
fn decl_text(decl: &Decl) -> String {
    let bound = |name: &str| decl.bindings.get(name).cloned();
    if let (ToyKind::Cell, Some(divisor)) = (decl.kind, bound("divisor")) {
        let mut undivided = decl.clone();
        undivided.bindings.remove("divisor");
        return format!(
            "divided({}, {})",
            decl_text(&undivided),
            bind_text(&divisor)
        );
    }
    let terms: Vec<_> = (0..decl.bindings.len())
        .map_while(|at| bound(&at.to_string()))
        .collect();
    if *decl == cell(terms.clone()) {
        return format!("cell([{}])", list(terms.iter().map(bind_text)));
    }
    if let (ToyKind::Call, Some(condition)) = (decl.kind, bound("when")) {
        let mut unconditional = decl.clone();
        unconditional.bindings.remove("when");
        return format!(
            "when({}, {})",
            decl_text(&unconditional),
            bind_text(&condition)
        );
    }
    if let Some(calls) = bound("calls")
        && *decl == call(terms.clone(), calls.clone())
    {
        let terms = list(terms.iter().map(bind_text));
        return format!("call([{terms}], {})", bind_text(&calls));
    }
    if let (Some(pick), Some(from)) = (bound("pick"), bound("from"))
        && *decl == picker(pick.clone(), from.clone())
    {
        return format!("picker({}, {})", bind_text(&pick), bind_text(&from));
    }
    if let Some(Bind::Input(members)) = bound("members") {
        if *decl == scope(&members) {
            return format!("scope({members:?})");
        }
        if let Some(condition) = bound("condition")
            && *decl == guard(condition.clone(), &members)
        {
            return format!("guard({}, {members:?})", bind_text(&condition));
        }
    }
    if let (Some(count), Some(element)) = (bound("count"), bound("element"))
        && *decl == collection(count.clone(), element.clone())
    {
        return format!("collection({}, {})", bind_text(&count), bind_text(&element));
    }
    if let Some(side) = bound("side")
        && *decl == shape(side.clone())
    {
        return format!("shape({})", bind_text(&side));
    }
    if let Some(of) = bound("shape")
        && *decl == measure(of.clone())
    {
        return format!("measure({})", bind_text(&of));
    }
    let bindings = decl
        .bindings
        .iter()
        .map(|(name, bind)| format!("({name:?}, {})", bind_text(bind)));
    format!("decl({}, [{}])", kind_text(decl.kind), list(bindings))
}

fn val_text(value: &Val) -> String {
    match value {
        Val::Int(value) => format!("int({value})"),
        Val::Bool(value) => format!("flag({value})"),
        Val::Members(members) => {
            let each = members
                .iter()
                .map(|(step, decl)| format!("({}, {})", step_text(step), decl_text(decl)));
            format!("members([{}])", list(each))
        }
        Val::Decl(decl) => format!("declaration({})", decl_text(decl)),
        Val::Bound(bound) => format!("bound({bound})"),
        Val::Calls(sites) => format!("calls_value([{}])", list(sites.iter().map(site_text))),
    }
}

/// A site as `site` makes it where its argument is at "0", else as the
/// struct.
fn site_text(site: &Site) -> String {
    let Site {
        sites,
        each,
        offset,
        argument,
        callee,
    } = site;
    let callee = decl_text(callee);
    if argument == "0" {
        return format!("site(&{sites:?}, {each}, {offset}, {callee})");
    }
    format!(
        "Site {{ sites: vec!{sites:?}, each: {each}, offset: {offset}, argument: \
         String::from({argument:?}), callee: {callee} }}"
    )
}

fn then_text(then: Then) -> &'static str {
    match then {
        Then::Leave => "Then::Leave",
        Then::End => "Then::End",
    }
}

fn budget_text(budget: Option<u64>) -> String {
    match budget {
        Some(budget) => format!("Some({budget})"),
        None => String::from("None"),
    }
}

fn paths_text(paths: &[ModelPath]) -> String {
    format!("vec![{}]", list(paths.iter().map(path_text)))
}

fn event_text(event: &HostEvent) -> String {
    match event {
        HostEvent::Edit { from, changes } => {
            let each = changes.iter().map(|(key, value)| {
                let value = value.as_ref().map_or_else(
                    || String::from("None"),
                    |value| format!("Some({})", val_text(value)),
                );
                format!("({}, {value})", key_text(key))
            });
            format!(
                "HostEvent::Edit {{ from: {from}, changes: vec![{}] }}",
                list(each)
            )
        }
        HostEvent::Evaluate {
            on,
            paths,
            budget,
            then,
        } => format!(
            "HostEvent::Evaluate {{ on: {on}, paths: {}, budget: {}, then: {} }}",
            paths_text(paths),
            budget_text(*budget),
            then_text(*then)
        ),
        HostEvent::Drive {
            on,
            paths,
            turns,
            then,
        } => format!(
            "HostEvent::Drive {{ on: {on}, paths: {}, turns: {turns}, then: {} }}",
            paths_text(paths),
            then_text(*then)
        ),
    }
}

mod tests {
    use reify_eval_core::{Space, Step, StepKey};

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::*;
    use super::{Expectation, print_test};

    const OTHER: Space = Space::Named("toy.other");

    /// A case using every key, value, binding, step, event and ending.
    fn every_variant() -> Case {
        let first = cell([
            input("i"),
            declarer([step("m1"), index(2)]),
            passed("count", [index(0)]),
        ]);
        Case {
            inputs: inputs([
                (root("r"), declaration(scope("s"))),
                (
                    named("s"),
                    members([
                        (step("m0"), first),
                        (step("m1"), collection(input("n"), input("e"))),
                        (step("m2"), picker(input("i"), declarer([step("m1")]))),
                        (
                            step("m3"),
                            divided(cell([input("i")]), declarer([step("m0")])),
                        ),
                        (step("m4"), shape(input("i"))),
                        (step("m5"), measure(declarer([step("m4")]))),
                        (
                            Step::new(OTHER, StepKey::Name("m6".into())),
                            shape(input("i")),
                        ),
                    ]),
                ),
                (named("f"), flag(true)),
                (named("i"), int(-3)),
                (named("e"), declaration(cell([input("x")]))),
                (root("g"), declaration(guard(input("f"), "s"))),
                (
                    root("odd"),
                    declaration(decl(ToyKind::Collection, [("count", input("n"))])),
                ),
                (
                    root("k"),
                    declaration(when(
                        call([input("i"), constant(int(2))], input("calls")),
                        input("f"),
                    )),
                ),
                (
                    named("calls"),
                    calls_value([
                        site(&[1, 7], true, -1, cell([input("x")])),
                        Site {
                            sites: vec![2],
                            each: false,
                            offset: 0,
                            argument: String::from("given"),
                            callee: cell([]),
                        },
                    ]),
                ),
            ]),
            events: vec![
                HostEvent::Edit {
                    from: 0,
                    changes: vec![(named("i"), Some(int(4))), (named("f"), None)],
                },
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![
                        path("r", [step("m0")]),
                        path("g", []),
                        path("k", [call_step(&[1, 7], Some(0))]),
                        path("k", [call_step(&[2], None)]),
                    ],
                    budget: Some(5),
                    then: Then::Leave,
                },
                HostEvent::Drive {
                    on: 0,
                    paths: vec![
                        path("r", [step("m1"), index(3)]),
                        path("r", [Step::new(OTHER, StepKey::Index(0))]),
                    ],
                    turns: 2,
                    then: Then::End,
                },
            ],
        }
    }

    const PRINTED: &str = r##"//! A case shrunk from seed 7 of the random sequences and printed by
//! sequences::print. To regenerate it, run the test that printed it and paste
//! the text its failure message shows.

use super::case::*;
use super::design::*;

#[rustfmt::skip]
#[test]
fn printed_case() {
    let case = Case {
        inputs: inputs([
            (root("g"), declaration(guard(input("f"), "s"))),
            (root("k"), declaration(when(call([input("i"), constant(int(2))], input("calls")), input("f")))),
            (root("odd"), declaration(decl(ToyKind::Collection, [("count", input("n"))]))),
            (root("r"), declaration(scope("s"))),
            (named("calls"), calls_value([site(&[1, 7], true, -1, cell([input("x")])), Site { sites: vec![2], each: false, offset: 0, argument: String::from("given"), callee: cell([]) }])),
            (named("e"), declaration(cell([input("x")]))),
            (named("f"), flag(true)),
            (named("i"), int(-3)),
            (named("s"), members([(step("m0"), cell([input("i"), declarer([step("m1"), index(2)]), passed("count", [index(0)])])), (step("m1"), collection(input("n"), input("e"))), (step("m2"), picker(input("i"), declarer([step("m1")]))), (step("m3"), divided(cell([input("i")]), declarer([step("m0")]))), (step("m4"), shape(input("i"))), (step("m5"), measure(declarer([step("m4")]))), (reify_eval_core::Step::new(reify_eval_core::Space::Named("toy.other"), reify_eval_core::StepKey::Name("m6".into())), shape(input("i")))])),
        ]),
        events: vec![
            HostEvent::Edit { from: 0, changes: vec![(named("i"), Some(int(4))), (named("f"), None)] },
            HostEvent::Evaluate { on: 1, paths: vec![path("r", [step("m0")]), path("g", []), path("k", [call_step(&[1, 7], Some(0))]), path("k", [call_step(&[2], None)])], budget: Some(5), then: Then::Leave },
            HostEvent::Drive { on: 0, paths: vec![path("r", [step("m1"), index(3)]), path("r", [reify_eval_core::Step::new(reify_eval_core::Space::Named("toy.other"), reify_eval_core::StepKey::Index(0))])], turns: 2, then: Then::End },
        ],
    };
"##;

    #[test]
    fn a_case_prints_as_constructor_calls_asserting_it_is_caught() {
        let printed = print_test(
            &every_variant(),
            7,
            "printed_case",
            Expectation::CaughtWithFaultyCell,
        );
        let expected = format!("{PRINTED}    assert!(super::faulty::caught(&case));\n}}\n");
        assert_eq!(printed, expected, "printed:\n{printed}");
    }

    #[test]
    fn a_case_prints_as_constructor_calls_asserting_the_toy_kinds_pass_it() {
        let printed = print_test(
            &every_variant(),
            7,
            "printed_case",
            Expectation::PassesWithToyKinds,
        );
        let check = "if let Err(mismatch) = super::session::check(&case, &super::session::toy_core) \
                     { panic!(\"{mismatch}\") }";
        let expected = format!("{PRINTED}    {check}\n}}\n");
        assert_eq!(printed, expected, "printed:\n{printed}");
    }
}
