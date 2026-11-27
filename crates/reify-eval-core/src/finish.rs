//! What a node's end does to the index, the wait relation and the
//! requests: a final answer, or a failure outside the inputs. The executor
//! acts on what each returns.

use std::collections::BTreeSet;

use crate::content::Content;
use crate::event::RequestId;
use crate::index::{Entry, Indexes, NodeKey};
use crate::kinds::Kinds;
use crate::outcome::{Erased, Outcome};
use crate::requests::Requests;
use crate::store::{Store, StoredResult};
use crate::task::Finished;
use crate::waits::Waits;

/// Files `outcome`, which the body `finished` left produced, in `store` at
/// the end of the chain its reads took it through, and returns the node's
/// final answer with its content, the steps of the children its run
/// declared and whether it depended on a substitution. The content of a
/// value not hashed by value is that of the chain's key, which covers what
/// the run read through its views. A result is filed unless its run opened
/// a view or it depended on a substitution: this is leaf ι's rule, and
/// leaf κ (#10562) files both sorts with what their trials rested on.
pub(crate) fn filed(
    finished: Finished,
    outcome: Outcome<Erased>,
    kinds: &Kinds,
    store: &mut Store,
) -> Entry {
    let Finished {
        declaration,
        chain,
        children,
        depended,
        opened_view,
    } = finished;
    let kind = kinds
        .get(declaration.kind())
        .expect("a body runs only for a kind the core was built with");
    let content = Content::of_produced(&outcome, kind, chain.key().hash());
    let result = StoredResult {
        kind: declaration.kind(),
        outcome: outcome.clone(),
        content,
    };
    if !opened_view && !depended {
        store.file(chain, result);
    }
    Entry {
        outcome,
        declaration: Some(declaration),
        content,
        children_declared: children,
        depended,
    }
}

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
    indexes.file(key.state, key.path, entry);
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
