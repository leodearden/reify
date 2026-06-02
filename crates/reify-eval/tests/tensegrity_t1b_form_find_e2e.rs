//! Tensegrity T1b — `solver::form_find_free` free-standing Force-Density form-finding.
//!
//! PRD: `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1b. This is
//! the free-standing variant of the T1a anchored form-finder: no fixed anchors,
//! a self-stressed equilibrium found via the adaptive GroupRatios eigenvalue search.
//!
//! Kernel topology (canonical triangular T-prism / triplex):
//!   - 6 nodes (top triangle z=1: nodes 0,1,2; bottom triangle z=0: nodes 3,4,5)
//!   - 3 struts: (0,4), (1,5), (2,3)   — long crossing diagonals
//!   - 9 cables: top (0,1),(1,2),(2,0); bottom (3,4),(4,5),(5,3); vertical (0,3),(1,4),(2,5)
//!   - group_ids: struts→0, six horizontals→1, verticals→2
//!   - seeds: [-1.0, 1.0, 1.0], reference_group: 1
//!   - closed-form q: struts ≈−√3, horizontals=1, verticals ≈+√3
//!
//! Test layers (TDD order, matching the plan):
//!   step-3  — trampoline-unit tests (crafted Values, no compile pipeline) — RED
//!   step-5  — end-to-end over examples/tensegrity_t_prism.ri — RED

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

// ── canonical triplex geometry ────────────────────────────────────────────────

/// A Length-typed coordinate Scalar (SI metres).
fn length(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
}

/// A 3-component `Value::Point` node.
fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

/// Extract an f64 from a Length Scalar (or bare Real) coordinate component.
fn coord(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        other => panic!("expected a Scalar/Real coordinate, got {other:?}"),
    }
}

/// Extract an f64 from a Force Scalar (or bare Real) force value.
fn force_val(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        other => panic!("expected a Scalar/Real force, got {other:?}"),
    }
}

/// The canonical symmetric triplex prism (circumradius R=1, height=1, twist=30°).
/// Node order: top 0,1,2 at z=1 (azimuth 120°·i); bottom 3,4,5 at z=0 (azimuth 120°·i+30°).
/// These are the exact coordinates of `canonical_prism()` in the kernel test.
fn canonical_prism_nodes() -> Vec<Value> {
    use std::f64::consts::PI;
    let deg = PI / 180.0;
    let top = |i: usize| {
        let a = 120.0 * (i as f64) * deg;
        node(a.cos(), a.sin(), 1.0)
    };
    let bot = |i: usize| {
        let a = (120.0 * (i as f64) + 30.0) * deg;
        node(a.cos(), a.sin(), 0.0)
    };
    vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
}

