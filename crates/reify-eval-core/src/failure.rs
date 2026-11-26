//! The two sorts of failure (contract 9). A `Failure` is an outcome: it
//! follows from the inputs. An `OutsideFailure` does not, so it is never an
//! outcome and nothing stores it.

use std::any::Any;

/// A failure that follows from the inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A read asked for one type and the value read is of another.
    WrongType { expected: &'static str, found: &'static str },
    /// A body made a second request before the first was answered (O2).
    TwoRequestsAtOnce,
}

/// A failure outside the inputs: a fault of the core's setup or of a kind's
/// own work, reported to the host and stored nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutsideFailure {
    /// A declaration names a kind the core was not built with.
    UnknownKind { kind: &'static str },
    /// A body was suspended waiting on something other than the core.
    WaitedOutsideCore,
    BodyPanicked { message: String },
    /// A body asked for a section on a resource nobody added.
    NoResource { resource: &'static str },
    /// A kind's own work ran out of memory in `resource`.
    OutOfMemory { resource: &'static str },
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
