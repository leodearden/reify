// Snapshot: immutable evaluation state with provenance tracking.

use reify_compiler::CompiledModule;
use reify_core::{ConstraintNodeId, ContentHash, SnapshotId, ValueCellId, VersionId};
use reify_ir::{DeterminacyState, PersistentMap, SnapshotProvenance, Value};

use crate::graph::EvaluationGraph;

/// An immutable snapshot of evaluation state.
///
/// Contains the topology graph, current values with determinacy states,
/// a topology fingerprint for cache invalidation, and provenance tracking.
/// Cloning is O(1) via PersistentMap structural sharing.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub version: VersionId,
    pub graph: EvaluationGraph,
    pub values: PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    pub topology_fingerprint: ContentHash,
    pub provenance: SnapshotProvenance,
    /// task 2629 — per-template ledger of `ConstraintNodeId`s and
    /// `ConstraintNodeId`s emitted by the runtime forall re-elaboration
    /// pass in `engine_edit::edit_param` for each `forall_template` in the
    /// graph. Indexed in lockstep with `graph.forall_templates`. Entries
    /// here are added when a deferred-count `forall` becomes known
    /// (Undef → Int(N)) and drained on count change so that prior per-element
    /// constraints are removed from `graph.constraints` before the
    /// fresh emission. An explicit ledger is preferred over label-scanning
    /// (`label.starts_with("forall@")`) because user-labelled constraints
    /// could collide with that prefix; the parallel-array shape mirrors
    /// `Engine::active_purposes` (`HashMap<purpose_name, Vec<ConstraintNodeId>>`).
    pub forall_emitted: Vec<Vec<ConstraintNodeId>>,
}

impl Snapshot {
    /// Create an initial snapshot from a compiled module.
    ///
    /// Builds the evaluation graph from the module's templates,
    /// wires `module.auto_type_substitution` into `graph.auto_type_substitution`
    /// BEFORE computing `topology_fingerprint` so the 7th bucket reflects the
    /// substitution (PRD task 5 criterion 7, tasks 2388/2778). Initializes all
    /// values to (Undef, Undetermined), and sets provenance to Initial with
    /// version/snapshot IDs starting at 0.
    pub fn from_compiled_module(module: &CompiledModule) -> Self {
        let mut graph = EvaluationGraph::from_templates(&module.templates);
        // Wire MultiParamResolutionOutcome.substitution from CompiledModule
        // into the graph BEFORE computing topology_fingerprint, so the 7th
        // bucket reflects the substitution (PRD task 5 criterion 7,
        // tasks 2388/2778). Uniqueness of param names is guaranteed by
        // AutoTypeSubstitution::new (always-panic checked constructor).
        graph.auto_type_substitution = module.auto_type_substitution.clone().into_inner();
        let topology_fingerprint = graph.topology_fingerprint();

        // Initialize all value cells: Auto cells get (Undef, Auto),
        // all others get (Undef, Undetermined).
        let mut values = PersistentMap::new();
        for (id, node) in graph.value_cells.iter() {
            let det = if node.kind.is_auto() {
                DeterminacyState::Auto
            } else {
                DeterminacyState::Undetermined
            };
            values.insert(id.clone(), (Value::Undef, det));
        }

        let forall_emitted = vec![Vec::new(); graph.forall_templates.len()];

        Snapshot {
            id: SnapshotId(0),
            version: VersionId(0),
            graph,
            values,
            topology_fingerprint,
            provenance: SnapshotProvenance::Initial,
            forall_emitted,
        }
    }
}

#[cfg(test)]
mod tests {
    use reify_core::ValueCellId;

    use super::*;

    #[test]
    fn snapshot_construction() {
        let graph = EvaluationGraph::default();
        let values = PersistentMap::new();
        let fingerprint = ContentHash::of_str("empty");

        let snap = Snapshot {
            id: SnapshotId(0),
            version: VersionId(0),
            graph: graph.clone(),
            values,
            topology_fingerprint: fingerprint,
            provenance: SnapshotProvenance::Initial,
            forall_emitted: Vec::new(),
        };

        assert_eq!(snap.id, SnapshotId(0));
        assert_eq!(snap.version, VersionId(0));
        assert_eq!(snap.topology_fingerprint, fingerprint);
        assert_eq!(snap.provenance, SnapshotProvenance::Initial);
    }

    #[test]
    fn snapshot_from_compiled_module() {
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let snap = Snapshot::from_compiled_module(&module);

        // Check snapshot IDs start at 0
        assert_eq!(snap.id, SnapshotId(0));
        assert_eq!(snap.version, VersionId(0));

        // Check provenance is Initial
        assert_eq!(snap.provenance, SnapshotProvenance::Initial);

        // Check graph has correct number of nodes
        // bracket_compiled_module has 5 params + 1 let (volume) = 6 value cells
        assert_eq!(snap.graph.value_cells.len(), 6);
        // 3 constraints
        assert_eq!(snap.graph.constraints.len(), 3);

        // Check values are initialized to (Undef, Undetermined)
        assert_eq!(snap.values.len(), 6);
        for (_id, (val, det)) in snap.values.iter() {
            assert!(val.is_undef(), "Expected Undef, got {:?}", val);
            assert_eq!(*det, DeterminacyState::Undetermined);
        }

        // Check specific value cell exists
        let width_id = ValueCellId::new("Bracket", "width");
        assert!(snap.values.get(&width_id).is_some());
        let (val, det) = snap.values.get(&width_id).unwrap();
        assert!(val.is_undef());
        assert_eq!(*det, DeterminacyState::Undetermined);

        // Topology fingerprint should be non-zero
        assert_ne!(snap.topology_fingerprint, ContentHash(0));
    }

    #[test]
    fn snapshot_debug_and_clone() {
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let snap = Snapshot::from_compiled_module(&module);

        let debug = format!("{:?}", snap);
        assert!(debug.contains("Snapshot"));

        let cloned = snap.clone();
        assert_eq!(cloned.id, snap.id);
        assert_eq!(cloned.version, snap.version);
        assert_eq!(cloned.provenance, snap.provenance);
    }

    #[test]
    fn snapshot_clone_structural_sharing() {
        use reify_test_support::bracket_compiled_module;

        let module = bracket_compiled_module();
        let snap = Snapshot::from_compiled_module(&module);

        // Clone the snapshot (O(1) via PersistentMap)
        let mut cloned = snap.clone();

        // Modify the clone's values — insert a new value for width
        let width_id = ValueCellId::new("Bracket", "width");
        cloned.values.insert(
            width_id.clone(),
            (Value::length(0.08), DeterminacyState::Determined),
        );

        // Also insert a completely new value cell in clone
        let extra_id = ValueCellId::new("Bracket", "extra");
        cloned.values.insert(
            extra_id.clone(),
            (Value::Int(42), DeterminacyState::Determined),
        );

        // Original snapshot is unchanged
        assert_eq!(snap.values.len(), 6);
        let (orig_val, orig_det) = snap.values.get(&width_id).unwrap();
        assert!(orig_val.is_undef());
        assert_eq!(*orig_det, DeterminacyState::Undetermined);
        assert!(!snap.values.contains_key(&extra_id));

        // Clone has the modified values
        assert_eq!(cloned.values.len(), 7); // 6 original + 1 extra
        let (clone_val, clone_det) = cloned.values.get(&width_id).unwrap();
        assert!(!clone_val.is_undef());
        assert_eq!(*clone_det, DeterminacyState::Determined);
        assert!(cloned.values.contains_key(&extra_id));
    }
}
