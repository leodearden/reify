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

use std::sync::atomic::{AtomicU32, Ordering};

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
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
    Value::Scalar {
        si_value: m,
        dimension: DimensionVector::LENGTH,
    }
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
        node(0.3, 0.2, 0.4),  // free node 0 — deliberately off-solution
        node(1.0, 0.0, 0.0),  // anchor 1
        node(-1.0, 0.0, 0.0), // anchor 2
        node(0.0, 1.0, 1.0),  // anchor 3
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
    assert_eq!(
        nodes.len(),
        5,
        "expected 5 solved nodes (1 free + 4 anchors)"
    );

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

// ── step-9 (amend): trampoline failure-path coverage ─────────────────────────
//
// The happy-path + single sign-violation tests above leave the other implemented
// failure paths unexercised — exactly the guards most likely to regress silently
// into an `Ok`/panic. These assert `ComputeOutcome::Failed` for each, and check a
// guard-specific phrase (not just the shared `E_FormFindInfeasible` prefix) so
// the `describe()` mapping and the trampoline's own range/length guards are
// actually covered.

/// A Tensegrity with a disconnected free node: node 0 is cabled to anchor 2,
/// while free node 1 floats with no member touching it — its row in the reduced
/// stiffness `D_ff` is zero, so the solve is singular (mirrors the kernel's
/// `disconnected_free_node_is_singular_reduced_stiffness` golden).
fn disconnected_free_node_tensegrity() -> Value {
    let nodes = Value::List(vec![
        node(0.0, 0.0, 0.0), // free node 0 — cabled to the anchor
        node(5.0, 0.0, 0.0), // free node 1 — floating: no members touch it
        node(1.0, 0.0, 0.0), // anchor 2
    ]);
    let struts = Value::List(vec![]);
    let cables = Value::List(vec![Value::List(vec![Value::Int(0), Value::Int(2)])]);
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

/// Assert the outcome is `Failed` with an `E_FormFindInfeasible` diagnostic whose
/// message also contains `needle` (proving the specific guard / `describe()` arm
/// fired, not merely *some* infeasibility).
fn assert_failed_infeasible(outcome: ComputeOutcome, needle: &str) {
    match outcome {
        ComputeOutcome::Failed { diagnostics } => {
            let joined = diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            assert!(
                joined.contains("E_FormFindInfeasible"),
                "expected an E_FormFindInfeasible diagnostic, got: {joined}"
            );
            assert!(
                joined.contains(needle),
                "expected the diagnostic to mention {needle:?}, got: {joined}"
            );
        }
        other => panic!("expected ComputeOutcome::Failed, got {other:?}"),
    }
}

/// All five nodes anchored ⇒ empty free set: the kernel has nothing to solve for
/// and the trampoline must surface a clean diagnostic, not a degenerate 0×0 solve.
#[test]
fn trampoline_all_anchored_is_failed_empty_free_set() {
    let value_inputs = vec![
        cable_net_tensegrity(),
        Value::List(vec![Value::Real(1.0); 4]),
        Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]),
    ];
    assert_failed_infeasible(call_form_find(&value_inputs), "every node is anchored");
}

/// A disconnected free node makes `D_ff` singular: the post-solve residual guard
/// must trip and the trampoline must report it (never NaN coordinates / a wrong
/// solve).
#[test]
fn trampoline_disconnected_free_node_is_failed_singular() {
    let value_inputs = vec![
        disconnected_free_node_tensegrity(),
        Value::List(vec![Value::Real(1.0)]), // one cable
        Value::List(vec![Value::Int(2)]),    // anchor 2
    ];
    assert_failed_infeasible(call_form_find(&value_inputs), "singular reduced stiffness");
}

/// force_densities shorter than the member count (4 cables, 3 densities) is a
/// dimension mismatch — caught by the kernel up front.
#[test]
fn trampoline_force_density_count_mismatch_is_failed() {
    let value_inputs = vec![
        cable_net_tensegrity(),                 // 4 cables
        Value::List(vec![Value::Real(1.0); 3]), // only 3 force densities
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ]),
    ];
    assert_failed_infeasible(call_form_find(&value_inputs), "member count");
}

/// An anchor index past the node array is rejected by the trampoline's own range
/// check (before the kernel runs), with the offending index located in the
/// message.
#[test]
fn trampoline_out_of_range_anchor_index_is_failed() {
    let value_inputs = vec![
        cable_net_tensegrity(), // 5 nodes ⇒ valid indices 0..5
        Value::List(vec![Value::Real(1.0); 4]),
        Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(99), // out of range
        ]),
    ];
    assert_failed_infeasible(call_form_find(&value_inputs), "out of range");
}

