//! Capstone integration test for the GR-031 / cluster C-29 typed-envelope leaf
//! observables (task #3575). Exercises the three stdlib deliverables end-to-end
//! through the public `reify_stdlib::eval_builtin` entry point (which routes to
//! `fea::eval_fea`):
//!
//!   A. `to_global(stress, frame)` — per-grid `sigma_global = F*sigma*F^T`.
//!   B. `linear_combine(mcr, weights)` — combined `frame` inherited from the
//!      reference (BTreeMap-lex-first weight) case; combined `stress` is the
//!      weighted sum of per-case stresses.
//!   C. `min_max_stress(mcr)` — per-grid min-over-cases of min-principal stress
//!      (eigs[0]) and max-over-cases of max-principal stress (eigs[2]).
//!
//! All references are exact-by-construction (f64 matmul / weighted sum / closed-
//! form eigenvalues of diagonal tensors), so the assertions use a tight
//! ~1e-9..1e-12 tolerance with no numeric-bound risk. Fixtures are built with
//! the public `reify_ir` Value/SampledField API (the fea.rs mod-test builders
//! are private), mirroring `make_valid_stress_field_3x3` /
//! `make_elastic_result_si_with_fields`.

use std::collections::BTreeMap;
use std::sync::Arc;

use reify_core::{DimensionVector, Type};
use reify_ir::{
    FieldSourceKind, InterpolationKind, PersistentMap, SampledField, SampledGridKind,
    StructureInstanceData, StructureTypeId, Value,
};
use reify_stdlib::eval_builtin;

const IDENTITY_3X3: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// +90deg rotation about z (row-major): [[0,-1,0],[1,0,0],[0,0,1]].
const Z90_3X3: [f64; 9] = [0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn pressure_ty() -> Type {
    Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }
}

fn length_ty() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

/// Build a 1-D Sampled `Value::Field` carrying stride-9 row-major 3x3 tensors
/// (one per grid point). `quantity` is the per-component scalar quantity (e.g.
/// `pressure_ty()` for stress, `dimensionless_scalar()` for a rotation frame).
fn make_matrix3x3_field(name: &str, axis: &[f64], tensors: &[[f64; 9]], quantity: Type) -> Value {
    assert_eq!(tensors.len(), axis.len(), "tensor count must match grid count");
    let mut data: Vec<f64> = Vec::with_capacity(axis.len() * 9);
    for t in tensors {
        data.extend_from_slice(t);
    }
    let n = axis.len();
    let spacing = if n > 1 { axis[1] - axis[0] } else { 1.0 };
    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![axis[0]],
        bounds_max: vec![axis[n - 1]],
        spacing: vec![spacing],
        axis_grids: vec![axis.to_vec()],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    };
    Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(quantity),
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Build a 1-D Sampled `Value::Field` carrying stride-3 [x,y,z] displacement
/// vectors (one per grid point).
fn make_vector3_field(name: &str, axis: &[f64], vectors: &[[f64; 3]]) -> Value {
    assert_eq!(vectors.len(), axis.len(), "vector count must match grid count");
    let mut data: Vec<f64> = Vec::with_capacity(axis.len() * 3);
    for v in vectors {
        data.extend_from_slice(v);
    }
    let n = axis.len();
    let spacing = if n > 1 { axis[1] - axis[0] } else { 1.0 };
    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![axis[0]],
        bounds_max: vec![axis[n - 1]],
        spacing: vec![spacing],
        axis_grids: vec![axis.to_vec()],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    };
    Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::Vector {
            n: 3,
            quantity: Box::new(length_ty()),
        },
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// Build a per-case `ElasticResult` as a `Value::StructureInstance` (the shape
/// `solve_load_cases` emits at runtime, task 4088).
fn make_case_si(displacement: Value, stress: Value, frame: Value) -> Value {
    let fields: PersistentMap<String, Value> = [
        ("displacement".to_string(), displacement),
        ("stress".to_string(), stress),
        ("frame".to_string(), frame),
        ("max_von_mises".to_string(), Value::Real(0.0)),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "ElasticResult".to_string(),
        version: 1,
        fields,
    }))
}

fn cases_inner(cases: &[(&str, Value)]) -> BTreeMap<Value, Value> {
    let mut inner = BTreeMap::new();
    for (name, val) in cases {
        inner.insert(Value::String((*name).to_string()), val.clone());
    }
    inner
}

/// `MultiCaseResult` in the raw `Value::Map { "cases" -> Map }` shape.
fn mcr_map(cases: &[(&str, Value)]) -> Value {
    let mut outer = BTreeMap::new();
    outer.insert(Value::String("cases".to_string()), Value::Map(cases_inner(cases)));
    Value::Map(outer)
}

