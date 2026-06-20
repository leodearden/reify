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
//! [2] youngs_modulus  : Pressure                (broadcast E, shared section)
//! [3] area            : Area                    (broadcast A, shared section)
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
use reify_test_support::make_simple_engine;

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
            force_vec(0.0, 0.0, 0.0), // node 0 (anchor) — no load
            force_vec(0.0, p_t, 0.0), // node 1 — transverse tip load
            force_vec(0.0, 0.0, 0.0), // node 2 (anchor) — no load
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

// ── step-11: slack-mask encoding + failure-path coverage ─────────────────────
//
// These target the three step-12 deliverables: (1) the real slack mask + zeroed
// slack-cable force in the result; (2) trampoline-level range/length guards; and
// (3) the per-variant `describe()` phrase mapping. Each failure test checks a
// guard-specific phrase (not just the shared `E_TensegrityLoadInfeasible`
// prefix), mirroring the T1a `assert_failed_infeasible` discipline.

/// Assert the outcome is `Failed` with an `E_TensegrityLoadInfeasible`
/// diagnostic whose message also contains `needle` (proving the specific guard /
/// `describe()` arm fired, not merely *some* infeasibility).
fn assert_failed_infeasible(outcome: ComputeOutcome, needle: &str) {
    match outcome {
        ComputeOutcome::Failed { diagnostics } => {
            let joined = diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            assert!(
                joined.contains("E_TensegrityLoadInfeasible"),
                "expected an E_TensegrityLoadInfeasible diagnostic, got: {joined}"
            );
            assert!(
                joined.contains(needle),
                "expected the diagnostic to mention {needle:?}, got: {joined}"
            );
        }
        other => panic!("expected ComputeOutcome::Failed, got {other:?}"),
    }
}

/// (a) Slackening axial load: an axial tip load `P = 3·N0` drives cable `(1,2)`
/// compressive, so the tension-only active set drops it. The result must flag
/// `slack[1] == true` with a zeroed `member_forces[1]`, the reduced single-cable
/// deflection `u_x[1] = P·L/(E·A)`, and the surviving cable carrying `N0 + P`.
/// Mirrors the kernel golden `slackening_cable_axial_load`, but through the
/// trampoline's Value encoding.
#[test]
fn trampoline_slackening_axial_load_flags_slack() {
    let l = 2.0_f64;
    let e = 200.0e9_f64;
    let a = 1.0e-4_f64;
    let n0 = 5_000.0_f64;
    let p = 3.0 * n0;

    let value_inputs = vec![
        two_cable_string(l),
        Value::List(vec![force(n0), force(n0)]),
        pressure(e),
        area(a),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(p, 0.0, 0.0), // axial tip load toward node 2
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]),
    ];

    let fields = match call_tensegrity_load(&value_inputs) {
        ComputeOutcome::Completed { result, .. } => match result {
            Value::StructureInstance(d) => d.fields,
            other => panic!("Completed result should be a StructureInstance, got {other:?}"),
        },
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    };

    // Slack mask flags the dropped cable (1,2); cable (0,1) stays taut.
    assert_eq!(
        fields.get(&"slack".to_string()),
        Some(&Value::List(vec![Value::Bool(false), Value::Bool(true)])),
        "cable (1,2) must be flagged slack, cable (0,1) taut",
    );

    // Slack cable reports zero force; surviving cable carries N0 + P.
    let member_forces = match fields.get(&"member_forces".to_string()) {
        Some(Value::List(fs)) => fs,
        other => panic!("member_forces must be a List, got {other:?}"),
    };
    assert!(
        coord(&member_forces[1]).abs() < 1e-6,
        "slack cable (1,2) must report zero force, got {}",
        coord(&member_forces[1]),
    );
    assert!(
        (coord(&member_forces[0]) - (n0 + p)).abs() < 1e-6,
        "taut cable (0,1) must carry N0 + P = {}, got {}",
        n0 + p,
        coord(&member_forces[0]),
    );

    // Reduced single-cable deflection u_x[1] = P·L/(E·A), NOT P·L/(2EA).
    let displacements = match fields.get(&"displacements".to_string()) {
        Some(Value::List(ds)) => ds,
        other => panic!("displacements must be a List, got {other:?}"),
    };
    let ux1 = vec3(&displacements[1])[0];
    let ux_expected = p * l / (e * a); // 0.0015
    assert!(
        (ux1 - ux_expected).abs() < 1e-9,
        "u_x[1] = {ux1} expected P·L/(EA) = {ux_expected}",
    );

    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "post-drop solve must converge",
    );
}