/// Fewer than three value_inputs (a caller that failed to let-bind all three
/// args — the shallow-walk capture contract) hits the `run()` length guard, which
/// must produce a located diagnostic rather than an index-out-of-bounds panic.
#[test]
fn trampoline_short_value_inputs_is_failed() {
    let value_inputs = vec![
        cable_net_tensegrity(),
        Value::List(vec![Value::Real(1.0); 4]),
        // anchors omitted → only 2 inputs reach the trampoline
    ];
    assert_failed_infeasible(call_form_find(&value_inputs), "expects 3 inputs");
}

// ── step-11: e2e + cache-hit + CLI over examples/tensegrity_cable_net.ri ──────

/// The committed anchored cable-net example. `include_str!` makes a *missing*
/// file a compile error — the step-11 RED signal until step-12 creates it.
fn cable_net_source() -> &'static str {
    include_str!("../../../examples/tensegrity_cable_net.ri")
}

/// Crack a `CableNet.form` FormFindResult cell into solved `[f64;3]` node
/// coordinates and its `converged` flag.
fn form_nodes_and_converged(form: &Value) -> (Vec<[f64; 3]>, bool) {
    let data = match form {
        Value::StructureInstance(d) => d,
        other => panic!("CableNet.form must be a FormFindResult StructureInstance, got {other:?}"),
    };
    assert_eq!(
        data.type_name, "FormFindResult",
        "CableNet.form should be a FormFindResult, got {:?}",
        data.type_name
    );
    let nodes = match data.fields.get(&"nodes".to_string()) {
        Some(Value::List(ns)) => ns
            .iter()
            .map(|p| match p {
                Value::Point(c) if c.len() == 3 => [coord(&c[0]), coord(&c[1]), coord(&c[2])],
                other => panic!("FormFindResult.nodes entry must be a 3-Point, got {other:?}"),
            })
            .collect(),
        other => panic!("FormFindResult.nodes must be a List, got {other:?}"),
    };
    let converged = matches!(
        data.fields.get(&"converged".to_string()),
        Some(Value::Bool(true))
    );
    (nodes, converged)
}

/// (a) End-to-end: the example compiles, `@optimized("solver::form_find")`
/// lowers to a ComputeNode (no body inlining), and the trampoline solves node 0
/// to the anchor centroid (0,0,0.5) with `converged == true`.
#[test]
fn e2e_cable_net_lowers_to_compute_node_and_solves() {
    let compiled = compile_source_with_stdlib(cable_net_source());

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No Error-severity diagnostics from the full compile + eval pipeline.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:#?}"
    );

    // A ComputeNode with target == "solver::form_find" must be in the graph —
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
        targets.contains(&"solver::form_find"),
        "expected a ComputeNode with target==\"solver::form_find\"; found {targets:?}"
    );

    // The result cell solves to the anchor centroid and reports convergence.
    let form = eval_result
        .values
        .get(&ValueCellId::new("CableNet", "form"))
        .unwrap_or_else(|| panic!("CableNet.form cell missing from eval result"));
    let (nodes, converged) = form_nodes_and_converged(form);
    assert_eq!(
        nodes.len(),
        5,
        "expected 5 solved nodes (1 free + 4 anchors)"
    );
    let expected = [0.0, 0.0, 0.5];
    for (i, (got, exp)) in nodes[0].iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-9,
            "nodes[0][{i}] = {got}, expected anchor-centroid component {exp}"
        );
    }
    assert!(
        converged,
        "a well-posed solve must report converged == true"
    );
}

/// Dispatch counter for the cache-hit counting wrapper.
static DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper around the production trampoline — increments DISPATCH_COUNT
/// then delegates, so a re-dispatch is observable.
fn counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::form_find::solve_form_find_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// (b) Cache hit: a second `eval()` of the same compiled module must NOT
/// re-dispatch the trampoline — the §8-η Final-gate (engine_eval.rs)
/// short-circuits when all inputs and the output VC are already Final.
#[test]
fn e2e_cable_net_second_eval_hits_cache() {
    DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let compiled = compile_source_with_stdlib(cable_net_source());
    let mut engine = make_simple_engine();
    engine.register_compute_fn("solver::form_find", counting_wrapper as ComputeFn);

    // First eval: cold start — exactly one dispatch.
    let eval1 = engine.eval(&compiled);
    let errors1: Vec<_> = eval1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "first eval must have no Error diagnostics, got: {errors1:#?}"
    );
    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch the trampoline exactly once"
    );

    // Second eval on the same module: Final-gate cache hit — no re-dispatch.
    let eval2 = engine.eval(&compiled);
    let errors2: Vec<_> = eval2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "second eval must have no Error diagnostics, got: {errors2:#?}"
    );
    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must hit the cache and NOT re-dispatch (DISPATCH_COUNT must stay at 1)"
    );
}

