//! What a body returns: an outcome, or a failure of its own work outside
//! the inputs, which is not an outcome (contract 9).

use crate::failure::OutsideFailure;
use crate::outcome::{Erased, Outcome, Value};

pub enum Produced<T: ?Sized> {
    Outcome(Outcome<T>),
    Outside(OutsideFailure),
}

impl<T: Value> Produced<T> {
    pub(crate) fn erase(self) -> Produced<Erased> {
        match self {
            Produced::Outcome(outcome) => Produced::Outcome(outcome.erase()),
            Produced::Outside(failure) => Produced::Outside(failure),
        }
    }
}
