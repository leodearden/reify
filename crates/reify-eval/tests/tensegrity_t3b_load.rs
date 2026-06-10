//! Tensegrity T3b — `solver::tensegrity_load` producer-side trampoline tests.
//!
//! PRD: `docs/prds/v0_6/tensegrity-structures.md` §6 / §8.1 / Tier-3 leaf T3b.
//! These mirror the T1a `tensegrity_t1a_form_find.rs` producer tests: they call
//! the trampoline directly with crafted `Value`s (no realization / warm-state /
//! compile pipeline), so they exercise the Value-cracking + result-building seam
//! in isolation from the lowering machinery.
//!
//! # Trampoline input contract (the future `tensegrity_load` stdlib signature)
//!
//! ```text
//! [0] structure       : Tensegrity              (Value::StructureInstance)
//! [1] prestress       : List<Force>             (List of Scalar{FORCE}) — one
//!                                                 per member, struts-then-cables
//! [2] youngs_modulus  : Scalar                  (broadcast E, shared section)
//! [3] area            : Scalar                  (broadcast A, shared section)
//! [4] loads           : List<Vector3<Force>>    (per-node external force)
//! [5] supports        : List<Int>               (fixed node indices)
//! ```
//!
//! The result is a `TensegrityLoadResult` `Value::StructureInstance` with
//! `displacements : List<Vector3<Length>>`, `member_forces` /
//! `member_force_deltas : List<Force>`, `slack : List<Bool>`, and
//! `converged : Bool`.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};

// ── Value crafting helpers ───────────────────────────────────────────────────

/// A Length-typed coordinate Scalar (SI metres) — how `point3(..m, ..)` lowers.
fn length(m: f64) -> Value {
    Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    }
}

/// A Force-typed Scalar (SI newtons).
fn force(n: f64) -> Value {
    Value::Scalar {
        si_value: n,
        dimension: DimensionVector::FORCE,
    }
}

/// A 3-component `Value::Point` node coordinate (Length scalars).
fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

/// A 3-component `Value::Vector` force load (Force scalars).
fn force_vec(fx: f64, fy: f64, fz: f64) -> Value {
    Value::Vector(vec![force(fx), force(fy), force(fz)])
}

/// A Pressure-typed Scalar (Young's modulus E).
fn pressure(p: f64) -> Value {
    Value::Scalar {
        si_value: p,
        dimension: DimensionVector::PRESSURE,
    }
}

/// An Area-typed Scalar (cross-section A).
fn area(a: f64) -> Value {
    Value::Scalar {
        si_value: a,
        dimension: DimensionVector::AREA,
    }
}

/// Collinear two-cable string `anchor(0) — free(1) — anchor(2)` with the free
/// node at `(L,0,0)` and anchors at the origin and `(2L,0,0)`. `struts: []`,
/// cables `[[0,1],[1,2]]` — the T3b golden topology.
fn two_cable_string(l: f64) -> Value {
    let nodes = Value::List(vec![
        node(0.0, 0.0, 0.0),     // node 0 — anchor
        node(l, 0.0, 0.0),       // node 1 — free
        node(2.0 * l, 0.0, 0.0), // node 2 — anchor
    ]);
    let struts = Value::List(vec![]);
    let cables = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(1)]),
        Value::List(vec![Value::Int(1), Value::Int(2)]),
    ]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), nodes),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// Invoke the trampoline with the standard no-realization / no-warm-state args.
fn call_tensegrity_load(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::tensegrity_load::solve_tensegrity_load_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Extract an f64 from a Scalar (any dimension) or a bare Real.
fn coord(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        other => panic!("expected a Scalar/Real component, got {other:?}"),
    }
}

/// Crack a 3-component displacement `Value::Vector` into `[f64; 3]`.
fn vec3(v: &Value) -> [f64; 3] {
    match v {
        Value::Vector(c) | Value::Point(c) if c.len() == 3 => {
            [coord(&c[0]), coord(&c[1]), coord(&c[2])]
        }
        other => panic!("expected a 3-component Vector, got {other:?}"),
    }
}

// ── step-9: happy path (no-slack transverse load) ────────────────────────────

