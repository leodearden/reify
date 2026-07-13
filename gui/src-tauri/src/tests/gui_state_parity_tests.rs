// Consumer-face parity/characterization test guarding the step-8 GuiState
// migration (L5 plan step 7). See `gui_state_schema.rs` for the `gui_state!`
// macro and `gui_state_macro_tests.rs` for its isolated behavior tests on a
// `MiniState` fixture.
//
// `mod legacy` below is a frozen, byte-identical copy of TODAY's (pre-L5)
// hand-written `crate::diff::{StateDelta, StateDelta::full, diff_gui_state,
// delta_to_events}` — renamed (`LegacyStateDelta`/`legacy_diff`/
// `legacy_events`) so it can sit alongside the real items without a name
// clash. It is the oracle every corpus pair below is checked against.
// DO NOT update `mod legacy` to track future `diff.rs`/`gui_state_schema.rs`
// changes — freezing it is the entire point: this test is green today
// (before step-8 swaps the real `diff_gui_state`/`delta_to_events`/
// `StateDelta` for macro-generated equivalents) and MUST stay green after,
// proving the swap is behavior-preserving.
//
// Corpus covers (per pair, see `corpus()`): identical states, a keyed
// change/add/remove/mixed per collection (meshes/values/constraints), a
// whole-field change and a whole-field clear (non-empty -> empty, which
// must still produce a `Some(vec![])` event — NOT `None`), full_reload_only
// fields (files/fea_diagnostics/fea_convergence/demand_prune_measurement)
// varying alone (must yield zero delta events, since none of them ever
// reach `StateDelta`), and the empty<->fully-populated transitions in both
// directions.
//
// `gui_state_full_snapshot_key_order_is_stable` separately pins `GuiState`'s
// serialized top-level key ORDER (the full-snapshot wire contract) —
// independent of `StateDelta`, since `GuiState` (not `StateDelta`) is what
// crosses the wire directly (see the task plan's parity design decision).

use std::collections::HashMap;

use reify_core::DiagnosticInfo;

use crate::diff::{StateDelta, delta_to_events, diff_gui_state};
use crate::types::*;

mod legacy {
    //! Frozen oracle: byte-identical copy of the pre-L5 hand-written
    //! `crate::diff` logic (as of task #5034's base commit). See the parent
    //! module doc comment — this must never be updated to track future
    //! `diff.rs`/`gui_state_schema.rs` changes.
    //!
    //! TODO(#5165): delete this module (and the parity assertions that
    //! compare against it, `assert_delta_parity`/`parity_holds_across_corpus`)
    //! once the L5 migration has landed on main and baked. It duplicates
    //! ~340 lines of retired logic, and because its oracle calls the *real*
    //! `push_serialized_event`, a future signature change to that helper
    //! would silently alter the "oracle" instead of failing loudly. That
    //! coupling is a known, accepted bake-window risk, not a deferred
    //! decision: #5165's plan is deletion (retiring the parity guarantee
    //! deliberately). If deletion is ever pushed past the bake window,
    //! self-contain this oracle instead — inline a frozen copy of
    //! `push_serialized_event` here — rather than leaving it coupled to
    //! live code indefinitely.

    use std::collections::HashMap;

    use serde::{Deserialize, Serialize};

    use reify_core::DiagnosticInfo;

    use crate::diff::push_serialized_event;
    use crate::types::{
        AppearanceDirective, ConstraintData, DisplayDirective, GuiState, MeshData,
        TensegritySurfaceData, TensegrityWireData, ValueData,
    };

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LegacyStateDelta {
        pub changed_meshes: Vec<MeshData>,
        pub changed_values: Vec<ValueData>,
        pub changed_constraints: Vec<ConstraintData>,
        pub removed_mesh_paths: Vec<String>,
        pub removed_value_ids: Vec<String>,
        pub removed_constraint_ids: Vec<String>,
        pub changed_tessellation_diagnostics: Option<Vec<DiagnosticInfo>>,
        pub changed_compile_diagnostics: Option<Vec<DiagnosticInfo>>,
        pub changed_tensegrity_wires: Option<Vec<TensegrityWireData>>,
        pub changed_tensegrity_surfaces: Option<Vec<TensegritySurfaceData>>,
        pub changed_display_panes: Option<Vec<DisplayDirective>>,
        pub changed_display_appearance: Option<Vec<AppearanceDirective>>,
    }

