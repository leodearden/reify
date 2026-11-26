//! What an answer was seen to be, in one form both the core's answers and
//! the reference interpreter's outcomes convert to, so the two compare.

use std::collections::BTreeMap;

use reify_eval_core::{Declaration, Failure, OutsideFailure, Step};

#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    Pending(OutsideFailure),
    Absent,
    Failure(Failure),
    Int(i64),
    Bool(bool),
    Members(BTreeMap<Step, Declaration>),
    Elements { count: u64, element: Declaration },
    Declaration(Declaration),
    /// A value of a type no toy kind produces, by its type's name.
    Other(&'static str),
}
