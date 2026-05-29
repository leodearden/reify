//! End-to-end tests for the shell-extract ζ dispatch-complete fold hook:
//! `shell_extraction_result_to_value` enrichment (step-1/2) and
//! `fold_mid_surface_attributes_into_table` wiring (step-5/6/7) — task ζ, #3596.
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §9 and
//! `crates/reify-eval/src/shell_extract_compute.rs` for the implementation.
//!
//! # Degraded observable signal
//!
//! Per PRD line 281 and the escalation esc-3596-14:
//! - `.ri` body.mid_surface().face("region_0") is NOT wired in scope (tasks
//!   2691/2699 own selector vocab). Records carry user_label=None and
//!   dot-method selectors fall to Value::Undef.
//! - Persistent-cache disk rehydration is task ι scope (ι depends on ζ).
//!   Round-trip is verified via re-derivation on two fresh engines (step-7).

use reify_core::{ComputeNodeId, ValueCellId, VersionId};
use reify_eval::{CancellationHandle, register_shell_extract_compute_fns};
use reify_eval::cache::NodeId;
use reify_ir::{
    Freshness, InterpolationKind, Role, SampledField, SampledGridKind, StructureInstanceData,
    Value,
};
use reify_test_support::make_simple_engine;

// ── Shared fixture ────────────────────────────────────────────────────────────

/// Construct a synthetic thin-slab `SampledField` (5×5×3 grid) whose SDF
/// encodes a slab centred at z=0 with half-thickness 0.1.
///
/// Copied verbatim from `shell_extract_compute_integration.rs:28` so this
/// file has a single-file fixture without cross-test imports.
///
/// - x: [0.0, 0.25, 0.5, 0.75, 1.0] (5 points, spacing=0.25)
/// - y: [0.0, 0.25, 0.5, 0.75, 1.0] (5 points, spacing=0.25)
/// - z: [-0.5, 0.0, 0.5] (3 points, spacing=0.5)
///
/// SDF(x,y,z) = |z| − 0.1  — negative inside the slab, positive outside.
fn synthetic_slab_field() -> SampledField {
    let x_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let y_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

    // Flat row-major order: iterate z outermost, then y, then x.
    let mut data = Vec::with_capacity(5 * 5 * 3);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - 0.1);
            }
        }
    }

    SampledField {
        name: "synthetic_slab".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -0.5],
        bounds_max: vec![1.0, 1.0, 0.5],
        spacing: vec![0.25, 0.25, 0.5],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

// ── Field access helper ───────────────────────────────────────────────────────

