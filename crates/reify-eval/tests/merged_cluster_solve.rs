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
use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{
    BestFoundReason, BinOp, ConstraintSolver, DeterminacyState, ObjectiveCombination,
    ObjectiveSense, ObjectiveSet, OptimalityStatus, RankedCandidate, RankedSolveResult,
    ResolutionProblem, SolveResult, UndefCause, Value,
};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, MultiCallSpyConstraintSolver,
    SpyConstraintSolver, TopologyTemplateBuilder, binop, gt, literal, mm, value_ref,
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

// ---------------------------------------------------------------------------
// step-05/06: BT3-precursor -- cross-scope solved-auto surface read.
// ---------------------------------------------------------------------------

/// Same {A, B} 2-cycle, but B additionally declares a `let` cell
/// `B.out = A.k * 2` that reads A's CO-SOLVED auto cell (not B's own).
///
/// Mirrors `two_cycle_cluster_module` exactly, plus one `let_binding` on B.
fn two_cycle_cluster_with_cross_scope_let_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        // A reads B.m — creates edge B→A in the read-DAG.
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        // B reads A.k — creates edge A→B in the read-DAG → cycle!
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        // Downstream let cell surfacing A's co-solved auto -- BT3.
        .let_binding(
            "B",
            "out",
            Type::length(),
            binop(BinOp::Mul, value_ref("A", "k"), literal(Value::Real(2.0))),
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// B's downstream `let` cell (`B.out = A.k * 2`) must surface the CO-SOLVED
/// A.k from the merged solve, not a frozen/undef value from B's main pass
/// (which ran before the merged solve, while A.k was still undetermined).
///
/// RED until step-06: today (step-04) only calls `evaluate_let_bindings` for
/// `idx` -- the first-in-order cluster member reached by the driver loop --
/// so whichever cluster member is NOT `idx` never gets its `let` cone
/// re-evaluated against the merged solution at all.
#[test]
fn merged_cluster_let_surfaces_co_solved_cross_scope_auto() {
    let module = two_cycle_cluster_with_cross_scope_let_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    let b_out = ValueCellId::new("B", "out");

    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(4.0));
    solved.insert(b_m.clone(), mm(1.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: solved,
        unique: true,
    }]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let out_val = result
        .values
        .get(&b_out)
        .expect("B.out missing from EvalResult.values");
    assert_eq!(
        *out_val,
        mm(8.0),
        "B.out = A.k * 2 must surface the CO-SOLVED A.k (4mm -> 8mm), not a \
         frozen/undef value from B's pre-merge-solve main pass; got {:?}",
        out_val,
    );
}

// ---------------------------------------------------------------------------
// step-07/08: objective consumed abstractly -- aggregate/spanning cluster.
// ---------------------------------------------------------------------------

/// Parent(idx0) declares `minimize (ChildA.cost + ChildB.cost)`, reading two
/// children's autos; ChildA(idx1)/ChildB(idx2) each own a `cost` auto and
/// declare no objective of their own => one MergedSolve cluster spanning
/// [0, 1, 2].
///
/// Mirrors resolve_order.rs's `aggregate_objective_forms_single_spanning_cluster`
/// exactly (same source order, same fixture shape) -- confirmed known-good
/// against the α unit test (exactly one cluster, scopes == [0, 1, 2], dim ==
/// 3, disposition == MergedSolve).
fn spanning_objective_cluster_module() -> CompiledModule {
    let parent = TopologyTemplateBuilder::new("Parent")
        .auto_param("Parent", "total", Type::length())
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            binop(
                BinOp::Add,
                value_ref("ChildA", "cost"),
                value_ref("ChildB", "cost"),
            ),
        ))
        .build();

    let child_a = TopologyTemplateBuilder::new("ChildA")
        .auto_param("ChildA", "cost", Type::length())
        .build();

    let child_b = TopologyTemplateBuilder::new("ChildB")
        .auto_param("ChildB", "cost", Type::length())
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(child_a)
        .template(child_b)
        .build()
}

