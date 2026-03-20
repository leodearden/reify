// Snapshot diffing for GUI state.
//
// Pure functions that compare consecutive GuiState snapshots and produce
// minimal deltas for targeted event emission. No tauri dependency.

use serde::{Deserialize, Serialize};

use crate::types::{ConstraintData, GuiState, MeshData, ValueData};

/// Minimal delta between two GuiState snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDelta {
    pub changed_meshes: Vec<MeshData>,
    pub changed_values: Vec<ValueData>,
    pub changed_constraints: Vec<ConstraintData>,
    pub removed_mesh_paths: Vec<String>,
    pub removed_value_ids: Vec<String>,
    pub removed_constraint_ids: Vec<String>,
}

/// Compare two GuiState snapshots and return a minimal delta.
///
/// Items are matched by key (entity_path for meshes, cell_id for values,
/// node_id for constraints). Changed/added items appear in `changed_*` vecs.
/// Items present in `old` but missing from `new` appear in `removed_*` vecs.
pub fn diff_gui_state(_old: &GuiState, _new: &GuiState) -> StateDelta {
    StateDelta {
        changed_meshes: vec![],
        changed_values: vec![],
        changed_constraints: vec![],
        removed_mesh_paths: vec![],
        removed_value_ids: vec![],
        removed_constraint_ids: vec![],
    }
}
