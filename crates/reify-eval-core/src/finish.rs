//! What a node's final answer does to the index, the wait relation and the
//! requests. The executor acts on what it returns.

use std::collections::BTreeSet;

use crate::event::RequestId;
use crate::index::{Entry, Indexes};
use crate::requests::Requests;
use crate::task::TaskKey;
use crate::waits::Waits;

/// Who a final answer concerns: the live requests to tell, and the nodes
/// that awaited it and now await nothing.
pub(crate) struct Answered {
    pub tell: BTreeSet<RequestId>,
    pub freed: Vec<TaskKey>,
}

/// Files `entry` as the final answer at `key`, which no request wants and
/// nothing awaits from then on.
pub(crate) fn answered(
    key: TaskKey,
    entry: Entry,
    indexes: &mut Indexes,
    waits: &mut Waits,
    requests: &mut Requests,
) -> Answered {
    indexes.file(key.snapshot, key.path, entry);
    Answered {
        tell: requests.settle(&key),
        freed: waits.release(key),
    }
}
