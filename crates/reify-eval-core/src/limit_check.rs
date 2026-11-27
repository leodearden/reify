//! The limits a snapshot sets, applied to its paths (contract 7.6). A limit
//! is checked when a path is first answered in a snapshot, against that
//! snapshot's settings, whether or not a result is in the store: what it
//! finds is a fact about the path, so it is given in the snapshot's index
//! only and never filed in the store.

use crate::failure::Failure;
use crate::limits::Limits;

/// The failure of a node `depth` steps below its root, if that is deeper
/// than the depth setting allows. It is asked only of a path that resolves
/// to a declaration: one that resolves to absent, or to its parent's
/// failure, answers that whatever its depth (contract 4.3, 7.4).
pub(crate) fn depth(depth: usize, limits: &Limits) -> Option<Failure> {
    let limit = limits.depth()?;
    let depth = u64::try_from(depth).expect("a path's depth fits in a u64");
    (depth > limit).then_some(Failure::TooDeep { limit })
}
