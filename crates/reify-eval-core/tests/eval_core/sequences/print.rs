//! Prints a case as a complete Rust test file, every value written as a
//! call of design.rs's constructors or an enum variant, so a failing case
//! found by the random run can be pasted in as a named test.

use super::case::Case;

/// What the printed test asserts of its case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expectation {
    /// Check 1 reports the faulty cell, and the honest kinds pass it.
    CaughtWithFaultyCell,
    /// The honest kinds pass it: a regression for a fault fixed.
    PassesWithToyKinds,
}

/// The test file named `name` holding `case`, shrunk from seed `seed`,
/// asserting `expectation`.
pub fn print_test(case: &Case, seed: u64, name: &str, expectation: Expectation) -> String {
    let _ = (case, seed, name, expectation);
    String::new()
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
                    ]),
                ),
                (
                    attached_to(path("r", []), "extra"),
                    members([(step("m2"), picker(input("i"), declarer([step("m1")])))]),
                ),
                (named("f"), flag(true)),
                (named("i"), int(-3)),
                (named("e"), declaration(cell([attached("x")]))),
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
            (named("e"), declaration(cell([attached("x")]))),
            (named("f"), flag(true)),
            (named("i"), int(-3)),
            (named("s"), members([(step("m0"), cell([input("i"), declarer([step("m1"), index(2)]), passed("count", [index(0)])])), (step("m1"), collection(input("n"), input("e")))])),
            (attached_to(path("r", []), "extra"), members([(step("m2"), picker(input("i"), declarer([step("m1")])))])),
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
        let printed = print_test(&every_variant(), 7, "printed_case", Expectation::CaughtWithFaultyCell);
        let expected = format!("{PRINTED}    assert!(super::faulty::caught(&case));\n}}\n");
        assert_eq!(printed, expected, "printed:\n{printed}");
    }

    #[test]
    fn a_case_prints_as_constructor_calls_asserting_the_toy_kinds_pass_it() {
        let printed = print_test(&every_variant(), 7, "printed_case", Expectation::PassesWithToyKinds);
        let check = "if let Err(mismatch) = super::session::check(&case, &super::session::toy_core) \
                     { panic!(\"{mismatch}\") }";
        let expected = format!("{PRINTED}    {check}\n}}\n");
        assert_eq!(printed, expected, "printed:\n{printed}");
    }
}
