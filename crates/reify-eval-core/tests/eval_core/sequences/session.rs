//! Runs a case on a core, as a host would, and checks what the core tells
//! it: every final answer equals the reference interpreter's for its
//! snapshot and path (check 1 of PRD 5.5), every request the core leaves
//! idle holds one answer per demanded path, and a fresh core asked for the
//! same paths in another order answers the same (the order check). What
//! the run reached is counted, so a test can show it is not vacuous.

use std::collections::BTreeMap;
use std::fmt;

use reify_eval_core::{Core, Path, RequestError};

use super::case::Case;
use super::design::ModelPath;
use super::reference::Invalid;
use super::seen::Seen;

/// A shape of design or event a run reached, counted to show the run is
/// not vacuous (PRD 12.1 rule 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shape {
    /// An answer checked for a request left live after a Drive of no
    /// turns, told after a later event.
    LeftLiveAnswer,
    /// A Drive of no turns whose request is then ended.
    DriveZeroEnded,
    /// An Edit from a snapshot other than the latest.
    EditFromNonLatest,
}

/// What a run reached.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reach {
    /// Final answers checked, by snapshot index.
    checked: BTreeMap<usize, u64>,
    shapes: BTreeMap<Shape, u64>,
}

impl Reach {
    /// How many final answers were checked on snapshot `snapshot`.
    pub fn checked_on(&self, snapshot: usize) -> u64 {
        self.checked.get(&snapshot).copied().unwrap_or(0)
    }

    /// How often `shape` was reached.
    pub fn count(&self, shape: Shape) -> u64 {
        self.shapes.get(&shape).copied().unwrap_or(0)
    }
}

/// What a check found wrong. `event` is the index of the host event after
/// which it was seen, `snapshot` the index of the snapshot concerned.
#[derive(Debug, Clone, PartialEq)]
pub enum Mismatch {
    /// The core's answer is not the reference interpreter's.
    Reference {
        event: usize,
        snapshot: usize,
        path: ModelPath,
        core: Seen,
        reference: Seen,
    },
    /// The core is idle and a live request holds no answer at a path it
    /// demanded.
    Unanswered {
        event: usize,
        snapshot: usize,
        path: ModelPath,
    },
    /// An answer at a path its request did not demand.
    Unasked {
        event: usize,
        snapshot: usize,
        path: Path,
    },
    /// A second answer at one path of one request.
    Twice {
        event: usize,
        snapshot: usize,
        path: ModelPath,
    },
    /// The core ended a request.
    Ended {
        event: usize,
        snapshot: usize,
        error: RequestError,
    },
    /// A fresh core, asked in another order, answered otherwise.
    Order {
        snapshot: usize,
        path: ModelPath,
        session: Seen,
        fresh: Seen,
    },
    /// The reference interpreter has no answer: the case is not one the
    /// checks can judge.
    Invalid {
        event: usize,
        snapshot: usize,
        invalid: Invalid,
    },
}

/// Which check a mismatch failed, for keeping a shrunk case on the same
/// fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    Reference,
    Unanswered,
    Unasked,
    Twice,
    Ended,
    Order,
    Invalid,
}

impl Mismatch {
    pub fn kind(&self) -> MismatchKind {
        match self {
            Mismatch::Reference { .. } => MismatchKind::Reference,
            Mismatch::Unanswered { .. } => MismatchKind::Unanswered,
            Mismatch::Unasked { .. } => MismatchKind::Unasked,
            Mismatch::Twice { .. } => MismatchKind::Twice,
            Mismatch::Ended { .. } => MismatchKind::Ended,
            Mismatch::Order { .. } => MismatchKind::Order,
            Mismatch::Invalid { .. } => MismatchKind::Invalid,
        }
    }
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Reference {
                event,
                snapshot,
                path,
                core,
                reference,
            } => write!(
                f,
                "after event {event}, at {path:?} on snapshot {snapshot}: the core answered \
                 {core:?}, the reference interpreter {reference:?}"
            ),
            Mismatch::Unanswered {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, the core is idle with no answer at {path:?} on snapshot \
                 {snapshot}"
            ),
            Mismatch::Unasked {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, an answer at {path:?} on snapshot {snapshot}, which its \
                 request did not demand"
            ),
            Mismatch::Twice {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, a second answer at {path:?} on snapshot {snapshot} to one \
                 request"
            ),
            Mismatch::Ended {
                event,
                snapshot,
                error,
            } => write!(
                f,
                "after event {event}, the core ended a request on snapshot {snapshot}: {error:?}"
            ),
            Mismatch::Order {
                snapshot,
                path,
                session,
                fresh,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: the session answered {session:?}, a fresh \
                 core asked in reverse order {fresh:?}"
            ),
            Mismatch::Invalid {
                event,
                snapshot,
                invalid,
            } => write!(
                f,
                "after event {event}, snapshot {snapshot} has no reference answer: {invalid:?}"
            ),
        }
    }
}

/// Runs `case` on a core `make_core` builds, checking after every host
/// event.
pub fn check(case: &Case, make_core: &dyn Fn() -> Core) -> Result<Reach, Mismatch> {
    let _ = (case, make_core);
    Ok(Reach::default())
}

