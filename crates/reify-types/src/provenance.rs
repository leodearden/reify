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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_provenance() {
        let prov = SnapshotProvenance::Initial;
        let prov2 = prov.clone();
        assert_eq!(prov, prov2);
        let debug = format!("{:?}", prov);
        assert!(debug.contains("Initial"));
    }

    #[test]
    fn edit_provenance() {
        let mut changed = HashSet::new();
        changed.insert(ValueCellId::new("Bracket", "width"));
        changed.insert(ValueCellId::new("Bracket", "height"));
        let parent = SnapshotId(0);

        let prov = SnapshotProvenance::Edit {
            changed: changed.clone(),
            parent,
        };
        let prov2 = prov.clone();
        assert_eq!(prov, prov2);

        match &prov {
            SnapshotProvenance::Edit { changed: c, parent: p } => {
                assert_eq!(c.len(), 2);
                assert!(c.contains(&ValueCellId::new("Bracket", "width")));
                assert_eq!(*p, SnapshotId(0));
            }
            _ => panic!("expected Edit"),
        }
    }

    #[test]
    fn elaboration_provenance() {
        let prov = SnapshotProvenance::Elaboration {
            parent: SnapshotId(1),
        };
        let prov2 = prov.clone();
        assert_eq!(prov, prov2);

        match &prov {
            SnapshotProvenance::Elaboration { parent } => {
                assert_eq!(*parent, SnapshotId(1));
            }
            _ => panic!("expected Elaboration"),
        }
    }

    #[test]
    fn resolution_provenance() {
        let mut resolved = HashSet::new();
        resolved.insert(ValueCellId::new("Bracket", "thickness"));

        let prov = SnapshotProvenance::Resolution {
            scope: "min_thickness".to_string(),
            resolved: resolved.clone(),
            parent: SnapshotId(2),
        };
        let prov2 = prov.clone();
        assert_eq!(prov, prov2);

        match &prov {
            SnapshotProvenance::Resolution { scope, resolved: r, parent } => {
                assert_eq!(scope, "min_thickness");
                assert_eq!(r.len(), 1);
                assert!(r.contains(&ValueCellId::new("Bracket", "thickness")));
                assert_eq!(*parent, SnapshotId(2));
            }
            _ => panic!("expected Resolution"),
        }
    }

    #[test]
    fn different_variants_not_equal() {
        let initial = SnapshotProvenance::Initial;
        let elab = SnapshotProvenance::Elaboration {
            parent: SnapshotId(0),
        };
        assert_ne!(initial, elab);
    }
}