    impl LegacyStateDelta {
        /// Create a delta containing all items from a GuiState as changed.
        pub fn full(state: &GuiState) -> Self {
            LegacyStateDelta {
                changed_meshes: state.meshes.clone(),
                changed_values: state.values.clone(),
                changed_constraints: state.constraints.clone(),
                removed_mesh_paths: vec![],
                removed_value_ids: vec![],
                removed_constraint_ids: vec![],
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
                changed_tensegrity_wires: if state.tensegrity_wires.is_empty() {
                    None
                } else {
                    Some(state.tensegrity_wires.clone())
                },
                changed_tensegrity_surfaces: if state.tensegrity_surfaces.is_empty() {
                    None
                } else {
                    Some(state.tensegrity_surfaces.clone())
                },
                changed_display_panes: if state.display_panes.is_empty() {
                    None
                } else {
                    Some(state.display_panes.clone())
                },
                changed_display_appearance: if state.display_appearance.is_empty() {
                    None
                } else {
                    Some(state.display_appearance.clone())
                },
            }
        }
    }

    /// Compare two GuiState snapshots and return a minimal delta. Verbatim
    /// copy of the pre-L5 `diff_gui_state` (diff.rs).
    pub fn legacy_diff(old: &GuiState, new: &GuiState) -> LegacyStateDelta {
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

        // --- Tensegrity wires: emit the new list when it differs ---
        let changed_tensegrity_wires = if old.tensegrity_wires != new.tensegrity_wires {
            Some(new.tensegrity_wires.clone())
        } else {
            None
        };

        // --- Tensegrity surfaces: emit the new list when it differs ---
        let changed_tensegrity_surfaces = if old.tensegrity_surfaces != new.tensegrity_surfaces {
            Some(new.tensegrity_surfaces.clone())
        } else {
            None
        };

        // --- Display panes: emit the new list when it differs ---
        let changed_display_panes = if old.display_panes != new.display_panes {
            Some(new.display_panes.clone())
        } else {
            None
        };

        // --- Display appearance: emit the new list when it differs ---
        let changed_display_appearance = if old.display_appearance != new.display_appearance {
            Some(new.display_appearance.clone())
        } else {
            None
        };

        LegacyStateDelta {
            changed_meshes,
            changed_values,
            changed_constraints,
            removed_mesh_paths,
            removed_value_ids,
            removed_constraint_ids,
            changed_tessellation_diagnostics,
            changed_compile_diagnostics,
            changed_tensegrity_wires,
            changed_tensegrity_surfaces,
            changed_display_panes,
            changed_display_appearance,
        }
    }

    /// Convert a LegacyStateDelta into a list of (event_name, payload)
    /// tuples. Verbatim copy of the pre-L5 `delta_to_events` (diff.rs),
    /// reusing the real (unchanged-by-L5) `push_serialized_event` helper.
    pub fn legacy_events(delta: &LegacyStateDelta) -> Vec<(String, serde_json::Value)> {
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

        if let Some(diags) = &delta.changed_tessellation_diagnostics {
            push_serialized_event(
                &mut events,
                "tessellation-diagnostics",
                "tessellation-diagnostics",
                "diagnostics",
                serde_json::to_value(diags),
            );
        }

        if let Some(diags) = &delta.changed_compile_diagnostics {
            push_serialized_event(
                &mut events,
                "compile-diagnostics",
                "compile-diagnostics",
                "diagnostics",
                serde_json::to_value(diags),
            );
        }

        if let Some(wires) = &delta.changed_tensegrity_wires {
            push_serialized_event(
                &mut events,
                "tensegrity-wires-update",
                "tensegrity-wires-update",
                "tensegrity-wires",
                serde_json::to_value(wires),
            );
        }

        if let Some(surfaces) = &delta.changed_tensegrity_surfaces {
            push_serialized_event(
                &mut events,
                "tensegrity-surfaces-update",
                "tensegrity-surfaces-update",
                "tensegrity-surfaces",
                serde_json::to_value(surfaces),
            );
        }

        if let Some(panes) = &delta.changed_display_panes {
            push_serialized_event(
                &mut events,
                "display-panes-update",
                "display-panes-update",
                "display-panes",
                serde_json::to_value(panes),
            );
        }

        if let Some(appearance) = &delta.changed_display_appearance {
            push_serialized_event(
                &mut events,
                "display-appearance-update",
                "display-appearance-update",
                "display-appearance",
                serde_json::to_value(appearance),
            );
        }

        events
    }
}

