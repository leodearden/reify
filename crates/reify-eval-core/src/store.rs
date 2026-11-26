//! The store (contract 4.2, 5.1): a decision tree from key to what was
//! read next there, or to the result filed there. Each entry knows the key
//! it was reached from. It owns its results until they are trimmed (5.5).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::content::Content;
use crate::key::{Chain, Key};
use crate::kind_id::KindId;
use crate::outcome::{Erased, Outcome};
use crate::reference::Ref;

/// A result as the store holds it: the kind that produced it, its outcome
/// and its content.
pub(crate) struct StoredResult {
    #[expect(
        dead_code,
        reason = "read by StoredResult::kind, plan step-10 of this leaf"
    )]
    pub kind: KindId,
    pub outcome: Outcome<Erased>,
    pub content: Content,
}

pub(crate) enum Stored {
    /// The read request a body made at this key.
    Next(Arc<[Ref]>),
    Result(StoredResult),
}

struct Entry {
    /// The key this one is a step on from; none at a recipe's key.
    #[expect(
        dead_code,
        reason = "read by selected_store, plan step-10 of this leaf"
    )]
    parent: Option<Key>,
    stored: Stored,
}

#[derive(Default)]
pub(crate) struct Store {
    tree: BTreeMap<Key, Entry>,
}

impl Store {
    pub fn probe(&self, key: Key) -> Option<&Stored> {
        self.tree.get(&key).map(|entry| &entry.stored)
    }

    /// Files `result` at the end of `chain`, with each request on the way.
    /// What is already filed at a key stays: the first result filed there
    /// wins.
    pub fn file(&mut self, chain: Chain, result: StoredResult) {
        let (steps, end) = chain.into_parts();
        let mut parent = None;
        for (key, refs) in steps {
            self.insert(key, parent, Stored::Next(refs));
            parent = Some(key);
        }
        self.insert(end, parent, Stored::Result(result));
    }

    pub fn clear(&mut self) {
        self.tree.clear();
    }

    fn insert(&mut self, key: Key, parent: Option<Key>, stored: Stored) {
        self.tree.entry(key).or_insert(Entry { parent, stored });
    }
}
