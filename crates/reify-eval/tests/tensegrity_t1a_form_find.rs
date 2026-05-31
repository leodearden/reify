//! Tensegrity T1a — `solver::form_find` anchored Force-Density form-finding.
//!
//! PRD: `docs/prds/v0_6/tensegrity-structures.md` §4 / Tier-1 leaf T1a. This is
//! the first real consumer slice through the landed ComputeNode seam (GR-002):
//! `@optimized("solver::form_find")` lowers to a ComputeNode whose trampoline
//! cracks the Tensegrity / force-densities / anchors `Value`s, calls the pure
//! FD kernel in `reify-solver-elastic`, and rebuilds a `FormFindResult`.
//!
//! Test layers (TDD order):
//!   step-7  — stdlib declaration of `form_find` / `FormFindResult` type-checks
//!   step-9  — trampoline-unit tests (crafted Values, no compile pipeline)
//!   step-11 — end-to-end + cache-hit + CLI smoke over the cable-net example

use reify_core::{DimensionVector, ValueCellId};
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

// ── step-7: stdlib declaration type-checks ───────────────────────────────────

/// `form_find(structure, force_densities, anchors) -> FormFindResult` and the
/// `FormFindResult.nodes` projection must be declared in the stdlib. Free node 0
/// is cabled to four anchors; all three call args are let-bound (the ComputeNode
/// shallow-walk capture contract — see step-12), though here we only require the
/// source to compile and the call to resolve to a `FormFindResult`.
///
/// RED→GREEN signal: Reify resolves an *undeclared* call leniently to `Undef`
/// (no Error diagnostic — only a benign empty-list warning for `struts: []`), so
/// "no Error diagnostics" alone can never fail. The real signal is the eval
/// result: with `form_find` declared but no trampoline registered here, the
/// `@optimized` call body-inlines its `FormFindResult()` fallback
/// (`engine_eval.rs` only dispatches a ComputeNode when a trampoline exists), so
/// `form` is a `FormFindResult` instance. While `form_find` is undeclared the
/// call is `Undef` and the match below fails RED.
#[test]
fn form_find_stdlib_declaration_type_checks() {
    const SOURCE: &str = r#"
structure def F {
    let t = Tensegrity(
        nodes: [
            point3(0m, 0m, 0m),
            point3(1m, 0m, 0m),
            point3(-1m, 0m, 0m),
            point3(0m, 1m, 1m),
            point3(0m, -1m, 1m)
        ],
        struts: [],
        cables: [[0, 1], [0, 2], [0, 3], [0, 4]]
    )
    let q = [1.0, 1.0, 1.0, 1.0]
    let a = [1, 2, 3, 4]
    let form = form_find(t, q, a)
    let ns = form.nodes
}
"#;

    let compiled = compile_source_with_stdlib(SOURCE);

    // Invariant: declaring `form_find` / `FormFindResult` must not introduce any
    // Error-severity diagnostic. (This cannot go RED on its own — an undeclared
    // call is lenient — but it guards against a malformed step-8 stdlib edit.)
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "form_find / FormFindResult stdlib declaration should compile without \
         Error-severity diagnostics; got {} error(s): {:#?}",
        errors.len(),
        errors,
    );

    // Signal: `form` resolves to a `FormFindResult` instance (inline-body
    // fallback, since no trampoline is registered in this test).
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let form = result
        .values
        .get(&ValueCellId::new("F", "form"))
        .unwrap_or_else(|| panic!("F.form cell missing from eval result"));
    match form {
        // The bare `FormFindResult()` inline-fallback ctor leaves the (no-default)
        // params absent, so we assert only on the resolved type — that proves
        // both `form_find` and its `FormFindResult` return type are declared. The
        // `form.nodes` projection in the source compiling without an Error
        // diagnostic (checked above) covers the `nodes` field declaration; the
        // field is populated with real data in the step-9 / step-11 trampoline
        // tests.
        Value::StructureInstance(data) => assert_eq!(
            data.type_name, "FormFindResult",
            "form_find should return a FormFindResult; got StructureInstance {:?}",
            data.type_name,
        ),
        other => panic!(
            "form_find(t, q, a) should evaluate to a FormFindResult StructureInstance \
             (declared in stdlib); got {other:?} — step-8 not yet implemented"
        ),
    }
}

