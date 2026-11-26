//! A case shrunk from seed 2 of the random sequences and printed by
//! sequences::print. To regenerate it, run the test that printed it and paste
//! the text its failure message shows.

use super::case::*;
use super::design::*;

#[rustfmt::skip]
#[test]
fn faulty_cell_case_from_seed_2() {
    let case = Case {
        inputs: inputs([
            (root("r0"), declaration(cell([input("i3"), input("i1"), input("i1")]))),
            (named("i3"), int(4)),
        ]),
        events: vec![
            HostEvent::Edit { from: 0, changes: vec![(named("i1"), Some(int(2)))] },
            HostEvent::Evaluate { on: 1, paths: vec![path("r0", [])], then: Then::End },
        ],
    };
    assert!(super::faulty::caught(&case));
}
