//! What a node's end does to the index, the wait relation and the
//! requests: a final answer, or a failure outside the inputs. The executor
//! acts on what each returns.

use std::collections::BTreeSet;

use crate::event::RequestId;
use crate::index::{Entry, Indexes, NodeKey};
use crate::requests::Requests;
use crate::waits::Waits;

/// Who a final answer concerns: the live requests to tell, the paths that
/// waited on it as their parent and can now be worked out, and the bodies
/// whose reads it was the last to answer.
pub(crate) struct Answered {
    pub tell: BTreeSet<RequestId>,
    pub resolve: Vec<NodeKey>,
    pub resume: Vec<NodeKey>,
}

/// Files `entry` as the final answer at `key`, which no request wants and
/// nothing awaits from then on. `resolving` holds the paths waiting on
/// their parent.
pub(crate) fn answered(
    key: NodeKey,
    entry: Entry,
    indexes: &mut Indexes,
    waits: &mut Waits,
    requests: &mut Requests,
    resolving: &mut BTreeSet<NodeKey>,
) -> Answered {
    indexes.file(key.snapshot, key.path, entry);
    let (resolve, resume) = waits
        .release(key)
        .into_iter()
        .partition(|waiter| resolving.remove(waiter));
    Answered {
        tell: requests.settle(&key),
        resolve,
        resume,
    }
}

/// Who a failure outside the inputs at a node concerns: the live requests
/// to tell it is pending, and the nodes that waited on it, which are to be
/// dropped and fail the same way.
pub(crate) struct FailedOutside {
    pub tell: BTreeSet<RequestId>,
    pub dropped: Vec<NodeKey>,
}

/// Ends the node at `key` with a failure outside the inputs (contract 9):
/// nothing is indexed, no request wants it from then on, and it and each
/// node waiting on it stop waiting on anything (point 13).
pub(crate) fn failed_outside(
    key: NodeKey,
    waits: &mut Waits,
    requests: &mut Requests,
) -> FailedOutside {
    waits.forget(key);
    let dropped = waits.waiters(key);
    for waiter in &dropped {
        waits.forget(*waiter);
    }
    FailedOutside {
        tell: requests.settle(&key),
        dropped,
    }
}
