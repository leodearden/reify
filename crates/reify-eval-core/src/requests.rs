//! The live requests, and which running tasks each wants: one relation,
//! seen from both ends, with one owner.

use std::collections::{BTreeMap, BTreeSet};

use crate::core_id::CoreId;
use crate::event::RequestId;
use crate::snapshot::SnapshotId;
use crate::task::TaskKey;

/// The host's handle on a request, given back to end it.
#[derive(Debug)]
#[must_use = "a request stays live until it is passed to `Core::end`"]
pub struct Request {
    id: RequestId,
}

impl Request {
    pub fn id(&self) -> RequestId {
        self.id
    }
}

pub(crate) struct Requests {
    core: CoreId,
    on: BTreeMap<RequestId, SnapshotId>,
    wants: BTreeMap<RequestId, BTreeSet<TaskKey>>,
    wanted_by: BTreeMap<TaskKey, BTreeSet<RequestId>>,
    opened: u64,
}

impl Requests {
    pub fn new(core: CoreId) -> Self {
        Self {
            core,
            on: BTreeMap::new(),
            wants: BTreeMap::new(),
            wanted_by: BTreeMap::new(),
            opened: 0,
        }
    }

    /// Opens a request on `snapshot`.
    pub fn open(&mut self, snapshot: SnapshotId) -> Request {
        self.opened += 1;
        let id = RequestId {
            core: self.core,
            serial: self.opened,
        };
        self.on.insert(id, snapshot);
        self.wants.insert(id, BTreeSet::new());
        Request { id }
    }

    pub fn want(&mut self, request: RequestId, key: TaskKey) {
        if let Some(wanted) = self.wants.get_mut(&request) {
            wanted.insert(key.clone());
            self.wanted_by.entry(key).or_default().insert(request);
        }
    }

    /// Whether a live request is on `snapshot`.
    pub fn any_on(&self, snapshot: SnapshotId) -> bool {
        self.on.values().any(|on| *on == snapshot)
    }

    /// The live requests that want `key`.
    pub fn wanting(&self, key: &TaskKey) -> impl Iterator<Item = RequestId> + '_ {
        self.wanted_by.get(key).into_iter().flatten().copied()
    }

    /// The live requests that wanted `key`, which has now been answered, so
    /// none of them wants it any more.
    pub fn settle(&mut self, key: &TaskKey) -> BTreeSet<RequestId> {
        let wanting = self.wanted_by.remove(key).unwrap_or_default();
        for request in &wanting {
            if let Some(wanted) = self.wants.get_mut(request) {
                wanted.remove(key);
            }
        }
        wanting
    }

    /// Ends `request`, returning the tasks no live request wants any more.
    pub fn end(&mut self, request: Request) -> Vec<TaskKey> {
        let mut unwanted = Vec::new();
        self.on.remove(&request.id);
        for key in self.wants.remove(&request.id).unwrap_or_default() {
            if let Some(wanting) = self.wanted_by.get_mut(&key) {
                wanting.remove(&request.id);
                if wanting.is_empty() {
                    self.wanted_by.remove(&key);
                    unwanted.push(key);
                }
            }
        }
        unwanted
    }
}