/// Access a field by `&str` key in a `StructureInstanceData`.
/// PersistentMap keys are `String`, so this helper avoids `"key".to_string()`
/// at every call site.
fn field<'a>(si: &'a StructureInstanceData, key: &str) -> Option<&'a Value> {
    si.fields.get(&key.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-1 test: RED — naming Value does not yet carry face_records/edges lists
// ─────────────────────────────────────────────────────────────────────────────

/// Dispatch `shell-extract::extract` on the synthetic slab and assert that the
/// result Value's `naming` StructureInstance carries:
///   - A `face_records` list with `len == naming.face_count` (≥ 1 for the slab).
///   - An `edges` list with `len == naming.edge_count`.
///
/// RED today because `shell_extraction_result_to_value` only projects
/// `face_count` / `edge_count` into the naming StructureInstance (line 168-179
/// of shell_extract_compute.rs).  GREEN after step-2 adds the full lists.
///
/// Uses `dispatch_compute_node` (&self) because we only need to inspect the
/// projected Value — no engine-side fold is needed at this stage.
#[test]
fn naming_value_carries_face_records_and_edges_lists() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);

    let field_sdf = synthetic_slab_field();
    let options = Value::Undef;
    let sdf_value = Value::SampledField(field_sdf);

    let (result, _diags) = engine
        .dispatch_compute_node(
            "shell-extract::extract",
            &[options, sdf_value],
            &[],
            &Value::Undef,
            None,
        )
        .expect("dispatch_compute_node must succeed on synthetic slab");

    // Drill into result.naming
    let outer = match &result {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance, got {other:?}"),
    };
    let naming_val = field(outer, "naming")
        .expect("ShellExtractionResult must have a 'naming' field");

    let naming = match naming_val {
        Value::StructureInstance(d) => d,
        other => panic!("expected naming to be StructureInstance, got {other:?}"),
    };

    // face_count and edge_count (already present)
    let face_count = match field(naming, "face_count") {
        Some(Value::Int(n)) => *n,
        other => panic!("expected face_count: Int, got {other:?}"),
    };
    let edge_count = match field(naming, "edge_count") {
        Some(Value::Int(n)) => *n,
        other => panic!("expected edge_count: Int, got {other:?}"),
    };
    assert!(
        face_count >= 1,
        "synthetic slab must produce ≥1 region (face_count was {face_count})"
    );

    // face_records: must be a List with len == face_count
    let face_records_val = field(naming, "face_records")
        .expect("naming must carry a 'face_records' field (step-2 enrichment)");
    let face_records = match face_records_val {
        Value::List(l) => l,
        other => panic!("expected face_records: List, got {other:?}"),
    };
    assert_eq!(
        face_records.len() as i64,
        face_count,
        "face_records.len() must equal face_count"
    );

    // Each face record must have feature_id: String and local_index: Int
    for (i, rec) in face_records.iter().enumerate() {
        let rec_data = match rec {
            Value::StructureInstance(d) => d,
            other => panic!("face_records[{i}] must be StructureInstance, got {other:?}"),
        };
        assert!(
            rec_data.fields.contains_key(&"feature_id".to_string()),
            "face_records[{i}] missing 'feature_id'"
        );
        assert!(
            rec_data.fields.contains_key(&"local_index".to_string()),
            "face_records[{i}] missing 'local_index'"
        );
        assert!(
            matches!(field(rec_data, "feature_id"), Some(Value::String(_))),
            "face_records[{i}].feature_id must be Value::String"
        );
        assert!(
            matches!(field(rec_data, "local_index"), Some(Value::Int(_))),
            "face_records[{i}].local_index must be Value::Int"
        );
    }

    // edges: must be a List with len == edge_count
    let edges_val = field(naming, "edges")
        .expect("naming must carry an 'edges' field (step-2 enrichment)");
    let edges = match edges_val {
        Value::List(l) => l,
        other => panic!("expected edges: List, got {other:?}"),
    };
    assert_eq!(
        edges.len() as i64,
        edge_count,
        "edges.len() must equal edge_count"
    );

    // Each edge record must have feature_id: String and local_index: Int
    for (i, rec) in edges.iter().enumerate() {
        let rec_data = match rec {
            Value::StructureInstance(d) => d,
            other => panic!("edges[{i}] must be StructureInstance, got {other:?}"),
        };
        assert!(
            rec_data.fields.contains_key(&"feature_id".to_string()),
            "edges[{i}] missing 'feature_id'"
        );
        assert!(
            rec_data.fields.contains_key(&"local_index".to_string()),
            "edges[{i}] missing 'local_index'"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-5 test: RED — engine table not yet populated at dispatch-complete
// ─────────────────────────────────────────────────────────────────────────────

/// Run `engine.run_compute_dispatch` on the synthetic slab and assert that
/// `engine.topology_attribute_table()` contains MidSurfaceFace entries
/// (count == naming.face_count, ≥ 1) and the expected MidSurfaceEdge entries.
///
/// RED today because `run_compute_dispatch` does not call
/// `fold_mid_surface_attributes_into_table` on the Completed path.
/// GREEN after step-6 wires the fold.
///
/// Mirrors the ComputeNode wiring pattern from
/// `shell_extract_compute_integration.rs:290` and `engine_compute.rs:649`.
#[test]
fn run_compute_dispatch_folds_mid_surface_attributes_into_engine_table() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);

    let field_sdf = synthetic_slab_field();
    let value_inputs = vec![Value::Undef, Value::SampledField(field_sdf)];

    let c_id = ComputeNodeId::new("MidSurfaceFoldFixture", 0);
    let cell = ValueCellId::new("MidSurfaceFoldFixture", "result");

    let (result, _diags) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "shell-extract::extract",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(1),
        )
        .expect("run_compute_dispatch must succeed on synthetic slab");

    // Extract face_count from the result Value's naming field.
    let face_count = extract_naming_int(&result, "face_count");
    assert!(
        face_count >= 1,
        "synthetic slab must yield ≥1 MidSurfaceFace (face_count={face_count})"
    );

    // The engine's topology_attribute_table must now contain exactly
    // face_count MidSurfaceFace entries and edge_count MidSurfaceEdge entries.
    let table = engine.topology_attribute_table();

    let face_entries: Vec<_> = table
        .iter()
        .filter(|(_, attr)| attr.role == Role::MidSurfaceFace)
        .collect();
    let edge_entries: Vec<_> = table
        .iter()
        .filter(|(_, attr)| attr.role == Role::MidSurfaceEdge)
        .collect();

    assert_eq!(
        face_entries.len() as i64,
        face_count,
        "topology_attribute_table must have face_count={face_count} MidSurfaceFace entries; \
         got {} (step-6 fold hook not yet wired?)",
        face_entries.len()
    );

    let edge_count = extract_naming_int(&result, "edge_count");
    assert_eq!(
        edge_entries.len() as i64,
        edge_count,
        "topology_attribute_table must have edge_count={edge_count} MidSurfaceEdge entries; \
         got {} (step-6 fold hook not yet wired?)",
        edge_entries.len()
    );

    // All synthetic IDs must have the high bit set (disjoint from OCCT handles).
    for (id, _attr) in table.iter() {
        assert_eq!(
            id.0 & 0x8000_0000_0000_0000,
            0x8000_0000_0000_0000,
            "synthetic GeometryHandleId {:#018x} must have the high bit set",
            id.0,
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-7 test: GREEN — restart round-trip via re-derivation
// ─────────────────────────────────────────────────────────────────────────────

/// Two independent fresh Engines running the same shell-extract dispatch must
/// produce byte-identical topology_attribute_table entries — the achievable
/// form of "round-trip across engine restart" given that the table is
/// rebuild-derived (persistent-cache disk rehydration is task ι scope).
///
/// Also verifies:
/// - VC is in Freshness::Final after both dispatches.
#[test]
fn mid_surface_fold_table_entries_are_deterministic_across_fresh_engines() {
    fn run_dispatch_and_collect(version: u64) -> Vec<(u64, Role, u32, String)> {
        let mut engine = make_simple_engine();
        register_shell_extract_compute_fns(&mut engine);

        let field_sdf = synthetic_slab_field();
        let value_inputs = vec![Value::Undef, Value::SampledField(field_sdf)];
        let c_id = ComputeNodeId::new("RoundTripFixture", 0);
        let cell = ValueCellId::new("RoundTripFixture", "result");

        engine
            .run_compute_dispatch(
                &c_id,
                std::slice::from_ref(&cell),
                "shell-extract::extract",
                &value_inputs,
                &[],
                &Value::Undef,
                &CancellationHandle::new(),
                VersionId(version),
            )
            .expect("run_compute_dispatch must succeed");

        // Confirm VC is Final
        let node = NodeId::Value(cell.clone());
        assert_eq!(
            engine.freshness(&node),
            Freshness::Final,
            "post-dispatch freshness must be Final"
        );

        // Collect sorted table entries for comparison.
        let mut entries: Vec<(u64, Role, u32, String)> = engine
            .topology_attribute_table()
            .iter()
            .map(|(id, attr)| {
                (
                    id.0,
                    attr.role,
                    attr.local_index,
                    attr.feature_id.to_string(),
                )
            })
            .collect();
        entries.sort_by_key(|(id, _, _, _)| *id);
        entries
    }

    let entries_a = run_dispatch_and_collect(1);
    let entries_b = run_dispatch_and_collect(2);

    assert!(
        !entries_a.is_empty(),
        "first engine must produce ≥1 table entry"
    );
    assert_eq!(
        entries_a, entries_b,
        "topology_attribute_table entries must be byte-identical across fresh engines \
         (deterministic re-derivation); \
         entries_a={entries_a:?}, entries_b={entries_b:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract an Int field from the `naming` sub-struct of a ShellExtractionResult Value.
fn extract_naming_int(result: &Value, key: &str) -> i64 {
    let outer = match result {
        Value::StructureInstance(d) => d,
        other => panic!("expected ShellExtractionResult StructureInstance, got {other:?}"),
    };
    match field(outer, "naming") {
        Some(Value::StructureInstance(n)) => match field(n, key) {
            Some(Value::Int(c)) => *c,
            other => panic!("naming.{key} not Int: {other:?}"),
        },
        other => panic!("result.naming not StructureInstance: {other:?}"),
    }
}
