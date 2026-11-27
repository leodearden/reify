//! The index of each live state (contract 4.4, 5.5, 6): the final answer at
//! every path answered in it, with the inputs it was made of. A state is a
//! snapshot, or a view a body of a snapshot opened, which has the
//! snapshot's inputs and its own answers. Within a state a path is answered
//! once; a snapshot's index is kept while the host holds its snapshot or a
//! live request is on it, and a view's while the run that opened it runs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use imbl::OrdMap;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::declared_children::ChildSteps;
use crate::input::{Input, InputKey};
use crate::limits::Limits;
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::snapshot::{Snapshot, SnapshotId};

/// A view, numbered in the order the core opened it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ViewId(u64);

/// Which state a node is of: a snapshot, or a view opened on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StateId {
    pub snapshot: SnapshotId,
    pub view: Option<ViewId>,
}

impl StateId {
    pub fn of(snapshot: SnapshotId) -> Self {
        Self {
            snapshot,
            view: None,
        }
    }
}

/// A node of one state, where its final answer is filed: within a state a
/// path is answered once, by at most one body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NodeKey {
    pub state: StateId,
    pub path: PathId,
}

/// A path's final answer: its outcome, its content and, when a body
/// produced it, the declaration of the node that body ran for and the
/// steps of the children that run declared (contract 7.1); and whether it
/// depended on the substitution of its state's view, which in a snapshot
/// it never does.
#[derive(Clone)]
pub(crate) struct Entry {
    pub outcome: Outcome<Erased>,
    pub declaration: Option<Arc<Declaration>>,
    pub content: Content,
    pub children_declared: ChildSteps,
    pub depended: bool,
}

impl Entry {
    /// An answer no body produced: it has no declaration and declared no
    /// children.
    pub fn settled(outcome: Outcome<Erased>, content: Content, depended: bool) -> Self {
        Self {
            outcome,
            declaration: None,
            content,
            children_declared: ChildSteps::none(),
            depended,
        }
    }

    /// The answer of the node declared `declaration` at a path its view
    /// supposes: the supposed outcome, which depended on the substitution.
    pub fn supposed(supposed: &Supposed, declaration: Arc<Declaration>) -> Self {
        Self {
            outcome: supposed.outcome.clone(),
            declaration: Some(declaration),
            content: supposed.content,
            children_declared: ChildSteps::none(),
            depended: true,
        }
    }
}

/// An outcome a view supposes at a path, with its content.
pub(crate) struct Supposed {
    pub outcome: Outcome<Erased>,
    pub content: Content,
}

/// What a view's index holds beyond a snapshot's: the path of the node
/// whose run opened it, and the outcome it supposes at each path.
struct Opened {
    opener: PathId,
    supposed: BTreeMap<PathId, Supposed>,
}

pub(crate) struct Index {
    inputs: OrdMap<InputKey, Input>,
    limits: Limits,
    entries: OrdMap<PathId, Entry>,
    /// The declaration of each node whose run has declared a child, filed
    /// when it first declares one: a child's passed binding is read
    /// through it while its declarer may still be running.
    declarers: OrdMap<PathId, Arc<Declaration>>,
    /// A view's opener and substitution; none for a snapshot.
    opened: Option<Opened>,
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

    /// The outcome the index's view supposes at `path`, if any.
    pub fn supposed(&self, path: PathId) -> Option<&Supposed> {
        self.opened.as_ref()?.supposed.get(&path)
    }

    /// The path of the node whose run opened the index's view; none for a
    /// snapshot.
    pub fn opener(&self) -> Option<PathId> {
        self.opened.as_ref().map(|opened| opened.opener)
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
    by_state: BTreeMap<StateId, Index>,
    /// How many views have been opened.
    views: u64,
}

impl Indexes {
    /// The index of `snapshot`, made empty if it has none.
    ///
    /// # Panics
    ///
    /// If a limit setting of `snapshot` is not a u64.
    pub fn open(&mut self, snapshot: &Snapshot) {
        self.by_state
            .entry(StateId::of(snapshot.id()))
            .or_insert_with(|| Index {
                inputs: snapshot.inputs().clone(),
                limits: Limits::of(snapshot.inputs()),
                entries: OrdMap::new(),
                declarers: OrdMap::new(),
                opened: None,
            });
    }

    /// Opens an empty index for a new view on `snapshot`, which the run of
    /// the node at `opener` opened, supposing `supposed`: it has the
    /// snapshot's inputs and limit settings.
    ///
    /// # Panics
    ///
    /// If the index of `snapshot` was never opened or has been dropped.
    pub fn open_view(
        &mut self,
        snapshot: SnapshotId,
        opener: PathId,
        supposed: BTreeMap<PathId, Supposed>,
    ) -> StateId {
        let inputs = self.of(StateId::of(snapshot)).inputs.clone();
        let index = Index {
            limits: Limits::of(&inputs),
            inputs,
            entries: OrdMap::new(),
            declarers: OrdMap::new(),
            opened: Some(Opened { opener, supposed }),
        };
        self.views += 1;
        let state = StateId {
            snapshot,
            view: Some(ViewId(self.views)),
        };
        self.by_state.insert(state, index);
        state
    }

    /// Drops the index of `state`, a view whose opener's run has ended.
    pub fn close(&mut self, state: StateId) {
        self.by_state.remove(&state);
    }

    /// Whether the index of `state` is kept.
    pub fn is_open(&self, state: StateId) -> bool {
        self.by_state.contains_key(&state)
    }

    /// # Panics
    ///
    /// If the index of `state` was never opened or has been dropped.
    pub fn of(&self, state: StateId) -> &Index {
        self.by_state.get(&state).unwrap_or_else(|| lost(state))
    }

    /// Files the final answer at `path` in the index of `state`.
    ///
    /// # Panics
    ///
    /// If the index of `state` was never opened or has been dropped, or if
    /// `entry` depended on a substitution and `state` is a snapshot, which
    /// supposes nothing.
    pub fn file(&mut self, state: StateId, path: PathId, entry: Entry) {
        assert!(
            state.view.is_some() || !entry.depended,
            "an answer in snapshot {state:?} depends on no substitution, but the one at {path:?} did"
        );
        let Some(index) = self.by_state.get_mut(&state) else {
            lost(state)
        };
        index.entries.insert(path, entry);
    }

    /// Files `declaration` as that of the node at `path` in the index of
    /// `state`, which declares a child: within a state a node has one
    /// declaration.
    ///
    /// # Panics
    ///
    /// If the index of `state` was never opened or has been dropped.
    pub fn file_declarer(&mut self, state: StateId, path: PathId, declaration: &Arc<Declaration>) {
        let Some(index) = self.by_state.get_mut(&state) else {
            lost(state)
        };
        index
            .declarers
            .entry(path)
            .or_insert_with(|| Arc::clone(declaration));
    }

    /// Drops the index of each state on a snapshot `live` says is not live,
    /// returning those snapshots.
    pub fn prune(&mut self, live: impl Fn(SnapshotId) -> bool) -> BTreeSet<SnapshotId> {
        let mut dropped = BTreeSet::new();
        self.by_state.retain(|state, _| {
            let kept = live(state.snapshot);
            if !kept {
                dropped.insert(state.snapshot);
            }
            kept
        });
        dropped
    }

    pub fn clear(&mut self) {
        self.by_state.clear();
    }
}

/// Refuses work on a state whose index is gone: work on a state keeps its
/// index.
pub(crate) fn lost(state: StateId) -> ! {
    panic!("work on a state keeps its index, and {state:?} has none")
}
