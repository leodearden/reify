//! The index of each live snapshot (contract 4.4, 5.5): the final answer at
//! every path answered in it, with the inputs it was made of. Within a
//! snapshot a path is answered once; an index is kept while the host holds
//! its snapshot or a live request is on it.

use std::collections::BTreeMap;
use std::sync::Arc;

use imbl::OrdMap;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::input::{Input, InputKey};
use crate::limits::Limits;
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::snapshot::{Snapshot, SnapshotId};
use crate::step::Step;

/// A node of one snapshot, where its final answer is filed: within a
/// snapshot a path is answered once, by at most one body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NodeKey {
    pub snapshot: SnapshotId,
    pub path: PathId,
}

/// A path's final answer: its outcome, its content and, when a body
/// produced it, the declaration of the node that body ran for and the
/// steps of the children that run declared (contract 7.1).
#[derive(Clone)]
pub(crate) struct Entry {
    pub outcome: Outcome<Erased>,
    pub declaration: Option<Arc<Declaration>>,
    pub content: Content,
    pub children_declared: Arc<[Step]>,
}

impl Entry {
    /// An answer no body produced: it has no declaration and declared no
    /// children.
    pub fn settled(outcome: Outcome<Erased>, content: Content) -> Self {
        Self {
            outcome,
            declaration: None,
            content,
            children_declared: Arc::new([]),
        }
    }
}

pub(crate) struct Index {
    inputs: OrdMap<InputKey, Input>,
    limits: Limits,
    entries: OrdMap<PathId, Entry>,
    /// The declaration of each node whose run has declared a child, filed
    /// when it first declares one: a child's passed binding is read
    /// through it while its declarer may still be running.
    declarers: OrdMap<PathId, Arc<Declaration>>,
}

impl Index {
    pub fn input(&self, key: &InputKey) -> Option<&Input> {
        self.inputs.get(key)
    }

    /// The snapshot's limit settings.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn entry(&self, path: PathId) -> Option<&Entry> {
        self.entries.get(&path)
    }

    /// The declaration of the node at `path`, known where its run declared
    /// a child or its answer is filed with one.
    pub fn declaration(&self, path: PathId) -> Option<&Declaration> {
        let filed = || self.entries.get(&path)?.declaration.as_ref();
        self.declarers
            .get(&path)
            .or_else(filed)
            .map(|declaration| &**declaration)
    }
}

#[derive(Default)]
pub(crate) struct Indexes {
    by_snapshot: BTreeMap<SnapshotId, Index>,
}

impl Indexes {
    /// The index of `snapshot`, made empty if it has none.
    ///
    /// # Panics
    ///
    /// If a limit setting of `snapshot` is not a u64.
    pub fn open(&mut self, snapshot: &Snapshot) {
        self.by_snapshot
            .entry(snapshot.id())
            .or_insert_with(|| Index {
                inputs: snapshot.inputs().clone(),
                limits: Limits::of(snapshot.inputs()),
                entries: OrdMap::new(),
                declarers: OrdMap::new(),
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

    /// Files `declaration` as that of the node at `path` in the index of
    /// `snapshot`, which declares a child: within a snapshot a node has one
    /// declaration.
    ///
    /// # Panics
    ///
    /// If the index of `snapshot` was never opened or has been dropped.
    pub fn file_declarer(
        &mut self,
        snapshot: SnapshotId,
        path: PathId,
        declaration: &Arc<Declaration>,
    ) {
        let Some(index) = self.by_snapshot.get_mut(&snapshot) else {
            lost(snapshot)
        };
        index
            .declarers
            .entry(path)
            .or_insert_with(|| Arc::clone(declaration));
    }

    /// Drops the index of each snapshot `live` says is not live, returning
    /// their ids.
    pub fn prune(&mut self, live: impl Fn(SnapshotId) -> bool) -> Vec<SnapshotId> {
        let mut dropped = Vec::new();
        self.by_snapshot.retain(|snapshot, _| {
            let kept = live(*snapshot);
            if !kept {
                dropped.push(*snapshot);
            }
            kept
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
