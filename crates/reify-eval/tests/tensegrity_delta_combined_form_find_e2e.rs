//! Tensegrity-membrane δ — `solver::form_find_free` combined struts+cables+membrane
//! form-finding end-to-end tests (task 4415).
//!
//! PRD: `docs/prds/v0_6/tensegrity-membrane.md` §4 M1b / D3 (δ).
//!
//! Tests the combined free-standing form-find path through the eval trampoline
//! (`solve_form_find_free_trampoline`), verifying:
//!
//! (a) **trampoline-unit** — craft a 5-input Value slice
//!     [structure(Tensegrity with struts+cables+surfaces), group_ids, seed_ratios,
//!     reference_group, surface_stresses], call `solve_form_find_free_trampoline`,
//!     and assert the `FormFindResult` has a non-empty `surface_stresses` echo,
//!     `converged == true`, struts compressive, cables tensile.
//!
//! (b) **inline-source compile-pipeline** — a small inline .ri source with a
//!     5-arg `form_find_free(prism, group_ids, seeds, reference_group, surface_stresses)`
//!     call compiles without Error diagnostics and evals to a `FormFindResult`
//!     via the `@optimized("solver::form_find_free")` ComputeNode dispatch.
//!
//! (c) **backward-compat** — a 4-arg `form_find_free` call (no surface_stresses)
//!     still yields the line-only solve with `converged == true` and an EMPTY
//!     `surface_stresses` echo (NEVER Undef/absent).
//!
//! RED signal (step-7):
//!   - (a) fails because `run_free` ignores the 5th input and calls `form_find_free`
//!     (not `form_find_free_surfaces`); the result's `surface_stresses` is empty.
//!   - (b) fails because the stdlib `form_find_free` only has 4 params; a 5-arg
//!     call triggers a compile-time arity error.
//!   - (c) passes (existing backward-compat behavior is already correct).

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

// ── helper types ──────────────────────────────────────────────────────────────

/// A Length-typed coordinate Scalar (SI metres) — how `point3(..m, ..)` lowers.
fn length(m: f64) -> Value {
    Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    }
}

/// A 3-component `Value::Point` node.
fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

/// Extract an f64 from a Length Scalar (or bare Real) coordinate component.
fn coord_f64(v: &Value) -> f64 {
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

// ── triplex prism with surfaces ───────────────────────────────────────────────

/// The canonical symmetric T-prism nodes (circumradius R=1, height=1, twist≈30°).
/// Top triangle z=+1: nodes 0,1,2 at azimuths 0°,120°,240°.
/// Bottom triangle z=−1: nodes 3,4,5 at azimuths 30°,150°,270° (≈+30° twist).
///
/// We use the same geometry as `canonical_prism()` in the kernel tests so the
/// combined solve starts from a near-symmetric geometry.
fn triplex_nodes() -> Vec<Value> {
    use std::f64::consts::PI;
    let deg = PI / 180.0;
    let top = |i: usize| {
        let a = 120.0 * (i as f64) * deg;
        node(a.cos(), a.sin(), 1.0)
    };
    let bot = |i: usize| {
        let a = (120.0 * (i as f64) + 30.0) * deg;
        node(a.cos(), a.sin(), -1.0)
    };
    vec![top(0), top(1), top(2), bot(0), bot(1), bot(2)]
}

/// Build the triplex Tensegrity Value WITH the `surfaces` field:
///   struts:   [[0,4],[1,5],[2,3]]
///   cables:   [[0,1],[1,2],[2,0],[3,4],[4,5],[5,3],[0,3],[1,4],[2,5]]
///   surfaces: [[0,1,2],[3,4,5]]   (top-cap + bottom-cap membrane)
fn triplex_tensegrity_with_surfaces() -> Value {
    let nodes = Value::List(triplex_nodes());
    let struts = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(4)]),
        Value::List(vec![Value::Int(1), Value::Int(5)]),
        Value::List(vec![Value::Int(2), Value::Int(3)]),
    ]);
    let cables = Value::List(vec![
        // top ring
        Value::List(vec![Value::Int(0), Value::Int(1)]),
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::List(vec![Value::Int(2), Value::Int(0)]),
        // bottom ring
        Value::List(vec![Value::Int(3), Value::Int(4)]),
        Value::List(vec![Value::Int(4), Value::Int(5)]),
        Value::List(vec![Value::Int(5), Value::Int(3)]),
        // verticals
        Value::List(vec![Value::Int(0), Value::Int(3)]),
        Value::List(vec![Value::Int(1), Value::Int(4)]),
        Value::List(vec![Value::Int(2), Value::Int(5)]),
    ]);
    let surfaces = Value::List(vec![
        // top cap: nodes 0,1,2
        Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]),
        // bottom cap: nodes 3,4,5
        Value::List(vec![Value::Int(3), Value::Int(4), Value::Int(5)]),
    ]);
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

