use std::collections::HashSet;

use crate::identity::{SnapshotId, ValueCellId};

/// Tracks how a snapshot was created, enabling provenance chains
/// for undo/redo and change auditing.
#[derive(Debug, Clone, PartialEq)]
pub enum SnapshotProvenance {
    /// The initial snapshot (no parent).
    Initial,
    /// User or API edit that changed specific value cells.
    Edit {
        changed: HashSet<ValueCellId>,
        parent: SnapshotId,
    },
    /// Elaboration pass (re-evaluation of derived values).
    Elaboration {
        parent: SnapshotId,
    },
    /// Constraint resolution pass that resolved specific value cells.
    Resolution {
        scope: String,
        resolved: HashSet<ValueCellId>,
        parent: SnapshotId,
    },
}