/// Build the triplex Tensegrity Value with the kernel topology:
///   struts:  [[0,4],[1,5],[2,3]]
///   cables:  [[0,1],[1,2],[2,0],[3,4],[4,5],[5,3],[0,3],[1,4],[2,5]]
fn triplex_tensegrity() -> Value {
    let nodes = Value::List(canonical_prism_nodes());
    let struts = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(4)]),
        Value::List(vec![Value::Int(1), Value::Int(5)]),
        Value::List(vec![Value::Int(2), Value::Int(3)]),
    ]);
    let cables = Value::List(vec![
        // top
        Value::List(vec![Value::Int(0), Value::Int(1)]),
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::List(vec![Value::Int(2), Value::Int(0)]),
        // bottom
        Value::List(vec![Value::Int(3), Value::Int(4)]),
        Value::List(vec![Value::Int(4), Value::Int(5)]),
        Value::List(vec![Value::Int(5), Value::Int(3)]),
        // vertical
        Value::List(vec![Value::Int(0), Value::Int(3)]),
        Value::List(vec![Value::Int(1), Value::Int(4)]),
        Value::List(vec![Value::Int(2), Value::Int(5)]),
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

/// Struts-then-cables group_ids: struts→0, six horizontals→1, verticals→2.
fn triplex_group_ids() -> Value {
    Value::List(vec![
        Value::Int(0),
        Value::Int(0),
        Value::Int(0), // struts
        Value::Int(1),
        Value::Int(1),
        Value::Int(1), // top horizontals
        Value::Int(1),
        Value::Int(1),
        Value::Int(1), // bottom horizontals
        Value::Int(2),
        Value::Int(2),
        Value::Int(2), // verticals
    ])
}

/// Seed ratios: struts compressive (−1), horizontals/verticals tensile (+1).
fn triplex_seeds() -> Value {
    Value::List(vec![Value::Real(-1.0), Value::Real(1.0), Value::Real(1.0)])
}

/// Invoke `solve_form_find_free_trampoline` with the standard no-realization /
/// no-warm-state args.
fn call_form_find_free(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::form_find::solve_form_find_free_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

// ── step-3: trampoline-unit tests ─────────────────────────────────────────────

/// (a) Happy path: the trampoline cracks the Tensegrity / group_ids / seed_ratios
/// / reference_group Values, runs the adaptive GroupRatios search, and returns
/// `Completed` with a `FormFindResult` whose:
///   - `converged` == Bool(true)
///   - `nodes` is a 6-element List of 3-component Points
///   - `member_forces[0..3]` (struts) < 0 (compressive)
///   - `member_forces[3..12]` (cables) > 0 (tensile)
///   - `force_densities[0..3]` ≈ −√3 (within 1e-6)
///   - `force_densities[3..9]` ≈ +1 (reference, within 1e-12)
///   - `force_densities[9..12]` ≈ +√3 (within 1e-6)
///
/// Numeric bounds are backed by the landed kernel test
/// `group_ratios_search_recovers_closed_form_prism_relative_q`.
#[test]
fn trampoline_happy_path_solves_triplex_prism() {
    let value_inputs = vec![
        triplex_tensegrity(),
        triplex_group_ids(),
        triplex_seeds(),
        Value::Int(1), // reference_group
    ];

    let fields = match call_form_find_free(&value_inputs) {
        ComputeOutcome::Completed { result, .. } => match result {
            Value::StructureInstance(d) => {
                assert_eq!(
                    d.type_name, "FormFindResult",
                    "result should be a FormFindResult, got {:?}",
                    d.type_name
                );
                d.fields
            }
            other => panic!("Completed result should be a StructureInstance, got {other:?}"),
        },
        other => panic!("expected ComputeOutcome::Completed for the triplex prism, got {other:?}"),
    };

    // converged == true
    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "triplex GroupRatios solve must report converged == true"
    );

    // nodes: 6 Points
    let nodes = match fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => ns,
        other => panic!("FormFindResult.nodes must be a List, got {other:?}"),
    };
    assert_eq!(nodes.len(), 6, "expected 6 recovered nodes for the triplex prism");
    for (i, n) in nodes.iter().enumerate() {
        match n {
            Value::Point(c) if c.len() == 3 => {}
            other => panic!("nodes[{i}] must be a 3-component Point, got {other:?}"),
        }
    }

    // member_forces: 12 entries (3 struts + 9 cables); signs must hold.
    let forces = match fields.get(&"member_forces".to_string()) {
        Some(Value::List(fs)) => fs,
        other => panic!("FormFindResult.member_forces must be a List, got {other:?}"),
    };
    assert_eq!(forces.len(), 12, "expected 12 member forces (3 struts + 9 cables)");
    for i in 0..3 {
        let f = force_val(&forces[i]);
        assert!(f < 0.0, "strut member_forces[{i}] must be compressive (< 0), got {f}");
    }
    for i in 3..12 {
        let f = force_val(&forces[i]);
        assert!(f > 0.0, "cable member_forces[{i}] must be tensile (> 0), got {f}");
    }

    // force_densities: 12 entries; closed-form values (backed by kernel test).
    let fds = match fields.get(&"force_densities".to_string()) {
        Some(Value::List(fds)) => fds,
        other => panic!("FormFindResult.force_densities must be a List, got {other:?}"),
    };
    assert_eq!(fds.len(), 12, "expected 12 force densities (3 struts + 9 cables)");
    let sqrt3 = 3.0_f64.sqrt();
    for i in 0..3 {
        let q = coord(&fds[i]);
        assert!(
            (q - (-sqrt3)).abs() < 1e-6,
            "strut force_densities[{i}] must be ≈ −√3, got {q}"
        );
    }
    for i in 3..9 {
        let q = coord(&fds[i]);
        assert!(
            (q - 1.0).abs() < 1e-12,
            "horizontal force_densities[{i}] (reference group) must be = 1, got {q}"
        );
    }
    for i in 9..12 {
        let q = coord(&fds[i]);
        assert!(
            (q - sqrt3).abs() < 1e-6,
            "vertical force_densities[{i}] must be ≈ +√3, got {q}"
        );
    }
}

/// (b) Infeasible: all members tagged Cable with all-positive seeds → the
/// adaptive GroupRatios search cannot reach nullity 4 → `Failed` carrying an
/// `E_FormFindInfeasible` diagnostic (SearchDidNotConverge).
#[test]
fn trampoline_all_positive_seeds_is_failed_infeasible() {
    // All 12 members as "cables" (all-positive group assignments): struts get
    // group 0 with seed +1 instead of −1. This keeps D a connected Laplacian
    // (nullity exactly 1 for any positive q), so the search cannot find nullity 4.
    let all_cable_group_ids = Value::List(vec![
        Value::Int(0),
        Value::Int(0),
        Value::Int(0), // would-be struts
        Value::Int(1),
        Value::Int(1),
        Value::Int(1),
        Value::Int(1),
        Value::Int(1),
        Value::Int(1),
        Value::Int(2),
        Value::Int(2),
        Value::Int(2),
    ]);
    let all_positive_seeds = Value::List(vec![
        Value::Real(1.0), // group 0: positive (no compression)
        Value::Real(1.0),
        Value::Real(1.0),
    ]);

    // Build a Tensegrity where every member is tagged as a cable (positive seeds).
    // We reuse triplex_tensegrity() but pass all-cable group ids + positive seeds.
    let value_inputs = vec![
        triplex_tensegrity(),
        all_cable_group_ids,
        all_positive_seeds,
        Value::Int(1), // reference_group
    ];

    match call_form_find_free(&value_inputs) {
        ComputeOutcome::Failed { diagnostics } => assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("E_FormFindInfeasible")),
            "expected an E_FormFindInfeasible diagnostic, got: {diagnostics:?}"
        ),
        other => panic!(
            "expected ComputeOutcome::Failed for all-positive seeds, got {other:?}"
        ),
    }
}