/// Struts-then-cables group_ids: struts→0, six horizontals→1, three verticals→2.
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

/// Surface stresses: one uniform σ=0.2 per triangle (top and bottom caps).
fn two_triangle_stresses(sigma: f64) -> Value {
    Value::List(vec![Value::Real(sigma), Value::Real(sigma)])
}

// ── trampoline helper ─────────────────────────────────────────────────────────

/// Invoke `solve_form_find_free_trampoline` with the standard no-realization /
/// no-warm-state args. Mirrors `call_form_find_free` in tensegrity_t1b_form_find_e2e.rs.
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

/// Extract `FormFindResult.surface_stresses` as a list of f64 echoes.
/// Panics if the field is absent or not a `Value::List` — it must NEVER be Undef.
fn surface_stress_echoes(fields: &PersistentMap<String, Value>) -> Vec<f64> {
    match fields.get(&"surface_stresses".to_string()) {
        Some(Value::List(items)) => items.iter().map(coord_f64).collect(),
        other => panic!(
            "FormFindResult.surface_stresses must be a Value::List \
             (never Undef / absent), got {other:?}"
        ),
    }
}

// ── (a) trampoline-unit: 5-input combined solve ───────────────────────────────

/// (a) The 5-input combined path: trampoline reads the OPTIONAL 5th
/// `surface_stresses` input, calls `form_find_free_surfaces` instead of
/// `form_find_free`, and returns a `FormFindResult` with:
///   - `converged == true`
///   - struts (members 0..3) compressive (force < 0)
///   - cables (members 3..12) tensile (force > 0)
///   - `surface_stresses` echo non-empty (2 entries, each ≈ σ)
///
/// RED until step-8 extends `run_free` to read `value_inputs[4]` and route to
/// `form_find_free_surfaces` — the current trampoline ignores the 5th input and
/// calls `form_find_free`, which returns an empty `surface_stresses`.
#[test]
fn trampoline_combined_prism_membrane_has_nonempty_surface_stresses() {
    const SIGMA: f64 = 0.2;
    let value_inputs = vec![
        triplex_tensegrity_with_surfaces(),
        triplex_group_ids(),
        triplex_seeds(),
        Value::Int(1), // reference_group = horizontals
        two_triangle_stresses(SIGMA),
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
        other => panic!(
            "expected ComputeOutcome::Completed for the combined triplex+membrane, got {other:?}"
        ),
    };

    // converged == true.
    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "combined triplex+membrane solve must report converged == true"
    );

    // struts (members 0..3) compressive; cables (members 3..12) tensile.
    let forces = match fields.get(&"member_forces".to_string()) {
        Some(Value::List(fs)) => fs,
        other => panic!("FormFindResult.member_forces must be a List, got {other:?}"),
    };
    assert_eq!(forces.len(), 12, "expected 12 member forces (3 struts + 9 cables)");
    for (i, v) in forces.iter().enumerate().take(3) {
        let f = force_val(v);
        assert!(
            f < 0.0,
            "strut member_forces[{i}] must be compressive (< 0), got {f}"
        );
    }
    for (i, v) in forces.iter().enumerate().skip(3) {
        let f = force_val(v);
        assert!(
            f > 0.0,
            "cable member_forces[{i}] must be tensile (> 0), got {f}"
        );
    }

    // surface_stresses echo must be non-empty (2 entries, one per triangle, ≈ σ).
    // RED: the current trampoline returns empty surface_stresses.
    let stresses = surface_stress_echoes(&fields);
    assert_eq!(
        stresses.len(),
        2,
        "combined solve must echo 2 surface_stresses (one per triangle), got: {stresses:?}"
    );
    for (t, &s) in stresses.iter().enumerate() {
        assert!(
            (s - SIGMA).abs() < 1e-12,
            "surface_stresses[{t}] = {s}, expected echoed σ = {SIGMA}"
        );
    }
}

// ── (b) inline-source compile-pipeline: 5-arg form_find_free call ─────────────

