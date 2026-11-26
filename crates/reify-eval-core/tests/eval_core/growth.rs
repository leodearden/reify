//! What the core does grows with what is asked, not with the model
//! (contract 11, G3), and a body runs once however many reads it makes
//! (G4).

use reify_eval_core::{
    Answer, Binding, Cx, Declaration, Kind, KindId, Outcome, Produced, Ref, Step, Turn,
};

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

/// The cells under `parts` at indexes 0..`n`, each over the input "v".
fn indexed_parts(n: u64) -> toy::Members {
    toy::Members(
        (0..n)
            .map(|i| (Step::index(i), toy::cell(&["v"])))
            .collect(),
    )
}

/// A binding to the part at index `i` of the declarer's member `parts`.
fn part(i: u64) -> Binding {
    Binding::Declarer(vec![Step::named("parts"), Step::index(i)])
}

#[test]
fn a_body_that_makes_n_reads_runs_once() {
    for n in [100_u64, 1000, 10_000] {
        let mut core = toy::with_scope().build();
        let mut inputs = vec![
            toy::named("v", 3_i64),
            toy::named("parts.members", indexed_parts(n)),
        ];
        inputs.extend(toy::root_scope(
            "R",
            toy::members([
                ("parts", toy::scope("parts.members")),
                ("reader", toy::cell_over((0..n).map(part))),
            ]),
        ));
        let snapshot = core.snapshot(inputs);
        let reader = toy::below("R", &["reader"]);

        let request = core.evaluate(&snapshot, toy::demand([&reader]));
        assert_eq!(core.turn(), Turn::Idle);

        let answer = toy::the_answer(&core.take_events(), &request, &reader);
        let sum = 3 * i64::try_from(n).expect("n is small");
        assert_eq!(answer.downcast::<i64>(), Answer::Final(Outcome::of(sum)));
        assert_eq!(core.counters().body_runs(Cell::ID), n + 1, "n = {n}");
    }
}

/// Reads the i64 count n at "count", then the i64s at "over"/[0] ..
/// "over"/[n-1] in one read, and adds them.
struct Total;

impl Kind for Total {
    type Out = i64;
    const ID: KindId = KindId("toy.total");
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<i64> {
        let count = match cx.read::<i64>(&Ref::binding("count")).await {
            Outcome::Value(count) => u64::try_from(*count).unwrap_or(0),
            other => return Produced::Outcome(other),
        };
        let refs: Vec<_> = (0..count)
            .map(|i| Ref::binding("over").step(Step::index(i)))
            .collect();
        let mut sum = 0;
        for read in cx.read_all::<i64>(&refs).await {
            match read {
                Outcome::Value(value) => sum += *value,
                other => return Produced::Outcome(other),
            }
        }
        Produced::Outcome(Outcome::of(sum))
    }
}

#[test]
fn a_body_that_reads_n_nodes_with_one_read_all_runs_once() {
    for n in [100_u64, 1000, 10_000] {
        let mut core = toy::with_scope()
            .kind::<Total>()
            .expect("distinct kinds")
            .build();
        let count = i64::try_from(n).expect("n is small");
        let total = Declaration::new(Total::ID)
            .bind("count", toy::input("n"))
            .bind("over", toy::sibling(&["parts"]));
        let mut inputs = vec![
            toy::named("v", 3_i64),
            toy::named("n", count),
            toy::named("parts.members", indexed_parts(n)),
        ];
        inputs.extend(toy::root_scope(
            "R",
            toy::members([("parts", toy::scope("parts.members")), ("total", total)]),
        ));
        let snapshot = core.snapshot(inputs);
        let at = toy::below("R", &["total"]);

        let request = core.evaluate(&snapshot, toy::demand([&at]));
        assert_eq!(core.turn(), Turn::Idle);

        let answer = toy::the_answer(&core.take_events(), &request, &at);
        assert_eq!(
            answer.downcast::<i64>(),
            Answer::Final(Outcome::of(3 * count))
        );
        assert_eq!(core.counters().body_runs(Total::ID), 1, "n = {n}");
        assert_eq!(core.counters().body_runs(Cell::ID), n, "n = {n}");
    }
}
