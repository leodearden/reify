//! RED test: element_order P1/P2 dispatch in `solve_buckling_trampoline` (task 4129).
//!
//! Verifies that passing `element_order: ElementOrder.P2` in BucklingOptions causes
//! the trampoline to take the P2 mesh branch (coarser cross-section nx=2, promote
//! to P2 tet mesh), producing a `base_node_positions` list whose length differs
//! from the P1 path.
//!
//! # Design (mirrors the modal trampoline dispatch test, modal_ops.rs:3517)
//!
//! The test calls `solve_buckling_trampoline` twice on an identical tiny geometry
//! (lz=1mm, lx=ly=10mm):
//!   - Run A: BucklingOptions with `element_order` field absent (defaults to P1)
//!   - Run B: BucklingOptions with `element_order = Value::Enum{ElementOrder, P2}`
//!
//! For the P1 path (nx=8): n_nodes = (8+1)·(8+1)·(nz+1) = 9·9·2 = 162 (TINY_N_NODES).
//! For the P2 path (nx=2): P1 corner count = (2+1)·(2+1)·(nz+1) = 3·3·2 = 18;
//! promotion inserts edge-midpoint nodes → n_p2 > 18.
//!
//! Assertions (all three required to prove distinct paths):
//!   1. p1_n == 162  (P1 path: the unchanged nx=8 grid)
//!   2. p2_n > 18    (P2 path: more than the nx=2 corner grid alone — promotion ran)
//!   3. p2_n != p1_n (paths observably distinct → trampoline branched on element_order)
//!
//! # RED status (step-3)
//!
//! The trampoline currently ignores `element_order` and always runs the nx=8 P1 path,
//! so the P2 run also produces 162 nodes → assertion 3 (`assert_ne!(p2_n, p1_n)`) fails.
//! GREEN after step-5 implements the P1/P2 dispatch.
//!
//! # Tiny-mesh rationale
//!
//! Geometry: lx=ly=10mm, lz=1mm.
//! Trampoline formula: cross_elem_size = min(lx,ly)/(nx/2).
//! For P1 (nx=8): cross_elem_size = 0.01/4 = 0.0025 m; nz = round(0.001/0.0025) = 0 → max(1) = 1.
//! For P2 (nx=2): cross_elem_size = 0.01/1 = 0.01 m;  nz = round(0.001/0.01)  = 0 → max(1) = 1.
//! DOFs for P1: 3·162 = 486 — fast in debug mode.
//! DOFs for P2 promoted: small enough for debug-mode CI.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_steel_material() -> Value {
    let fields: PersistentMap<String, Value> = [
        (
            "youngs_modulus".to_string(),
            Value::Scalar {
                si_value: 205.0e9,
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("poisson_ratio".to_string(), Value::Real(0.3)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticMaterial".to_string(),
        version: 1,
        fields,
    }))
}

fn make_scalar_length(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::LENGTH,
    }
}

/// Build a BucklingOptions StructureInstance.
///
/// `element_order_variant`: if `Some("P2")`, inserts
/// `element_order: Value::Enum{type_name:"ElementOrder", variant:"P2"}`.
/// If `None`, omits the field entirely (defaults to P1 in the trampoline).
fn make_buckling_options(element_order_variant: Option<&str>) -> Value {
    let mut map: Vec<(String, Value)> = vec![
        ("n_modes".to_string(), Value::Int(1)),
        ("tol".to_string(), Value::Real(1e-4)),
        ("max_iters".to_string(), Value::Int(500)),
    ];
    if let Some(variant) = element_order_variant {
        map.push((
            "element_order".to_string(),
            Value::Enum {
                type_name: "ElementOrder".to_string(),
                variant: variant.to_string(),
            },
        ));
    }
    let fields: PersistentMap<String, Value> = map.into_iter().collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingOptions".to_string(),
        version: 1,
        fields,
    }))
}

fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

