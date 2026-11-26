//! What a node's final answer does to the index, the wait relation and the
//! requests. The executor acts on what it returns.

use std::collections::BTreeSet;

use crate::event::RequestId;
use crate::index::{Entry, Indexes};
use crate::requests::Requests;
use crate::task::TaskKey;
use crate::waits::Waits;

/// Who a final answer concerns: the live requests to tell, the paths that
/// waited on it as their parent and can now be worked out, and the bodies
/// whose reads it was the last to answer.
pub(crate) struct Answered {
    pub tell: BTreeSet<RequestId>,
    pub resolve: Vec<TaskKey>,
    pub resume: Vec<TaskKey>,
}

/// Files `entry` as the final answer at `key`, which no request wants and
/// nothing awaits from then on. `resolving` holds the paths waiting on
/// their parent.
pub(crate) fn answered(
    key: TaskKey,
    entry: Entry,
    indexes: &mut Indexes,
    waits: &mut Waits,
    requests: &mut Requests,
    resolving: &mut BTreeSet<TaskKey>,
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
