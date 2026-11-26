//! What the core does grows with what is asked, not with the model
//! (contract 11, G3).

use reify_eval_core::{Kind, Turn};

use crate::toy::{self, Cell, Scope};

/// The bodies of each kind run, and the paths worked out, when one member
/// of a root scope with `parts` cell members is demanded.
fn work_for_one_of(parts: usize) -> (u64, u64, u64) {
    let mut core = toy::with_scope().build();
    let names: Vec<String> = (0..parts).map(|i| format!("part{i}")).collect();
    let cells = names
        .iter()
        .map(|name| (name.as_str(), toy::cell(&["one"])));
    let mut inputs = vec![toy::named("one", 1_i64)];
    inputs.extend(toy::root_scope("R", toy::members(cells)));
    let snapshot = core.snapshot(inputs);

    let _request = core.evaluate(&snapshot, toy::demand([&toy::below("R", &["part0"])]));
    assert_eq!(core.turn(), Turn::Idle);
    let counters = core.counters();
    (
        counters.body_runs(Scope::ID),
        counters.body_runs(Cell::ID),
        counters.paths_resolved(),
    )
}

#[test]
fn nothing_unasked_is_computed_n_parts_one_demanded() {
    let work: Vec<_> = [100, 1000, 10_000].map(work_for_one_of).into();
    assert_eq!(work, [(1, 1, 2); 3]);
}
