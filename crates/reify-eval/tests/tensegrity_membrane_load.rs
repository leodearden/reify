//! Producer-side acceptance signal for the `solver::membrane_load` ComputeNode
//! trampoline (task #4418 / η, layer M2 of
//! `docs/prds/v0_6/tensegrity-membrane.md` §5 / §10 / §11).
//!
//! Mirrors `tensegrity_t3b_load.rs`: crafts a pavilion `Tensegrity` Value plus the
//! 10-input membrane-load payload, calls `solve_membrane_load_trampoline`
//! directly, and asserts the `MembraneLoadResult` fields are all present and
//! populated with REAL (non-`Undef`) values — the G6 field-population invariant
//! that guards the historical `ElasticResult.{stress,displacement} = Undef` shape.
//! A registration test confirms `register_compute_fns` wires the target so
//! `engine.dispatch_compute_node("solver::membrane_load", …)` resolves to the
//! trampoline rather than the unregistered-target `Err`.
//!
//! # Result encoding contract (defined here, implemented in the trampoline)
//!
//! - `displacements`     : `List<Vector3<Length>>` — one per node.
//! - `member_forces`     : `List<Force>` — total axial force per line member
//!   (struts-then-cables order).
//! - `member_force_deltas`: `List<Force>` — per line member.
//! - `member_slack`      : `List<Bool>` — per line member.
//! - `surface_stress_deltas`: `List<List<Pressure>>` — per patch, each the three
//!   independent Voigt components `[Δσxx, Δσyy, Δσxy]`.
//! - `surface_principal_stresses`: `List<List<Pressure>>` — per patch, each the
//!   `[min, max]` principal pair of the total stress `σ₀·I + Δσ`.
//! - `surface_slack`     : `List<Bool>` — per patch.
//! - `converged`         : `Bool`.

use reify_core::DimensionVector;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::make_simple_engine;

// ---- Value-construction helpers (mirror tensegrity_t3b_load.rs) -------------

/// A Length-typed coordinate Scalar (SI metres).
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

/// A Pressure-typed Scalar (Young's modulus / membrane prestress).
fn pressure(p: f64) -> Value {
    Value::Scalar {
        si_value: p,
        dimension: DimensionVector::PRESSURE,
    }
}