// ---------------------------------------------------------------------------
// Fixtures — mirrors the diff_tests.rs sample_*/empty_gui_state() pattern,
// duplicated locally (test modules are independent namespaces; diff_tests.rs's
// helpers are private to that module).
// ---------------------------------------------------------------------------

fn sample_diagnostic(severity: &str, message: &str) -> DiagnosticInfo {
    DiagnosticInfo {
        file_path: "test.ri".to_string(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
        severity: severity.to_string(),
        message: message.to_string(),
        code: None,
        has_location: false,
    }
}

fn sample_mesh(entity_path: &str, vertices: Vec<f32>) -> MeshData {
    MeshData {
        entity_path: entity_path.to_string(),
        vertices,
        indices: vec![0, 1, 2],
        normals: None,
        scalar_channels: HashMap::new(),
        displaced_positions: None,
        element_kind: None,
        region_tags: None,
        element_index: None,
        vector_channels: HashMap::new(),
        appearance: None,
    }
}

fn sample_value(cell_id: &str, value: &str) -> ValueData {
    ValueData {
        cell_id: cell_id.to_string(),
        name: cell_id
            .split('.')
            .next_back()
            .unwrap_or(cell_id)
            .to_string(),
        value: value.to_string(),
        unit: "mm".to_string(),
        determinacy: "determined".to_string(),
        entity_path: cell_id.split('.').next().unwrap_or("").to_string(),
        kind: "Param".to_string(),
        freshness: "final".to_string(),
        reason: None,
        last_substantive_value: None,
    }
}

fn sample_constraint(node_id: &str, status: &str) -> ConstraintData {
    ConstraintData {
        node_id: node_id.to_string(),
        expression: "x > 0".to_string(),
        status: status.to_string(),
        label: None,
        parameter_ids: vec![],
    }
}

fn sample_tensegrity_wire(entity_path: &str) -> TensegrityWireData {
    TensegrityWireData {
        entity_path: entity_path.to_string(),
        kind: "strut".to_string(),
        x1: 0.0,
        y1: 0.0,
        z1: 0.0,
        x2: 1.0,
        y2: 0.0,
        z2: 0.0,
    }
}

fn sample_tensegrity_surface(entity_path: &str) -> TensegritySurfaceData {
    TensegritySurfaceData {
        entity_path: entity_path.to_string(),
        kind: "membrane".to_string(),
        i0: 0,
        i1: 1,
        i2: 2,
        x0: 0.0,
        y0: 0.0,
        z0: 0.0,
        x1: 1.0,
        y1: 0.0,
        z1: 0.0,
        x2: 0.0,
        y2: 1.0,
        z2: 0.0,
    }
}

fn sample_display_directive(subject: &str, pane: i32) -> DisplayDirective {
    DisplayDirective {
        subject: subject.to_string(),
        pane,
    }
}

fn sample_appearance_directive(subject: &str) -> AppearanceDirective {
    AppearanceDirective {
        subject: subject.to_string(),
        style: DisplayStyleData {
            color: [0.5, 0.3, 0.1, 1.0],
            finish: 1,
            opacity: 1.0,
            wireframe: false,
        },
    }
}

/// All-empty `GuiState`. Spread with `..empty_gui_state()` and override just
/// the field(s) under test (mirrors diff_tests.rs's helper of the same name).
fn empty_gui_state() -> GuiState {
    GuiState {
        meshes: vec![],
        values: vec![],
        constraints: vec![],
        files: vec![],
        tessellation_diagnostics: vec![],
        compile_diagnostics: vec![],
        tensegrity_wires: vec![],
        tensegrity_surfaces: vec![],
        demand_prune_measurement: None,
        display_panes: vec![],
        display_appearance: vec![],
        fea_diagnostics: vec![],
        fea_convergence: None,
    }
}

/// A `GuiState` fixture with every field populated (including
/// `fea_convergence`, which carries `#[serde(skip_serializing_if =
/// "Option::is_none")]` and so would otherwise vanish from a JSON
/// key-order/reflection check).
fn fully_populated_gui_state() -> GuiState {
    GuiState {
        meshes: vec![sample_mesh("Bracket.body", vec![1.0, 2.0, 3.0])],
        values: vec![sample_value("Bracket.width", "120")],
        constraints: vec![sample_constraint("Bracket.0", "Satisfied")],
        files: vec![FileData {
            path: "main.ri".to_string(),
            content: "structure Bracket {}".to_string(),
        }],
        tessellation_diagnostics: vec![sample_diagnostic("warning", "tessellation warning")],
        compile_diagnostics: vec![sample_diagnostic("error", "compile error")],
        tensegrity_wires: vec![sample_tensegrity_wire("TPrism.wire[0]")],
        tensegrity_surfaces: vec![sample_tensegrity_surface("TPatch.surface[0]")],
        demand_prune_measurement: Some(DemandPruneMeasurementDto {
            eval_set_size: 10,
            observed_retained: 6,
            would_prune: WouldPruneByKindDto {
                value: 1,
                constraint: 1,
                realization: 1,
                resolution: 0,
                compute: 1,
            },
        }),
        display_panes: vec![sample_display_directive("Bracket.body", 0)],
        display_appearance: vec![sample_appearance_directive("Bracket.body")],
        fea_diagnostics: vec![FeaDiagnosticInfo::ProblemElements { ids: vec![1, 2] }],
        fea_convergence: Some(FeaConvergenceInfo {
            converged: true,
            reason: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Corpus — ~10 (old, new) GuiState pairs exercising every classification.
// ---------------------------------------------------------------------------

fn pair_identical_empty() -> (&'static str, GuiState, GuiState) {
    ("identical_empty", empty_gui_state(), empty_gui_state())
}

/// A key present in both snapshots changes value; another key present in
/// both stays identical (must be excluded from `changed_*`); a whole field
/// (`tessellation_diagnostics`) is identical non-empty in both (must
/// produce no event even though non-empty).
fn pair_keyed_change() -> (&'static str, GuiState, GuiState) {
    let shared_diagnostics = vec![sample_diagnostic("warning", "unchanged warning")];
    let old = GuiState {
        meshes: vec![
            sample_mesh("A.body", vec![1.0, 2.0, 3.0]),
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
        ],
        values: vec![sample_value("A.w", "1"), sample_value("Keep.x", "5")],
        constraints: vec![
            sample_constraint("A.0", "Satisfied"),
            sample_constraint("Keep.0", "Satisfied"),
        ],
        tessellation_diagnostics: shared_diagnostics.clone(),
        ..empty_gui_state()
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("A.body", vec![9.0, 9.0, 9.0]),
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
        ],
        values: vec![sample_value("A.w", "2"), sample_value("Keep.x", "5")],
        constraints: vec![
            sample_constraint("A.0", "Violated"),
            sample_constraint("Keep.0", "Satisfied"),
        ],
        tessellation_diagnostics: shared_diagnostics,
        ..empty_gui_state()
    };
    ("keyed_change", old, new)
}

/// Each keyed collection gains an item; a shared "Keep" item is unchanged.
fn pair_keyed_add() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        meshes: vec![sample_mesh("Keep.body", vec![9.0, 9.0, 9.0])],
        values: vec![sample_value("Keep.x", "5")],
        constraints: vec![sample_constraint("Keep.0", "Satisfied")],
        ..empty_gui_state()
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
            sample_mesh("New.body", vec![1.0, 1.0, 1.0]),
        ],
        values: vec![sample_value("Keep.x", "5"), sample_value("New.y", "7")],
        constraints: vec![
            sample_constraint("Keep.0", "Satisfied"),
            sample_constraint("New.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    ("keyed_add", old, new)
}

/// Each keyed collection drops an item; a shared "Keep" item survives.
fn pair_keyed_remove() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        meshes: vec![
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
            sample_mesh("Gone.body", vec![2.0, 2.0, 2.0]),
        ],
        values: vec![sample_value("Keep.x", "5"), sample_value("Gone.y", "3")],
        constraints: vec![
            sample_constraint("Keep.0", "Satisfied"),
            sample_constraint("Gone.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    let new = GuiState {
        meshes: vec![sample_mesh("Keep.body", vec![9.0, 9.0, 9.0])],
        values: vec![sample_value("Keep.x", "5")],
        constraints: vec![sample_constraint("Keep.0", "Satisfied")],
        ..empty_gui_state()
    };
    ("keyed_remove", old, new)
}

/// All three keyed effects at once: "Keep" changes, "Gone" is removed, "New"
/// is added — per keyed collection.
fn pair_keyed_mixed_add_remove_change() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        meshes: vec![
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
            sample_mesh("Gone.body", vec![2.0, 2.0, 2.0]),
        ],
        values: vec![sample_value("Keep.x", "5"), sample_value("Gone.y", "3")],
        constraints: vec![
            sample_constraint("Keep.0", "Satisfied"),
            sample_constraint("Gone.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("Keep.body", vec![1.0, 1.0, 1.0]),
            sample_mesh("New.body", vec![4.0, 4.0, 4.0]),
        ],
        values: vec![sample_value("Keep.x", "6"), sample_value("New.y", "8")],
        constraints: vec![
            sample_constraint("Keep.0", "Violated"),
            sample_constraint("New.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    ("keyed_mixed_add_remove_change", old, new)
}

/// Two pre-existing items per keyed collection are simultaneously changed
/// (no adds/removes), with `new`'s vector order reversed relative to
/// `old`'s ("B" before "A", vs. "A" before "B" in `old`). This pins that
/// `changed_*` events follow `new`'s vector iteration order specifically —
/// not `old`'s, and not e.g. a key-sorted order a future HashMap-driven
/// refactor of `diff_keyed` might introduce. A corpus with at most one
/// changed item per collection (as `pair_keyed_change` and
/// `pair_keyed_mixed_add_remove_change` have) can't catch an
/// intra-collection reorder, since a single-element list has no order to
/// get wrong.
fn pair_keyed_multiple_changed_reordered() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        meshes: vec![
            sample_mesh("A.body", vec![1.0, 1.0, 1.0]),
            sample_mesh("B.body", vec![2.0, 2.0, 2.0]),
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
        ],
        values: vec![
            sample_value("A.w", "1"),
            sample_value("B.w", "2"),
            sample_value("Keep.x", "5"),
        ],
        constraints: vec![
            sample_constraint("A.0", "Satisfied"),
            sample_constraint("B.0", "Satisfied"),
            sample_constraint("Keep.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    let new = GuiState {
        meshes: vec![
            sample_mesh("B.body", vec![22.0, 22.0, 22.0]),
            sample_mesh("A.body", vec![11.0, 11.0, 11.0]),
            sample_mesh("Keep.body", vec![9.0, 9.0, 9.0]),
        ],
        values: vec![
            sample_value("B.w", "22"),
            sample_value("A.w", "11"),
            sample_value("Keep.x", "5"),
        ],
        constraints: vec![
            sample_constraint("B.0", "Violated"),
            sample_constraint("A.0", "Violated"),
            sample_constraint("Keep.0", "Satisfied"),
        ],
        ..empty_gui_state()
    };
    ("keyed_multiple_changed_reordered", old, new)
}

/// All six whole-value list fields change (keyed collections held identical
/// and empty, isolating the whole-field diff behavior).
fn pair_whole_change() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        tessellation_diagnostics: vec![sample_diagnostic("warning", "tess-old")],
        compile_diagnostics: vec![sample_diagnostic("error", "compile-old")],
        tensegrity_wires: vec![sample_tensegrity_wire("Old.wire")],
        tensegrity_surfaces: vec![sample_tensegrity_surface("Old.surface")],
        display_panes: vec![sample_display_directive("Old.body", 0)],
        display_appearance: vec![sample_appearance_directive("Old.body")],
        ..empty_gui_state()
    };
    let new = GuiState {
        tessellation_diagnostics: vec![sample_diagnostic("warning", "tess-new")],
        compile_diagnostics: vec![sample_diagnostic("error", "compile-new")],
        tensegrity_wires: vec![sample_tensegrity_wire("New.wire")],
        tensegrity_surfaces: vec![sample_tensegrity_surface("New.surface")],
        display_panes: vec![sample_display_directive("New.body", 1)],
        display_appearance: vec![sample_appearance_directive("New.body")],
        ..empty_gui_state()
    };
    ("whole_change", old, new)
}

/// All six whole-value list fields go from non-empty to empty — must still
/// produce a `Some(vec![])` diff (a "clear" event), not `None`.
fn pair_whole_clear() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        tessellation_diagnostics: vec![sample_diagnostic("warning", "tess")],
        compile_diagnostics: vec![sample_diagnostic("error", "compile")],
        tensegrity_wires: vec![sample_tensegrity_wire("Old.wire")],
        tensegrity_surfaces: vec![sample_tensegrity_surface("Old.surface")],
        display_panes: vec![sample_display_directive("Old.body", 0)],
        display_appearance: vec![sample_appearance_directive("Old.body")],
        ..empty_gui_state()
    };
    let new = empty_gui_state();
    ("whole_clear_nonempty_to_empty", old, new)
}

/// Only the four `full_reload_only` fields vary (files, fea_diagnostics,
/// fea_convergence, demand_prune_measurement); every diffed (keyed + whole)
/// field is identical. Must yield zero delta events on both real and legacy.
fn pair_full_reload_only_fields_vary_only() -> (&'static str, GuiState, GuiState) {
    let old = GuiState {
        files: vec![FileData {
            path: "a.ri".to_string(),
            content: "old".to_string(),
        }],
        fea_diagnostics: vec![FeaDiagnosticInfo::UnresolvedSelector {
            selector_path: "Old.selector".to_string(),
        }],
        fea_convergence: Some(FeaConvergenceInfo {
            converged: false,
            reason: Some("MaxDofs".to_string()),
        }),
        demand_prune_measurement: Some(DemandPruneMeasurementDto {
            eval_set_size: 5,
            observed_retained: 2,
            would_prune: WouldPruneByKindDto {
                value: 1,
                constraint: 0,
                realization: 1,
                resolution: 0,
                compute: 1,
            },
        }),
        ..empty_gui_state()
    };
    let new = GuiState {
        files: vec![FileData {
            path: "a.ri".to_string(),
            content: "new".to_string(),
        }],
        fea_diagnostics: vec![FeaDiagnosticInfo::ProblemElements { ids: vec![3, 4] }],
        fea_convergence: Some(FeaConvergenceInfo {
            converged: true,
            reason: None,
        }),
        demand_prune_measurement: Some(DemandPruneMeasurementDto {
            eval_set_size: 5,
            observed_retained: 5,
            would_prune: WouldPruneByKindDto {
                value: 0,
                constraint: 0,
                realization: 0,
                resolution: 0,
                compute: 0,
            },
        }),
        ..empty_gui_state()
    };
    ("full_reload_only_fields_vary_only", old, new)
}

fn pair_empty_to_fully_populated() -> (&'static str, GuiState, GuiState) {
    (
        "empty_to_fully_populated",
        empty_gui_state(),
        fully_populated_gui_state(),
    )
}

fn pair_fully_populated_to_empty() -> (&'static str, GuiState, GuiState) {
    (
        "fully_populated_to_empty",
        fully_populated_gui_state(),
        empty_gui_state(),
    )
}

fn corpus() -> Vec<(&'static str, GuiState, GuiState)> {
    vec![
        pair_identical_empty(),
        pair_keyed_change(),
        pair_keyed_add(),
        pair_keyed_remove(),
        pair_keyed_mixed_add_remove_change(),
        pair_keyed_multiple_changed_reordered(),
        pair_whole_change(),
        pair_whole_clear(),
        pair_full_reload_only_fields_vary_only(),
        pair_empty_to_fully_populated(),
        pair_fully_populated_to_empty(),
    ]
}

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

/// Asserts `diff_gui_state`/`delta_to_events` produce the identical event
/// stream as the frozen `legacy` oracle for `(old, new)`, and that
/// `StateDelta::full`/`delta_to_events` do too for both `old` and `new`
/// individually (the `full` path is exercised on every corpus member, not
/// just a dedicated empty/fully-populated pair, per the plan's "for every
/// pair" requirement).
fn assert_delta_parity(label: &str, old: &GuiState, new: &GuiState) {
    let real_events = delta_to_events(&diff_gui_state(old, new));
    let legacy_events_out = legacy::legacy_events(&legacy::legacy_diff(old, new));
    assert_eq!(
        real_events, legacy_events_out,
        "[{label}] diff_gui_state/delta_to_events diverged from the legacy oracle"
    );

    for (which, state) in [("old", old), ("new", new)] {
        let real_full = delta_to_events(&StateDelta::full(state));
        let legacy_full = legacy::legacy_events(&legacy::LegacyStateDelta::full(state));
        assert_eq!(
            real_full, legacy_full,
            "[{label}:{which}] StateDelta::full/delta_to_events diverged from the legacy oracle"
        );
    }
}

#[test]
fn parity_holds_across_corpus() {
    for (label, old, new) in corpus() {
        assert_delta_parity(label, &old, &new);
    }
}

/// Stronger than parity alone: proves the four `full_reload_only` fields
/// never reach the diff channel at all (parity would still hold if both
/// real and legacy accidentally carried them, as long as they agreed).
#[test]
fn full_reload_only_fields_never_produce_delta_events() {
    let (_, old, new) = pair_full_reload_only_fields_vary_only();
    let events = delta_to_events(&diff_gui_state(&old, &new));
    assert!(
        events.is_empty(),
        "full_reload_only fields must never reach the diff channel, got: {events:?}"
    );
}

/// Pins `GuiState`'s full-snapshot wire contract: the serialized top-level
/// key order. `GuiState` (unlike `StateDelta`) is serialized directly to the
/// wire (the full-snapshot command return), so its field order must be
/// byte-preserved across the step-8 migration to the `gui_state!` macro.
///
/// Reads the key order via the custom `Deserialize` below (`TopLevelKeys`)
/// rather than `serde_json::Value::as_object().keys()`: this workspace does
/// not enable serde_json's `preserve_order` feature, so `Value`'s object map
/// is a `BTreeMap` that silently re-sorts keys alphabetically — round-
/// tripping through it would discard the very ordering information this
/// test exists to pin. `TopLevelKeys` instead reads keys directly off
/// `MapAccess` (the true serialization order, structurally skipping each
/// value via `IgnoredAny`), which also means a same-named key nested inside
/// a value (e.g. `demand_prune_measurement`'s `WouldPruneByKindDto` payload)
/// can never be mistaken for a top-level key — unlike a raw substring scan.
struct TopLevelKeys(Vec<String>);

impl<'de> serde::Deserialize<'de> for TopLevelKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KeyOrderVisitor;

        impl<'de> serde::de::Visitor<'de> for KeyOrderVisitor {
            type Value = TopLevelKeys;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut keys = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    map.next_value::<serde::de::IgnoredAny>()?;
                    keys.push(key);
                }
                Ok(TopLevelKeys(keys))
            }
        }

        deserializer.deserialize_map(KeyOrderVisitor)
    }
}

#[test]
fn gui_state_full_snapshot_key_order_is_stable() {
    let state = fully_populated_gui_state();
    let json = serde_json::to_string(&state).expect("fully-populated GuiState must serialize");

    let expected_key_order = [
        "meshes",
        "values",
        "constraints",
        "files",
        "tessellation_diagnostics",
        "compile_diagnostics",
        "tensegrity_wires",
        "tensegrity_surfaces",
        "demand_prune_measurement",
        "display_panes",
        "display_appearance",
        "fea_diagnostics",
        "fea_convergence",
    ];

    let TopLevelKeys(actual_key_order) = serde_json::from_str(&json)
        .expect("fully-populated GuiState JSON must deserialize as a top-level object");
    assert_eq!(
        actual_key_order, expected_key_order,
        "GuiState top-level key order changed (full-snapshot wire contract); got JSON: {json}"
    );
}
