//! What the host is told about a demanded path: a final outcome, or that
//! the answer is pending because of a failure outside the inputs.

use crate::failure::OutsideFailure;
use crate::outcome::{Erased, Outcome, Value};

#[derive(Debug, PartialEq)]
pub enum Answer<T: ?Sized> {
    Final(Outcome<T>),
    Pending(OutsideFailure),
}

impl<T: ?Sized> Clone for Answer<T> {
    fn clone(&self) -> Self {
        match self {
            Answer::Final(outcome) => Answer::Final(outcome.clone()),
            Answer::Pending(failure) => Answer::Pending(failure.clone()),
        }
    }
}

impl Answer<Erased> {
    /// The same answer typed as `T`, by the same rule as a body's read.
    pub fn downcast<T: Value>(self) -> Answer<T> {
        match self {
            Answer::Final(outcome) => Answer::Final(outcome.downcast()),
            Answer::Pending(failure) => Answer::Pending(failure),
        }
    }
}
