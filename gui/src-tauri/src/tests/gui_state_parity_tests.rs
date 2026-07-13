// Tests for `GuiState`'s delta/event channel and full-snapshot wire
// contract, exercised directly against the macro-generated
// `crate::diff::{diff_gui_state, delta_to_events}` (see `gui_state_schema.rs`
// for the `gui_state!` macro and `gui_state_macro_tests.rs` for its isolated
// behavior tests on a `MiniState` fixture).
//
// `full_reload_only_fields_never_produce_delta_events` proves the four
// full_reload_only fields (files, fea_diagnostics, fea_convergence,
// demand_prune_measurement) never reach the delta/event channel.
//
// `gui_state_full_snapshot_key_order_is_stable` pins `GuiState`'s serialized
// top-level key ORDER (the full-snapshot wire contract) — independent of
// `StateDelta`, since `GuiState` (not `StateDelta`) is what crosses the wire
// directly.

use std::collections::HashMap;

use reify_core::DiagnosticInfo;

use crate::diff::{delta_to_events, diff_gui_state};
use crate::types::*;

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

// ---------------------------------------------------------------------------
// Assertions
// ---------------------------------------------------------------------------

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
/// No current frontend consumer reads this positionally: `bridge.ts`'s
/// `getInitialState`/`refreshFullState`/etc. deserialize the Tauri IPC
/// response into `RawGuiState` (Tauri's `invoke` parses JSON into a plain
/// object) and `convertRawGuiState` reads fields by name, not position — so
/// this test's value is as a migration-stability tripwire (catching an
/// accidental field reorder in the `gui_state!` invocation) rather than
/// protection of a live positional contract. Unlike the frozen `mod legacy`
/// oracle above, this test has no dependency on retired pre-L5 code and
/// nothing to rot — task #5165's cleanup scope is limited to `mod legacy`
/// and the oracle-comparison assertions (`assert_delta_parity`,
/// `parity_holds_across_corpus`); this test is explicitly called out there
/// to be kept.
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
