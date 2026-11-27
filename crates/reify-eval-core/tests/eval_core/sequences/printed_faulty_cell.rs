//! A case shrunk from seed 10 of the random sequences and printed by
//! sequences::print. To regenerate it, run the test that printed it and paste
//! the text its failure message shows.

use super::case::*;
use super::design::*;

#[rustfmt::skip]
#[test]
fn faulty_cell_case_from_seed_10() {
    let case = Case {
        inputs: inputs([
            (named("f4"), flag(true)),
            (named("i1"), int(4)),
        ]),
        events: vec![
            HostEvent::Edit { from: 0, changes: vec![(root("r1"), Some(declaration(guard(input("f4"), "s11")))), (named("s12"), Some(members([(step("m0"), cell([input("i1"), input("i1")]))]))), (named("s11"), Some(members([(step("m1"), guard(input("f4"), "s12"))])))] },
            HostEvent::Evaluate { on: 1, paths: vec![path("r1", [step("m1"), step("m0")])], budget: None, then: Then::End },
        ],
    };
    assert!(super::faulty::caught(&case));
}
