//! Worked-example smoke tests for the topology-selector function family
//! (PRD `docs/prds/topology-selectors.md` task 7).
//!
//! Three `.ri` fixtures live under `examples/topology_selectors/`:
//!
//! * `all_topology_selectors_wiring.ri` — compile-time wiring for all 11 names
//! * `block_inertia.ri` — `moment_of_inertia` on a steel box
//! * `fillet_top_edges.ri` — `single`/`flat_map`/`adjacent_faces`/`shared_edges`/`faces_by_normal`/`fillet`
//!
//! ## Scope: parse + compile-with-stdlib + `#[ignore]`-gated eval
//!
//! Coverage follows the four-level staircase from `field_source_kinds_smoke.rs`
//! (parse → compile_with_stdlib → eval → constraints):
//!
//! | Fixture                              | Parse | Compile | Eval (gated) |
//! |--------------------------------------|-------|---------|--------------|
//! | `all_topology_selectors_wiring.ri`   | ✓ (via examples_smoke walker) | ✓ active | ✗ (eval dispatch pending) |
//! | `block_inertia.ri`                   | ✓     | ✓ active (`block_inertia_compiles_with_stdlib_no_errors`) | `#[ignore]` (dispatch pending) |
//! | `fillet_top_edges.ri`                | ✓     | `#[ignore]` (3-arg fillet binding missing) | `#[ignore]` (3-arg fillet + dispatch) |
//!
//! The `#[ignore]`-gated eval tests document future-state contracts and pin the
//! API surface so a follow-up agent knows exactly what to implement to unblock them.
//! They are not run in CI but their presence is intentional — see each test's
//! `#[ignore]` string for the precise blocker and pointer into the code.

use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};
use reify_types::{ExportFormat, ModulePath, Value, ValueCellId};

const BLOCK_INERTIA_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/block_inertia.ri"
);

const FILLET_TOP_EDGES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/fillet_top_edges.ri"
);

const ALL_TOPOLOGY_SELECTORS_WIRING_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/topology_selectors/all_topology_selectors_wiring.ri"
);

/// Verify that `all_topology_selectors_wiring.ri` passes a real face handle (not
/// a Solid) as the second argument to `adjacent_faces` and `shared_edges`.
///
/// Two assertions fire:
///
/// 1. **Structural shape (RED until S2)**: The source must NOT contain the
///    old placeholder strings `"adjacent_faces(body, body)"` or
///    `"shared_edges(body, body)"`.  These were sufficient for compile-time
///    wiring (which keys only on the function name, not arg types) but are
///    semantically wrong — both relational selectors expect a face handle as
///    their second argument (§3.9 PRD signature), not a Solid.  The assertion
///    pins runtime arg-shape correctness and will FAIL RED until Step S2
///    introduces `let top_face = single(top_faces)` and threads it through.
///
/// 2. **Compile clean**: After the fix, the new face-handle form must still
///    compile with no Error-severity diagnostics (tasks 2699 and 2698 are
///    both landed on HEAD).
#[test]
fn all_topology_selectors_wiring_passes_face_handles_to_relational_selectors() {
    let source = std::fs::read_to_string(ALL_TOPOLOGY_SELECTORS_WIRING_PATH)
        .expect("examples/topology_selectors/all_topology_selectors_wiring.ri should exist");

    assert!(
        !source.contains("adjacent_faces(body, body)"),
        "all_topology_selectors_wiring.ri should pass a face handle (not a Solid) as the \
         second argument to adjacent_faces; found the old placeholder `adjacent_faces(body, body)` \
         — fix by introducing `let top_face = single(top_faces)` and using it instead"
    );
    assert!(
        !source.contains("shared_edges(body, body)"),
        "all_topology_selectors_wiring.ri should pass face handles to shared_edges; found the old \
         placeholder `shared_edges(body, body)` — fix by threading `top_face` through the call"
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/all_topology_selectors_wiring.ri should compile with \
         no error-severity diagnostics after the face-handle fix, got:\n{:#?}",
        errors_only(&compiled)
    );
}

#[test]
fn all_topology_selectors_wiring_compiles_with_stdlib() {
    let source = std::fs::read_to_string(ALL_TOPOLOGY_SELECTORS_WIRING_PATH)
        .expect("examples/topology_selectors/all_topology_selectors_wiring.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/all_topology_selectors_wiring.ri should compile with \
         no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );
}

/// Verify that `block_inertia.ri` compiles with no Error-severity diagnostics.
///
/// This deepens the existing parse-only smoke to compile-with-stdlib, which became
/// achievable once task 2699 wired `moment_of_inertia` (and its return type
/// `Tensor<2,3,MomentOfInertia>`) as a language-level stdlib binding.
///
/// Note: the runtime value of the `i` cell is still `Value::Undef` because
/// eval-side dispatch for `moment_of_inertia` has not yet been added to
/// `try_eval_topology_selector` in `crates/reify-eval/src/geometry_ops.rs`.
/// That gap is documented by the `#[ignore]`-gated test
/// `block_inertia_evals_moment_of_inertia_to_tensor` (step S5).
#[test]
fn block_inertia_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/block_inertia.ri should compile with no error-severity \
         diagnostics (task 2699 wired moment_of_inertia), got:\n{:#?}",
        errors_only(&compiled)
    );
}

/// Eval-deepening contract: the `i` cell in `BlockInertia` must resolve to a
/// rank-2 `Value::Tensor` (a `Tensor<2, 3, MomentOfInertia>` per PRD §3.9 —
/// a 3×3 matrix of `MomentOfInertia`-dimensioned scalars).
///
/// For a uniform-density box with side lengths W=50mm, H=30mm, D=10mm and
/// density ρ=7850 kg/m³, the principal moments satisfy I_xx = (1/12)·m·(H²+D²),
/// etc.  The test asserts the nested-Tensor shape rather than numeric values so
/// it remains valid for any kernel-precision implementation.
///
/// **Blocked by**: eval-side dispatch for `moment_of_inertia` in
/// `crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector` — the
/// 11 task-2699 names fall through to `_ => return None` today (see
/// `geometry_ops.rs:1646-1661`), leaving cells at `Value::Undef`.  Tracked by
/// PRD `docs/prds/topology-selectors.md` task 8 as task 2691's eval arms.
/// This smoke test documents the contract and lands the gating test; remove the
/// `#[ignore]` once the `moment_of_inertia` arm is added to
/// `try_eval_topology_selector`.
#[test]
#[ignore = "pending eval-side dispatch for moment_of_inertia in \
            crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector — \
            the 11 task-2699 names fall through to None today (see geometry_ops.rs:1646-1661), \
            leaving cells at Value::Undef. Tracked by PRD docs/prds/topology-selectors.md \
            task 8 as task 2691's eval arms; this smoke task documents the contract and \
            lands the gating test."]
fn block_inertia_evals_moment_of_inertia_to_tensor() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "compile errors: {:#?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("BlockInertia", "i");
    match result.values.get(&cell) {
        Some(Value::Tensor(_)) => {} // rank-2 nested Tensor — correct
        other => panic!(
            "expected BlockInertia.i to be Value::Tensor(_) (rank-2 MomentOfInertia tensor), \
             got {other:?}"
        ),
    }
}

