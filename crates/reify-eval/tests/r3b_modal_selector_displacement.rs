//! Task 4122 (R3b): modal location readers resolve a symbolic `Selector` `at` /
//! `location` to a mesh node against the carried result topology — the capability
//! flip over task 3823's global-antinode fallback.
//!
//! These integration tests drive `reify_eval::modal_ops::displacement_at_trampoline`
//! directly with a constructed `DisplacementTimeHistory` fixture whose echoed
//! `ModalResult` carries a populated `topology` field (a `CarriedTopology` encoding,
//! task 4654 R3a). The fixture is built so the GLOBAL fundamental antinode (the
//! argmax‖Φ₀‖ node 3823 falls back to) is node **A=0**, while the `faces_by_normal(+Z)`
//! selector resolves to a face attached to nodes **{1, 2}** — EXCLUDING A — whose
//! peak-response representative is node **B=2**.
//!
//! Verdict-shaped acceptance (no tuned float tolerance on a response magnitude —
//! the fixture's projections are exact small integers; equality is discrete by
//! construction):
//!   (a) the outcome is a non-Undef, non-empty `List<Real>` with all entries finite;
//!   (b) the Selector-driven series equals the projection at the selector-resolved
//!       representative node B and NOT the global antinode A (the flip);
//!   (c) a `String` location still yields the node-A antinode series (3823 preserved).
//!
//! RED today (step-06): `displacement_at_trampoline` reads `value_inputs[1]` only as
//! `Value::String` (else `""`), so a `Value::Selector` falls through the `_ => ""`
//! arm → `resolve_location_node("", …)` → `dominant_antinode_index` → node A. The
//! flip assertion (b) fails. GREEN after step-07 wires the Selector dispatch.

use reify_core::identity::RealizationNodeId;
use reify_core::ty::SelectorKind;
use reify_ir::boundary_attachment::{BoundaryAssociation, NodeAttachment};
use reify_ir::geometry::{ElementOrderTag, GeometryHandleId, VolumeMesh};
use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

use reify_eval::compute_targets::result_topology::{CarriedTopology, from_realized_mesh};
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};

// ── Fixture constants ─────────────────────────────────────────────────────────

/// Per-node mode-0 displacement shape Φ₀ (one `[x,y,z]` per node). Node 0 has the
/// largest norm → it is the GLOBAL antinode A (what `dominant_antinode_index`
/// picks). Node 2 is the largest WITHIN the +Z face's node-set {1,2} → the
/// selector representative B.
const MODE0_SHAPE: [[f64; 3]; 4] = [
    [10.0, 0.0, 0.0], // node 0 — global antinode A (‖Φ‖²=100)
    [3.0, 0.0, 0.0],  // node 1 — on +Z face (‖Φ‖²=9)
    [5.0, 0.0, 0.0],  // node 2 — on +Z face, representative B (‖Φ‖²=25)
    [1.0, 0.0, 0.0],  // node 3 — off +Z face (‖Φ‖²=1)
];

/// One modal coordinate series ξ₀(tⱼ) (non-zero). The reconstructed displacement
/// is `coeff₀ · ξ₀`, where `coeff₀ = Φ₀[node]·direction`.
const MODE_COORDS0: [f64; 3] = [1.0, 2.0, 3.0];

/// Global antinode node index (3823 fallback target).
const NODE_A: usize = 0;
/// Selector-resolved representative node index (peak within the +Z face set {1,2}).
const NODE_B: usize = 2;

/// One degree in radians — the `faces_by_normal` cone tolerance.
fn one_degree_rad() -> f64 {
    1.0_f64.to_radians()
}

// ── Fixture builders ────────────────────────────────────────────────────────────

/// A symbolic part reference (kernel_handle: None) — the carried topology's identity.
fn symbolic_part() -> GeometryHandleRef {
    GeometryHandleRef {
        realization_ref: RealizationNodeId::new("beam", 0),
        upstream_values_hash: [7u8; 32],
        kernel_handle: None,
    }
}

