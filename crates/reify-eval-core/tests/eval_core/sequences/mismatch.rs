//! What a check of a case found wrong, told apart by which check failed so
//! a shrunk case can be kept on the same fault.

use std::fmt;

use reify_eval_core::{Path, RequestError};

use super::design::ModelPath;
use super::reference::Invalid;
use super::seen::Seen;

/// What a check found wrong. `event` is the index of the host event after
/// which it was seen, `snapshot` the index of the snapshot concerned.
#[derive(Debug, Clone, PartialEq)]
pub enum Mismatch {
    /// The core's answer is not the reference interpreter's.
    Reference {
        event: usize,
        snapshot: usize,
        path: ModelPath,
        core: Box<Seen>,
        reference: Box<Seen>,
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
    /// The core ended a request otherwise than by the budget it was given.
    Ended {
        event: usize,
        snapshot: usize,
        error: RequestError,
    },
    /// An answer told to a request after the core ended it.
    AnsweredAfterEnd {
        event: usize,
        snapshot: usize,
        path: Path,
    },
    /// An Evaluate given a budget of `budget` paths, whose demand needs
    /// `needed` resolved, of which at least `least` whatever the order of
    /// the core's work, was `ended` by the core, or answered, when the
    /// budget does not allow it: ended within `needed`, or, as the first
    /// request on its snapshot, answered past `least`.
    Budget {
        event: usize,
        snapshot: usize,
        budget: u64,
        needed: u64,
        least: u64,
        ended: bool,
    },
    /// A fresh core built on the session core's selection (check 2),
    /// asked in the session's order, answered otherwise.
    Fresh {
        snapshot: usize,
        path: ModelPath,
        session: Box<Seen>,
        fresh: Box<Seen>,
    },
    /// A fresh core built on the session core's selection told no answer
    /// at a path the session answered: it ended the request with `ended`,
    /// or left it with neither.
    FreshUnanswered {
        snapshot: usize,
        path: ModelPath,
        ended: Option<RequestError>,
    },
    /// A fresh core, asked in another order, answered otherwise.
    Order {
        snapshot: usize,
        path: ModelPath,
        session: Box<Seen>,
        fresh: Box<Seen>,
    },
    /// A fresh core, asked in another order, told no answer at a path the
    /// session answered: it ended the request with `ended`, or left it
    /// with neither.
    OrderUnanswered {
        snapshot: usize,
        path: ModelPath,
        ended: Option<RequestError>,
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
    AnsweredAfterEnd,
    Budget,
    Fresh,
    FreshUnanswered,
    Order,
    OrderUnanswered,
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
            Mismatch::AnsweredAfterEnd { .. } => MismatchKind::AnsweredAfterEnd,
            Mismatch::Budget { .. } => MismatchKind::Budget,
            Mismatch::Fresh { .. } => MismatchKind::Fresh,
            Mismatch::FreshUnanswered { .. } => MismatchKind::FreshUnanswered,
            Mismatch::Order { .. } => MismatchKind::Order,
            Mismatch::OrderUnanswered { .. } => MismatchKind::OrderUnanswered,
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
            Mismatch::AnsweredAfterEnd {
                event,
                snapshot,
                path,
            } => write!(
                f,
                "after event {event}, an answer at {path:?} on snapshot {snapshot} to a request \
                 the core had ended"
            ),
            Mismatch::Budget {
                event,
                snapshot,
                budget,
                needed,
                least,
                ended,
            } => {
                let outcome = if *ended { "ended" } else { "answered" };
                write!(
                    f,
                    "after event {event}, a request on snapshot {snapshot} given a budget of \
                     {budget} paths, whose demand needs {needed} resolved, at least {least} \
                     in any order, was {outcome}"
                )
            }
            Mismatch::Fresh {
                snapshot,
                path,
                session,
                fresh,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: the session answered {session:?}, a fresh \
                 core built on the session's selection {fresh:?}"
            ),
            Mismatch::FreshUnanswered {
                snapshot,
                path,
                ended,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: a fresh core built on the session's \
                 selection told no answer, the request ended with {ended:?}"
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
            Mismatch::OrderUnanswered {
                snapshot,
                path,
                ended,
            } => write!(
                f,
                "at {path:?} on snapshot {snapshot}: a fresh core asked in reverse order told no \
                 answer, the request ended with {ended:?}"
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
