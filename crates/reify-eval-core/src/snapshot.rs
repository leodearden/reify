//! A snapshot: the inputs one evaluation sees (contract 3).

use std::sync::{Arc, Weak};

use imbl::OrdMap;

use crate::core_id::CoreId;
use crate::input::{Input, InputKey};

/// Which snapshot: the core that made it, and its place among that core's
/// snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId {
    pub(crate) core: CoreId,
    pub(crate) serial: u64,
}

/// Every clone of a snapshot shares one token, so the core can tell
/// whether any clone is still held (contract 4.4).
#[derive(Clone)]
pub struct Snapshot {
    id: SnapshotId,
    inputs: OrdMap<InputKey, Input>,
    token: Arc<()>,
}

/// Says whether any clone of a snapshot is still held, without holding one.
pub(crate) struct SnapshotWatch(Weak<()>);

impl SnapshotWatch {
    pub fn is_held(&self) -> bool {
        self.0.strong_count() > 0
    }
}

impl Snapshot {
    pub(crate) fn new(id: SnapshotId, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Self {
        Self {
            id,
            inputs: inputs.into_iter().collect(),
            token: Arc::new(()),
        }
    }

    pub fn id(&self) -> SnapshotId {
        self.id
    }

    pub(crate) fn inputs(&self) -> &OrdMap<InputKey, Input> {
        &self.inputs
    }

    pub(crate) fn watch(&self) -> SnapshotWatch {
        SnapshotWatch(Arc::downgrade(&self.token))
    }
}