/// The merged `ResolutionProblem` for a spanning cluster must carry the
/// PARENT's governing objective through OPAQUELY: same combination
/// (`WeightedSum`), same term count, same `ObjectiveSense`/`weight` per term
/// -- i.e. terms are concatenated verbatim, never re-folded (§5.2 "objective
/// fold consumed abstractly").
///
/// RED until step-08: `build_merged_solver_problem` still hard-codes
/// `objective: None`, so `problem.objective.is_some()` fails today.
#[test]
fn merged_cluster_objective_passed_through_opaquely() {
    let module = spanning_objective_cluster_module();

    let parent_total = ValueCellId::new("Parent", "total");
    let child_a_cost = ValueCellId::new("ChildA", "cost");
    let child_b_cost = ValueCellId::new("ChildB", "cost");

    let mut solved = HashMap::new();
    solved.insert(parent_total.clone(), mm(1.0));
    solved.insert(child_a_cost.clone(), mm(2.0));
    solved.insert(child_b_cost.clone(), mm(3.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let _result = engine.eval(&module);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("solver must have been called with a merged ResolutionProblem");

    let objective = problem.objective.expect(
        "merged problem must carry the spanning objective (Parent's) -- got None",
    );
    assert_eq!(
        objective.combination,
        ObjectiveCombination::WeightedSum,
        "merged objective must combine terms via WeightedSum",
    );

    // Expected == Parent's own objective, rebuilt identically to the fixture
    // above (ObjectiveSet/ObjectiveTerm/CompiledExpr have no PartialEq, so
    // terms are compared structurally field-by-field).
    let expected = ObjectiveSet::single(
        ObjectiveSense::Minimize,
        binop(
            BinOp::Add,
            value_ref("ChildA", "cost"),
            value_ref("ChildB", "cost"),
        ),
    );
    assert_eq!(
        objective.terms.len(),
        expected.terms.len(),
        "merged objective must carry exactly Parent's term count -- terms are \
         concatenated verbatim, not re-folded; got {} term(s)",
        objective.terms.len(),
    );
    for (i, (actual, want)) in objective.terms.iter().zip(expected.terms.iter()).enumerate() {
        assert_eq!(
            actual.sense, want.sense,
            "term {i}'s ObjectiveSense must match Parent's verbatim",
        );
        assert_eq!(
            actual.weight, want.weight,
            "term {i}'s weight must be passed through verbatim -- never \
             re-folded while assembling the merged objective (§5.2)",
        );
        assert_eq!(
            format!("{:?}", actual.expr),
            format!("{:?}", want.expr),
            "term {i}'s expression must be Parent's own expr cloned whole, \
             not reconstructed",
        );
    }
}

// ---------------------------------------------------------------------------
// step-08 hardening: inherited-objective dedup (§6.1 INV-4).
// ---------------------------------------------------------------------------

/// `Parent`(idx0) declares `minimize (ChildA.cost + ChildB.cost)` AND CONTAINS
/// `ChildA`(idx1)/`ChildB`(idx2) as sub-components. Because the children own no
/// objective of their own, they INHERIT Parent's objective
/// (`ContainerObjective::Inherited`, §6.1 INV-4). The objective's cross-scope
/// reads couple all three scopes into ONE spanning `MergedSolve` cluster
/// [0, 1, 2] -- the same shape as `spanning_objective_cluster_module`, plus the
/// two containment edges that turn the children into inheritors.
///
/// `governing_objective` therefore attaches the SAME Parent objective THREE
/// times across the cluster: once as Parent's own governance, and once each as
/// ChildA's / ChildB's inherited copy.
fn contained_children_inherit_parent_objective_module() -> CompiledModule {
    let parent = TopologyTemplateBuilder::new("Parent")
        .auto_param("Parent", "total", Type::length())
        .sub_component("child_a_inst", "ChildA", vec![])
        .sub_component("child_b_inst", "ChildB", vec![])
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            binop(
                BinOp::Add,
                value_ref("ChildA", "cost"),
                value_ref("ChildB", "cost"),
            ),
        ))
        .build();

    let child_a = TopologyTemplateBuilder::new("ChildA")
        .auto_param("ChildA", "cost", Type::length())
        .build();

    let child_b = TopologyTemplateBuilder::new("ChildB")
        .auto_param("ChildB", "cost", Type::length())
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent)
        .template(child_a)
        .template(child_b)
        .build()
}