/// An Area-typed Scalar (line-member cross-section).
fn area(a: f64) -> Value {
    Value::Scalar {
        si_value: a,
        dimension: DimensionVector::AREA,
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

/// Extract an f64 from a Scalar (any dimension) or a bare Real.
fn coord(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        other => panic!("expected a Scalar/Real component, got {other:?}"),
    }
}

/// Crack a 3-component displacement `Value::Vector`/`Value::Point` into `[f64; 3]`.
fn vec3(v: &Value) -> [f64; 3] {
    match v {
        Value::Vector(c) | Value::Point(c) if c.len() == 3 => {
            [coord(&c[0]), coord(&c[1]), coord(&c[2])]
        }
        other => panic!("expected a 3-component Vector, got {other:?}"),
    }
}

/// A combined pavilion `Tensegrity` value: a flat membrane patch braced by one
/// cable (up) and one strut (down) at the free center node, sharing node 2.
///
/// - node 0 = (1,0,0) membrane corner (anchored)
/// - node 1 = (0,1,0) membrane corner (anchored)
/// - node 2 = (0,0,0) free center (membrane + cable + strut)
/// - node 3 = (0,0,1) cable anchor (above)
/// - node 4 = (0,0,-1) strut anchor (below)
///
/// `surfaces = [(2,0,1)]`, `struts = [(2,4)]`, `cables = [(2,3)]`. The trampoline
/// cracks members struts-then-cables, so line-member index 0 is the strut and
/// index 1 the cable; the `prestress` payload must follow that order.
fn combined_pavilion() -> Value {
    let nodes = Value::List(vec![
        node(1.0, 0.0, 0.0),
        node(0.0, 1.0, 0.0),
        node(0.0, 0.0, 0.0),
        node(0.0, 0.0, 1.0),
        node(0.0, 0.0, -1.0),
    ]);
    let struts = Value::List(vec![Value::List(vec![Value::Int(2), Value::Int(4)])]);
    let cables = Value::List(vec![Value::List(vec![Value::Int(2), Value::Int(3)])]);
    let surfaces = Value::List(vec![Value::List(vec![
        Value::Int(2),
        Value::Int(0),
        Value::Int(1),
    ])]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), nodes),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
        ("surfaces".to_string(), surfaces),
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

/// The 10-input membrane-load payload for [`combined_pavilion`] under a downward
/// `-z` tip load at the free center node. The strut compresses, the cable
/// stretches (stays taut), and the flat patch deflects purely transverse (Δσ ≈ 0,
/// total stress stays the tensile prestress) — so nothing slackens and the solve
/// converges in one active-set pass.
fn combined_pavilion_payload() -> Vec<Value> {
    let n_strut = -1_000.0_f64;
    let n_cable = 3_000.0_f64;
    let e_bar = 2.0e9_f64;
    let a_bar = 1.0e-4_f64;
    let sigma = 1.0e5_f64; // membrane prestress [Pa]
    let t = 0.01_f64; // membrane thickness [m]
    let e_fab = 1.0e6_f64; // fabric Young's modulus [Pa]
    let nu_fab = 0.0_f64; // fabric Poisson ratio
    let p = 4_010.0_f64; // downward (-z) tip load [N]

    vec![
        combined_pavilion(),
        // [1] line prestress, struts-then-cables order.
        Value::List(vec![force(n_strut), force(n_cable)]),
        // [2] line youngs_modulus, [3] line area.
        pressure(e_bar),
        area(a_bar),
        // [4] per-node loads.
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 0.0, -p),
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        // [5] supports.
        Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(3),
            Value::Int(4),
        ]),
        // [6] per-triangle surface prestress (one patch).
        Value::List(vec![pressure(sigma)]),
        // [7] membrane thickness, [8] membrane youngs, [9] membrane poisson.
        length(t),
        pressure(e_fab),
        Value::Real(nu_fab),
    ]
}

/// Invoke the membrane-load trampoline with the standard no-realization /
/// no-warm-state / no-options args.
fn call_membrane_load(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::membrane_load::solve_membrane_load_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Pull the result `StructureInstance` field map out of a `Completed` outcome,
/// asserting it is a `MembraneLoadResult`.
fn completed_fields(outcome: ComputeOutcome) -> PersistentMap<String, Value> {
    match outcome {
        ComputeOutcome::Completed { result, .. } => match result {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name, "MembraneLoadResult",
                    "result should be a MembraneLoadResult, got {:?}",
                    d.type_name,
                );
                d.fields
            }
            other => panic!("Completed result should be a StructureInstance, got {other:?}"),
        },
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    }
}

/// Borrow a result field as a `List`, panicking (with the field name) if absent
/// or a different shape — also the non-`Undef` G6 guard.
fn list_field<'a>(fields: &'a PersistentMap<String, Value>, name: &str) -> &'a Vec<Value> {
    match fields.get(&name.to_string()) {
        Some(Value::List(items)) => items,
        other => panic!("field {name:?} must be a populated List, got {other:?}"),
    }
}

// ---- (1) Happy path + G6 field population -----------------------------------