/// (b) The stdlib `form_find_free` must accept a 5th optional `surface_stresses`
/// param so a combined form_find_free call compiles without arity errors.
///
/// RED until step-8 adds `surface_stresses : List<Real> = []` as the 5th param
/// to the `form_find_free` decl in `stdlib/tensegrity.ri` — the current 4-param
/// decl makes a 5-arg call a compile-time arity error.
#[test]
fn inline_source_five_arg_form_find_free_compiles_and_evals() {
    // The canonical triplex prism (circumradius 1, height 2, 30° twist) with two
    // horizontal cap triangles, expressed in DSL source.
    // Top nodes (z=+1): azimuth 0°, 120°, 240°.
    // Bottom nodes (z=−1): azimuth 30°, 150°, 270° (30° twist → crossing struts).
    // The 5th arg `surface_stresses` is a List<Real> with one σ per triangle.
    // With the 5th param defaulted to [] in the stdlib, this 5-arg call compiles
    // and evals through the ComputeNode dispatch to the combined kernel.
    const SOURCE: &str = r#"
structure def CombinedPrism {
    let prism = Tensegrity(
        nodes: [
            point3(1m, 0m, 1m),
            point3(-0.5m, 0.866m, 1m),
            point3(-0.5m, -0.866m, 1m),
            point3(0.866m, 0.5m, -1m),
            point3(-0.866m, 0.5m, -1m),
            point3(0m, -1m, -1m)
        ],
        struts: [[0, 4], [1, 5], [2, 3]],
        cables: [
            [0, 1], [1, 2], [2, 0],
            [3, 4], [4, 5], [5, 3],
            [0, 3], [1, 4], [2, 5]
        ],
        surfaces: [[0, 1, 2], [3, 4, 5]]
    )
    let gids     = [0, 0, 0,  1, 1, 1, 1, 1, 1,  2, 2, 2]
    let seeds    = [-1.0, 1.0, 1.0]
    let ref_grp  = 1
    let sigmas   = [0.2, 0.2]
    let form     = form_find_free(prism, gids, seeds, ref_grp, sigmas)
    let cvg      = form.converged
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);

    // Must compile without any Error-severity diagnostics.
    // RED: a 5-arg call to a 4-param function emits an arity Error.
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "form_find_free with 5 args should compile without Error diagnostics; \
         got {} error(s): {:#?}",
        errors.len(),
        errors,
    );

    // Eval through the ComputeNode dispatch.
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "expected no Error diagnostics at eval time, got: {eval_errors:#?}"
    );

    // The `form` cell must be a FormFindResult (not Undef / inline fallback).
    let form = eval_result
        .values
        .get(&ValueCellId::new("CombinedPrism", "form"))
        .unwrap_or_else(|| panic!("CombinedPrism.form cell missing from eval result"));
    match form {
        Value::StructureInstance(d) => assert_eq!(
            d.type_name, "FormFindResult",
            "form_find_free (5-arg) should return a FormFindResult; got {:?}",
            d.type_name
        ),
        other => panic!(
            "CombinedPrism.form should be a FormFindResult StructureInstance, got {other:?}"
        ),
    }

    // A ComputeNode with target == "solver::form_find_free" must exist in the graph
    // (proof the @optimized call lowered to a ComputeNode, not body-inlined).
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
}

// ── (c) backward-compat: 4-arg form_find_free still works ────────────────────

/// (c) The existing 4-arg `form_find_free` call (no surface_stresses) must still
/// return `converged == true` and an EMPTY `surface_stresses` echo — the
/// backward-compatibility invariant. This test should PASS even before step-8
/// (the 4-arg path is already green from task 3795).
#[test]
fn trampoline_four_arg_backward_compat_has_empty_surface_stresses() {
    // The triplex WITHOUT a surfaces field — the 4-arg line-only case.
    // We simply omit surfaces from the structure and send only 4 value_inputs.
    let nodes = Value::List(triplex_nodes());
    let struts = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(4)]),
        Value::List(vec![Value::Int(1), Value::Int(5)]),
        Value::List(vec![Value::Int(2), Value::Int(3)]),
    ]);
    let cables = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(1)]),
        Value::List(vec![Value::Int(1), Value::Int(2)]),
        Value::List(vec![Value::Int(2), Value::Int(0)]),
        Value::List(vec![Value::Int(3), Value::Int(4)]),
        Value::List(vec![Value::Int(4), Value::Int(5)]),
        Value::List(vec![Value::Int(5), Value::Int(3)]),
        Value::List(vec![Value::Int(0), Value::Int(3)]),
        Value::List(vec![Value::Int(1), Value::Int(4)]),
        Value::List(vec![Value::Int(2), Value::Int(5)]),
    ]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), nodes),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
        // no surfaces field → line-only path
    ]
    .into_iter()
    .collect();
    let tensegrity = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }));

    let value_inputs = vec![
        tensegrity,
        triplex_group_ids(),
        triplex_seeds(),
        Value::Int(1), // reference_group = horizontals
        // no 5th input — 4-arg backward-compat path
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
        other => panic!("expected ComputeOutcome::Completed for 4-arg triplex, got {other:?}"),
    };

    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "4-arg line-only triplex must report converged == true"
    );

    // The surface_stresses echo must be EMPTY — NEVER Undef / absent.
    let stresses = surface_stress_echoes(&fields);
    assert!(
        stresses.is_empty(),
        "4-arg line-only path must return empty surface_stresses echo, got {stresses:?}"
    );
}
