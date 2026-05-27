// Snapshot diffing for GUI state.
//
// Pure functions that compare consecutive GuiState snapshots and produce
// minimal deltas for targeted event emission. No tauri dependency.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::warn;

use reify_core::DiagnosticInfo;

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
    /// Some(vec) when the tessellation diagnostics list changed; None when unchanged.
    pub changed_tessellation_diagnostics: Option<Vec<DiagnosticInfo>>,
    /// Some(vec) when the compile diagnostics list changed; None when unchanged.
    pub changed_compile_diagnostics: Option<Vec<DiagnosticInfo>>,
}

impl StateDelta {
    /// Create a delta containing all items from a GuiState as changed.
    ///
    /// Used for the initial state emission where there is no previous snapshot.
    pub fn full(state: &GuiState) -> Self {
        StateDelta {
            changed_meshes: state.meshes.clone(),
            changed_values: state.values.clone(),
            changed_constraints: state.constraints.clone(),
            removed_mesh_paths: vec![],
            removed_value_ids: vec![],
            removed_constraint_ids: vec![],
            // Initial full emission: skip when empty (the frontend default is already []).
            // Subsequent diffs (diff_gui_state) still emit Some(vec![]) when clearing a
            // previously non-empty list so subscribers can clear their view.
            changed_tessellation_diagnostics: if state.tessellation_diagnostics.is_empty() {
                None
            } else {
                Some(state.tessellation_diagnostics.clone())
            },
            changed_compile_diagnostics: if state.compile_diagnostics.is_empty() {
                None
            } else {
                Some(state.compile_diagnostics.clone())
            },
        }
    }
}

/// Compare two GuiState snapshots and return a minimal delta.
///
/// Items are matched by key (entity_path for meshes, cell_id for values,
/// node_id for constraints). Changed/added items appear in `changed_*` vecs.
/// Items present in `old` but missing from `new` appear in `removed_*` vecs.
pub fn diff_gui_state(old: &GuiState, new: &GuiState) -> StateDelta {
    // --- Values: keyed by cell_id ---
    let old_values: HashMap<&str, &ValueData> =
        old.values.iter().map(|v| (v.cell_id.as_str(), v)).collect();
    let new_values: HashMap<&str, &ValueData> =
        new.values.iter().map(|v| (v.cell_id.as_str(), v)).collect();

    let changed_values: Vec<ValueData> = new
        .values
        .iter()
        .filter(|v| {
            old_values
                .get(v.cell_id.as_str())
                .is_none_or(|old_v| *old_v != *v)
        })
        .cloned()
        .collect();

    let removed_value_ids: Vec<String> = old
        .values
        .iter()
        .filter(|v| !new_values.contains_key(v.cell_id.as_str()))
        .map(|v| v.cell_id.clone())
        .collect();

    // --- Constraints: keyed by node_id ---
    let old_constraints: HashMap<&str, &ConstraintData> = old
        .constraints
        .iter()
        .map(|c| (c.node_id.as_str(), c))
        .collect();
    let new_constraints: HashMap<&str, &ConstraintData> = new
        .constraints
        .iter()
        .map(|c| (c.node_id.as_str(), c))
        .collect();

    let changed_constraints: Vec<ConstraintData> = new
        .constraints
        .iter()
        .filter(|c| {
            old_constraints
                .get(c.node_id.as_str())
                .is_none_or(|old_c| *old_c != *c)
        })
        .cloned()
        .collect();

    let removed_constraint_ids: Vec<String> = old
        .constraints
        .iter()
        .filter(|c| !new_constraints.contains_key(c.node_id.as_str()))
        .map(|c| c.node_id.clone())
        .collect();

    // --- Meshes: keyed by entity_path ---
    let old_meshes: HashMap<&str, &MeshData> = old
        .meshes
        .iter()
        .map(|m| (m.entity_path.as_str(), m))
        .collect();
    let new_meshes: HashMap<&str, &MeshData> = new
        .meshes
        .iter()
        .map(|m| (m.entity_path.as_str(), m))
        .collect();

    let changed_meshes: Vec<MeshData> = new
        .meshes
        .iter()
        .filter(|m| {
            old_meshes
                .get(m.entity_path.as_str())
                .is_none_or(|old_m| *old_m != *m)
        })
        .cloned()
        .collect();

    let removed_mesh_paths: Vec<String> = old
        .meshes
        .iter()
        .filter(|m| !new_meshes.contains_key(m.entity_path.as_str()))
        .map(|m| m.entity_path.clone())
        .collect();

    // --- Tessellation diagnostics: emit the new list when it differs ---
    let changed_tessellation_diagnostics =
        if old.tessellation_diagnostics != new.tessellation_diagnostics {
            Some(new.tessellation_diagnostics.clone())
        } else {
            None
        };

    // --- Compile diagnostics: emit the new list when it differs ---
    let changed_compile_diagnostics = if old.compile_diagnostics != new.compile_diagnostics {
        Some(new.compile_diagnostics.clone())
    } else {
        None
    };

    StateDelta {
        changed_meshes,
        changed_values,
        changed_constraints,
        removed_mesh_paths,
        removed_value_ids,
        removed_constraint_ids,
        changed_tessellation_diagnostics,
        changed_compile_diagnostics,
    }
}

