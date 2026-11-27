//! The live requests, and which nodes in progress each wants: one relation,
//! seen from both ends, with one owner.

use std::collections::{BTreeMap, BTreeSet};

use crate::budget::{Allowance, Budget};
use crate::core_id::CoreId;
use crate::event::RequestId;
use crate::index::NodeKey;
use crate::snapshot::SnapshotId;

/// The host's handle on a request, given back to end it.
#[derive(Debug)]
#[must_use = "a request stays live until it is passed to `Core::end` or its budget ends it"]
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
    /// How many live requests are on each snapshot that has one.
    live_on: CountOn,
    /// How many live requests with a bounded budget are on each snapshot
    /// that has one.
    bounded_on: CountOn,
    wants: BTreeMap<RequestId, BTreeSet<NodeKey>>,
    wanted_by: BTreeMap<NodeKey, BTreeSet<RequestId>>,
    /// What each live request has spent of its budget.
    allowances: BTreeMap<RequestId, Allowance>,
    opened: u64,
}

impl Requests {
    pub fn new(core: CoreId) -> Self {
        Self {
            core,
            on: BTreeMap::new(),
            live_on: CountOn::default(),
            bounded_on: CountOn::default(),
            wants: BTreeMap::new(),
            wanted_by: BTreeMap::new(),
            allowances: BTreeMap::new(),
            opened: 0,
        }
    }

    /// Opens a request on `snapshot` that may spend `budget`.
    pub fn open(&mut self, snapshot: SnapshotId, budget: Budget) -> Request {
        self.opened += 1;
        let id = RequestId {
            core: self.core,
            serial: self.opened,
        };
        self.on.insert(id, snapshot);
        self.live_on.add(snapshot);
        let allowance = Allowance::new(budget);
        if allowance.is_bounded() {
            self.bounded_on.add(snapshot);
        }
        self.wants.insert(id, BTreeSet::new());
        self.allowances.insert(id, allowance);
        Request { id }
    }

    pub fn is_live(&self, id: RequestId) -> bool {
        self.on.contains_key(&id)
    }

    /// Whether a live request on `snapshot` has a bounded budget.
    pub fn any_bounded_on(&self, snapshot: SnapshotId) -> bool {
        self.bounded_on.any(snapshot)
    }

    /// Spends one path resolved from each live request in `needing`, and
    /// gives, in request order, each whose budget allowed no more, with its
    /// snapshot and budget; those are not charged.
    pub fn charge(&mut self, needing: &BTreeSet<RequestId>) -> Vec<(RequestId, SnapshotId, u64)> {
        let mut exhausted = Vec::new();
        for id in needing {
            let Some(allowance) = self.allowances.get_mut(id) else {
                continue;
            };
            if let Err(budget) = allowance.spend_path() {
                exhausted.push((*id, self.on[id], budget));
            }
        }
        exhausted
    }

    pub fn want(&mut self, request: RequestId, key: NodeKey) {
        if let Some(wanted) = self.wants.get_mut(&request) {
            wanted.insert(key);
            self.wanted_by.entry(key).or_default().insert(request);
        }
    }

    /// Whether a live request is on `snapshot`.
    pub fn any_on(&self, snapshot: SnapshotId) -> bool {
        self.live_on.any(snapshot)
    }

    /// The live requests that want `key`.
    pub fn wanting(&self, key: &NodeKey) -> impl Iterator<Item = RequestId> + '_ {
        self.wanted_by.get(key).into_iter().flatten().copied()
    }

    /// The live requests that wanted `key`, which has now been answered, so
    /// none of them wants it any more.
    pub fn settle(&mut self, key: &NodeKey) -> BTreeSet<RequestId> {
        let wanting = self.wanted_by.remove(key).unwrap_or_default();
        for request in &wanting {
            if let Some(wanted) = self.wants.get_mut(request) {
                wanted.remove(key);
            }
        }
        wanting
    }

    /// Every node some live request wants.
    pub fn wanted(&self) -> impl Iterator<Item = NodeKey> + '_ {
        self.wanted_by.keys().copied()
    }

    /// Each live request that still wants a node, with its snapshot and
    /// the nodes it wants.
    pub fn wanting_any(&self) -> Vec<(RequestId, SnapshotId, Vec<NodeKey>)> {
        self.wants
            .iter()
            .filter(|(_, wanted)| !wanted.is_empty())
            .map(|(request, wanted)| {
                let snapshot = self.on[request];
                (*request, snapshot, wanted.iter().copied().collect())
            })
            .collect()
    }

    pub fn end(&mut self, request: Request) {
        self.end_id(request.id);
    }

    /// Ends the request `id`, for the core's own use; ending it again does
    /// nothing.
    pub fn end_id(&mut self, id: RequestId) {
        let Some(snapshot) = self.on.remove(&id) else {
            return;
        };
        self.live_on.remove(snapshot);
        let allowance = self
            .allowances
            .remove(&id)
            .expect("a live request has an allowance");
        if allowance.is_bounded() {
            self.bounded_on.remove(snapshot);
        }
        for key in self.wants.remove(&id).unwrap_or_default() {
            if let Some(wanting) = self.wanted_by.get_mut(&key) {
                wanting.remove(&id);
                if wanting.is_empty() {
                    self.wanted_by.remove(&key);
                }
            }
        }
    }
}

/// A count of live requests on each snapshot that has one.
#[derive(Default)]
struct CountOn(BTreeMap<SnapshotId, usize>);

impl CountOn {
    fn add(&mut self, snapshot: SnapshotId) {
        *self.0.entry(snapshot).or_default() += 1;
    }

    fn remove(&mut self, snapshot: SnapshotId) {
        let count = self
            .0
            .get_mut(&snapshot)
            .expect("a live request is counted on its snapshot");
        *count -= 1;
        if *count == 0 {
            self.0.remove(&snapshot);
        }
    }

    fn any(&self, snapshot: SnapshotId) -> bool {
        self.0.contains_key(&snapshot)
    }
}
