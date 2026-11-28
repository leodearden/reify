//! The seam between this crate's kinds and the core's body interface (PRD
//! 5.1 rule 8): the only file that builds what a body returns (`Produced`,
//! `Outcome`, `Failure`), and the only one that may call `read_all`. A core
//! task that changes those types edits this file and `host/core_calls`
//! only.
//!
//! The claim of determinism (PRD 5.4, rule 8): no kind of this crate opens a
//! view or uses what was there last time, so every result follows from its
//! reads alone.
//!
//! What each helper promises:
//! - `need`: the value at a ref, or what the body answers instead: absent
//!   where the ref names nothing, blocked by the ref where it failed;
//! - `optional`: the value at a ref, or none where the ref names nothing;
//!   blocked by the ref where it failed;
//! - `value` and `passed_on`: the body answers a value it made, or one it
//!   read, unchanged;
//! - `failed`: the body answers this crate's own failure, described.

use std::sync::Arc;

use reify_eval_core::{Cx, Description, Failure, KindId, Outcome, Produced, Ref, Value};

use crate::failure::InstanceFailure;

pub(crate) async fn need<T: Value, Out: Value>(cx: &Cx, at: &Ref) -> Result<Arc<T>, Produced<Out>> {
    match optional::<T, Out>(cx, at).await? {
        Some(value) => Ok(value),
        None => Err(Produced::Outcome(Outcome::Absent)),
    }
}

pub(crate) async fn optional<T: Value, Out: Value>(
    cx: &Cx,
    at: &Ref,
) -> Result<Option<Arc<T>>, Produced<Out>> {
    match cx.read::<T>(at).await {
        Outcome::Value(value) => Ok(Some(value)),
        Outcome::Absent => Ok(None),
        Outcome::Failure(_) => Err(Produced::Outcome(Outcome::Failure(Failure::BlockedBy {
            at: at.clone(),
        }))),
    }
}

pub(crate) fn value<T: Value>(value: T) -> Produced<T> {
    Produced::Outcome(Outcome::of(value))
}

pub(crate) fn passed_on<T: Value>(value: Arc<T>) -> Produced<T> {
    Produced::Outcome(Outcome::Value(value))
}

pub(crate) fn failed<T: Value>(kind: KindId, failure: &InstanceFailure) -> Produced<T> {
    Produced::Outcome(Outcome::Failure(Failure::Described {
        kind,
        code: failure.code(),
        description: Description::of(failure.clone(), failure.content()),
    }))
}
