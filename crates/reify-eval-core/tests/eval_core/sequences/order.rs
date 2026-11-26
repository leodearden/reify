//! The order check (PRD 5.5 item 4): a fresh core asked for the paths a
//! session answered, one path per request and in the reverse of the order
//! the session first answered them, gives the same answers.

use std::fmt::Debug;

use reify_eval_core::{Answer, Core, Erased, Input, InputKey};

use super::fresh::{Answered, FreshMismatch, Order, fresh_check};

/// Asks `core`, fresh, for each of `answered` alone, latest first; how
/// many answers it compared, or the first that differs.
pub fn order_check<O: PartialEq + Debug + Clone>(
    core: Core,
    snapshots: &[Vec<(InputKey, Input)>],
    answered: &[Answered<O>],
    observe: impl Fn(Answer<Erased>) -> O,
) -> Result<usize, FreshMismatch<O>> {
    fresh_check(core, Order::Reverse, snapshots, answered, observe)
}

mod tests {
    use reify_eval_core::{
        Answer, Binding, Builder, Core, Cx, Declaration, Demand, Erased, Input, InputKey, Kind,
        KindId, Outcome, Path, Produced, Ref, Root,
    };

    use super::super::fresh::{Answered, FreshMismatch};
    use super::order_check;
    use crate::toy::{self, BoxShape, ToyKernel, Watch};

    /// A resource that counts the sections run on it.
    struct Turns(u64);

    /// Reads its binding "tag", so that turnstiles of different tags are
    /// different results in the store, then answers how many sections its
    /// resource ran before its own: it depends on the order of requests on
    /// purpose (it breaks O1).
    struct Turnstile;

    impl Kind for Turnstile {
        type Out = u64;
        const ID: KindId = KindId("test.turnstile");
        const VERSION: u32 = 1;

        async fn run(cx: &Cx) -> Produced<u64> {
            let _ = cx.read::<i64>(&Ref::binding("tag")).await;
            let before = cx
                .exclusive(|turns: &mut Turns, _| {
                    turns.0 += 1;
                    turns.0 - 1
                })
                .await;
            Produced::Outcome(Outcome::of(before))
        }
    }

    fn turnstile_core() -> Core {
        let mut core = Builder::new()
            .kind::<Turnstile>()
            .expect("a fresh builder has no kinds")
            .build();
        core.add_resource(|| Turns(0));
        core
    }

    fn at(name: &str) -> Path {
        Path::root(Root::named(name))
    }

    fn turns(answer: Answer<Erased>) -> Answer<u64> {
        answer.downcast()
    }

    /// Root `name`, a turnstile tagged with the input `name`, which holds
    /// `tag`.
    fn turnstile(name: &str, tag: i64) -> [(InputKey, Input); 2] {
        let declaration =
            Declaration::new(Turnstile::ID).bind("tag", Binding::Input(InputKey::named(name)));
        [toy::named(name, tag), toy::root(name, declaration)]
    }

    #[test]
    fn a_kind_whose_answer_depends_on_the_order_of_requests_is_caught() {
        let inputs = [turnstile("a", 1), turnstile("b", 2)].concat();
        let mut core = turnstile_core();
        let snapshot = core.snapshot(inputs.clone());
        let mut answered = Vec::new();
        for name in ["a", "b"] {
            let request = core.evaluate(&snapshot, Demand::of([at(name)]));
            core.run();
            let events = core.take_events();
            answered.push(Answered {
                snapshot: 0,
                path: at(name),
                seen: turns(toy::the_answer(&events, &request, &at(name))),
            });
            core.end(request);
        }
        let seen: Vec<_> = answered
            .iter()
            .map(|answered| answered.seen.clone())
            .collect();
        assert_eq!(
            seen,
            [Answer::Final(Outcome::of(0)), Answer::Final(Outcome::of(1))]
        );
        let found = order_check(turnstile_core(), &[inputs], &answered, turns);
        let expected = FreshMismatch::Differs {
            snapshot: 0,
            path: at("b"),
            session: Answer::Final(Outcome::of(1)),
            fresh: Answer::Final(Outcome::of(0)),
        };
        assert_eq!(found, Err(expected));
    }

