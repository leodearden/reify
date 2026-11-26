//! The order check (PRD 5.5 item 4): a fresh core asked for the paths a
//! session answered, one path per request and in the reverse of the order
//! the session first answered them, gives the same answers. It needs no
//! interpreter and knows nothing of the toy kinds: a caller hands it core
//! inputs, core paths and how to observe an answer.

use std::collections::BTreeMap;
use std::fmt::Debug;

use reify_eval_core::{
    Answer, Core, Demand, Erased, Event, Input, InputKey, Path, RequestError, RequestId,
};

/// What a session first observed at `path` of snapshot `snapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered<O> {
    pub snapshot: usize,
    pub path: Path,
    pub seen: O,
}

/// Where the fresh core's answers differ from the session's.
#[derive(Debug, Clone, PartialEq)]
pub enum OrderMismatch<O> {
    /// The fresh core answered `path` otherwise.
    Differs {
        snapshot: usize,
        path: Path,
        session: O,
        fresh: O,
    },
    /// The fresh core told no answer at `path`: it ended the request with
    /// `ended`, or left it with neither.
    Unanswered {
        snapshot: usize,
        path: Path,
        ended: Option<RequestError>,
    },
}

/// Asks a fresh core for each of `answered` alone, latest first, on the
/// snapshot made from that index's inputs in `snapshots`; how many answers
/// it compared, or the first that differs.
pub fn order_check<O: PartialEq + Debug + Clone>(
    make_core: &dyn Fn() -> Core,
    snapshots: &[Vec<(InputKey, Input)>],
    answered: &[Answered<O>],
    observe: impl Fn(Answer<Erased>) -> O,
) -> Result<usize, OrderMismatch<O>> {
    let mut core = make_core();
    let mut made = BTreeMap::new();
    for first in answered.iter().rev() {
        let snapshot = made
            .entry(first.snapshot)
            .or_insert_with(|| core.snapshot(snapshots[first.snapshot].clone()));
        let request = core.evaluate(snapshot, Demand::of([first.path.clone()]));
        core.run();
        let told = told(core.take_events(), request.id(), &first.path);
        core.end(request);
        let fresh = match told {
            Ok(answer) => observe(answer),
            Err(ended) => {
                return Err(OrderMismatch::Unanswered {
                    snapshot: first.snapshot,
                    path: first.path.clone(),
                    ended,
                });
            }
        };
        if fresh != first.seen {
            return Err(OrderMismatch::Differs {
                snapshot: first.snapshot,
                path: first.path.clone(),
                session: first.seen.clone(),
                fresh,
            });
        }
    }
    core.drain();
    Ok(answered.len())
}

/// The answer `request` was told at `at` among `events`; failing that, the
/// error the core ended it with, if it did.
fn told(
    events: Vec<Event>,
    request: RequestId,
    at: &Path,
) -> Result<Answer<Erased>, Option<RequestError>> {
    let mut ended = None;
    for event in events {
        match event {
            Event::Answered {
                request: to,
                path,
                answer,
                ..
            } if to == request && path == *at => return Ok(answer),
            Event::Ended {
                request: to, error, ..
            } if to == request => ended = Some(error),
            _ => {}
        }
    }
    Err(ended)
}

mod tests {
    use reify_eval_core::{
        Answer, Binding, Builder, Core, Cx, Declaration, Demand, Erased, Input, InputKey, Kind,
        KindId, Outcome, Path, Produced, Ref, RequestError, Root,
    };

    use super::{Answered, OrderMismatch, order_check};
    use crate::toy;

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
        let found = order_check(&turnstile_core, &[inputs], &answered, turns);
        let expected = OrderMismatch::Differs {
            snapshot: 0,
            path: at("b"),
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
        let make_core = || toy::with_cell().build();
        let found = order_check(&make_core, &[inputs], &answered, |answer| {
            answer.downcast::<i64>()
        });
        assert_eq!(found, Ok(2));
    }

    #[test]
    fn a_fresh_core_that_ends_the_request_is_reported_with_its_error() {
        let below_itself = toy::cell_over([toy::sibling(&["a", "x"])]);
        let inputs = toy::root_scope("r", toy::members([("a", below_itself)])).to_vec();
        let a = toy::below("r", &["a"]);
        let answered = [Answered {
            snapshot: 0,
            path: a.clone(),
            seen: Answer::Final(Outcome::of(0_i64)),
        }];
        let make_core = || toy::with_scope().build();
        let found = order_check(&make_core, &[inputs], &answered, |answer| {
            answer.downcast::<i64>()
        });
        let expected = OrderMismatch::Unanswered {
            snapshot: 0,
            path: a.clone(),
            ended: Some(RequestError::NothingCanRun { waiting: vec![a] }),
        };
        assert_eq!(found, Err(expected));
    }
}
