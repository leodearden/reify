//! The two sorts of failure (contract 9). A `Failure` is an outcome: it
//! follows from the inputs. An `OutsideFailure` does not, so it is never an
//! outcome and nothing stores it.

use std::any::Any;

use crate::hash::Encoder;
use crate::kind_id::KindId;
use crate::reference::Ref;

/// A failure that follows from the inputs. None names a path (contract
/// 5.4), so equal failures at two places have one content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A read asked for one type and the value read is of another.
    WrongType {
        expected: &'static str,
        found: &'static str,
    },
    /// A body made a second request before the first was answered (O2).
    TwoRequestsAtOnce,
    /// A reader's answer: what it read at `at` failed. The chain to the
    /// cause is the recorded reads (contract 9).
    BlockedBy { at: Ref },
    /// A failure a body states in its kind's own terms, by a code the kind
    /// defines and the core never interprets.
    OfKind { kind: KindId, code: &'static str },
    /// The node is on a ring of waits, each node on it needing the next
    /// one's answer (contract 7.7). It names and counts no place, so every
    /// node on the ring, wherever the ring was entered, has this one value.
    /// A node on the ring is answered in the snapshot's index only, and
    /// nothing is filed in the store for it; a reader off the ring that
    /// answers this failure as its own is filed like any other result.
    Cycle,
}

impl Failure {
    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        match self {
            Failure::WrongType { expected, found } => {
                encoder.tag("failure.wrong_type").text(expected).text(found);
            }
            Failure::TwoRequestsAtOnce => {
                encoder.tag("failure.two_requests_at_once");
            }
            Failure::BlockedBy { at } => {
                encoder.tag("failure.blocked_by");
                at.encode(encoder);
            }
            Failure::OfKind { kind, code } => {
                encoder.tag("failure.of_kind").text(kind.0).text(code);
            }
            Failure::Cycle => {
                encoder.tag("failure.cycle");
            }
        }
    }
}

/// A failure outside the inputs: a fault of the core's setup or of a kind's
/// own work, reported to the host and stored nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutsideFailure {
    /// A declaration names a kind the core was not built with.
    UnknownKind {
        kind: &'static str,
    },
    /// A body was suspended waiting on something other than the core.
    WaitedOutsideCore,
    BodyPanicked {
        message: String,
    },
    /// A body asked for a section on a resource nobody added.
    NoResource {
        resource: &'static str,
    },
    /// The section a body waited on panicked; its resource is dead from
    /// then on.
    SectionPanicked {
        resource: &'static str,
        message: String,
    },
    /// A body asked for a section on a dead resource; `death` says what
    /// killed it.
    ResourceDead {
        resource: &'static str,
        death: ResourceDeath,
    },
    /// A kind's own work ran out of memory in `resource`.
    OutOfMemory {
        resource: &'static str,
    },
}

/// What killed a resource: the work on its thread that panicked, with the
/// panic's message. A dead resource is never touched again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceDeath {
    /// Making it panicked, so it never existed.
    MakePanicked { message: String },
    /// A section on it panicked.
    SectionPanicked { message: String },
    /// Giving back a thing held in it panicked.
    ReleasePanicked { message: String },
}

/// The text a panic was raised with.
pub(crate) fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        String::from("the panic's payload was not text")
    }
}