/// A container objective inherited by several cluster members must contribute
/// its terms to the merged objective EXACTLY ONCE. `governing_objective` hands
/// the same `Inherited` objective to every child of a governing container, so a
/// blind concatenation would fold Parent's single term three times (once own,
/// once per inheriting child), inflating its weight in the merged fold and
/// shifting the argmin away from the true inherited-objective optimum (§6.1
/// INV-4 -- the exact coupling this feature targets).
///
/// RED before the dedup fix: `build_merged_solver_problem`'s spanning-objective
/// loop appended `governance[idx].objective.terms` for every governed member,
/// yielding 3 terms here.
#[test]
fn merged_cluster_inherited_objective_folded_once() {
    let module = contained_children_inherit_parent_objective_module();

    let parent_total = ValueCellId::new("Parent", "total");
    let child_a_cost = ValueCellId::new("ChildA", "cost");
    let child_b_cost = ValueCellId::new("ChildB", "cost");

    let mut solved = HashMap::new();
    solved.insert(parent_total.clone(), mm(1.0));
    solved.insert(child_a_cost.clone(), mm(2.0));
    solved.insert(child_b_cost.clone(), mm(3.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let _result = engine.eval(&module);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("solver must have been called with a merged ResolutionProblem");

    let objective = problem.objective.expect(
        "merged problem must carry the spanning objective (Parent's) -- got None",
    );

    // Parent's objective is a single term. Even though it governs THREE cluster
    // members (Parent's own + ChildA's/ChildB's inherited copies), it must be
    // folded into the merged objective EXACTLY once.
    assert_eq!(
        objective.terms.len(),
        1,
        "Parent's inherited objective must contribute its single term EXACTLY \
         once, not once per governed cluster member (would be 3 without dedup); \
         got {} term(s)",
        objective.terms.len(),
    );
}

// ---------------------------------------------------------------------------
// step-09/10: determinism + back-compat guards (BT5, BT6/INV-2, over-cap).
// ---------------------------------------------------------------------------

/// Run `engine.eval` once against `module` with a FRESH engine + spy, and
/// return the first captured merged problem's `auto_params` id-sequence plus
/// the run's `resolved_params` -- used to compare two independent runs.
fn run_merged_cluster_once(
    module: &CompiledModule,
) -> (Vec<ValueCellId>, HashMap<ValueCellId, Value>) {
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
    let result = engine.eval(module);

    let problems = captured.lock().unwrap();
    let ids: Vec<ValueCellId> = problems[0]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    (ids, result.resolved_params)
}

/// (BT5) Running `engine.eval` twice on the SAME within-cap {A, B} cluster,
/// with a fresh engine + spy each run, must produce byte-identical merged
/// `auto_params` id-sequences and identical `resolved_params`.
///
/// The merged auto-param union ordering is a pure function of
/// `cluster.scopes` (already ascending) x each scope's declaration-order
/// `value_cells` -- no HashMap/HashSet iteration in the ordering path (a set
/// is used only for constraint-filtering membership tests, never to drive
/// order). A leak in that discipline would show up as run-to-run reordering.
#[test]
fn merged_cluster_auto_param_ordering_is_deterministic_across_runs() {
    let module = two_cycle_cluster_module();

    let (ids1, resolved1) = run_merged_cluster_once(&module);
    let (ids2, resolved2) = run_merged_cluster_once(&module);

    assert_eq!(
        ids1, ids2,
        "merged auto_params ordering must be byte-identical across \
         independent runs on the same cluster; got {:?} vs {:?}",
        ids1, ids2,
    );
    assert_eq!(
        resolved1, resolved2,
        "resolved_params must be identical across independent runs on the \
         same cluster; got {:?} vs {:?}",
        resolved1, resolved2,
    );
}

/// (BT6/INV-2) An UNCOUPLED 2-scope module -- each scope constrains only its
/// own auto cell, no cross-scope reads at all -- forms no cluster, so the
/// merged path must never be taken: the engine dispatches ONE solve PER
/// scope (2 total), and each call's `ResolutionProblem.auto_params` carries
/// ONLY that scope's own auto id, never a cross-scope union.
///
/// Both canned solves return the SAME combined map (X.a and Y.b together) --
/// `MultiCallSpyConstraintSolver` repeats the last/only sequence entry for
/// every call, so this sidesteps any assumption about which scope's solve
/// the driver dispatches first while still exercising real ID-keyed
/// write-back for both cells.
#[test]
fn uncoupled_module_solves_per_template_not_merged() {
    let x = TopologyTemplateBuilder::new("X")
        .auto_param("X", "a", Type::length())
        .constraint("X", 0, None, gt(value_ref("X", "a"), literal(mm(0.0))))
        .build();
    let y = TopologyTemplateBuilder::new("Y")
        .auto_param("Y", "b", Type::length())
        .constraint("Y", 0, None, gt(value_ref("Y", "b"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(x)
        .template(y)
        .build();

    let x_a = ValueCellId::new("X", "a");
    let y_b = ValueCellId::new("Y", "b");

    let mut combined = HashMap::new();
    combined.insert(x_a.clone(), mm(5.0));
    combined.insert(y_b.clone(), mm(9.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: combined,
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        2,
        "an uncoupled 2-scope module must dispatch ONE solve PER scope (2 \
         total), never a single merged cross-scope solve; got {} call(s)",
        problems.len(),
    );

    let ids0: Vec<ValueCellId> = problems[0].auto_params.iter().map(|ap| ap.id.clone()).collect();
    let ids1: Vec<ValueCellId> = problems[1].auto_params.iter().map(|ap| ap.id.clone()).collect();
    assert!(
        (ids0 == vec![x_a.clone()] && ids1 == vec![y_b.clone()])
            || (ids0 == vec![y_b.clone()] && ids1 == vec![x_a.clone()]),
        "each per-template solve must carry ONLY its own scope's single auto \
         id -- no cross-scope union; got {:?} then {:?}",
        ids0, ids1,
    );
    drop(problems);

    assert_eq!(
        result.resolved_params.get(&x_a),
        Some(&mm(5.0)),
        "X.a must resolve from its own per-template solve",
    );
    assert_eq!(
        result.resolved_params.get(&y_b),
        Some(&mm(9.0)),
        "Y.b must resolve from its own per-template solve",
    );
}

/// Auto cells per scope in the over-cap fixture below: comfortably over the
/// `pub(crate) WHOLE_MODEL_CLUSTER_DIM_CAP` (12) -- mirrors
/// coupling_approximated.rs's `OVER_CAP_AUTOS`, since this external test
/// crate cannot read the private cap constant either.
const OVER_CAP_AUTOS: usize = 20;

/// Add `n` auto cells (`<entity>.a0..a{n-1}`) to a template builder. Mirrors
/// coupling_approximated.rs's `with_n_autos` helper exactly.
fn with_n_autos(
    mut b: TopologyTemplateBuilder,
    entity: &str,
    n: usize,
) -> TopologyTemplateBuilder {
    for i in 0..n {
        b = b.auto_param(entity, &format!("a{i}"), Type::length());
    }
    b
}

/// (over-cap back-compat) A 2-cycle {Alpha, Beta} whose merged dim
/// (2 * OVER_CAP_AUTOS = 40) exceeds the cap degrades to
/// `ClusterDisposition::ApproximatedFallback` (α; confirmed by
/// resolve_order.rs's `over_cap_two_cycle_degrades_to_approximated_fallback`
/// and coupling_approximated.rs's `over_cap_cycle_emits_coupling_approximated`)
/// -- β must NOT merge it. It must still solve per-template exactly as
/// pre-β: one solve call per scope, each scope's own auto cells resolved.
///
/// As above, both canned solves return the SAME combined map so the
/// assertions don't depend on which scope's solve the driver dispatches
/// first.
#[test]
fn over_cap_cluster_still_solves_per_template() {
    let alpha = with_n_autos(TopologyTemplateBuilder::new("Alpha"), "Alpha", OVER_CAP_AUTOS)
        .constraint("Alpha", 0, None, gt(value_ref("Beta", "a0"), literal(mm(0.0))))
        .build();
    let beta = with_n_autos(TopologyTemplateBuilder::new("Beta"), "Beta", OVER_CAP_AUTOS)
        .constraint("Beta", 0, None, gt(value_ref("Alpha", "a0"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(alpha)
        .template(beta)
        .build();

    let alpha_ids: Vec<ValueCellId> = (0..OVER_CAP_AUTOS)
        .map(|i| ValueCellId::new("Alpha", format!("a{i}")))
        .collect();
    let beta_ids: Vec<ValueCellId> = (0..OVER_CAP_AUTOS)
        .map(|i| ValueCellId::new("Beta", format!("a{i}")))
        .collect();

    let mut combined: HashMap<ValueCellId, Value> = HashMap::new();
    for id in &alpha_ids {
        combined.insert(id.clone(), mm(1.0));
    }
    for id in &beta_ids {
        combined.insert(id.clone(), mm(2.0));
    }

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: combined,
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let problems = captured.lock().unwrap();
    assert_eq!(
        problems.len(),
        2,
        "an over-cap (ApproximatedFallback) cluster must still solve \
         per-template -- one call per scope, never a merged cross-scope \
         solve; got {} call(s)",
        problems.len(),
    );

    let ids0: Vec<ValueCellId> = problems[0].auto_params.iter().map(|ap| ap.id.clone()).collect();
    let ids1: Vec<ValueCellId> = problems[1].auto_params.iter().map(|ap| ap.id.clone()).collect();
    assert!(
        (ids0 == alpha_ids && ids1 == beta_ids) || (ids0 == beta_ids && ids1 == alpha_ids),
        "each per-template solve must carry ONLY its own scope's {} auto ids \
         -- no cross-scope union with the other scope; got {} then {} ids",
        OVER_CAP_AUTOS, ids0.len(), ids1.len(),
    );
    drop(problems);

    for id in &alpha_ids {
        assert_eq!(
            result.resolved_params.get(id),
            Some(&mm(1.0)),
            "{id:?} must resolve to Alpha's own per-template solve result, \
             matching the pre-β frozen-cascade path",
        );
    }
    for id in &beta_ids {
        assert_eq!(
            result.resolved_params.get(id),
            Some(&mm(2.0)),
            "{id:?} must resolve to Beta's own per-template solve result, \
             matching the pre-β frozen-cascade path",
        );
    }
}

// ---------------------------------------------------------------------------
// amendment (review round 1): strict connector-instance auto exclusion guard.
// ---------------------------------------------------------------------------

/// Same {A, B} 2-cycle, but A ALSO declares a strict (`free: false`) auto
/// cell whose entity is `A.__connector_0` -- the synthesized-connector-
/// instance naming convention `is_strict_connector_instance_auto` keys off
/// (task #4710). `build_merged_solver_problem`'s exclusion guard only checks
/// the cell's `id.entity` shape + auto kind (never `sub_components`), so no
/// real connector/sub-component wiring is needed to trigger it.
fn two_cycle_cluster_with_connector_auto_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        // A reads B.m — creates edge B→A in the read-DAG.
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        // Strict connector-instance auto -- must be EXCLUDED from the merged
        // auto_params union, never handed to the solver unconstrained.
        .auto_param("A.__connector_0", "len", Type::length())
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

/// A strict connector-instance auto contributed by a `MergedSolve` cluster
/// member must be EXCLUDED from the merged `auto_params` (never handed to
/// the solver unconstrained), must surface an error `Diagnostic` naming the
/// member/cell, and must remain un-resolved in the final snapshot -- this
/// guard was previously undertested: no fixture placed a connector-pinned
/// auto inside a `MergedSolve` cluster (amendment, task #5014).
#[test]
fn merged_cluster_excludes_strict_connector_instance_auto() {
    let module = two_cycle_cluster_with_connector_auto_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    let connector_len = ValueCellId::new("A.__connector_0", "len");

    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(3.0));
    solved.insert(b_m.clone(), mm(7.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    // (a) absent from the captured merged problem's auto_params.
    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("solver must have been called with a merged ResolutionProblem");
    let ids: Vec<ValueCellId> = problem.auto_params.iter().map(|ap| ap.id.clone()).collect();
    assert_eq!(
        ids,
        vec![a_k.clone(), b_m.clone()],
        "merged auto_params must be exactly [A.k, B.m] -- the strict \
         connector-instance auto must be excluded, not unioned in \
         unconstrained; got {:?}",
        ids,
    );

    // (b) an error Diagnostic mentioning the cell/template is present.
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("MergedSolve cluster") && d.message.contains("`len`")),
        "expected an error Diagnostic naming the excluded connector-instance \
         auto cell (member `len`); got: {:#?}",
        result.diagnostics,
    );

    // (c) the cell remains un-resolved: it stays in its pre-seeded `Auto`
    // leaf state, never promoted to `Determined` -- the merged solve never
    // saw it ("remains Undetermined" colloquially, per the exclusion loop's
    // doc comment; the literal `DeterminacyState::Undetermined` variant is a
    // distinct failure mode for non-auto cells, see `classify_undef_origins`).
    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()");
    let (val, det) = snapshot
        .values
        .get(&connector_len)
        .expect("excluded connector cell must still be pre-seeded in the snapshot");
    assert_eq!(*val, Value::Undef, "excluded connector cell must stay Undef");
    assert_eq!(
        *det,
        DeterminacyState::Auto,
        "excluded connector cell must never reach Determined -- got {:?}",
        det,
    );
    assert!(
        !result.resolved_params.contains_key(&connector_len),
        "excluded connector cell must not appear in resolved_params",
    );
}

// ---------------------------------------------------------------------------
// amendment (review round 1): cost_robustness_lambda first-found-wins.
// ---------------------------------------------------------------------------

/// Same {A, B} 2-cycle SCC (guarantees ONE `MergedSolve` cluster spanning
/// both scopes, independent of objectives), but A and B EACH declare their
/// OWN `cost_robustness_tradeoff` objective with a DIFFERENT lambda.
fn two_cycle_cluster_with_differing_lambdas_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .objective(ObjectiveSet::cost_robustness_tradeoff(value_ref("A", "k"), 0.25))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .objective(ObjectiveSet::cost_robustness_tradeoff(value_ref("B", "m"), 0.75))
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// When two cluster members set DIFFERING `cost_robustness_lambda`, the
/// merged objective must keep the FIRST-FOUND value (`cluster.scopes` order
/// -- A before B) and push a warning `Diagnostic` about the divergence
/// (documented precedence, §5.2, amendment task #5014).
#[test]
fn merged_cluster_first_found_lambda_wins_with_divergence_warning() {
    let module = two_cycle_cluster_with_differing_lambdas_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let mut solved = HashMap::new();
    solved.insert(a_k.clone(), mm(3.0));
    solved.insert(b_m.clone(), mm(7.0));

    let spy = SpyConstraintSolver::new_solved(solved);
    let captured = spy.captured_problem();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("solver must have been called with a merged ResolutionProblem");
    let objective = problem
        .objective
        .expect("merged problem must carry a spanning objective");
    assert_eq!(
        objective.cost_robustness_lambda,
        Some(0.25),
        "merged objective must keep A's (first-found, cluster.scopes order) \
         lambda -- not B's later-set 0.75; got {:?}",
        objective.cost_robustness_lambda,
    );

    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning && d.message.contains("cost_robustness_lambda")
        })
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a warning Diagnostic about the cost_robustness_lambda \
         divergence between cluster members; got: {:#?}",
        result.diagnostics,
    );
    assert!(
        warnings[0].message.contains("0.75") && warnings[0].message.contains("0.25"),
        "divergence warning must name both the differing (0.75) and the \
         already-governing (0.25) lambda values; got: {}",
        warnings[0].message,
    );
}

// ---------------------------------------------------------------------------
// amendment (review round 1): Infeasible / NoProgress / iteration-limit.
// ---------------------------------------------------------------------------

/// An `Infeasible` merged solve must propagate the solver's diagnostics
/// verbatim into `EvalResult.diagnostics`, and -- with `capture_undef_causes`
/// enabled -- record `UndefCause::SolveFailed` for EVERY cluster member's
/// auto cell (not just the first-in-order member), keyed off the merged
/// problem's full `auto_params` union (amendment, task #5014).
#[test]
fn merged_cluster_infeasible_propagates_diagnostics_and_failed_autos_for_every_member() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Infeasible {
        diagnostics: vec![Diagnostic::error(
            "no feasible assignment satisfies the merged {A,B} constraint set",
        )],
    }]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    engine.set_capture_undef_causes(true);
    let result = engine.eval(&module);

    assert!(
        result.diagnostics.iter().any(
            |d| d.severity == Severity::Error && d.message.contains("no feasible assignment")
        ),
        "solver's Infeasible diagnostics must propagate into \
         EvalResult.diagnostics; got: {:#?}",
        result.diagnostics,
    );

    let causes = engine.undef_causes();
    assert_eq!(
        causes.get(&a_k),
        Some(&UndefCause::SolveFailed {
            detail: "infeasible".to_string()
        }),
        "A.k must be recorded as SolveFailed(\"infeasible\") -- got {:?}",
        causes.get(&a_k),
    );
    assert_eq!(
        causes.get(&b_m),
        Some(&UndefCause::SolveFailed {
            detail: "infeasible".to_string()
        }),
        "B.m must ALSO be recorded as SolveFailed -- not just the \
         first-in-order cluster member; got {:?}",
        causes.get(&b_m),
    );

    assert!(
        !result.resolved_params.contains_key(&a_k) && !result.resolved_params.contains_key(&b_m),
        "an Infeasible merged solve must not write back any resolved_params",
    );
}

/// A `NoProgress` merged solve must push a warning `Diagnostic` naming the
/// reason, and -- with `capture_undef_causes` enabled -- record
/// `UndefCause::SolveFailed` for EVERY cluster member's auto cell.
#[test]
fn merged_cluster_no_progress_propagates_diagnostic_and_failed_autos_for_every_member() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::NoProgress {
        reason: "solver stalled on the merged {A,B} problem".to_string(),
    }]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    engine.set_capture_undef_causes(true);
    let result = engine.eval(&module);

    assert!(
        result.diagnostics.iter().any(|d| d.severity == Severity::Warning
            && d.message.contains("solver stalled on the merged")),
        "solver's NoProgress reason must surface as a warning Diagnostic; \
         got: {:#?}",
        result.diagnostics,
    );

    let expected_detail = "no progress: solver stalled on the merged {A,B} problem".to_string();
    let causes = engine.undef_causes();
    assert_eq!(
        causes.get(&a_k),
        Some(&UndefCause::SolveFailed {
            detail: expected_detail.clone()
        }),
        "A.k must be recorded as SolveFailed with the NoProgress reason -- \
         got {:?}",
        causes.get(&a_k),
    );
    assert_eq!(
        causes.get(&b_m),
        Some(&UndefCause::SolveFailed {
            detail: expected_detail
        }),
        "B.m must ALSO be recorded as SolveFailed -- not just the \
         first-in-order cluster member; got {:?}",
        causes.get(&b_m),
    );
}

/// A solver that returns `RankedSolveResult::Ranked` with
/// `OptimalityStatus::BestFound { reason: BestFoundReason::IterationLimit }`.
///
/// No shared `reify-test-support` mock constructs this directly: its spies
/// only implement `ConstraintSolver::solve` and rely on the trait's default
/// `solve_ranked` lift, which always reports `BestFoundReason::Unreported`
/// (see `reify_ir::ConstraintSolver::solve_ranked`'s doc) -- never
/// `IterationLimit`. Mirrors the locally-defined `EmptyRankedSolver` in
/// `reify-eval/tests/solver_optimality_unproven.rs`.
struct IterationLimitRankedSolver {
    values: HashMap<ValueCellId, Value>,
}

impl ConstraintSolver for IterationLimitRankedSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        SolveResult::Solved {
            values: self.values.clone(),
            unique: true,
        }
    }

    fn solve_ranked(&self, _problem: &ResolutionProblem) -> RankedSolveResult {
        RankedSolveResult::Ranked {
            candidates: vec![RankedCandidate {
                values: self.values.clone(),
                objective_score: None,
                unique: true,
            }],
            optimality: OptimalityStatus::BestFound {
                reason: BestFoundReason::IterationLimit,
            },
        }
    }
}

