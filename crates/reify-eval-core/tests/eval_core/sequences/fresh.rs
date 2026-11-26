//! A fresh core asked for the paths a session answered, one path per
//! request, gives the same answers: the comparison both the order check
//! (PRD 5.5 item 4) and check 2 (item 2) make, differing only in the core
//! and the order. It needs no interpreter and knows nothing of the toy
//! kinds: a caller hands it core inputs, core paths and how to observe an
//! answer.

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

/// The order the fresh core is asked in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// The order the session first answered the paths in.
    Session,
    /// Latest first.
    Reverse,
}

/// Where the fresh core's answers differ from the session's.
#[derive(Debug, Clone, PartialEq)]
pub enum FreshMismatch<O> {
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

/// Asks `core` for each of `answered` alone, in `order`, on the snapshot
/// made from that index's inputs in `snapshots`; how many answers it
/// compared, or the first that differs.
pub fn fresh_check<O: PartialEq + Debug + Clone>(
    mut core: Core,
    order: Order,
    snapshots: &[Vec<(InputKey, Input)>],
    answered: &[Answered<O>],
    observe: impl Fn(Answer<Erased>) -> O,
) -> Result<usize, FreshMismatch<O>> {
    let mut asked: Vec<_> = answered.iter().collect();
    if order == Order::Reverse {
        asked.reverse();
    }
    let mut made = BTreeMap::new();
    for first in asked {
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
                return Err(FreshMismatch::Unanswered {
                    snapshot: first.snapshot,
                    path: first.path.clone(),
                    ended,
                });
            }
        };
        if fresh != first.seen {
            return Err(FreshMismatch::Differs {
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
