//! Prints a case as a complete Rust test file, every value written as a
//! call of design.rs's constructors or an enum variant, so a failing case
//! found by the random run can be pasted in as a named test.

use reify_eval_core::Step;

use super::case::{Case, HostEvent, Then};
use super::design::{
    Bind, Decl, Key, ModelPath, ToyKind, Val, cell, collection, guard, measure, picker, scope,
    shape,
};

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

fn step_text(step: &Step) -> String {
    match step {
        Step::Named(name) => format!("step({:?})", &**name),
        Step::Index(index) => format!("index({index})"),
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
    }
}

fn bind_text(bind: &Bind) -> String {
    match bind {
        Bind::Input(name) => format!("input({name:?})"),
        Bind::Declarer(place) => format!("declarer({})", steps_text(place)),
        Bind::Passed(r) => format!("passed({:?}, {})", r.binding, steps_text(&r.steps)),
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
    }
}

fn then_text(then: Then) -> &'static str {
    match then {
        Then::Leave => "Then::Leave",
        Then::End => "Then::End",
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
        HostEvent::Evaluate { on, paths, then } => format!(
            "HostEvent::Evaluate {{ on: {on}, paths: {}, then: {} }}",
            paths_text(paths),
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
    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::*;
    use super::{Expectation, print_test};

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
            ]),
            events: vec![
                HostEvent::Edit {
                    from: 0,
                    changes: vec![(named("i"), Some(int(4))), (named("f"), None)],
                },
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![path("r", [step("m0")]), path("g", [])],
                    then: Then::Leave,
                },
                HostEvent::Drive {
                    on: 0,
                    paths: vec![path("r", [step("m1"), index(3)])],
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
            (root("odd"), declaration(decl(ToyKind::Collection, [("count", input("n"))]))),
            (root("r"), declaration(scope("s"))),
            (named("e"), declaration(cell([input("x")]))),
            (named("f"), flag(true)),
            (named("i"), int(-3)),
            (named("s"), members([(step("m0"), cell([input("i"), declarer([step("m1"), index(2)]), passed("count", [index(0)])])), (step("m1"), collection(input("n"), input("e"))), (step("m2"), picker(input("i"), declarer([step("m1")]))), (step("m3"), divided(cell([input("i")]), declarer([step("m0")]))), (step("m4"), shape(input("i"))), (step("m5"), measure(declarer([step("m4")])))])),
        ]),
        events: vec![
            HostEvent::Edit { from: 0, changes: vec![(named("i"), Some(int(4))), (named("f"), None)] },
            HostEvent::Evaluate { on: 1, paths: vec![path("r", [step("m0")]), path("g", [])], then: Then::Leave },
            HostEvent::Drive { on: 0, paths: vec![path("r", [step("m1"), index(3)])], turns: 2, then: Then::End },
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