/// (c) CLI smoke: `reify eval examples/tensegrity_cable_net.ri` exits zero and
/// prints the solved z (0.5) — the user-observable `result.nodes` signal.
///
/// `CARGO_BIN_EXE_reify` is only injected for `reify-cli`'s own integration
/// tests, so this cross-crate test execs the pre-built `reify` binary
/// directly. It deliberately does NOT use `cargo run`: even when the binary
/// is already compiled, `cargo run` re-fingerprints the entire workspace and
/// blocks on the global cargo build-lock before exec, and under concurrent
/// multi-worktree verify load that overhead can push the test past its time
/// budget (esc-4340-32, exit 124). The merge gate's debug `--workspace` pass
/// builds all `[[bin]]` targets (including `reify`) at `target/debug/reify`;
/// its release pass is scoped to release-sensitive crates and does NOT rebuild
/// `reify-cli`, so the resolution below prefers the profile-local bin and falls
/// back to the debug-profile one when it is absent. The cargo runner
/// (`.cargo/run-with-occt.sh`) exports `LD_LIBRARY_PATH` into this test
/// process's environment, which the spawned child inherits, so OCCT shared
/// libraries resolve without going through cargo.
#[test]
fn cli_cable_net_prints_solved_z() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root is two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/tensegrity_cable_net.ri");

    // Resolve the prebuilt `reify` binary from this test binary's own location.
    // The integration-test binary lives at `…/target/<profile>/deps/<testbin>`,
    // so its grandparent is `…/target/<profile>` and the `reify` bin sits beside
    // it at `…/target/<profile>/reify`.
    //
    // Cross-task seam (task/4390 HAS LANDED): the merge gate's RELEASE pass
    // (verify.sh, DF_VERIFY_ROLE=merge --profile both) is scoped to
    // release-sensitive crates and deliberately does NOT build `reify-cli`, so
    // `target/release/reify` is absent during the release test pass. The
    // preceding DEBUG pass runs the full `--workspace` (building
    // `target/debug/reify`), and the reify CLI's golden output is
    // profile-independent (the release pass exists to re-check reify-eval's own
    // overflow/debug-assert behaviour, not the spawned CLI). So prefer the
    // profile-local bin but fall back to the debug-profile sibling when it is
    // absent. (Per-task verifies are unaffected: a reify-eval change pulls
    // `reify-cli` into the affected set as a reverse-dep, so the debug bin is
    // built.)
    let test_bin = std::env::current_exe().expect("current_exe");
    let profile_dir = test_bin
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary lives in target/<profile>/deps");
    let profile_local = profile_dir.join("reify");
    let reify_bin = if profile_local.exists() {
        profile_local
    } else {
        // Release pass: target/release/reify is absent (reify-cli not built);
        // fall back to the debug-profile bin the debug pass built.
        profile_dir
            .parent()
            .map(|target_dir| target_dir.join("debug").join("reify"))
            .filter(|p| p.exists())
            .unwrap_or(profile_local)
    };

    let output = std::process::Command::new(&reify_bin)
        .current_dir(&workspace_root)
        .arg("eval")
        .arg(&example)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; is it built? \
                 The gated verify pass builds it when it compiles `reify-cli` \
                 (`cargo test -p reify-cli`, or the merge gate's debug \
                 `--workspace` pass that builds all `[[bin]]` targets). Note: an \
                 ad-hoc `cargo test -p reify-eval` alone does NOT build the \
                 `reify` bin.",
                reify_bin.display()
            )
        });

    assert!(
        output.status.success(),
        "`reify eval examples/tensegrity_cable_net.ri` exited non-zero.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");
    // Tight assertion: the solved free node 0 prints at the anchor centroid
    // (0, 0, 0.5) m, i.e. the exact token `point(0 m, 0 m, 0.5 m)`. A bare "0.5"
    // substring would also match "0.50" / "10.5" / any incidental 0.5 in another
    // cell, so a *wrong* solve could pass; the full point string ties z = 0.5 to
    // node 0 being at the centroid. The 1×1 reduced solve is bit-exact here
    // (2 / 4 = 0.5 in IEEE-754), so the printed form needs no tolerance.
    assert!(
        stdout.contains("point(0 m, 0 m, 0.5 m)"),
        "expected the solved node 0 at the anchor centroid `point(0 m, 0 m, 0.5 m)` \
         in `reify eval` stdout; got:\n{stdout}"
    );
}
