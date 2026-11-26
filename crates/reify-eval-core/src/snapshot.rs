//! A snapshot: the inputs one evaluation sees (contract 3).

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

#[derive(Clone)]
pub struct Snapshot {
    id: SnapshotId,
    inputs: OrdMap<InputKey, Input>,
}

impl Snapshot {
    pub(crate) fn new(id: SnapshotId, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Self {
        Self {
            id,
            inputs: inputs.into_iter().collect(),
        }
    }

    pub fn id(&self) -> SnapshotId {
        self.id
    }

    pub(crate) fn input(&self, key: &InputKey) -> Option<&Input> {
        self.inputs.get(key)
    }
}