/// `MultiCaseResult` in the SIR-alpha `Value::StructureInstance` shape.
fn mcr_si(cases: &[(&str, Value)]) -> Value {
    let fields: PersistentMap<String, Value> =
        std::iter::once(("cases".to_string(), Value::Map(cases_inner(cases)))).collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "MultiCaseResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Pull the raw `SampledField.data` buffer from a Sampled `Value::Field`.
fn sampled_data(v: &Value) -> Vec<f64> {
    match v {
        Value::Field { lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("expected SampledField lambda, got {:?}", other),
        },
        other => panic!("expected Value::Field, got {:?}", other),
    }
}

fn as_map(v: &Value) -> &BTreeMap<Value, Value> {
    match v {
        Value::Map(m) => m,
        other => panic!("expected Value::Map, got {:?}", other),
    }
}

fn map_field<'a>(m: &'a BTreeMap<Value, Value>, key: &str) -> &'a Value {
    m.get(&Value::String(key.to_string()))
        .unwrap_or_else(|| panic!("result map missing key {:?}", key))
}

fn assert_slice_approx(got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(
        got.len(),
        want.len(),
        "length mismatch: got {:?}, want {:?}",
        got,
        want
    );
    for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        assert!(
            (g - w).abs() <= tol,
            "index {}: got {}, want {} (tol {}); full got={:?} want={:?}",
            i,
            g,
            w,
            tol,
            got,
            want
        );
    }
}

/// Deliverable A: `to_global(stress, frame)` rotates each per-grid stress tensor
/// by the corresponding frame, `sigma_global = F*sigma*F^T`.
#[test]
fn to_global_rotates_stress_by_frame() {
    let grid = vec![0.0, 1.0];
    // Point 0: a full symmetric tensor; Point 1: a diagonal tensor.
    let s0 = [1.0, 4.0, 5.0, 4.0, 2.0, 6.0, 5.0, 6.0, 3.0];
    let s1 = [10.0, 0.0, 0.0, 0.0, 20.0, 0.0, 0.0, 0.0, 30.0];
    let stress = make_matrix3x3_field("stress", &grid, &[s0, s1], pressure_ty());
    let frame = make_matrix3x3_field(
        "frame",
        &grid,
        &[Z90_3X3, Z90_3X3],
        Type::dimensionless_scalar(),
    );

    let result = eval_builtin("to_global", &[stress, frame]);

    // Hand-computed sigma_global = R90z * sigma * R90z^T.
    // Point 0: xx<->yy swap, xy negated, xz<->yz swap-with-sign.
    let e0 = [2.0, -4.0, -6.0, -4.0, 1.0, 5.0, -6.0, 5.0, 3.0];
    // Point 1 (diagonal): xx<->yy swap → diag(20,10,30).
    let e1 = [20.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 30.0];
    let mut expected = Vec::new();
    expected.extend_from_slice(&e0);
    expected.extend_from_slice(&e1);
    assert_slice_approx(&sampled_data(&result), &expected, 1e-9);

    // Identity frame is a no-op (regression sanity for the rotation kernel).
    let stress2 = make_matrix3x3_field("stress", &grid, &[s0, s1], pressure_ty());
    let ident = make_matrix3x3_field(
        "frame",
        &grid,
        &[IDENTITY_3X3, IDENTITY_3X3],
        Type::dimensionless_scalar(),
    );
    let noop = eval_builtin("to_global", &[stress2, ident]);
    let mut flat = Vec::new();
    flat.extend_from_slice(&s0);
    flat.extend_from_slice(&s1);
    assert_slice_approx(&sampled_data(&noop), &flat, 1e-12);
}

