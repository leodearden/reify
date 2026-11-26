//! What an answer was seen to be, in one form both the core's answers and
//! the reference interpreter's outcomes convert to, so the two compare.

use std::any::Any;
use std::collections::BTreeMap;

use reify_eval_core::{Answer, Declaration, Erased, Failure, Outcome, OutsideFailure, Step, Value};

use super::design::core_members;
use super::reference::{Expect, Out};
use crate::toy;

#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    Pending(OutsideFailure),
    Absent,
    Failure(Failure),
    Int(i64),
    Bool(bool),
    Members(BTreeMap<Step, Declaration>),
    Elements {
        count: u64,
        element: Declaration,
    },
    Declaration(Declaration),
    /// A box on the toy kernel, by its description alone: where it lives
    /// differs from core to core (contract 5.4).
    Shape {
        volume: i64,
    },
    /// A value of a type no toy kind produces, by its type's name.
    Other(&'static str),
}

impl Seen {
    pub fn of_answer(answer: Answer<Erased>) -> Seen {
        match answer {
            Answer::Pending(failure) => Seen::Pending(failure),
            Answer::Final(Outcome::Absent) => Seen::Absent,
            Answer::Final(Outcome::Failure(failure)) => Seen::Failure(failure),
            Answer::Final(Outcome::Value(value)) => Seen::of_value(&*value),
        }
    }

    pub fn of_expected(expect: &Expect) -> Seen {
        match expect {
            Expect::Absent => Seen::Absent,
            Expect::Failure(failure) => Seen::Failure(failure.clone()),
            Expect::Value(Out::Int(value)) => Seen::Int(*value),
            Expect::Value(Out::Bool(value)) => Seen::Bool(*value),
            Expect::Value(Out::Members(members)) => Seen::Members(core_members(members)),
            Expect::Value(Out::Elements { count, element }) => Seen::Elements {
                count: *count,
                element: element.to_core(),
            },
            Expect::Value(Out::Decl(decl)) => Seen::Declaration(decl.to_core()),
            Expect::Value(Out::Shape { volume }) => Seen::Shape { volume: *volume },
        }
    }

    fn of_value(value: &Erased) -> Seen {
        let any: &dyn Any = value;
        if let Some(value) = any.downcast_ref::<i64>() {
            Seen::Int(*value)
        } else if let Some(value) = any.downcast_ref::<bool>() {
            Seen::Bool(*value)
        } else if let Some(members) = any.downcast_ref::<toy::Members>() {
            Seen::Members(members.0.clone())
        } else if let Some(elements) = any.downcast_ref::<toy::Elements>() {
            Seen::Elements {
                count: elements.count,
                element: elements.element.clone(),
            }
        } else if let Some(declaration) = any.downcast_ref::<Declaration>() {
            Seen::Declaration(declaration.clone())
        } else if let Some(shape) = any.downcast_ref::<toy::BoxShape>() {
            Seen::Shape {
                volume: shape.volume,
            }
        } else {
            Seen::Other(<Erased as Value>::type_name(value))
        }
    }
}
