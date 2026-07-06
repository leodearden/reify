// Integration tests for the M-WHOLE β merged cross-scope ResolutionProblem
// builder + N-scope write-back (task #5014).
//
// PRD: docs/prds/v0_6/whole-model-objective-coupling.md §5.2.
//
// For a within-cap `MergedSolve` cluster (α, task #5013), the cold `eval()`
// driver must UNION the cluster's regular auto cells, constraints, and
// objectives into ONE `ResolutionProblem`, solve ONCE, and write the solved
// values back to ALL cluster member scopes (marked `Determined`). Scopes
// outside a `MergedSolve` cluster (uncoupled scopes, or `ApproximatedFallback`
// clusters) keep the pre-β per-template freeze-as-you-go path unchanged.
//
// ## Test harness
//
// Engine-level builder tests via `MultiCallSpyConstraintSolver` /
// `SpyConstraintSolver`, mirroring resolve_order.rs's and scope_coupling.rs's
// idiom: build fixtures with `TopologyTemplateBuilder`/`CompiledModuleBuilder`,
// inject a spy solver via `Engine::new(checker, None).with_solver(...)`, call
// `engine.eval(&module)`, and assert on the captured `ResolutionProblem`(s)
// and/or the returned `EvalResult`.

use std::collections::HashMap;

use reify_compiler::CompiledModule;
use reify_core::{ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{DeterminacyState, SolveResult};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MultiCallSpyConstraintSolver,
    TopologyTemplateBuilder, gt, literal, mm, value_ref,
};

/// Build the canonical within-cap 2-cycle cluster {A, B}: A reads B.m, B reads
/// A.k (irreducible SCC of size 2; 1 auto cell each ⇒ merged dim 2, well
/// within `WHOLE_MODEL_CLUSTER_DIM_CAP` ⇒ `ClusterDisposition::MergedSolve`).
///
/// Mirrors resolve_order.rs's `irreducible_two_cycle_forms_single_merged_cluster`
/// and scope_coupling.rs test H fixtures exactly, so this shape is already
/// known-good against the α unit tests (confirmed: exactly one cluster,
/// scopes == [0, 1], dim == 2, disposition == MergedSolve).
fn two_cycle_cluster_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        // A reads B.m — creates edge B→A in the read-DAG.
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        // B reads A.k — creates edge A→B in the read-DAG → cycle!
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

// ---------------------------------------------------------------------------
// step-01/02: single merged dispatch + union auto_params + ordering.
// ---------------------------------------------------------------------------

/// A within-cap cluster must be solved with exactly ONE merged solver call,
/// whose `auto_params` is the union `[A.k, B.m]` (cluster.scopes ascending ×
/// per-scope declaration order) — NOT two separate per-template calls.
///
/// RED until step-02 wires `build_merged_solver_problem` + the merged-cluster
/// dispatch into the cold `eval()` driver loop: today's per-template loop
/// calls the solver once per scope (twice total for this fixture).
#[test]
fn merged_cluster_dispatches_single_solve_with_union_auto_params() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(3.0));
    solved.insert(b_m.clone(), mm(7.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: solved,
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let _result = engine.eval(&module);

    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        1,
        "within-cap cluster {{A,B}} must dispatch exactly ONE merged solve, not one per scope; got {} call(s)",
        problems.len(),
    );

    let ids: Vec<ValueCellId> = problems[0]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    assert_eq!(
        ids,
        vec![a_k, b_m],
        "merged auto_params must be exactly the union [A.k, B.m] in \
         cluster.scopes × declaration order; got {:?}",
        ids,
    );
}

// ---------------------------------------------------------------------------
// step-03/04: N-scope write-back -- ALL cluster members' cells resolved.
// ---------------------------------------------------------------------------

/// The merged solve's FULL solution must be written back to every cluster
/// member scope, not just the first one reached in `ro.order`: both
/// `EvalResult.values`/`resolved_params` AND the final snapshot's
/// `DeterminacyState` must reflect BOTH A.k and B.m.
///
/// RED until step-04 widens the merged Solved-arm write-back from "idx's own
/// cells only" (step-02) to the FULL merged `solver_values` across every
/// cluster member: today only the first-iterated member's slice is written
/// back, so the other cluster scope's cell is still unresolved/undetermined.
#[test]
fn merged_cluster_writes_back_solved_values_to_every_member_scope() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(3.0));
    solved.insert(b_m.clone(), mm(7.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: solved,
        unique: true,
    }]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    assert_eq!(
        result.resolved_params.get(&a_k),
        Some(&mm(3.0)),
        "resolved_params must contain A.k from the merged solve",
    );
    assert_eq!(
        result.resolved_params.get(&b_m),
        Some(&mm(7.0)),
        "resolved_params must contain B.m from the merged solve -- not just \
         A.k (the first-iterated cluster member)",
    );

    let a_val = result
        .values
        .get(&a_k)
        .expect("A.k missing from EvalResult.values");
    assert_eq!(*a_val, mm(3.0), "A.k's value must be the merged solve's result");
    let b_val = result
        .values
        .get(&b_m)
        .expect("B.m missing from EvalResult.values -- not just A.k (the \
                 first-iterated cluster member)");
    assert_eq!(*b_val, mm(7.0), "B.m's value must be the merged solve's result");

    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()");
    let (a_snap_val, a_det) = snapshot
        .values
        .get(&a_k)
        .expect("A.k missing from the final snapshot");
    assert_eq!(*a_snap_val, mm(3.0));
    assert_eq!(
        *a_det,
        DeterminacyState::Determined,
        "A.k must be Determined in the final snapshot",
    );

    let (b_snap_val, b_det) = snapshot
        .values
        .get(&b_m)
        .expect("B.m missing from the final snapshot");
    assert_eq!(*b_snap_val, mm(7.0));
    assert_eq!(
        *b_det,
        DeterminacyState::Determined,
        "B.m must be Determined in the final snapshot -- not still Undetermined",
    );
}