/// (b1) Every node listed as a support ⇒ empty free set: the kernel has nothing
/// to solve for and the trampoline must surface a clean diagnostic.
#[test]
fn trampoline_all_anchored_is_failed_empty_free_set() {
    let value_inputs = vec![
        two_cable_string(2.0),
        Value::List(vec![force(5_000.0), force(5_000.0)]),
        pressure(200.0e9),
        area(1.0e-4),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]), // all anchored
    ];
    assert_failed_infeasible(
        call_tensegrity_load(&value_inputs),
        "every node is anchored",
    );
}

/// (b2) prestress shorter than the member count (2 cables, 1 prestress) is a
/// dimension mismatch — the trampoline must reject it rather than silently
/// truncating to a one-member solve.
#[test]
fn trampoline_prestress_count_mismatch_is_failed() {
    let value_inputs = vec![
        two_cable_string(2.0),
        Value::List(vec![force(5_000.0)]), // only 1 prestress for 2 cables
        pressure(200.0e9),
        area(1.0e-4),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 50.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]),
    ];
    assert_failed_infeasible(call_tensegrity_load(&value_inputs), "member count");
}

/// (b3) A support index past the node array is rejected by the trampoline's own
/// range check, with the offending index located in the message.
#[test]
fn trampoline_out_of_range_support_is_failed() {
    let value_inputs = vec![
        two_cable_string(2.0), // 3 nodes ⇒ valid indices 0..3
        Value::List(vec![force(5_000.0), force(5_000.0)]),
        pressure(200.0e9),
        area(1.0e-4),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 50.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(99)]), // 99 out of range
    ];
    assert_failed_infeasible(call_tensegrity_load(&value_inputs), "out of range");
}

/// (b4) The section scalars carry SI units — `youngs_modulus` is a Pressure and
/// `area` is an Area. Swapping the two positionally-adjacent arguments (a
/// Pressure where an Area is expected and vice versa) must surface a located
/// `E_TensegrityLoadInfeasible` "wrong unit" diagnostic rather than silently
/// solving a physically wrong problem — the dimension guard the relaxed
/// bare-f64 cracker used to skip. `area(a)` sits in the `youngs_modulus` slot,
/// so the Pressure check fires first.
#[test]
fn trampoline_swapped_section_units_is_failed() {
    let value_inputs = vec![
        two_cable_string(2.0),
        Value::List(vec![force(5_000.0), force(5_000.0)]),
        area(1.0e-4),      // an Area where youngs_modulus (Pressure) is expected
        pressure(200.0e9), // a Pressure where area (Area) is expected
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 50.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]),
    ];
    assert_failed_infeasible(call_tensegrity_load(&value_inputs), "wrong unit");
}

// ── step-13: dedicated solver::tensegrity_load target registration ───────────
//
// PRD §11 Q2: the load solver is wired as its OWN ComputeNode target (not an
// extension of solver::elastic_static). This pins that, after the slice's
// `register_compute_fns`, an engine dispatch of a "solver::tensegrity_load"
// ComputeNode resolves to the trampoline (Ok) instead of falling through to the
// unregistered-target Err — the registered-vs-unregistered discrimination from
// `compute_dispatch_registry.rs`.

