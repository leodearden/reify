//! A snapshot: the inputs one evaluation sees (contract 3).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use imbl::OrdMap;

use crate::core_id::CoreId;
use crate::input::{Input, InputKey};

/// Which snapshot: the core that made it, and a serial no other snapshot
/// in this process has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SnapshotId {
    pub(crate) core: CoreId,
    pub(crate) serial: u64,
}

impl SnapshotId {
    fn fresh(core: CoreId) -> Self {
        static MADE: AtomicU64 = AtomicU64::new(0);
        Self {
            core,
            serial: MADE.fetch_add(1, Ordering::Relaxed) + 1,
        }
    }
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
    pub(crate) fn new(core: CoreId, inputs: impl IntoIterator<Item = (InputKey, Input)>) -> Self {
        Self {
            id: SnapshotId::fresh(core),
            inputs: inputs.into_iter().collect(),
            token: Arc::new(()),
        }
    }

    /// A new snapshot of the same core: these inputs, with each `Some` in
    /// `changed` set and each `None` removed (contract 2.4). This snapshot
    /// is unchanged.
    pub fn edit(&self, changed: impl IntoIterator<Item = (InputKey, Option<Input>)>) -> Snapshot {
        let mut inputs = self.inputs.clone();
        for (key, input) in changed {
            match input {
                Some(input) => inputs.insert(key, input),
                None => inputs.remove(&key),
            };
        }
        Self {
            id: SnapshotId::fresh(self.id.core),
            inputs,
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