// ── step-9: trampoline-unit tests (crafted Values, no compile pipeline) ───────

/// A Length-typed coordinate Scalar (SI metres) — how `point3(..m, ..)` lowers.
fn length(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
}

/// A 3-component `Value::Point` node.
fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

/// Anchored cable net: free node 0 (off-solution initial guess) plus four
/// anchors at (±1,0,0),(0,±1,1); `struts: []`, four cable spokes 0→{1,2,3,4}.
/// With equal q the analytic solution for node 0 is the anchor centroid
/// (0, 0, 0.5).
fn cable_net_tensegrity() -> Value {
    let nodes = Value::List(vec![
        node(0.3, 0.2, 0.4), // free node 0 — deliberately off-solution
        node(1.0, 0.0, 0.0), // anchor 1
        node(-1.0, 0.0, 0.0), // anchor 2
        node(0.0, 1.0, 1.0), // anchor 3
        node(0.0, -1.0, 1.0), // anchor 4
    ]);
    let struts = Value::List(vec![]);
    let cables = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(1)]),
        Value::List(vec![Value::Int(0), Value::Int(2)]),
        Value::List(vec![Value::Int(0), Value::Int(3)]),
        Value::List(vec![Value::Int(0), Value::Int(4)]),
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
fn call_form_find(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::form_find::solve_form_find_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Extract an f64 from a Length Scalar (or bare Real) coordinate component.
fn coord(v: &Value) -> f64 {
    match v {
        Value::Scalar { si_value, .. } => *si_value,
        Value::Real(r) => *r,
        other => panic!("expected a Scalar/Real coordinate, got {other:?}"),
    }
}

/// (a) Happy path: the trampoline cracks the Tensegrity / force-densities /
/// anchors Values, calls the FD kernel, and returns `Completed` with a
/// `FormFindResult` whose `nodes` is a List of 5 Points, node 0 at the anchor
/// centroid (0,0,0.5), and `converged == Bool(true)`.
#[test]
fn trampoline_happy_path_solves_to_anchor_centroid() {
    let value_inputs = vec![
        cable_net_tensegrity(),
        Value::List(vec![
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
        ]),
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]),
    ];

    let fields = match call_form_find(&value_inputs) {
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
        other => panic!("expected ComputeOutcome::Completed, got {other:?}"),
    };

    let nodes = match fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => ns,
        other => panic!("FormFindResult.nodes must be a List, got {other:?}"),
    };
    assert_eq!(nodes.len(), 5, "expected 5 solved nodes (1 free + 4 anchors)");

    let n0 = match &nodes[0] {
        Value::Point(c) if c.len() == 3 => [coord(&c[0]), coord(&c[1]), coord(&c[2])],
        other => panic!("nodes[0] must be a 3-component Point, got {other:?}"),
    };
    let expected = [0.0, 0.0, 0.5];
    for (i, (got, exp)) in n0.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-9,
            "nodes[0][{i}] = {got}, expected anchor centroid component {exp}",
        );
    }

    assert_eq!(
        fields.get(&"converged".to_string()),
        Some(&Value::Bool(true)),
        "a well-posed solve must report converged == true",
    );
}

/// (b) Sign violation: one cable given q = −1 violates the cable-tension
/// contract. The trampoline must surface `Failed` with an E_FormFindInfeasible
/// diagnostic — never a panic or a silently wrong result.
#[test]
fn trampoline_sign_violation_is_failed_with_diagnostic() {
    let value_inputs = vec![
        cable_net_tensegrity(),
        Value::List(vec![
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(1.0),
            Value::Real(-1.0), // cable 4 violates the q > 0 tension contract
        ]),
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]),
    ];

    match call_form_find(&value_inputs) {
        ComputeOutcome::Failed { diagnostics } => assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("E_FormFindInfeasible")),
            "expected an E_FormFindInfeasible diagnostic, got: {diagnostics:?}",
        ),
        other => panic!("expected ComputeOutcome::Failed for a sign violation, got {other:?}"),
    }
}