/// A merged spanning-objective solve whose solver hits the iteration limit
/// must surface `W_SOLVER_OPTIMALITY_UNPROVEN` -- identically to the
/// per-template objective branch (γ, task #4804) -- generalized to the
/// merged cluster path (amendment, task #5014).
#[test]
fn merged_cluster_iteration_limit_emits_solver_optimality_unproven() {
    let module = spanning_objective_cluster_module();

    let mut solved = HashMap::new();
    solved.insert(ValueCellId::new("Parent", "total"), mm(1.0));
    solved.insert(ValueCellId::new("ChildA", "cost"), mm(2.0));
    solved.insert(ValueCellId::new("ChildB", "cost"), mm(3.0));

    let spy = IterationLimitRankedSolver { values: solved };

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverOptimalityUnproven))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a DiagnosticCode::SolverOptimalityUnproven warning when \
         the merged solve hits the iteration limit; got: {:#?}",
        result.diagnostics,
    );
    assert_eq!(warnings[0].severity, Severity::Warning);
    assert!(
        warnings[0].message.contains("W_SOLVER_OPTIMALITY_UNPROVEN"),
        "message must contain the user-observable mnemonic; got: {}",
        warnings[0].message,
    );
}

// ---------------------------------------------------------------------------
// amendment (review round 1): all-excluded cluster skips the solver call.
// ---------------------------------------------------------------------------

