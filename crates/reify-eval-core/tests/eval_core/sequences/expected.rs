//! A model answer, and how one is read as the Rust type the core reads it
//! as: what the reference interpreter answers for a path and what a
//! session checks the core's answer against.

use std::any::type_name;
use std::collections::BTreeMap;

use reify_eval_core::{Declaration, Failure, Step};

use super::design::{Decl, ModelPath, Site, ToyKind, Val};
use crate::toy;

/// What a path answers: the kind of the node declared there, if any, and
/// its outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct Expected {
    pub node: Option<ToyKind>,
    pub outcome: Expect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    Value(Out),
    Failure(Failure),
    Absent,
}

/// A value an answer or a snapshot input holds, in the model's form.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Int(i64),
    Bool(bool),
    Members(BTreeMap<Step, Decl>),
    Elements { count: u64, element: Decl },
    Decl(Decl),
    Shape { volume: i64 },
    Calls(Vec<Site>),
}

/// Why a path has no answer in this snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// What the node at `path` works out is past i64.
    Overflow { path: ModelPath },
}

/// A read typed as the Rust type the core reads it as.
pub enum Typed<T> {
    Value(T),
    Absent,
    Failure(Failure),
}

pub fn out_of(value: &Val) -> Out {
    match value {
        Val::Int(value) => Out::Int(*value),
        Val::Bool(value) => Out::Bool(*value),
        Val::Members(members) => Out::Members(members.clone()),
        Val::Decl(decl) => Out::Decl(decl.clone()),
        Val::Calls(sites) => Out::Calls(sites.clone()),
        Val::Bound(_) => {
            unreachable!("a limit setting is under a limit key, which no binding reads")
        }
    }
}

impl Out {
    /// The name of the Rust type the core holds this value as.
    pub fn type_name(&self) -> &'static str {
        match self {
            Out::Int(_) => type_name::<i64>(),
            Out::Bool(_) => type_name::<bool>(),
            Out::Members(_) => type_name::<toy::Members>(),
            Out::Elements { .. } => type_name::<toy::Elements>(),
            Out::Decl(_) => type_name::<Declaration>(),
            Out::Shape { .. } => type_name::<toy::BoxShape>(),
            Out::Calls(_) => type_name::<toy::Calls>(),
        }
    }
}

/// `read` typed as `expected`: `take` gives the value of that type, and a
/// value of another type is a failure naming both.
fn typed<T>(read: Expect, expected: &'static str, take: impl FnOnce(Out) -> Option<T>) -> Typed<T> {
    match read {
        Expect::Value(out) => {
            let found = out.type_name();
            take(out).map_or(
                Typed::Failure(Failure::WrongType { expected, found }),
                Typed::Value,
            )
        }
        Expect::Absent => Typed::Absent,
        Expect::Failure(failure) => Typed::Failure(failure),
    }
}

pub fn typed_int(read: Expect) -> Typed<i64> {
    typed(read, type_name::<i64>(), |out| match out {
        Out::Int(value) => Some(value),
        _ => None,
    })
}

pub fn typed_bool(read: Expect) -> Typed<bool> {
    typed(read, type_name::<bool>(), |out| match out {
        Out::Bool(value) => Some(value),
        _ => None,
    })
}

pub fn typed_members(read: Expect) -> Typed<BTreeMap<Step, Decl>> {
    typed(read, type_name::<toy::Members>(), |out| match out {
        Out::Members(members) => Some(members),
        _ => None,
    })
}

/// A box read as the toy kernel's box: its volume.
pub fn typed_shape(read: Expect) -> Typed<i64> {
    typed(read, type_name::<toy::BoxShape>(), |out| match out {
        Out::Shape { volume } => Some(volume),
        _ => None,
    })
}

pub fn typed_calls(read: Expect) -> Typed<Vec<Site>> {
    typed(read, type_name::<toy::Calls>(), |out| match out {
        Out::Calls(sites) => Some(sites),
        _ => None,
    })
}

pub fn typed_decl(read: Expect) -> Typed<Decl> {
    typed(read, type_name::<Declaration>(), |out| match out {
        Out::Decl(decl) => Some(decl),
        _ => None,
    })
}