/// Run the trampoline on the tiny geometry (lz=1mm, lx=ly=10mm) with the
/// given BucklingOptions value.
///
/// Empty loads list triggers the 1.0 N sentinel default in the trampoline.
fn run_trampoline_with_opts(opts: Value) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;

    let value_inputs = vec![
        make_steel_material(),
        make_scalar_length(0.001), // length (lz) = 1 mm  (compression axis)
        make_scalar_length(0.01),  // width  (lx) = 10 mm
        make_scalar_length(0.01),  // height (ly) = 10 mm
        Value::List(vec![]),       // loads  — empty → default 1.0 N sentinel
        Value::List(vec![]),       // supports — pin-pin (empty → trampoline default)
        opts,
    ];

    reify_eval::compute_targets::buckling::solve_buckling_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Helper: extract n_nodes from a ComputeOutcome by reading base_node_positions.len() / 3.
fn extract_n_nodes(outcome: ComputeOutcome) -> usize {
    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    };
    let base = extract_field(&result, "base_node_positions")
        .unwrap_or_else(|| panic!("BucklingResult must have 'base_node_positions' field"));
    match base {
        Value::List(v) => {
            assert_eq!(
                v.len() % 3,
                0,
                "base_node_positions length must be divisible by 3"
            );
            v.len() / 3
        }
        other => panic!("base_node_positions must be Value::List, got: {:?}", other),
    }
}

// ── constants ─────────────────────────────────────────────────────────────────

/// Expected n_nodes for the P1 path (nx=8 grid, tiny geometry).
/// nx1=9, ny1=9, nz1=2 (nz=1 from formula, +1 per axis) → 9·9·2 = 162.
const P1_EXPECTED_N_NODES: usize = 9 * 9 * 2;

/// Minimum n_nodes for the P2 path (nx=2 corner grid, tiny geometry).
/// nx1=3, ny1=3, nz1=2 → 3·3·2 = 18 CORNER nodes; promotion adds midpoints.
/// The promoted count must be STRICTLY GREATER than this.
const P2_CORNER_GRID_MIN: usize = 3 * 3 * 2;

// ── tests ─────────────────────────────────────────────────────────────────────

/// The trampoline must branch on `element_order` and produce observably different
/// mesh sizes for P1 vs P2.
///
/// Assertions:
///   1. P1 (absent element_order) → n_nodes == 162 (nx=8 grid, unchanged).
///   2. P2 (element_order: P2)   → n_nodes  > 18  (promotion ran: > nx=2 corners).
///   3. P2 node count != P1 node count            (paths are distinct).
///
/// RED at step-3: the trampoline ignores element_order → both runs yield 162 →
/// assertion 3 fails.  GREEN after step-5 dispatch implementation.
#[test]
fn trampoline_honors_element_order_p2_buckling() {
    // Run A: P1 (element_order field absent → default P1)
    let p1_outcome = run_trampoline_with_opts(make_buckling_options(None));
    let p1_n = extract_n_nodes(p1_outcome);

    // Run B: P2 (element_order = ElementOrder.P2 explicitly)
    let p2_outcome = run_trampoline_with_opts(make_buckling_options(Some("P2")));
    let p2_n = extract_n_nodes(p2_outcome);

    // 1. P1 path produces the unchanged nx=8 grid.
    assert_eq!(
        p1_n, P1_EXPECTED_N_NODES,
        "P1 (absent element_order) should produce {} nodes (nx=8 grid), got {}",
        P1_EXPECTED_N_NODES, p1_n
    );

    // 2. P2 path produces MORE than the bare nx=2 corner grid (promotion ran).
    assert!(
        p2_n > P2_CORNER_GRID_MIN,
        "P2 path should produce > {} nodes (nx=2 corner grid + midpoints from promotion), got {}",
        P2_CORNER_GRID_MIN,
        p2_n
    );

    // 3. The two paths must produce different node counts — proving dispatch ran.
    assert_ne!(
        p2_n, p1_n,
        "P2 path (n_nodes={}) must differ from P1 path (n_nodes={}) — \
         trampoline should branch on element_order but currently ignores it",
        p2_n, p1_n
    );
}
