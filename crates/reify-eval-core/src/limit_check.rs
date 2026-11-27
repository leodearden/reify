//! The limits a snapshot sets, applied to its paths (contract 7.6). A limit
//! is checked when a path is first answered in a snapshot, against that
//! snapshot's settings, whether or not a result is in the store: what it
//! finds is a fact about the path, so it is given in the snapshot's index
//! only and never filed in the store.

use crate::content::Content;
use crate::failure::Failure;
use crate::index::Entry;
use crate::kinds::Kinds;
use crate::limits::Limits;
use crate::outcome::Outcome;

/// The failure of a node `depth` steps below its root, if that is deeper
/// than the depth setting allows. It is asked only of a path that resolves
/// to a declaration: one that resolves to absent, or to its parent's
/// failure, answers that whatever its depth (contract 4.3, 7.4).
pub(crate) fn depth(depth: usize, limits: &Limits) -> Option<Failure> {
    let limit = limits.depth()?;
    let depth = u64::try_from(depth).expect("a path's depth fits in a u64");
    (depth > limit).then_some(Failure::TooDeep { limit })
}

/// `entry`, a node's final answer in a snapshot, or, where it declares more
/// children than the children setting allows, that failure with the node's
/// declaration. Both sorts of child count (contract 7.6): the steps the run
/// that produced it declared, and the steps its value declares, a step
/// declared both ways once; they are counted only up to one past the
/// setting. A failure, an absent answer, or a value no body produced (by
/// its own run, or by the run a lookup found) comes back unchanged.
pub(crate) fn children(entry: Entry, limits: &Limits, kinds: &Kinds) -> Entry {
    let Some(limit) = limits.children_per_node() else {
        return entry;
    };
    if !declares_more_than(&entry, limit, kinds) {
        return entry;
    }
    let failure = Failure::TooManyChildren { limit };
    Entry {
        content: Content::of_failure(&failure),
        outcome: Outcome::Failure(failure),
        ..entry
    }
}

/// Whether `entry`, whose value a body of the entry's kind produced,
/// declares more than `limit` distinct children, by its run or its value.
/// A kind's steps are distinct, so a step of the value's is counted unless
/// the run declared it too.
fn declares_more_than(entry: &Entry, limit: u64, kinds: &Kinds) -> bool {
    let (Outcome::Value(value), Some(declaration)) = (&entry.outcome, &entry.declaration) else {
        return false;
    };
    let Ok(first_past) = usize::try_from(limit) else {
        return false;
    };
    let kind = kinds
        .get(declaration.kind())
        .expect("a node whose body ran has a kind the core was built with");
    let by_run = &entry.children_declared;
    let Some(past_the_run) = first_past.checked_sub(by_run.len()) else {
        return true;
    };
    kind.steps(&**value)
        .filter(|step| !by_run.contains(step))
        .nth(past_the_run)
        .is_some()
}