/// Build the carried topology: a +Z face = id(10) attached to nodes {1, 2} and a
/// +Y face = id(11) attached to nodes {0, 3}. `faces_by_normal(+Z)` therefore
/// selects id(10) → node-set {1, 2}, which EXCLUDES the global antinode A=0.
fn make_carried() -> CarriedTopology {
    let part = symbolic_part();

    // Flat XYZ for 4 nodes (coords are immaterial to resolution; finite values).
    let vertices: Vec<f32> = vec![
        0.0, 0.0, 0.0, // node 0
        1.0, 0.0, 0.0, // node 1
        0.0, 1.0, 0.0, // node 2
        0.0, 0.0, 1.0, // node 3
    ];

    let mut boundary = BoundaryAssociation::default();
    boundary.associate(0, NodeAttachment::OnFace(GeometryHandleId(11))); // +Y face
    boundary.associate(1, NodeAttachment::OnFace(GeometryHandleId(10))); // +Z face
    boundary.associate(2, NodeAttachment::OnFace(GeometryHandleId(10))); // +Z face
    boundary.associate(3, NodeAttachment::OnFace(GeometryHandleId(11))); // +Y face

    let mesh = VolumeMesh {
        vertices,
        tet_indices: vec![0, 1, 2, 3],
        element_order: ElementOrderTag::P1,
        normals: None,
        boundary: Some(boundary),
    };

    let face_normals = vec![
        (GeometryHandleId(10), [0.0_f64, 0.0, 1.0]), // +Z
        (GeometryHandleId(11), [0.0_f64, 1.0, 0.0]), // +Y
    ];

    from_realized_mesh(part, &mesh, face_normals)
}

/// The mode-0 `shape` field — a `List<Vector3>` of per-node displacements.
fn mode0_shape_value() -> Value {
    Value::List(
        MODE0_SHAPE
            .iter()
            .map(|c| Value::Vector(vec![Value::Real(c[0]), Value::Real(c[1]), Value::Real(c[2])]))
            .collect(),
    )
}

/// A single-mode `ModalResult` StructureInstance carrying the mode-0 shape and the
/// populated `topology` field (so the Selector path can read the carried topology).
fn make_modal_result(carried: &CarriedTopology) -> Value {
    let mode_fields: PersistentMap<String, Value> = [
        ("shape".to_string(), mode0_shape_value()),
        // Dynamics for the transient forcing solve (step-08). Harmless to
        // displacement_at, which reads only `shape`: a 1 Hz lightly-damped mode.
        ("frequency".to_string(), Value::Real(1.0)),
        ("damping_ratio".to_string(), Value::Real(0.05)),
    ]
    .into_iter()
    .collect();
    let mode = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version: 1,
        fields: mode_fields,
    }));

    let result_fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![mode])),
        ("topology".to_string(), carried.to_value()),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ModalResult".to_string(),
        version: 1,
        fields: result_fields,
    }))
}

/// A `DisplacementTimeHistory` echoing the `ModalResult` plus the stored modal
/// coordinates ξ₀(tⱼ) (one mode → one series).
fn make_history(carried: &CarriedTopology) -> Value {
    let mode_coords = Value::List(vec![Value::List(
        MODE_COORDS0.iter().copied().map(Value::Real).collect(),
    )]);

    let fields: PersistentMap<String, Value> = [
        ("modal_result".to_string(), make_modal_result(carried)),
        ("mode_coords".to_string(), mode_coords),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "DisplacementTimeHistory".to_string(),
        version: 1,
        fields,
    }))
}

/// A `faces_by_normal(+Z)` leaf selector over the carried symbolic part.
fn plus_z_selector(carried: &CarriedTopology) -> Value {
    let sv = SelectorValue::leaf(
        SelectorKind::Face,
        carried.part().clone(),
        LeafQuery::ByNormal {
            dir: [0.0, 0.0, 1.0],
            tol_rad: one_degree_rad(),
        },
    )
    .expect("Face kind matches ByNormal required kind");
    Value::Selector(sv)
}

/// The query direction +X — the displacement axis along which Φ is projected.
fn direction_x() -> Value {
    Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
}

/// Drive `displacement_at_trampoline([history, location, direction])` and read the
/// reconstructed series as `Vec<f64>`.
fn run_displacement_at(history: &Value, location: Value, direction: Value) -> Vec<f64> {
    let value_inputs = vec![history.clone(), location, direction];
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;

    let outcome = reify_eval::modal_ops::displacement_at_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    );
    match outcome {
        ComputeOutcome::Completed { result, .. } => read_real_list(&result),
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    }
}