#[test]
fn trampoline_combined_pavilion_solves() {
    let fields = completed_fields(call_membrane_load(&combined_pavilion_payload()));

    // displacements: one finite Vector3 per node (5 nodes); the free center (node
    // 2) moves toward the -z load, anchors do not move.
    let displacements = list_field(&fields, "displacements");
    assert_eq!(displacements.len(), 5, "one displacement per node");
    for (i, d) in displacements.iter().enumerate() {
        let u = vec3(d);
        assert!(
            u.iter().all(|c| c.is_finite()),
            "displacement[{i}] must be finite (non-Undef), got {u:?}",
        );
    }
    let u_center = vec3(&displacements[2]);
    assert!(u_center[2] < 0.0, "free center moves toward the -z load, got {u_center:?}");
    for &anchor in &[0usize, 1, 3, 4] {
        let u = vec3(&displacements[anchor]);
        assert!(
            u.iter().all(|c| c.abs() < 1e-9),
            "anchored node {anchor} must not move, got {u:?}",
        );
    }

    // member_forces / member_force_deltas: one real Force per line member (strut
    // index 0, cable index 1).
    let member_forces = list_field(&fields, "member_forces");
    assert_eq!(member_forces.len(), 2, "one member force per line member");
    for (i, f) in member_forces.iter().enumerate() {
        assert!(coord(f).is_finite(), "member_forces[{i}] must be finite, got {f:?}");
    }
    let member_force_deltas = list_field(&fields, "member_force_deltas");
    assert_eq!(member_force_deltas.len(), 2, "one delta per line member");
    for (i, d) in member_force_deltas.iter().enumerate() {
        assert!(
            coord(d).is_finite(),
            "member_force_deltas[{i}] must be finite, got {d:?}",
        );
    }
    // The cable (index 1) stays in tension (taut) under the downward pull.
    assert!(
        coord(&member_forces[1]) > 0.0,
        "cable (member 1) stays in tension, got {}",
        coord(&member_forces[1]),
    );

    // member_slack: per line member, none slack (cable stays taut; struts never
    // drop).
    assert_eq!(
        fields.get(&"member_slack".to_string()),
        Some(&Value::List(vec![Value::Bool(false), Value::Bool(false)])),
        "no line member should be slack",
    );

    // surface_stress_deltas: one entry per patch, each the three Voigt components
    // [Δσxx, Δσyy, Δσxy] as finite Pressure scalars (REAL, non-Undef).
    let stress_deltas = list_field(&fields, "surface_stress_deltas");
    assert_eq!(stress_deltas.len(), 1, "one stress delta per patch");
    for (p, ds) in stress_deltas.iter().enumerate() {
        match ds {
            Value::List(comps) => {
                assert_eq!(comps.len(), 3, "Δσ[{p}] encodes 3 Voigt components");
                for (k, c) in comps.iter().enumerate() {
                    assert!(coord(c).is_finite(), "Δσ[{p}][{k}] must be finite, got {c:?}");
                }
            }
            other => panic!("surface_stress_deltas[{p}] must be a List, got {other:?}"),
        }
    }

    // surface_principal_stresses: one [min, max] pair per patch, finite; the taut
    // patch keeps a positive minimum principal (no slack).
    let principals = list_field(&fields, "surface_principal_stresses");
    assert_eq!(principals.len(), 1, "one principal pair per patch");
    match &principals[0] {
        Value::List(pair) => {
            assert_eq!(pair.len(), 2, "principal pair is [min, max]");
            let (min, max) = (coord(&pair[0]), coord(&pair[1]));
            assert!(min.is_finite() && max.is_finite(), "principals must be finite");
            assert!(min <= max, "principal pair must be sorted [min, max], got [{min}, {max}]");
            assert!(min > 0.0, "taut patch keeps a positive minimum principal, got {min}");
        }
        other => panic!("surface_principal_stresses[0] must be a List, got {other:?}"),
    }

    // surface_slack: per patch, none slack.
    assert_eq!(
        fields.get(&"surface_slack".to_string()),
        Some(&Value::List(vec![Value::Bool(false)])),
        "the flat transverse-loaded patch should not slacken",
    );

    // converged: true.
    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "a well-posed combined solve must report converged == true",
    );
}

// ---- (2) Registration / dispatch --------------------------------------------

#[test]
fn solver_membrane_load_target_is_registered() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let value_inputs = combined_pavilion_payload();
    let dispatch =
        engine.dispatch_compute_node("solver::membrane_load", &value_inputs, &[], &Value::Undef, None);

    match dispatch {
        Ok((result, _diags)) => match result {
            Value::StructureInstance(d) => assert_eq!(
                d.type_name, "MembraneLoadResult",
                "registered solver::membrane_load trampoline should return a \
                 MembraneLoadResult, got {:?}",
                d.type_name,
            ),
            other => panic!("expected a MembraneLoadResult StructureInstance, got {other:?}"),
        },
        Err(diags) => {
            let joined = diags
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            panic!(
                "solver::membrane_load must be a registered ComputeNode target, but \
                 dispatch returned the unregistered-target Err: {joined}"
            );
        }
    }
}