/// A `MergedSolve` cluster whose EVERY auto cell (across ALL member scopes)
/// is a strict connector-instance auto. The cycle is formed structurally by
/// having each constraint read the OTHER scope's connector-shaped auto cell
/// directly by its exact `ValueCellId` -- clustering in resolve_order.rs
/// keys strictly off `auto_owner` (exact `ValueCellId` match), not scope
/// name, so no real connector/sub-component wiring is needed here either.
fn all_connector_autos_two_cycle_cluster_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A.__connector_0", "k", Type::length())
        .constraint(
            "A",
            0,
            None,
            gt(value_ref("B.__connector_0", "m"), literal(mm(0.0))),
        )
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B.__connector_0", "m", Type::length())
        .constraint(
            "B",
            0,
            None,
            gt(value_ref("A.__connector_0", "k"), literal(mm(0.0))),
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// After the union/exclusion loop, if EVERY auto cell a `MergedSolve`
/// cluster contributed was a strict connector-instance auto, `auto_params`
/// comes back empty. `dispatch_merged_cluster_solve` must skip the solver
/// call entirely (mirroring `build_solver_problem`'s `None`-means-skip
/// contract) rather than issuing a spurious/misleading solve over zero auto
/// params (amendment, task #5014).
#[test]
fn merged_cluster_skips_solver_when_every_auto_is_excluded() {
    let module = all_connector_autos_two_cycle_cluster_module();

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: HashMap::new(),
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    assert_eq!(
        captured.lock().unwrap().len(),
        0,
        "solver must NEVER be invoked when every auto cell in the cluster is \
         excluded by the strict connector-instance guard -- a zero-auto-param \
         solve would be spurious/misleading",
    );

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        2,
        "both cluster members' excluded connector autos must each surface \
         their own error Diagnostic; got: {:#?}",
        result.diagnostics,
    );
    let combined: String = errors
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        combined.contains("`A`") && combined.contains("`k`"),
        "expected an error naming template `A` and member `k`; got: {combined}",
    );
    assert!(
        combined.contains("`B`") && combined.contains("`m`"),
        "expected an error naming template `B` and member `m`; got: {combined}",
    );

    let connector_k = ValueCellId::new("A.__connector_0", "k");
    let connector_m = ValueCellId::new("B.__connector_0", "m");
    assert!(
        !result.resolved_params.contains_key(&connector_k)
            && !result.resolved_params.contains_key(&connector_m),
        "neither excluded connector cell can appear in resolved_params -- no \
         solve ever ran",
    );
}