mod tests {
    use reify_eval_core::{Builder, Core, Cx, Kind, KindId, Outcome, OutsideFailure, Produced};

    use super::super::case::{Case, HostEvent, Then};
    use super::super::design::*;
    use super::super::seen::Seen;
    use super::{Mismatch, Shape, check};
    use crate::toy;

    fn honest() -> Core {
        toy::with_structure().build()
    }

    /// Root scope "r" with a cell "a" over "x" and a cell "b" adding "a"
    /// and "y".
    fn design() -> Inputs {
        inputs([
            (root("r"), declaration(scope("r.m"))),
            (
                named("r.m"),
                members([
                    (step("a"), cell([input("x")])),
                    (step("b"), cell([declarer([step("a")]), input("y")])),
                ]),
            ),
            (named("x"), int(1)),
            (named("y"), int(2)),
        ])
    }

    fn r(steps: &[&str]) -> ModelPath {
        path("r", steps.iter().map(|name| step(name)))
    }

    fn edit_x(from: usize, x: i64) -> HostEvent {
        HostEvent::Edit {
            from,
            changes: vec![(named("x"), Some(int(x)))],
        }
    }

    #[test]
    fn answers_on_an_edited_snapshot_and_on_the_one_it_came_from_are_checked() {
        let case = Case {
            inputs: design(),
            events: vec![
                HostEvent::Evaluate {
                    on: 0,
                    paths: vec![r(&["a"]), r(&["b"])],
                    then: Then::End,
                },
                edit_x(0, 5),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![r(&["b"])],
                    then: Then::End,
                },
                HostEvent::Evaluate {
                    on: 0,
                    paths: vec![r(&["b"]), r(&[])],
                    then: Then::Leave,
                },
            ],
        };
        let reach = check(&case, &honest).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.checked_on(0) > 0, "{reach:?}");
        assert!(reach.checked_on(1) > 0, "{reach:?}");
    }

    fn driven_then_edited(then: Then) -> Case {
        Case {
            inputs: design(),
            events: vec![
                HostEvent::Drive {
                    on: 0,
                    paths: vec![r(&["b"])],
                    turns: 0,
                    then,
                },
                edit_x(0, 5),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![r(&["b"])],
                    then: Then::End,
                },
            ],
        }
    }

    #[test]
    fn a_request_left_live_unfinished_is_answered_and_checked_after_later_events() {
        let case = driven_then_edited(Then::Leave);
        let reach = check(&case, &honest).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.count(Shape::LeftLiveAnswer) > 0, "{reach:?}");
    }

    #[test]
    fn a_request_ended_unfinished_is_owed_no_answer() {
        let case = driven_then_edited(Then::End);
        let reach = check(&case, &honest).unwrap_or_else(|mismatch| panic!("{mismatch}"));
        assert!(reach.count(Shape::DriveZeroEnded) > 0, "{reach:?}");
    }

    /// Registered in place of the toy cell: answers 0 whatever it binds.
    struct ZeroCell;

    impl Kind for ZeroCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;

        async fn run(_cx: &Cx) -> Produced<i64> {
            Produced::Outcome(Outcome::of(0))
        }
    }

    /// Registered in place of the toy cell: fails outside the inputs.
    struct OutsideCell;

    impl Kind for OutsideCell {
        type Out = i64;
        const ID: KindId = toy::Cell::ID;

        async fn run(_cx: &Cx) -> Produced<i64> {
            Produced::Outside(OutsideFailure::OutOfMemory { resource: "test" })
        }
    }

    fn with_cell_kind<K: Kind>() -> Core {
        Builder::new()
            .kind::<K>()
            .and_then(|builder| builder.kind::<toy::Scope>())
            .expect("the kind ids are distinct")
            .build()
    }

    fn sum_of_two() -> Case {
        Case {
            inputs: inputs([
                (root("c"), declaration(cell([input("a"), input("b")]))),
                (named("a"), int(2)),
                (named("b"), int(3)),
            ]),
            events: vec![
                edit_x(0, 1),
                HostEvent::Evaluate {
                    on: 1,
                    paths: vec![path("c", [])],
                    then: Then::End,
                },
            ],
        }
    }

    #[test]
    fn a_cell_that_answers_zero_is_reported_against_the_reference() {
        let found = check(&sum_of_two(), &with_cell_kind::<ZeroCell>);
        let expected = Mismatch::Reference {
            event: 1,
            snapshot: 1,
            path: path("c", []),
            core: Seen::Int(0),
            reference: Seen::Int(5),
        };
        assert_eq!(found, Err(expected));
    }

    #[test]
    fn a_pending_answer_is_reported_against_the_reference() {
        let found = check(&sum_of_two(), &with_cell_kind::<OutsideCell>);
        let pending = OutsideFailure::OutOfMemory { resource: "test" };
        let expected = Mismatch::Reference {
            event: 1,
            snapshot: 1,
            path: path("c", []),
            core: Seen::Pending(pending),
            reference: Seen::Int(5),
        };
        assert_eq!(found, Err(expected));
    }
}