/// (c) Short value_inputs guard: fewer than 4 inputs → Failed with a located
/// "expects 4 inputs" diagnostic rather than an index-out-of-bounds panic.
#[test]
fn trampoline_short_value_inputs_is_failed() {
    let value_inputs = vec![
        triplex_tensegrity(),
        triplex_group_ids(),
        // seed_ratios and reference_group omitted → only 2 inputs
    ];
    match call_form_find_free(&value_inputs) {
        ComputeOutcome::Failed { diagnostics } => {
            let joined: String =
                diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join(" | ");
            assert!(
                joined.contains("E_FormFindInfeasible"),
                "expected E_FormFindInfeasible in short-inputs diagnostic, got: {joined}"
            );
            assert!(
                joined.contains("4 inputs") || joined.contains("4"),
                "expected mention of 4 inputs in diagnostic, got: {joined}"
            );
        }
        other => panic!("expected ComputeOutcome::Failed for short inputs, got {other:?}"),
    }
}

// ── step-5: end-to-end over examples/tensegrity_t_prism.ri ───────────────────

/// The T-prism example source (updated in step-6 to include form_find_free).
/// `include_str!` makes a compile error if the file is missing, but the e2e
/// tests below will fail RED until step-6 extends the example.
fn t_prism_source() -> &'static str {
    include_str!("../../../examples/tensegrity_t_prism.ri")
}

