//! What a core is built with: its kinds, each under an id of its own, and
//! its resources, one of each type.

use std::any::type_name;
use std::panic::{AssertUnwindSafe, catch_unwind};

use reify_eval_core::{Cx, DuplicateKind, Kind, KindId, Outcome, Produced};

use crate::toy::{self, Cell};

/// A kind that claims the toy cell's id.
struct Impostor;

impl Kind for Impostor {
    type Out = i64;
    const ID: KindId = Cell::ID;
    const VERSION: u32 = Cell::VERSION;

    async fn run(_: &Cx) -> Produced<i64> {
        Produced::Outcome(Outcome::Absent)
    }
}

#[test]
fn a_second_kind_under_one_id_is_refused_naming_the_id() {
    let refused = toy::with_cell().kind::<Impostor>();
    assert_eq!(refused.err(), Some(DuplicateKind(Cell::ID)));
}

struct Plain;

#[test]
fn a_second_resource_of_one_type_is_refused_naming_the_type() {
    let mut core = toy::core();
    core.add_resource(|| Plain);

    let refused = catch_unwind(AssertUnwindSafe(|| core.add_resource(|| Plain)));
    let payload = refused.expect_err("a second resource of one type is refused");
    let message = payload
        .downcast_ref::<String>()
        .expect("the refusal's message is formatted");
    assert!(message.contains(type_name::<Plain>()), "{message}");
}
