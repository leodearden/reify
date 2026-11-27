//! A core built on a store that already holds results, and the calls that
//! hand a core's results to one (PRD section 13 point 26, 5.2): nothing is
//! seeded into a running core.

use std::collections::BTreeSet;

use reify_eval_core::{Answer, Builder, Core, Cx, Erased, Input, InputKey, Kind, KindId, Produced};

use crate::toy::{self, Cell, Scope};

/// A root scope `R` of two cells, the second reading the first.
fn scope_of_cells() -> Vec<(InputKey, Input)> {
    let mut inputs = vec![toy::named("a", 2_i64), toy::named("b", 3_i64)];
    inputs.extend(toy::root_scope(
        "R",
        toy::members([
            ("x", toy::cell(&["a"])),
            ("y", toy::cell_over([toy::sibling(&["x"]), toy::input("b")])),
        ]),
    ));
    inputs
}

/// The answers at `R`/x and `R`/y, as i64s, on a fresh snapshot of
/// `scope_of_cells`.
fn cells_of(core: &mut Core) -> Vec<Answer<i64>> {
    let snapshot = core.snapshot(scope_of_cells());
    let paths = [toy::below("R", &["x"]), toy::below("R", &["y"])];
    toy::answers_to(core, &snapshot, &[&paths[0], &paths[1]])
        .into_iter()
        .map(Answer::<Erased>::downcast)
        .collect()
}

fn runs(core: &Core, kinds: &[KindId]) -> u64 {
    kinds
        .iter()
        .map(|kind| core.counters().body_runs(*kind))
        .sum()
}

/// A core that has answered `scope_of_cells`, and its answers.
fn evaluated() -> (Core, Vec<Answer<i64>>) {
    let mut core = toy::with_scope().build();
    let answers = cells_of(&mut core);
    (core, answers)
}

#[test]
fn a_core_built_on_another_core_s_store_answers_without_running_bodies() {
    let (first, answers) = evaluated();

    let mut second = toy::with_scope().on_store(first.into_store()).build();

    assert_eq!(cells_of(&mut second), answers);
    assert_eq!(runs(&second, &[Cell::ID, Scope::ID]), 0);
}

/// The toy cell's body under its id, one version later.
struct NextCell;

impl Kind for NextCell {
    type Out = i64;
    const ID: KindId = Cell::ID;
    const VERSION: u32 = Cell::VERSION + 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        <Cell as Kind>::run(cx).await
    }
}

#[test]
fn a_kind_with_the_same_id_and_a_higher_version_finds_none_of_the_first_kind_s_results() {
    let (first, answers) = evaluated();

    let mut same_version = toy::with_scope()
        .on_store(first.selected_store(|_| true))
        .build();
    assert_eq!(cells_of(&mut same_version), answers);
    assert_eq!(same_version.counters().body_runs(Cell::ID), 0);

    let next = Builder::new()
        .kind::<NextCell>()
        .and_then(|builder| builder.kind::<Scope>())
        .expect("distinct kinds");
    let mut next_version = next.on_store(first.into_store()).build();
    assert_eq!(cells_of(&mut next_version), answers);
    assert_eq!(next_version.counters().body_runs(Cell::ID), 2);
    assert_eq!(next_version.counters().body_runs(Scope::ID), 0);
}

#[test]
fn selected_store_keeps_only_the_results_keep_selects() {
    let (first, answers) = evaluated();
    let mut seen = Vec::new();

    let cells_only = first.selected_store(|result| {
        seen.push(result.kind());
        result.kind() == Cell::ID
    });
    let mut second = toy::with_scope().on_store(cells_only).build();

    assert_eq!(cells_of(&mut second), answers);
    assert_eq!(second.counters().body_runs(Scope::ID), 1);
    assert_eq!(second.counters().body_runs(Cell::ID), 0);
    assert_eq!(
        seen.len(),
        3,
        "the scope's result and each cell's: {seen:?}"
    );
    assert_eq!(
        seen.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([Cell::ID, Scope::ID])
    );
}

#[test]
fn selected_store_leaves_the_core_as_it_was() {
    let (mut first, answers) = evaluated();
    let runs_before = runs(&first, &[Cell::ID, Scope::ID]);

    let _ = first.selected_store(|_| false);

    assert_eq!(cells_of(&mut first), answers);
    assert_eq!(runs(&first, &[Cell::ID, Scope::ID]), runs_before);
}