/// Read a `List<Real>` value into `Vec<f64>`; panics if the value is not a List
/// (so assertion (a)'s non-Undef contract is enforced at the read site).
fn read_real_list(v: &Value) -> Vec<f64> {
    match v {
        Value::List(items) => items
            .iter()
            .map(|x| match x {
                Value::Real(r) => *r,
                Value::Scalar { si_value, .. } => *si_value,
                Value::Int(n) => *n as f64,
                other => panic!("series entry must be a Real, got: {:?}", other),
            })
            .collect(),
        other => panic!("displacement_at must return a Value::List, got: {:?}", other),
    }
}

/// The exact reconstructed series for a given query node: `Φ₀[node]·[1,0,0] · ξ₀`.
/// All values are exact small integers in f64 (no tuned tolerance needed).
fn expected_series_at(node: usize) -> Vec<f64> {
    let coeff = MODE0_SHAPE[node][0]; // dir = +X → coeff = Φ₀[node].x
    MODE_COORDS0.iter().map(|&xi| coeff * xi).collect()
}

/// Elementwise approximate equality (defensive 1e-9 floor; values are exact here).
fn series_approx_eq(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

// ── step-06 tests (RED until step-07 wires Selector dispatch) ─────────────────────

/// (a)+(b): a `Selector` `location` resolves to the +Z face's representative node
/// B=2 (peak within {1,2}), NOT the global antinode A=0. The series is a non-empty,
/// all-finite `List<Real>` equal to node B's projection and distinct from node A's.
///
/// RED today: the Selector falls through `value_inputs[1]`'s `_ => ""` arm →
/// antinode A → the series equals node A and the flip assertion fails.
#[test]
fn selector_location_flips_to_face_representative_node() {
    let carried = make_carried();
    let history = make_history(&carried);

    let series = run_displacement_at(&history, plus_z_selector(&carried), direction_x());

    // (a) non-empty, all-finite series.
    assert!(!series.is_empty(), "Selector series must be non-empty, got: {:?}", series);
    assert!(
        series.iter().all(|x| x.is_finite()),
        "Selector series must be all-finite, got: {:?}",
        series
    );

    // (b) equals node B's projection, NOT node A's (the capability flip).
    let expected_b = expected_series_at(NODE_B);
    let expected_a = expected_series_at(NODE_A);
    assert!(
        series_approx_eq(&series, &expected_b),
        "Selector(+Z) must resolve to representative node B={NODE_B}: \
         expected {:?}, got {:?}",
        expected_b,
        series
    );
    assert!(
        !series_approx_eq(&series, &expected_a),
        "Selector(+Z) must NOT fall back to the global antinode A={NODE_A} \
         (the capability flip over 3823): antinode series {:?}, got {:?}",
        expected_a,
        series
    );
}

/// (c) Control: a `String` location preserves the task-3823 behavior — a
/// non-numeric string resolves to the global fundamental antinode A=0. This must
/// hold both today and after the Selector overload lands (the String overload is
/// preserved, not replaced).
#[test]
fn string_location_preserves_antinode_3823() {
    let carried = make_carried();
    let history = make_history(&carried);

    let series = run_displacement_at(
        &history,
        Value::String("anything".to_string()),
        direction_x(),
    );

    let expected_a = expected_series_at(NODE_A);
    assert!(
        series_approx_eq(&series, &expected_a),
        "String location must keep the 3823 global-antinode behavior (node A={NODE_A}): \
         expected {:?}, got {:?}",
        expected_a,
        series
    );
}

// ── step-08 test: forcing path resolves a Selector `at` (RED until step-09) ───────

/// A `StepForce`-shaped forcing source with the given `at` location and a +X
/// direction (magnitude 1 N from t=0).
fn make_step_force(at: Value) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("magnitude".to_string(), Value::Real(1.0)),
        ("start_time".to_string(), Value::Real(0.0)),
        ("at".to_string(), at),
        ("direction".to_string(), direction_x()),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "StepForce".to_string(),
        version: 1,
        fields,
    }))
}

/// A `ForcingTimeHistory` carrying a single `StepForce` source at `at`.
fn make_forcing(at: Value) -> Value {
    let fields: PersistentMap<String, Value> =
        [("sources".to_string(), Value::List(vec![make_step_force(at)]))]
            .into_iter()
            .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ForcingTimeHistory".to_string(),
        version: 1,
        fields,
    }))
}