/// Happy path: the trampoline cracks the Tensegrity / prestress / section /
/// loads / supports Values, calls the kernel, and returns `Completed` with a
/// `TensegrityLoadResult` whose `displacements` / `member_forces` /
/// `member_force_deltas` / `slack` fields are populated and the no-slack
/// transverse deflection matches `u_y[1] = P_t·L / (2·N0)`.
///
/// Setup mirrors the kernel golden `no_slack_prestressed_string_transverse_load`
/// (`reify-solver-elastic/tests/tensegrity_t3b_load.rs`): the combined transverse
/// string stiffness is `2·N0/L`, so a transverse tip load `P_t` deflects node 1
/// by `P_t·L/(2·N0)` with both member forces unchanged (`≈ N0`) and no slack.
#[test]
fn trampoline_no_slack_transverse_load_solves() {
    let l = 2.0_f64;
    let e = 200.0e9_f64;
    let a = 1.0e-4_f64;
    let n0 = 5_000.0_f64;
    let p_t = 50.0_f64;

    let value_inputs = vec![
        two_cable_string(l),
        Value::List(vec![force(n0), force(n0)]), // prestress per cable
        pressure(e),
        area(a),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),  // node 0 (anchor) — no load
            force_vec(0.0, p_t, 0.0),  // node 1 — transverse tip load
            force_vec(0.0, 0.0, 0.0),  // node 2 (anchor) — no load
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]), // supports
    ];

    let fields = match call_tensegrity_load(&value_inputs) {
        ComputeOutcome::Completed { result, .. } => match result {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name, "TensegrityLoadResult",
                    "result should be a TensegrityLoadResult, got {:?}",
                    d.type_name
                );
                d.fields
            }
            other => panic!("Completed result should be a StructureInstance, got {other:?}"),
        },
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    };

    // displacements: one Vector3 per node (3 nodes).
    let displacements = match fields.get(&"displacements".to_string()) {
        Some(Value::List(ds)) => ds,
        other => panic!("displacements must be a List, got {other:?}"),
    };
    assert_eq!(displacements.len(), 3, "expected 3 nodal displacements");

    // The user-observable signal: u_y[1] = P_t·L / (2·N0).
    let u1 = vec3(&displacements[1]);
    let uy_expected = p_t * l / (2.0 * n0); // 0.01
    assert!(
        (u1[1] - uy_expected).abs() < 1e-9,
        "u_y[1] = {} expected P_t·L/(2·N0) = {uy_expected}",
        u1[1],
    );
    assert!((u1[0]).abs() < 1e-9, "u_x[1] = {} expected 0", u1[0]);
    assert!((u1[2]).abs() < 1e-9, "u_z[1] = {} expected 0", u1[2]);
    // Anchored nodes do not move.
    for (i, n) in [0usize, 2].iter().enumerate() {
        let u = vec3(&displacements[*n]);
        assert!(
            u.iter().all(|c| c.abs() < 1e-9),
            "anchor node {n} (idx {i}) must not move, got {u:?}",
        );
    }

    // member_forces: both cables stay at ≈ N0 (transverse load has zero axial
    // projection to first order).
    let member_forces = match fields.get(&"member_forces".to_string()) {
        Some(Value::List(fs)) => fs,
        other => panic!("member_forces must be a List, got {other:?}"),
    };
    assert_eq!(member_forces.len(), 2, "expected 2 member forces");
    for (i, f) in member_forces.iter().enumerate() {
        assert!(
            (coord(f) - n0).abs() < 1e-9,
            "member_forces[{i}] = {} expected ≈ N0 = {n0}",
            coord(f),
        );
    }

    // member_force_deltas: both ≈ 0.
    let deltas = match fields.get(&"member_force_deltas".to_string()) {
        Some(Value::List(ds)) => ds,
        other => panic!("member_force_deltas must be a List, got {other:?}"),
    };
    assert_eq!(deltas.len(), 2, "expected 2 member-force deltas");
    for (i, d) in deltas.iter().enumerate() {
        assert!(
            coord(d).abs() < 1e-9,
            "member_force_deltas[{i}] = {} expected ≈ 0",
            coord(d),
        );
    }

    // slack: no cable goes compressive ⇒ all false.
    assert_eq!(
        fields.get(&"slack".to_string()),
        Some(&Value::List(vec![Value::Bool(false), Value::Bool(false)])),
        "no cable should be slack under a transverse load",
    );

    // converged: true.
    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "a well-posed solve must report converged == true",
    );
}
