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
#[derive(Clone)]
pub struct StoredResult {
    pub(crate) kind: KindId,
    pub(crate) outcome: Outcome<Erased>,
    pub(crate) content: Content,
}

impl StoredResult {
    /// The id of the kind whose body produced the result.
    pub fn kind(&self) -> KindId {
        self.kind
    }
}

#[derive(Clone)]
pub(crate) enum Stored {
    /// The read request a body made at this key.
    Next(Arc<[Ref]>),
    Result(StoredResult),
}

#[derive(Clone)]
struct Entry {
    /// The key this one is a step on from; none at a recipe's key.
    parent: Option<Key>,
    stored: Stored,
}

/// Results filed under their keys, to build a core on (`Builder::on_store`)
/// and to take from one (`Core::into_store`, `Core::selected_store`).
#[derive(Default)]
pub struct Store {
    tree: BTreeMap<Key, Entry>,
}

impl Store {
    pub(crate) fn probe(&self, key: Key) -> Option<&Stored> {
        self.tree.get(&key).map(|entry| &entry.stored)
    }

    /// Files `result` at the end of `chain`, with each request on the way.
    /// What is already filed at a key stays: the first result filed there
    /// wins.
    pub(crate) fn file(&mut self, chain: Chain, result: StoredResult) {
        let (steps, end) = chain.into_parts();
        let mut parent = None;
        for (key, refs) in steps {
            self.insert(key, parent, Stored::Next(refs));
            parent = Some(key);
        }
        self.insert(end, parent, Stored::Result(result));
    }

    pub(crate) fn clear(&mut self) {
        self.tree.clear();
    }

    /// A new store of each result `keep` accepts, in key order, with every
    /// request on the chain that leads to it, found by following parent
    /// keys up to its recipe's key. Outcomes are shared, not copied.
    pub(crate) fn selected(&self, mut keep: impl FnMut(&StoredResult) -> bool) -> Store {
        let mut selected = Store::default();
        for (key, entry) in &self.tree {
            let Stored::Result(result) = &entry.stored else {
                continue;
            };
            if !keep(result) {
                continue;
            }
            selected.tree.insert(*key, entry.clone());
            let mut parent = entry.parent;
            while let Some(at) = parent.filter(|at| !selected.tree.contains_key(at)) {
                let above = &self.tree[&at];
                selected.tree.insert(at, above.clone());
                parent = above.parent;
            }
        }
        selected
    }

    fn insert(&mut self, key: Key, parent: Option<Key>, stored: Stored) {
        self.tree.entry(key).or_insert(Entry { parent, stored });
    }
}