/// Drive `solve_transient_response_trampoline([modal_result, forcing, 0, 1, 0.1])`
/// and return the resulting `DisplacementTimeHistory`.
fn run_transient(modal_result: &Value, forcing: Value) -> Value {
    let value_inputs = vec![
        modal_result.clone(),
        forcing,
        Value::Real(0.0), // t_start
        Value::Real(1.0), // t_end
        Value::Real(0.1), // dt → 11-point uniform grid
    ];
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;

    let outcome = reify_eval::modal_ops::solve_transient_response_trampoline(
        &value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    );
    match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("expected ComputeOutcome::Completed, got: {:?}", other),
    }
}

/// Read a `DisplacementTimeHistory`'s `mode_coords` field (`List<List<Real>>`) into
/// `Vec<Vec<f64>>`.
fn read_mode_coords(history: &Value) -> Vec<Vec<f64>> {
    let coords = match history {
        Value::StructureInstance(d) => d.fields.get("mode_coords"),
        _ => None,
    };
    match coords {
        Some(Value::List(series)) => series.iter().map(read_real_list).collect(),
        _ => Vec::new(),
    }
}

/// Peak |value| across a `mode_coords` matrix (the response magnitude).
fn peak_abs(coords: &[Vec<f64>]) -> f64 {
    coords
        .iter()
        .flat_map(|s| s.iter())
        .fold(0.0_f64, |m, &x| m.max(x.abs()))
}

/// The transient FORCING path resolves a `Selector` `at` to the +Z face's
/// representative node B=2 — NOT the global antinode A=0. Verdict-shaped:
///   (a) the response is a non-Undef history whose `mode_coords` is non-empty and
///       all-finite;
///   (b) the Selector-forced response DIFFERS from the String-forced response
///       (which excites the antinode A), AND its peak magnitude is strictly
///       smaller (node B has a smaller Φ than the antinode A → smaller modal
///       forcing → smaller response). Both are exact, tolerance-free verdicts.
///
/// RED today: the forcing reads `at` only as `Value::String` (else `""`), so the
/// Selector and String runs both excite antinode A → identical responses → (b)
/// fails. GREEN after step-09 routes `at` through `resolve_location_value`.
#[test]
fn forcing_selector_at_resolves_representative_node() {
    let carried = make_carried();
    let modal_result = make_modal_result(&carried);

    let selector_resp = run_transient(&modal_result, make_forcing(plus_z_selector(&carried)));
    let string_resp = run_transient(
        &modal_result,
        make_forcing(Value::String("anything".to_string())),
    );

    let sel_coords = read_mode_coords(&selector_resp);
    let str_coords = read_mode_coords(&string_resp);

    // (a) non-empty, all-finite modal-coordinate response.
    assert!(
        !sel_coords.is_empty() && sel_coords.iter().all(|s| !s.is_empty()),
        "Selector-forced response must have non-empty mode_coords, got: {:?}",
        sel_coords
    );
    assert!(
        sel_coords.iter().flat_map(|s| s.iter()).all(|x| x.is_finite()),
        "Selector-forced mode_coords must be all-finite, got: {:?}",
        sel_coords
    );

    // (b) the two responses differ (forcing applied at a different node).
    let differ = sel_coords.len() != str_coords.len()
        || sel_coords.iter().zip(&str_coords).any(|(a, b)| {
            a.len() != b.len() || a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1e-9)
        });
    assert!(
        differ,
        "Selector `at` (node B={NODE_B}) and String `at` (antinode A={NODE_A}) must \
         excite DIFFERENT nodes → different responses; got identical mode_coords \
         (Selector fell back to the antinode): {:?}",
        sel_coords
    );

    // (b cont.) node B has a smaller Φ than antinode A → strictly smaller response.
    let sel_peak = peak_abs(&sel_coords);
    let str_peak = peak_abs(&str_coords);
    assert!(
        str_peak > sel_peak && sel_peak > 0.0,
        "antinode-A forcing (String) must yield a strictly larger non-zero peak than \
         the node-B forcing (Selector): str_peak={str_peak}, sel_peak={sel_peak}"
    );
}