/// Convert a StateDelta into a list of (event_name, payload) tuples.
///
/// This is a pure function with no Tauri dependency — fully testable.
/// Changed items produce update events; removed IDs produce removal events.
pub fn delta_to_events(delta: &StateDelta) -> Vec<(String, serde_json::Value)> {
    let mut events = Vec::new();

    for mesh in &delta.changed_meshes {
        push_serialized_event(
            &mut events,
            "mesh-update",
            "mesh",
            &mesh.entity_path,
            serde_json::to_value(mesh),
        );
    }
    for value in &delta.changed_values {
        push_serialized_event(
            &mut events,
            "value-update",
            "value",
            &value.cell_id,
            serde_json::to_value(value),
        );
    }
    for constraint in &delta.changed_constraints {
        push_serialized_event(
            &mut events,
            "constraint-update",
            "constraint",
            &constraint.node_id,
            serde_json::to_value(constraint),
        );
    }
    for path in &delta.removed_mesh_paths {
        events.push((
            "mesh-removed".to_string(),
            serde_json::Value::String(path.clone()),
        ));
    }
    for id in &delta.removed_value_ids {
        events.push((
            "value-removed".to_string(),
            serde_json::Value::String(id.clone()),
        ));
    }
    for id in &delta.removed_constraint_ids {
        events.push((
            "constraint-removed".to_string(),
            serde_json::Value::String(id.clone()),
        ));
    }

    // Tessellation diagnostics: emit full list when it changed.
    if let Some(diags) = &delta.changed_tessellation_diagnostics {
        push_serialized_event(
            &mut events,
            "tessellation-diagnostics",
            "tessellation-diagnostics",
            "diagnostics",
            serde_json::to_value(diags),
        );
    }

    // Compile diagnostics: emit full list when it changed.
    if let Some(diags) = &delta.changed_compile_diagnostics {
        push_serialized_event(
            &mut events,
            "compile-diagnostics",
            "compile-diagnostics",
            "diagnostics",
            serde_json::to_value(diags),
        );
    }

    events
}

/// Push a serialized event onto `events`, or a structured error event on failure.
///
/// On `Ok(val)`: pushes `(event_name, val)`.
/// On `Err(err)`: emits a `warn!` and pushes a `"serialization-error"` event
/// with `item_type`, `item_id`, and `error` fields.
///
/// `pub(crate)` so the test module in `src/tests/` can unit-test it directly.
pub(crate) fn push_serialized_event(
    events: &mut Vec<(String, serde_json::Value)>,
    event_name: &str,
    item_type: &str,
    item_id: &str,
    result: Result<serde_json::Value, serde_json::Error>,
) {
    match result {
        Ok(val) => events.push((event_name.to_string(), val)),
        Err(err) => {
            warn!("failed to serialize {item_type} {item_id}: {err}");
            events.push((
                "serialization-error".to_string(),
                serde_json::json!({
                    "item_type": item_type,
                    "item_id": item_id,
                    "error": err.to_string(),
                }),
            ));
        }
    }
}

/// Compute a delta against the last known state, then store the new state.
///
/// If `last_state` is `None` (first call), returns a full delta.
/// Otherwise diffs against the previous state and returns the minimal delta.
pub fn compute_delta(last_state: &Mutex<Option<GuiState>>, new_state: &GuiState) -> StateDelta {
    let mut guard = last_state.lock().unwrap_or_else(|e| e.into_inner());
    let delta = match guard.as_ref() {
        Some(old) => diff_gui_state(old, new_state),
        None => StateDelta::full(new_state),
    };
    *guard = Some(new_state.clone());
    delta
}