/// Deliverable B: `linear_combine` inherits the reference case's `frame` (the
/// BTreeMap-lex-first weight key, "A") and weighted-sums the per-case stresses.
/// Covered over BOTH MultiCaseResult container shapes.
#[test]
fn linear_combine_inherits_reference_frame_and_sums_stress() {
    let grid = vec![0.0, 1.0];

    // Case A: frame = Z90 (distinct from B, to prove A is the inherited one).
    let disp_a = make_vector3_field("dispA", &grid, &[[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]]);
    let stress_a = make_matrix3x3_field(
        "stressA",
        &grid,
        &[IDENTITY_3X3, [2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0]],
        pressure_ty(),
    );
    let frame_a = make_matrix3x3_field(
        "frameA",
        &grid,
        &[Z90_3X3, Z90_3X3],
        Type::dimensionless_scalar(),
    );
    let case_a = make_case_si(disp_a, stress_a, frame_a);

    // Case B: frame = identity (must NOT be the inherited frame).
    let disp_b = make_vector3_field("dispB", &grid, &[[3.0, 0.0, 0.0], [4.0, 0.0, 0.0]]);
    let stress_b = make_matrix3x3_field(
        "stressB",
        &grid,
        &[
            [10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0],
            [20.0, 0.0, 0.0, 0.0, 20.0, 0.0, 0.0, 0.0, 20.0],
        ],
        pressure_ty(),
    );
    let frame_b = make_matrix3x3_field(
        "frameB",
        &grid,
        &[IDENTITY_3X3, IDENTITY_3X3],
        Type::dimensionless_scalar(),
    );
    let case_b = make_case_si(disp_b, stress_b, frame_b);

    // weights: A=2.0, B=3.0 → combined_stress = 2*A + 3*B.
    let mut weights = BTreeMap::new();
    weights.insert(Value::String("A".to_string()), Value::Real(2.0));
    weights.insert(Value::String("B".to_string()), Value::Real(3.0));
    let weights = Value::Map(weights);

    // Reference (first weighted, lex-first) is "A" → inherited frame is Z90.
    let mut expected_frame = Vec::new();
    expected_frame.extend_from_slice(&Z90_3X3);
    expected_frame.extend_from_slice(&Z90_3X3);
    // combined_stress: P0 = 2*1 + 3*10 = 32 (diag); P1 = 2*2 + 3*20 = 64 (diag).
    let expected_stress = vec![
        32.0, 0.0, 0.0, 0.0, 32.0, 0.0, 0.0, 0.0, 32.0, // P0
        64.0, 0.0, 0.0, 0.0, 64.0, 0.0, 0.0, 0.0, 64.0, // P1
    ];

    for mcr in [
        mcr_map(&[("A", case_a.clone()), ("B", case_b.clone())]),
        mcr_si(&[("A", case_a.clone()), ("B", case_b.clone())]),
    ] {
        let result = eval_builtin("linear_combine", &[mcr, weights.clone()]);
        let rmap = as_map(&result);

        let frame_out = map_field(rmap, "frame");
        assert!(
            !frame_out.is_undef(),
            "linear_combine frame must be inherited (non-Undef), got Undef"
        );
        assert_slice_approx(&sampled_data(frame_out), &expected_frame, 1e-12);

        let stress_out = map_field(rmap, "stress");
        assert_slice_approx(&sampled_data(stress_out), &expected_stress, 1e-9);
    }
}

/// Deliverable C: `min_max_stress(mcr)` → Map{ "min", "max" } principal-stress
/// envelope. Diagonal tensors → eigenvalues are the sorted diagonal entries.
/// Covered over BOTH MultiCaseResult container shapes.
#[test]
fn min_max_stress_principal_envelope() {
    let grid = vec![0.0, 1.0];

    // Case A: P0 eigs [10,50,100]; P1 eigs [-30,5,20].
    let a0 = [100.0, 0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0, 10.0];
    let a1 = [-30.0, 0.0, 0.0, 0.0, 20.0, 0.0, 0.0, 0.0, 5.0];
    // Case B: P0 eigs [40,60,80]; P1 eigs [-50,0,100].
    let b0 = [80.0, 0.0, 0.0, 0.0, 60.0, 0.0, 0.0, 0.0, 40.0];
    let b1 = [0.0, 0.0, 0.0, 0.0, 100.0, 0.0, 0.0, 0.0, -50.0];

    let disp = make_vector3_field("disp", &grid, &[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]);
    let frame = make_matrix3x3_field(
        "frame",
        &grid,
        &[IDENTITY_3X3, IDENTITY_3X3],
        Type::dimensionless_scalar(),
    );
    let case_a = make_case_si(
        disp.clone(),
        make_matrix3x3_field("sa", &grid, &[a0, a1], pressure_ty()),
        frame.clone(),
    );
    let case_b = make_case_si(
        disp,
        make_matrix3x3_field("sb", &grid, &[b0, b1], pressure_ty()),
        frame,
    );

    // min over cases of eigs[0]: P0 min(10,40)=10, P1 min(-30,-50)=-50.
    let expected_min = [10.0, -50.0];
    // max over cases of eigs[2]: P0 max(100,80)=100, P1 max(20,100)=100.
    let expected_max = [100.0, 100.0];

    for mcr in [
        mcr_map(&[("A", case_a.clone()), ("B", case_b.clone())]),
        mcr_si(&[("A", case_a.clone()), ("B", case_b.clone())]),
    ] {
        let result = eval_builtin("min_max_stress", &[mcr]);
        let rmap = as_map(&result);
        assert_slice_approx(&sampled_data(map_field(rmap, "min")), &expected_min, 1e-9);
        assert_slice_approx(&sampled_data(map_field(rmap, "max")), &expected_max, 1e-9);
    }
}
