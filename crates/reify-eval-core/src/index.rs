//! The index of each live snapshot (contract 4.4, 5.5): the final answer at
//! every path answered in it, with the inputs it was made of. Within a
//! snapshot a path is answered once; an index is kept while a clone of its
//! snapshot is held or a live request is on it.

use std::collections::BTreeMap;
use std::sync::Arc;

use imbl::OrdMap;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::input::{Input, InputKey};
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::snapshot::{Snapshot, SnapshotId, SnapshotWatch};

/// A node of one snapshot, where its final answer is filed: within a
/// snapshot a path is answered once, by at most one body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NodeKey {
    pub snapshot: SnapshotId,
    pub path: PathId,
}

/// A path's final answer: its outcome, its content and, when a body
/// produced it, the declaration of the node that body ran for.
#[derive(Clone)]
pub(crate) struct Entry {
    pub outcome: Outcome<Erased>,
    pub declaration: Option<Arc<Declaration>>,
    pub content: Content,
}

pub(crate) struct Index {
    inputs: OrdMap<InputKey, Input>,
    watch: SnapshotWatch,
    entries: OrdMap<PathId, Entry>,
}

impl Index {
    pub fn input(&self, key: &InputKey) -> Option<&Input> {
        self.inputs.get(key)
    }

    pub fn entry(&self, path: PathId) -> Option<&Entry> {
        self.entries.get(&path)
    }
}

#[derive(Default)]
pub(crate) struct Indexes {
    by_snapshot: BTreeMap<SnapshotId, Index>,
}

impl Indexes {
    /// The index of `snapshot`, made empty if it has none.
    pub fn open(&mut self, snapshot: &Snapshot) {
        self.by_snapshot
            .entry(snapshot.id())
            .or_insert_with(|| Index {
                inputs: snapshot.inputs().clone(),
                watch: snapshot.watch(),
                entries: OrdMap::new(),
            });
    }

    /// # Panics
    ///
    /// If the index of `snapshot` was never opened or has been dropped.
    pub fn of(&self, snapshot: SnapshotId) -> &Index {
        self.by_snapshot
            .get(&snapshot)
            .unwrap_or_else(|| lost(snapshot))
    }

    /// Files the final answer at `path` in the index of `snapshot`.
    ///
    /// # Panics
    ///
    /// If the index of `snapshot` was never opened or has been dropped.
    pub fn file(&mut self, snapshot: SnapshotId, path: PathId, entry: Entry) {
        let Some(index) = self.by_snapshot.get_mut(&snapshot) else {
            lost(snapshot)
        };
        index.entries.insert(path, entry);
    }

    /// Drops the index of each snapshot no clone of which is held and on
    /// which `requested` says no live request is, returning their ids.
    pub fn prune(&mut self, requested: impl Fn(SnapshotId) -> bool) -> Vec<SnapshotId> {
        let mut dropped = Vec::new();
        self.by_snapshot.retain(|snapshot, index| {
            let live = index.watch.is_held() || requested(*snapshot);
            if !live {
                dropped.push(*snapshot);
            }
            live
        });
        dropped
    }

    pub fn clear(&mut self) {
        self.by_snapshot.clear();
    }
}

/// Refuses work on a snapshot whose index is gone: work on a snapshot keeps
/// its index.
pub(crate) fn lost(snapshot: SnapshotId) -> ! {
    panic!("work on a snapshot keeps its index, and {snapshot:?} has none")
}