/// Crack a `FormFindResult` StructureInstance into its typed components.
fn crack_form_find_result(v: &Value) -> (&PersistentMap<String, Value>, bool) {
    match v {
        Value::StructureInstance(d) if d.type_name == "FormFindResult" => {
            let converged = matches!(
                d.fields.get(&"converged".to_string()),
                Some(Value::Bool(true))
            );
            (&d.fields, converged)
        }
        other => panic!(
            "expected a FormFindResult StructureInstance, got {other:?}"
        ),
    }
}

/// (a) End-to-end: the T-prism example compiles, `@optimized("solver::form_find_free")`
/// lowers to a ComputeNode (not body-inlined), and the trampoline solves the
/// free-standing prism with correct force signs and recovered q.
///
/// RED until step-6 extends examples/tensegrity_t_prism.ri with the full 9-cable
/// topology and form_find_free call.
#[test]
fn e2e_t_prism_lowers_to_compute_node_and_solves() {
    let compiled = compile_source_with_stdlib(t_prism_source());

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No Error-severity diagnostics from the full compile + eval pipeline.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no Error diagnostics, got: {errors:#?}");

    // A ComputeNode with target == "solver::form_find_free" must be in the graph —
    // proof the @optimized call lowered to a ComputeNode and was NOT inlined.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let targets: Vec<&str> = snapshot
        .graph
        .compute_nodes
        .iter()
        .map(|(_, d)| d.target.as_str())
        .collect();
    assert!(
        targets.contains(&"solver::form_find_free"),
        "expected a ComputeNode with target==\"solver::form_find_free\"; found {targets:?}"
    );

    // TPrism.form must be a solved FormFindResult.
    let form = eval_result
        .values
        .get(&ValueCellId::new("TPrism", "form"))
        .unwrap_or_else(|| panic!("TPrism.form cell missing from eval result"));
    let (fields, converged) = crack_form_find_result(form);
    assert!(converged, "free-standing triplex form-find must report converged == true");

    // nodes: 6 entries
    let nodes = match fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => ns,
        other => panic!("FormFindResult.nodes must be a List, got {other:?}"),
    };
    assert_eq!(nodes.len(), 6, "expected 6 solved nodes for the triplex prism");

    // member_forces: 12 entries; struts compressive, cables tensile.
    let forces = match fields.get(&"member_forces".to_string()) {
        Some(Value::List(fs)) => fs,
        other => panic!("FormFindResult.member_forces must be a List, got {other:?}"),
    };
    assert_eq!(forces.len(), 12, "expected 12 member forces (3 struts + 9 cables)");
    for i in 0..3 {
        let f = force_val(&forces[i]);
        assert!(f < 0.0, "strut member_forces[{i}] must be compressive (< 0), got {f}");
    }
    for i in 3..12 {
        let f = force_val(&forces[i]);
        assert!(f > 0.0, "cable member_forces[{i}] must be tensile (> 0), got {f}");
    }

    // force_densities: closed-form values ≈ {struts −√3, horizontals +1, verticals +√3}.
    let fds = match fields.get(&"force_densities".to_string()) {
        Some(Value::List(fds)) => fds,
        other => panic!("FormFindResult.force_densities must be a List, got {other:?}"),
    };
    assert_eq!(fds.len(), 12, "expected 12 force densities");
    let sqrt3 = 3.0_f64.sqrt();
    for i in 0..3 {
        let q = coord(&fds[i]);
        assert!(
            (q - (-sqrt3)).abs() < 1e-6,
            "strut force_densities[{i}] ≈ −√3, got {q}"
        );
    }
    for i in 3..9 {
        let q = coord(&fds[i]);
        assert!(
            (q - 1.0).abs() < 1e-12,
            "horizontal force_densities[{i}] (reference) = 1, got {q}"
        );
    }
    for i in 9..12 {
        let q = coord(&fds[i]);
        assert!(
            (q - sqrt3).abs() < 1e-6,
            "vertical force_densities[{i}] ≈ +√3, got {q}"
        );
    }
}