/// After `register_compute_fns`, dispatching a `solver::tensegrity_load`
/// ComputeNode must NOT yield the unregistered-target `Err`: the dedicated
/// target is wired and resolves to the trampoline, which returns a
/// `TensegrityLoadResult`. A valid 6-input payload (the no-slack golden) is used
/// so a *registered* target returns `Completed → Ok`; an *unregistered* target
/// would instead return `Err` naming `solver::tensegrity_load`.
#[test]
fn solver_tensegrity_load_target_is_registered() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let value_inputs = vec![
        two_cable_string(2.0),
        Value::List(vec![force(5_000.0), force(5_000.0)]),
        pressure(200.0e9),
        area(1.0e-4),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),
            force_vec(0.0, 50.0, 0.0),
            force_vec(0.0, 0.0, 0.0),
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]),
    ];

    let dispatch = engine.dispatch_compute_node(
        "solver::tensegrity_load",
        &value_inputs,
        &[],
        &Value::Undef,
        None,
    );

    match dispatch {
        Ok((result, _diags)) => match result {
            Value::StructureInstance(d) => assert_eq!(
                d.type_name, "TensegrityLoadResult",
                "registered solver::tensegrity_load trampoline should return a \
                 TensegrityLoadResult, got {:?}",
                d.type_name,
            ),
            other => {
                panic!("expected a TensegrityLoadResult StructureInstance, got {other:?}")
            }
        },
        Err(diags) => {
            let joined = diags
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            panic!(
                "solver::tensegrity_load must be a registered ComputeNode target, but \
                 dispatch returned the unregistered-target Err: {joined}"
            );
        }
    }
}

// ── step-19: disconnected-free-node never-panic regression guard ─────────────
//
// The review's explicit producer-side ask (robustness_panic_on_valid_input): a
// Tensegrity carrying a free ORPHAN node must reach the kernel (clearing every
// trampoline guard) and come back as a clean `Failed` diagnostic, NOT a panic.
// The 6-input payload is well-formed (prestress len 2 == members 2, loads len 4
// == nodes 4, supports in range), so it reaches the kernel, whose step-16
// up-front guard returns `SingularSystem`; the existing `run()`/`describe()`
// mapping turns that into an `E_TensegrityLoadInfeasible` "singular tangent
// system" `Failed` outcome with NO reify-eval production change. RED against the
// original unfixed code (the orphan panics through the uncatchable trampoline);
// GREEN once step-16 lands — this locks the never-panic-on-valid-input contract
// end-to-end.

/// Collinear two-cable string with a FREE ORPHAN node 3 at `(5L, 5L, 0)` that is
/// touched by no member and absent from the supports: `anchor(0) — free(1) —
/// anchor(2)` plus the isolated node 3. `struts: []`, cables `[[0,1],[1,2]]`.
fn two_cable_string_with_orphan(l: f64) -> Value {
    let nodes = Value::List(vec![
        node(0.0, 0.0, 0.0),         // 0 — anchor
        node(l, 0.0, 0.0),           // 1 — free + cabled
        node(2.0 * l, 0.0, 0.0),     // 2 — anchor
        node(5.0 * l, 5.0 * l, 0.0), // 3 — FREE ORPHAN: no member, not a support
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

/// A free orphan node (referenced by no member, not a support) must surface as
/// an `E_TensegrityLoadInfeasible` "singular tangent system" `Failed` outcome
/// rather than panicking through the uncatchable trampoline. The 6-input payload
/// clears every trampoline guard (prestress len 2 == members 2, loads len 4 ==
/// nodes 4, supports in range 0..4), so the kernel is actually reached, and its
/// step-16 up-front guard returns `SingularSystem`.
#[test]
fn trampoline_disconnected_free_node_is_failed_not_panic() {
    let l = 2.0_f64;
    let value_inputs = vec![
        two_cable_string_with_orphan(l),
        Value::List(vec![force(5_000.0), force(5_000.0)]), // 2 prestress == 2 cables
        pressure(200.0e9),
        area(1.0e-4),
        Value::List(vec![
            force_vec(0.0, 0.0, 0.0),  // node 0 (anchor)
            force_vec(0.0, 50.0, 0.0), // node 1 (free) — transverse load
            force_vec(0.0, 0.0, 0.0),  // node 2 (anchor)
            force_vec(0.0, 0.0, 0.0),  // node 3 (orphan) — unloaded
        ]),
        Value::List(vec![Value::Int(0), Value::Int(2)]), // node 3 deliberately free
    ];
    assert_failed_infeasible(
        call_tensegrity_load(&value_inputs),
        "singular tangent system",
    );
}
