//! What the host answers at a typed path, in this crate's own terms: no
//! engine type leaks into these.

use std::any::Any;

use reify_eval_core::{Erased, Value};

use crate::failure::InstanceFailure;
use crate::instance::PresentInstance;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAnswer {
    Absent,
    /// An instance.
    Present,
    Failed(AnswerFailure),
    /// No answer follows from the inputs: a failure outside them, as text.
    Pending(String),
    /// A value of a type no answer classifies yet, by its type's name.
    Internal(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerFailure {
    Cycle,
    /// A depth or children limit of the snapshot.
    Limit,
    /// What the node read failed.
    Blocked,
    /// This crate's own failure.
    Instance(InstanceFailure),
    /// Any other failure of the core's, as text: a fault, not an outcome a
    /// corpus row expects.
    Fault(String),
}

/// The answer a value gives.
pub(crate) fn of_value(value: &Erased) -> HostAnswer {
    let any: &dyn Any = value;
    if any.is::<PresentInstance>() {
        HostAnswer::Present
    } else {
        HostAnswer::Internal(<Erased as Value>::type_name(value))
    }
}
