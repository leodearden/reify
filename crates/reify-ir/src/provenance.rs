use std::collections::HashSet;

use reify_core::hash::ContentHash;
use reify_core::identity::{SnapshotId, ValueCellId};

/// Provenance record for a field imported from an external file.
///
/// Created when the evaluation engine ingests an external volumetric field
/// (e.g. an OpenVDB grid) via an `Input` occurrence. This struct captures the
/// five pieces of information that the resolved design records for each import
/// event — see `docs/prds/v0_2/imported-field-source.md` ("Resolved design
/// decisions" → "Provenance via Input occurrence") and arch §14.5.
///
/// All five fields are `pub` so downstream crates (task 5 call site, tests)
/// can read them via direct field access without needing getters.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldImportProvenance {
    /// Absolute or relative file path of the source file.
    pub path: String,
    /// Format name of the source file (e.g. `"OpenVDB"`, `"STEP"`).
    pub format: String,
    /// XXH3-128 content hash of the raw file bytes at ingestion time, used
    /// for cache-invalidation (see `reify_types::ContentHash`).
    pub content_hash: ContentHash,
    /// Unix epoch seconds at which the file was ingested (caller-supplied so
    /// the builder stays a pure function — no `SystemTime::now()` inside).
    pub ingestion_timestamp_secs: u64,
    /// Declared tolerance in SI units (metres) from the `Input` occurrence's
    /// `param tolerance : Length = …` declaration, after the Gate 4 filter
    /// (`is_finite() && >= 0.0`). `None` when no declaration is present or
    /// when the declared value is malformed (NaN / ±Inf / negative).
    pub declared_tolerance_si: Option<f64>,
}

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
    Elaboration { parent: SnapshotId },
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
    use reify_core::hash::ContentHash;

    #[test]
    fn field_import_provenance_clone_eq_and_hash_inequality() {
        let prov = FieldImportProvenance {
            path: "/data/fea_results.vdb".to_string(),
            format: "OpenVDB".to_string(),
            content_hash: ContentHash::of(b"bytes_a"),
            ingestion_timestamp_secs: 1_700_000_000,
            declared_tolerance_si: Some(50e-6),
        };

        // Clone + PartialEq round-trip.
        assert_eq!(prov.clone(), prov);

        // PartialEq is sensitive to content_hash: a struct that differs only in
        // that field must compare unequal.
        let prov_other = FieldImportProvenance {
            content_hash: ContentHash::of(b"bytes_b"),
            ..prov.clone()
        };
        assert_ne!(prov, prov_other);
    }

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
            SnapshotProvenance::Edit {
                changed: c,
                parent: p,
            } => {
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
            SnapshotProvenance::Resolution {
                scope,
                resolved: r,
                parent,
            } => {
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