#[test]
fn block_inertia_ri_parses_cleanly() {
    let source = std::fs::read_to_string(BLOCK_INERTIA_PATH)
        .expect("examples/topology_selectors/block_inertia.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

/// Compile-with-stdlib contract for `fillet_top_edges.ri`.
///
/// **Blocked by**: the missing 3-arg `fillet(solid, edges, radius)` stdlib
/// binding — the current compiler only wires 2-arg `fillet(solid, radius)` at
/// `crates/reify-compiler/src/geometry_modify.rs:115`.  The example uses the
/// 3-arg form on line 25 (`fillet(b, top_edges, 1mm)`), so this currently
/// fails with `fillet() expects 2 arguments, got 3`.
///
/// This is **NOT** a task 2698/2699 gap — `single`, `flat_map`,
/// `faces_by_normal`, `adjacent_faces`, and `shared_edges` are all landed on
/// HEAD.  The sole remaining compile blocker is the 3-arg fillet binding.
///
/// Do NOT modify `fillet_top_edges.ri` to use the 2-arg form: that would
/// fillet ALL edges and defeat the example's pedagogic purpose (demonstrating
/// topology-relational edge selection).
///
/// Remove the `#[ignore]` once a 3-arg `fillet(solid, edges, radius)` stdlib
/// binding is wired in `crates/reify-compiler/src/geometry_modify.rs`.
#[test]
#[ignore = "pending 3-arg fillet(solid, edges, radius) stdlib binding — current compiler \
            only wires 2-arg fillet(solid, radius) per \
            crates/reify-compiler/src/geometry_modify.rs:115. \
            The example uses the 3-arg form (line 25 of fillet_top_edges.ri). \
            This is NOT a task 2698/2699 binding gap (those are landed); \
            it is a separate stdlib-binding gap tracked outside this PRD."]
fn fillet_top_edges_compiles_with_stdlib_no_errors() {
    let source = std::fs::read_to_string(FILLET_TOP_EDGES_PATH)
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/topology_selectors/fillet_top_edges.ri should compile with no error-severity \
         diagnostics (unblocked by 3-arg fillet stdlib binding), got:\n{:#?}",
        errors_only(&compiled)
    );
}

#[test]
fn fillet_top_edges_ri_parses_cleanly() {
    let source = std::fs::read_to_string(FILLET_TOP_EDGES_PATH)
        .expect("examples/topology_selectors/fillet_top_edges.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}