    /// Marks, in the toy kernel, the shape it reads at "shape", and
    /// answers 1: it changes what it read (it breaks O6).
    struct Marks;

    impl Kind for Marks {
        type Out = i64;
        const ID: KindId = KindId("test.marks");
        const VERSION: u32 = 1;

        async fn run(cx: &Cx) -> Produced<i64> {
            let Outcome::Value(shape) = cx.read::<BoxShape>(&Ref::binding("shape")).await else {
                return Produced::Outcome(Outcome::Absent);
            };
            cx.exclusive(move |kernel: &mut ToyKernel, _| kernel.mark(shape.handle.0))
                .await;
            Produced::Outcome(Outcome::of(1))
        }
    }

    /// 1 if the shape it reads at "shape" is marked in the toy kernel,
    /// else 0.
    struct Examines;

    impl Kind for Examines {
        type Out = i64;
        const ID: KindId = KindId("test.examines");
        const VERSION: u32 = 1;

        async fn run(cx: &Cx) -> Produced<i64> {
            let Outcome::Value(shape) = cx.read::<BoxShape>(&Ref::binding("shape")).await else {
                return Produced::Outcome(Outcome::Absent);
            };
            let marked = cx
                .exclusive(move |kernel: &mut ToyKernel, _| kernel.is_marked(shape.handle.0))
                .await;
            Produced::Outcome(Outcome::of(i64::from(marked)))
        }
    }

    fn marking_core() -> Core {
        let builder = toy::with_every_kind()
            .kind::<Marks>()
            .and_then(|builder| builder.kind::<Examines>())
            .expect("distinct kinds");
        toy::ungated(builder, &Watch::default())
    }

    #[test]
    fn order_check_detects_result_mutated_through_exclusive_resource() {
        let reads_s = |kind| Declaration::new(kind).bind("shape", toy::sibling(&["s"]));
        let mut inputs = vec![toy::named("side", 2_i64)];
        inputs.extend(toy::root_scope(
            "R",
            toy::members([
                ("s", toy::shape("side")),
                ("f", reads_s(Marks::ID)),
                ("e", reads_s(Examines::ID)),
            ]),
        ));
        let mut core = marking_core();
        let snapshot = core.snapshot(inputs.clone());
        let mut answered = Vec::new();
        for name in ["f", "e"] {
            let at = toy::below("R", &[name]);
            let request = core.evaluate(&snapshot, Demand::of([at.clone()]));
            core.run();
            let events = core.take_events();
            answered.push(Answered {
                snapshot: 0,
                seen: toy::the_answer(&events, &request, &at).downcast::<i64>(),
                path: at,
            });
            core.end(request);
        }
        core.drain();

        let found = order_check(marking_core(), &[inputs], &answered, |answer| {
            answer.downcast::<i64>()
        });
        let expected = FreshMismatch::Differs {
            snapshot: 0,
            path: toy::below("R", &["e"]),
            session: Answer::Final(Outcome::of(1)),
            fresh: Answer::Final(Outcome::of(0)),
        };
        assert_eq!(found, Err(expected));
    }

    #[test]
    fn honest_cells_answer_the_same_in_any_order() {
        let inputs = vec![
            toy::root("c1", toy::cell(&["x"])),
            toy::root("c2", toy::cell(&["x", "y"])),
            toy::named("x", 2_i64),
            toy::named("y", 3_i64),
        ];
        let answered = [("c1", 2_i64), ("c2", 5)].map(|(name, sum)| Answered {
            snapshot: 0,
            path: at(name),
            seen: Answer::Final(Outcome::of(sum)),
        });
        let found = order_check(toy::with_cell().build(), &[inputs], &answered, |answer| {
            answer.downcast::<i64>()
        });
        assert_eq!(found, Ok(2));
    }
}
