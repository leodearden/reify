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
use reify_core::{Diagnostic, DiagnosticCode, ModulePath, Severity, Type, ValueCellId, VersionId};
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::journal::{EventKind, EventPayload};
use reify_ir::{
    BestFoundReason, BinOp, CompiledExpr, CompiledExprKind, ConstraintSolver, DeterminacyState,
    ObjectiveCombination, ObjectiveSense, ObjectiveSet, ObjectiveTerm, OptimalityStatus,
    RankedCandidate, RankedSolveResult, ResolutionProblem, SnapshotProvenance, SolveResult,
    UndefCause, Value,
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
    assert_eq!(
        *a_val,
        mm(3.0),
        "A.k's value must be the merged solve's result"
    );
    let b_val = result.values.get(&b_m).expect(
        "B.m missing from EvalResult.values -- not just A.k (the \
                 first-iterated cluster member)",
    );
    assert_eq!(
        *b_val,
        mm(7.0),
        "B.m's value must be the merged solve's result"
    );

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

/// Amendment (task #5014, reviewer_comprehensive round 3, suggestion 2;
/// loosened round 4 suggestion 3): pins the MEMBER-NAME SET AND ORDER
/// encoded in `SnapshotProvenance::Resolution.scope` after a merged solve --
/// not the exact joined string -- so a future change to `merged_scope_label`
/// (`dispatch_merged_cluster_solve`) still fails this test if it drops or
/// reorders a member name, while surviving a benign separator/format change
/// (e.g. switching from ", " to another delimiter). `scope` stays a plain
/// `String` -- confirmed by grepping every in-tree read site: the only OTHER
/// consumers are (a) this crate's own per-template write site (a single
/// template name, unaffected by this change), (b) `reify-ir`'s definition +
/// its own unit test, and (c) `resolution.rs`'s per-template equality
/// assertion -- none parse or key off the string, all treat it as opaque
/// display/equality text. The unrelated `ResolutionNodeId`/
/// `ResolutionNodeData` cache-key type (a different "Resolution" concept
/// entirely, in `graph.rs`/`cache.rs`) is always constructed directly from a
/// template/entity name, never derived from
/// `SnapshotProvenance::Resolution.scope`, so it cannot be affected by this
/// format either.
#[test]
fn merged_cluster_snapshot_provenance_scope_is_comma_joined_member_names() {
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
    engine.eval(&module);

    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()");
    match &snapshot.provenance {
        SnapshotProvenance::Resolution { scope, .. } => {
            let member_names: Vec<&str> = scope.split(", ").collect();
            assert_eq!(
                member_names,
                vec!["A", "B"],
                "merged scope label must list every cluster member's \
                 template name, in cluster.scopes (ascending source index) \
                 order -- NOT a single member's name; got {scope:?}",
            );
        }
        other => panic!(
            "expected SnapshotProvenance::Resolution after a merged solve; \
             got {other:?}"
        ),
    }
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

/// Structural equality check for the `CompiledExprKind` shapes this test
/// module's fixtures ever construct (`BinOp` of `ValueRef` leaves, e.g.
/// `ChildA.cost + ChildB.cost`). `CompiledExpr`/`CompiledExprKind` have no
/// `PartialEq`, and comparing via `format!("{:?}", ..)` (the prior approach
/// here) is brittle: it couples the test to `CompiledExprKind`'s Debug
/// derive (any field-order/metadata change breaks the test with no
/// behavioral regression) and would falsely PASS two structurally different
/// exprs that happen to Debug-format identically. Recurses through `BinOp`
/// so nested sums compare correctly too; panics on any other variant since
/// no fixture in this file needs one -- extend here if a future test does.
fn assert_expr_structurally_eq(actual: &CompiledExpr, expected: &CompiledExpr, ctx: &str) {
    match (&actual.kind, &expected.kind) {
        (
            CompiledExprKind::BinOp {
                op: a_op,
                left: a_left,
                right: a_right,
            },
            CompiledExprKind::BinOp {
                op: e_op,
                left: e_left,
                right: e_right,
            },
        ) => {
            assert_eq!(a_op, e_op, "{ctx}: BinOp operator mismatch");
            assert_expr_structurally_eq(a_left, e_left, &format!("{ctx} (left operand)"));
            assert_expr_structurally_eq(a_right, e_right, &format!("{ctx} (right operand)"));
        }
        (CompiledExprKind::ValueRef(a_id), CompiledExprKind::ValueRef(e_id)) => {
            assert_eq!(a_id, e_id, "{ctx}: ValueRef target mismatch");
        }
        (a_kind, e_kind) => panic!(
            "{ctx}: expected matching CompiledExprKind variants (both BinOp \
             or both ValueRef -- the only shapes this helper supports); got \
             actual={a_kind:?} vs expected={e_kind:?}",
        ),
    }
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

    let objective = problem
        .objective
        .expect("merged problem must carry the spanning objective (Parent's) -- got None");
    assert_eq!(
        objective.combination,
        ObjectiveCombination::WeightedSum,
        "merged objective must combine terms via WeightedSum",
    );

    // Expected == Parent's own objective, rebuilt identically to the fixture
    // above (ObjectiveSet/ObjectiveTerm/CompiledExpr have no PartialEq, so
    // terms are compared structurally field-by-field via
    // `assert_expr_structurally_eq`, not Debug-string equality).
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
    for (i, (actual, want)) in objective
        .terms
        .iter()
        .zip(expected.terms.iter())
        .enumerate()
    {
        assert_eq!(
            actual.sense, want.sense,
            "term {i}'s ObjectiveSense must match Parent's verbatim",
        );
        assert_eq!(
            actual.weight, want.weight,
            "term {i}'s weight must be passed through verbatim -- never \
             re-folded while assembling the merged objective (§5.2)",
        );
        assert_expr_structurally_eq(&actual.expr, &want.expr, &format!("term {i}'s expression"));
    }
}

/// Every resolved cluster cell must carry `ObjectiveProvenance` populated
/// from the spanning objective the merged solve actually used -- not
/// `None`/empty, the pre-β per-template fidelity this path must preserve
/// (`dispatch_merged_cluster_solve`'s per-cell write-back, amendment task
/// #5014): `objective.is_some()`, `combination == WeightedSum`, non-empty
/// `term_contributions`, and `scope` equal to the CELL'S OWN owning entity
/// (not the cluster's anchor/first-dispatched member).
#[test]
fn merged_cluster_objective_provenance_populated_for_every_resolved_cell() {
    let module = spanning_objective_cluster_module();

    let parent_total = ValueCellId::new("Parent", "total");
    let child_a_cost = ValueCellId::new("ChildA", "cost");
    let child_b_cost = ValueCellId::new("ChildB", "cost");

    let mut solved = HashMap::new();
    solved.insert(parent_total.clone(), mm(1.0));
    solved.insert(child_a_cost.clone(), mm(2.0));
    solved.insert(child_b_cost.clone(), mm(3.0));

    let spy = SpyConstraintSolver::new_solved(solved);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval(&module);

    for (id, want_scope) in [
        (&parent_total, "Parent"),
        (&child_a_cost, "ChildA"),
        (&child_b_cost, "ChildB"),
    ] {
        let provenance = result.objective_provenance.get(id).unwrap_or_else(|| {
            panic!(
                "objective_provenance missing an entry for {id:?} -- every \
                 resolved cluster cell must carry provenance from the \
                 spanning objective"
            )
        });
        assert!(
            provenance.objective.is_some(),
            "{id:?}'s objective_provenance.objective must be Some (the \
             spanning objective), not None",
        );
        assert_eq!(
            provenance.combination,
            Some(ObjectiveCombination::WeightedSum),
            "{id:?}'s objective_provenance.combination must be WeightedSum",
        );
        assert!(
            !provenance.term_contributions.is_empty(),
            "{id:?}'s objective_provenance.term_contributions must be \
             non-empty; got an empty Vec",
        );
        assert_eq!(
            provenance.scope, want_scope,
            "{id:?}'s objective_provenance.scope must be the CELL'S OWN \
             owning entity, not the cluster's anchor member",
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
    let result = engine.eval(&module);

    let problem = captured
        .lock()
        .unwrap()
        .clone()
        .expect("solver must have been called with a merged ResolutionProblem");

    let objective = problem
        .objective
        .expect("merged problem must carry the spanning objective (Parent's) -- got None");

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

    // Per-cell `inherited_from` routing (reviewer_comprehensive, amendment
    // task #5014): `dispatch_merged_cluster_solve` looks up each resolved
    // cell's OWNING member's governance, not `idx`'s (whichever cluster
    // member triggered the merged dispatch) -- so ChildA/ChildB (which
    // INHERIT Parent's objective) must carry `inherited_from ==
    // Some("Parent")`, while Parent (which owns its objective) must carry
    // `inherited_from == None`.
    assert_eq!(
        result
            .objective_provenance
            .get(&child_a_cost)
            .expect("objective_provenance missing an entry for ChildA.cost")
            .inherited_from,
        Some("Parent".to_string()),
        "ChildA.cost's objective is INHERITED from Parent -- its \
         objective_provenance.inherited_from must route to the OWNING \
         member's (ChildA's) governance",
    );
    assert_eq!(
        result
            .objective_provenance
            .get(&child_b_cost)
            .expect("objective_provenance missing an entry for ChildB.cost")
            .inherited_from,
        Some("Parent".to_string()),
        "ChildB.cost's objective is INHERITED from Parent -- same per-cell \
         routing as ChildA.cost",
    );
    assert_eq!(
        result
            .objective_provenance
            .get(&parent_total)
            .expect("objective_provenance missing an entry for Parent.total")
            .inherited_from,
        None,
        "Parent.total's objective is its OWN (not inherited) -- \
         inherited_from must be None, not mistakenly propagated from \
         another cluster member's governance",
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

    let ids0: Vec<ValueCellId> = problems[0]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    let ids1: Vec<ValueCellId> = problems[1]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    assert!(
        (ids0 == vec![x_a.clone()] && ids1 == vec![y_b.clone()])
            || (ids0 == vec![y_b.clone()] && ids1 == vec![x_a.clone()]),
        "each per-template solve must carry ONLY its own scope's single auto \
         id -- no cross-scope union; got {:?} then {:?}",
        ids0,
        ids1,
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
fn with_n_autos(mut b: TopologyTemplateBuilder, entity: &str, n: usize) -> TopologyTemplateBuilder {
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
    let alpha = with_n_autos(
        TopologyTemplateBuilder::new("Alpha"),
        "Alpha",
        OVER_CAP_AUTOS,
    )
    .constraint(
        "Alpha",
        0,
        None,
        gt(value_ref("Beta", "a0"), literal(mm(0.0))),
    )
    .build();
    let beta = with_n_autos(TopologyTemplateBuilder::new("Beta"), "Beta", OVER_CAP_AUTOS)
        .constraint(
            "Beta",
            0,
            None,
            gt(value_ref("Alpha", "a0"), literal(mm(0.0))),
        )
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

    let ids0: Vec<ValueCellId> = problems[0]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    let ids1: Vec<ValueCellId> = problems[1]
        .auto_params
        .iter()
        .map(|ap| ap.id.clone())
        .collect();
    assert!(
        (ids0 == alpha_ids && ids1 == beta_ids) || (ids0 == beta_ids && ids1 == alpha_ids),
        "each per-template solve must carry ONLY its own scope's {} auto ids \
         -- no cross-scope union with the other scope; got {} then {} ids",
        OVER_CAP_AUTOS,
        ids0.len(),
        ids1.len(),
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
    assert_eq!(
        *val,
        Value::Undef,
        "excluded connector cell must stay Undef"
    );
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
        .objective(ObjectiveSet::cost_robustness_tradeoff(
            value_ref("A", "k"),
            0.25,
        ))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .objective(ObjectiveSet::cost_robustness_tradeoff(
            value_ref("B", "m"),
            0.75,
        ))
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
        .filter(|d| d.severity == Severity::Warning && d.message.contains("cost_robustness_lambda"))
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
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("no feasible assignment")),
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
        result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
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

    // Amendment (task #5014, reviewer_comprehensive round 3, suggestion 1):
    // when the WHOLE cluster's auto_params comes back empty, one more
    // diagnostic must name the full cluster so the (here, vacuous, since
    // BOTH members are excluded) collateral is an observable signal rather
    // than silence. A dedicated two-member fixture where only one member
    // contributes the excluded cell isn't needed to pin this: this test
    // already exercises the exact early-return code path the new
    // diagnostic is pushed from, and the message names every cluster
    // member unconditionally (not just the excluded one).
    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings
            .iter()
            .any(|d| d.message.contains("MergedSolve cluster")
                && d.message.contains("[A, B]")
                && d.message.contains("entirely unresolved")),
        "expected a warning Diagnostic naming the WHOLE cluster ([A, B]) as \
         left entirely unresolved; got: {:#?}",
        result.diagnostics,
    );

    let connector_k = ValueCellId::new("A.__connector_0", "k");
    let connector_m = ValueCellId::new("B.__connector_0", "m");
    assert!(
        !result.resolved_params.contains_key(&connector_k)
            && !result.resolved_params.contains_key(&connector_m),
        "neither excluded connector cell can appear in resolved_params -- no \
         solve ever ran",
    );

    // Amendment (task #5014): the early-return arm must leave EVERY member's
    // cells in their pre-seeded `Auto`/`Undef` state, not just absent from
    // `resolved_params` -- mirrors the snapshot-state check
    // `merged_cluster_excludes_strict_connector_instance_auto` makes for its
    // (partial-exclusion) connector cell.
    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()");
    for id in [&connector_k, &connector_m] {
        let (val, det) = snapshot
            .values
            .get(id)
            .unwrap_or_else(|| panic!("{id:?} must still be pre-seeded in the snapshot"));
        assert_eq!(
            *val,
            Value::Undef,
            "{id:?} must stay Undef -- no solve ever ran"
        );
        assert_eq!(
            *det,
            DeterminacyState::Auto,
            "{id:?} must never reach Determined when the whole cluster is left \
             unresolved -- got {det:?}",
        );
    }
}

// ---------------------------------------------------------------------------
// task #5118: warm eval_cached() co-solves within-cap MergedSolve clusters,
// closing the cold/warm fidelity divergence (esc-5014-10 Option A) for
// constraint-only clusters. Objective-bearing clusters retain a documented,
// permitted solve()/solve_ranked divergence (design decision #4) — see
// `eval_vs_eval_cached_merged_cluster_objective_cluster_may_diverge_by_solve_entrypoint`
// further below.
// ---------------------------------------------------------------------------

/// `eval_cached()` (the warm LSP/GUI incremental path) must now merge the
/// SAME within-cap {A, B} `MergedSolve` cluster that the cold `eval()`
/// driver merges into ONE solve
/// (`merged_cluster_dispatches_single_solve_with_union_auto_params` above)
/// -- NOT solve it per-template (2 calls). Both paths call the SAME
/// `build_merged_solver_problem`, so warm and cold now feed byte-identical
/// inputs to the solver (task #5118, closing the divergence the earlier
/// `eval_cached_does_not_merge_within_cap_cluster_unlike_cold_eval` pinned).
#[test]
fn eval_cached_merges_within_cap_cluster_like_cold_eval() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let mut combined = HashMap::new();
    combined.insert(a_k.clone(), mm(3.0));
    combined.insert(b_m.clone(), mm(7.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: combined,
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval_cached(&module, VersionId(1));

    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "eval_cached must merge the {{A,B}} cluster into exactly ONE solve, \
         just like the cold eval() path -- got {} call(s)",
        captured.lock().unwrap().len(),
    );

    assert_eq!(
        result.eval_result.values.get(&a_k),
        Some(&mm(3.0)),
        "A.k must reflect the merged solve's co-solved value written back \
         to every cluster member, not a frozen/per-template value",
    );
    assert_eq!(
        result.eval_result.values.get(&b_m),
        Some(&mm(7.0)),
        "B.m must reflect the merged solve's co-solved value written back \
         to every cluster member, not a frozen/per-template value",
    );
}

/// Warm analog of `merged_cluster_let_surfaces_co_solved_cross_scope_auto`
/// (above, cold `eval()`): `eval_cached()`'s merged-cluster dispatch must
/// ALSO re-evaluate B's downstream `let` cell (`B.out = A.k * 2`) against the
/// CO-SOLVED A.k, not a frozen/undef value from B's pre-merge-solve main
/// pass.
///
/// GREEN at step-06: `dispatch_merged_cluster_solve_cached` re-evaluates
/// downstream let cones on the warm path (wave-2), byte-for-byte mirroring the
/// per-template warm arm (engine_eval.rs ~6636).
///
/// PRECONDITION (esc-5118-2): warm downstream-let wave-2 re-eval is gated on a
/// populated `self.eval_state` (built by a prior cold `eval()`), exactly like
/// every other warm-backprop test in this repo — see
/// `warm_eval_cached_with_solver` (tests/common/differential.rs:1380), whose
/// doc-comment states the cold `eval()` is required "so eval_state is
/// populated". This is NOT a test crutch: the sole G1 consumer, the LSP
/// keystroke path (reify-lsp/src/diagnostics.rs:137-149), routes an
/// uninitialized engine to cold `eval()` BY CONSTRUCTION
/// (`content_unchanged = … && state.is_engine_initialized()`), so
/// `eval_cached()` never runs downstream-let back-prop on a fresh engine in
/// production. The cold `eval()` below establishes that same precondition; the
/// subsequent `eval_cached()` is what this test actually exercises (merged
/// co-solve + wave-2 on the WARM path). The spy repeats its last sequenced
/// result, so both solves see the same `Solved` outcome.
///
/// SCOPE NOTE (reviewer_comprehensive, task #5118 amendment): this pins ONE
/// warm call following the cold `eval()`, not a SECOND consecutive
/// `eval_cached` call. `eval_cached` unconditionally rebuilds
/// `self.eval_state` at the end of every call with an empty
/// `reverse_index` (`ReverseDependencyIndex::default()`), so the warm merged
/// dispatch's inline wave-2 re-eval this test exercises only
/// finds real dependents on the FIRST warm call after a cold `eval()` — a
/// second consecutive `eval_cached` would see an empty reverse index and
/// silently skip re-evaluating `B.out` against that call's newly co-solved
/// `A.k`, even though `A.k` itself stays current (written unconditionally
/// by the primary, non-wave-2 write-back). This mirrors a pre-existing
/// limitation of the per-template warm arm the helper was extracted from —
/// see that helper's doc comment for the full rationale — and is not
/// exercised here; broader multi-call coverage is a follow-up.
#[test]
fn eval_cached_merged_cluster_let_surfaces_co_solved_cross_scope_auto() {
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
    // Establish the warm precondition: cold eval() populates eval_state (and
    // demand) so the subsequent eval_cached() wave-2 can re-eval downstream
    // lets. Mirrors warm_eval_cached_with_solver + the LSP consumer's
    // is_engine_initialized() guard (see doc-comment above).
    engine.eval(&module);
    let result = engine.eval_cached(&module, VersionId(1));

    let out_val = result
        .eval_result
        .values
        .get(&b_out)
        .expect("B.out missing from EvalResult.values");
    assert_eq!(
        *out_val,
        mm(8.0),
        "B.out = A.k * 2 must surface the CO-SOLVED A.k (4mm -> 8mm) on the \
         warm eval_cached path too, not a frozen/undef value from B's \
         pre-merge-solve main pass; got {:?}",
        out_val,
    );
}

/// Pins the SECOND-consecutive-`eval_cached`-call staleness the SCOPE NOTE
/// above (and the warm merged dispatch's inline wave-2, engine_eval.rs)
/// documents but did not test (reviewer_comprehensive, task #5118 amendment,
/// suggestion 3): `eval_cached` unconditionally rebuilds `self.eval_state` at
/// the end of EVERY call with an empty `ReverseDependencyIndex::default()`,
/// so the wave-2 downstream-let re-eval only finds real dependents on the
/// FIRST warm call after a cold `eval()`. A SECOND consecutive `eval_cached`
/// call whose merged solve produces a NEW co-solved auto value leaves B's
/// downstream `let` cell (`B.out = A.k * 2`) STALE -- still reflecting the
/// FIRST call's co-solved value -- even though the auto cell it reads
/// (`A.k`) itself stays current (written unconditionally every call by the
/// primary, non-wave-2 write-back loop). Tracked as a follow-up, task #5224,
/// not fixed here; this test exists so a future reverse-index fix has a
/// failing assertion to flip (suggested_fix: "add a test that asserts ... the
/// second-call downstream-let staleness").
#[test]
fn eval_cached_second_consecutive_call_downstream_let_stays_stale_pending_5224() {
    let module = two_cycle_cluster_with_cross_scope_let_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    let b_out = ValueCellId::new("B", "out");

    let mut cold_solved = HashMap::new();
    cold_solved.insert(a_k.clone(), mm(1.0));
    cold_solved.insert(b_m.clone(), mm(1.0));

    let mut first_warm_solved = HashMap::new();
    first_warm_solved.insert(a_k.clone(), mm(4.0));
    first_warm_solved.insert(b_m.clone(), mm(1.0));

    let mut second_warm_solved = HashMap::new();
    second_warm_solved.insert(a_k.clone(), mm(10.0));
    second_warm_solved.insert(b_m.clone(), mm(1.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved {
            values: cold_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: first_warm_solved,
            unique: true,
        },
        SolveResult::Solved {
            values: second_warm_solved,
            unique: true,
        },
    ]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Establish the warm precondition (see SCOPE NOTE above): cold eval()
    // populates eval_state so the FIRST eval_cached's wave-2 can re-eval
    // downstream lets.
    engine.eval(&module);

    // First warm call: wave-2 finds B.out's dependency on A.k via the
    // populated reverse index and re-evaluates it -- matches
    // `eval_cached_merged_cluster_let_surfaces_co_solved_cross_scope_auto`.
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1.eval_result.values.get(&b_out),
        Some(&mm(8.0)),
        "first warm call: B.out must reflect the co-solved A.k (4mm -> 8mm)",
    );

    // Second consecutive warm call: eval_cached rebuilt self.eval_state at
    // the end of the FIRST call with an empty reverse index, so THIS call's
    // wave-2 finds no dependents for the newly co-solved A.k.
    let result2 = engine.eval_cached(&module, VersionId(2));

    assert_eq!(
        result2.eval_result.values.get(&a_k),
        Some(&mm(10.0)),
        "A.k itself must stay current -- it is written unconditionally by \
         the primary (non-wave-2) write-back loop every call, regardless of \
         the reverse-index gap",
    );
    assert_eq!(
        result2.eval_result.values.get(&b_out),
        Some(&mm(8.0)),
        "known limitation (task #5224): B.out is NOT re-evaluated against \
         the second call's new A.k (10mm -> would be 20mm) -- it stays \
         stale at the FIRST call's co-solved value because wave-2's \
         reverse-index is empty on every call after the first. If this \
         assertion starts failing with Some(mm(20.0)), #5224 has been fixed \
         and this test (plus its doc comment and the SCOPE NOTE above) \
         should be updated to match.",
    );
}

/// Warm analog of `merged_cluster_skips_solver_when_every_auto_is_excluded`
/// (above, cold `eval()`): when every auto cell a `MergedSolve` cluster
/// contributed is a strict connector-instance auto, `eval_cached()`'s
/// `dispatch_merged_cluster_solve_cached` must ALSO skip the solver call
/// entirely and push the cluster-wide "left entirely unresolved" warning,
/// rather than attempting a spurious solve over zero auto params
/// (reviewer_comprehensive, task #5118 amendment: this warm early-return
/// branch — the empty-`auto_params` guard — had no dedicated test; every
/// other warm test in this file exercises a cluster that actually solves).
#[test]
fn eval_cached_skips_solver_when_every_auto_is_excluded() {
    let module = all_connector_autos_two_cycle_cluster_module();

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: HashMap::new(),
        unique: true,
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval_cached(&module, VersionId(1));

    assert_eq!(
        captured.lock().unwrap().len(),
        0,
        "eval_cached must NEVER invoke the solver when every auto cell in \
         the cluster is excluded by the strict connector-instance guard -- \
         a zero-auto-param solve would be spurious/misleading; got {} \
         call(s)",
        captured.lock().unwrap().len(),
    );

    let errors: Vec<_> = result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        2,
        "both cluster members' excluded connector autos must each surface \
         their own error Diagnostic on the warm path too; got: {:#?}",
        result.eval_result.diagnostics,
    );

    let warnings: Vec<_> = result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert!(
        warnings
            .iter()
            .any(|d| d.message.contains("MergedSolve cluster")
                && d.message.contains("[A, B]")
                && d.message.contains("entirely unresolved")),
        "expected a warning Diagnostic naming the WHOLE cluster ([A, B]) as \
         left entirely unresolved on the warm path too; got: {:#?}",
        result.eval_result.diagnostics,
    );

    let connector_k = ValueCellId::new("A.__connector_0", "k");
    let connector_m = ValueCellId::new("B.__connector_0", "m");
    assert!(
        result.eval_result.values.get_or_undef(&connector_k) == Value::Undef
            && result.eval_result.values.get_or_undef(&connector_m) == Value::Undef,
        "neither excluded connector cell can have been resolved in \
         EvalResult.values -- no solve ever ran",
    );
}

/// Warm analog of
/// `merged_cluster_infeasible_propagates_diagnostics_and_failed_autos_for_every_member`
/// (above, cold `eval()`): an `Infeasible` merged solve on the warm
/// `eval_cached` path must ALSO propagate the solver's diagnostics into
/// `EvalResult.diagnostics` and must not write back any cluster member's
/// value (reviewer_comprehensive, task #5118 amendment:
/// `dispatch_merged_cluster_solve_cached`'s `Infeasible` arm had no
/// dedicated warm test — every other warm test in this file seeds the spy
/// with `Solved`).
#[test]
fn eval_cached_merged_cluster_infeasible_propagates_diagnostics() {
    let module = two_cycle_cluster_module();

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Infeasible {
        diagnostics: vec![Diagnostic::error(
            "no feasible assignment satisfies the merged {A,B} constraint set",
        )],
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval_cached(&module, VersionId(1));

    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "the merged cluster must still dispatch exactly ONE solve attempt \
         even though it comes back Infeasible; got {} call(s)",
        captured.lock().unwrap().len(),
    );
    assert!(
        result
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.message.contains("no feasible assignment")),
        "solver's Infeasible diagnostics must propagate into \
         EvalResult.diagnostics on the warm path too; got: {:#?}",
        result.eval_result.diagnostics,
    );

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    assert!(
        result.eval_result.values.get_or_undef(&a_k) == Value::Undef
            && result.eval_result.values.get_or_undef(&b_m) == Value::Undef,
        "an Infeasible merged solve must not write back any cluster \
         member's value on the warm path",
    );
}

/// Warm analog of
/// `merged_cluster_no_progress_propagates_diagnostic_and_failed_autos_for_every_member`
/// (above, cold `eval()`): a `NoProgress` merged solve on the warm
/// `eval_cached` path must ALSO push a warning `Diagnostic` naming the
/// solver's reason, and must not write back any cluster member's value
/// (reviewer_comprehensive, task #5118 amendment:
/// `dispatch_merged_cluster_solve_cached`'s `NoProgress` arm had no
/// dedicated warm test).
#[test]
fn eval_cached_merged_cluster_no_progress_propagates_diagnostic() {
    let module = two_cycle_cluster_module();

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::NoProgress {
        reason: "solver stalled on the merged {A,B} problem".to_string(),
    }]);
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));
    let result = engine.eval_cached(&module, VersionId(1));

    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "the merged cluster must still dispatch exactly ONE solve attempt \
         even though it makes NoProgress; got {} call(s)",
        captured.lock().unwrap().len(),
    );
    assert!(
        result
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Warning
                && d.message.contains("solver stalled on the merged")),
        "solver's NoProgress reason must surface as a warning Diagnostic on \
         the warm path too; got: {:#?}",
        result.eval_result.diagnostics,
    );

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");
    assert!(
        result.eval_result.values.get_or_undef(&a_k) == Value::Undef
            && result.eval_result.values.get_or_undef(&b_m) == Value::Undef,
        "a NoProgress merged solve must not write back any cluster \
         member's value on the warm path",
    );
}

// ---------------------------------------------------------------------------
// step-07/08: capstone G2 value-parity — cold eval() vs warm eval_cached()
// for an OBJECTIVE-formed (not SCC-read-cycle) MergedSolve cluster.
// ---------------------------------------------------------------------------

/// The user-observable signal task #5118 exists to fix: cold `eval()` and
/// warm `eval_cached()` must now co-solve the SAME `MergedSolve` cluster to
/// the SAME values, closing the cold/warm fidelity divergence (esc-5014-10
/// Option A). Uses `spanning_objective_cluster_module` (Parent minimizes
/// ChildA.cost+ChildB.cost) rather than the {A,B} SCC read-cycle fixture
/// steps 3/5 use -- here the cluster is formed by CROSS-SCOPE OBJECTIVE
/// coupling, proving `resolve_order_ordering_and_clusters` /
/// `cluster_of_scope` picks up both coupling shapes on the warm path, not
/// just read-cycles.
///
/// Also exercises the warm edit-recompute dimension: `eval_cached`'s solver
/// sub-pass is not dirty-gated (it must re-run on every call so
/// Infeasible/NoProgress can surface on every keystroke), so a second warm
/// call at a bumped `VersionId` re-solves the same cluster and must yield
/// the SAME values.
///
/// Would fail if the warm merged dispatch did not thread `governance` into
/// `build_merged_solver_problem` (the objective would be silently dropped,
/// shifting the argmin cold-vs-warm) or otherwise mishandled an
/// objective-formed (as opposed to SCC) cluster. Passes today: step-4 wired
/// `&governance` through `dispatch_merged_cluster_solve_cached`'s call to
/// the SAME shared `build_merged_solver_problem` cold uses, so this test
/// locks that parity as a regression guard.
///
/// NOTE on solver fidelity: `SpyConstraintSolver` returns a FIXED canned
/// `Solved` result regardless of the `ResolutionProblem` it is handed, so a
/// value-equality check ALONE cannot distinguish "objective correctly
/// threaded" from "objective silently dropped" (a dumb mock can't compute a
/// different argmin either way). This test therefore ALSO directly captures
/// the warm dispatch's `ResolutionProblem` and asserts `objective.is_some()`
/// -- the direct regression lock for the governance-threading failure mode
/// this step guards against -- alongside the value-parity check for the
/// write-back/filtering logic.
///
/// Compares `values` only, NOT `resolved_params`: warm `eval_cached()` never
/// populates `resolved_params` (always `HashMap::new()`, by design -- see
/// `EvalResult::resolved_params`'s doc comment and this task's design
/// decision #4), so `values` is the sole field both paths populate and the
/// one the G2 signal (LSP-observed values) actually depends on.
#[test]
fn eval_vs_eval_cached_merged_cluster_values_equal() {
    let module = spanning_objective_cluster_module();

    let parent_total = ValueCellId::new("Parent", "total");
    let child_a_cost = ValueCellId::new("ChildA", "cost");
    let child_b_cost = ValueCellId::new("ChildB", "cost");

    let mut solved = HashMap::new();
    solved.insert(parent_total.clone(), mm(1.0));
    solved.insert(child_a_cost.clone(), mm(2.0));
    solved.insert(child_b_cost.clone(), mm(3.0));

    // Cold eval() -- baseline.
    let solver_a = SpyConstraintSolver::new_solved(solved.clone());
    let mut engine_a =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver_a));
    let result_a = engine_a.eval(&module);

    // Warm eval_cached() -- an independent engine with an identically-seeded
    // deterministic solver, so any value difference is attributable to the
    // warm dispatch, not to solver nondeterminism.
    let solver_b = SpyConstraintSolver::new_solved(solved.clone());
    let captured_b = solver_b.captured_problem();
    let mut engine_b =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver_b));
    let result_b1 = engine_b.eval_cached(&module, VersionId(1));

    // Direct governance-threading check (see NOTE above): the warm merged
    // dispatch's ResolutionProblem must carry Parent's spanning objective,
    // not `None`.
    let problem_b = captured_b
        .lock()
        .unwrap()
        .clone()
        .expect("warm merged dispatch must have called the solver with a ResolutionProblem");
    assert!(
        problem_b.objective.is_some(),
        "warm eval_cached()'s merged dispatch must thread `governance` into \
         build_merged_solver_problem so the spanning objective reaches the \
         solver -- got objective: None",
    );

    for (id, want) in [
        (&parent_total, mm(1.0)),
        (&child_a_cost, mm(2.0)),
        (&child_b_cost, mm(3.0)),
    ] {
        assert_eq!(
            result_a.values.get(id),
            Some(&want),
            "{id:?}: cold eval() must resolve to the solver's co-solved value",
        );
        assert_eq!(
            result_b1.eval_result.values.get(id),
            Some(&want),
            "{id:?}: warm eval_cached() must resolve to the SAME co-solved \
             value as cold eval() for an objective-formed cluster",
        );
    }

    // Warm edit-recompute: a second eval_cached() call at a bumped VersionId
    // must reproduce the SAME co-solved values -- the merged solve reruns
    // unconditionally every call (it is not cache-gated).
    let result_b2 = engine_b.eval_cached(&module, VersionId(2));
    for (id, want) in [
        (&parent_total, mm(1.0)),
        (&child_a_cost, mm(2.0)),
        (&child_b_cost, mm(3.0)),
    ] {
        assert_eq!(
            result_b2.eval_result.values.get(id),
            Some(&want),
            "{id:?}: a second warm recompute (VersionId(2)) must yield the \
             SAME co-solved value as the first",
        );
    }
}

// ---------------------------------------------------------------------------
// amendment (reviewer_comprehensive, task #5118 amendment, suggestion 1/2):
// pin the ONE documented, permitted cold/warm VALUE divergence this task
// leaves open for objective-bearing clusters -- `.solve()` (warm) vs.
// `.solve_ranked()` (cold) disagreeing on which optimum to return.
// ---------------------------------------------------------------------------

/// A solver whose `.solve()` and `.solve_ranked()` deliberately return
/// DIFFERENT resolved values, modeling the REAL divergence documented at
/// `SolverRegistry::solve_ranked` (reify-constraints/src/registry.rs): for a
/// multi-basin, dim>=2 objective-bearing problem, best-of-K multistart can
/// return a candidate that "is not guaranteed byte-identical" to the
/// single-start `solve()` result (see
/// `solve_ranked_multistart_dominates_single_start_solve`,
/// reify-constraints/tests/solver_integration.rs). This is not a contrived
/// mock-only quirk -- it is the same divergence CLASS a real solver exhibits;
/// unlike `SpyConstraintSolver`/`IterationLimitRankedSolver` (which return
/// the SAME values from both entry points), this spy makes the divergence
/// observable.
struct DivergentRankedSolver {
    solve_values: HashMap<ValueCellId, Value>,
    ranked_values: HashMap<ValueCellId, Value>,
}

impl ConstraintSolver for DivergentRankedSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        SolveResult::Solved {
            values: self.solve_values.clone(),
            unique: true,
        }
    }

    fn solve_ranked(&self, _problem: &ResolutionProblem) -> RankedSolveResult {
        RankedSolveResult::Ranked {
            candidates: vec![RankedCandidate {
                values: self.ranked_values.clone(),
                objective_score: Some(0.0),
                unique: true,
            }],
            optimality: OptimalityStatus::BestFound {
                reason: BestFoundReason::IterationLimit,
            },
        }
    }
}

/// `eval_vs_eval_cached_merged_cluster_values_equal` (above) cannot detect
/// the cold/warm divergence documented at `dispatch_merged_cluster_solve_cached`
/// (engine_eval.rs, "Documented, PERMITTED cold/warm divergence") because
/// `SpyConstraintSolver` returns the SAME canned result regardless of whether
/// `.solve()` or `.solve_ranked()` is invoked -- a value-equality check alone
/// is structurally blind to this divergence class (reviewer_comprehensive,
/// task #5118 amendment, suggestion 2). This test uses `DivergentRankedSolver`
/// to make the two entry points disagree and asserts the INTENTIONALLY-PINNED
/// divergence explicitly: cold `eval()` (objective-bearing merged cluster ⇒
/// `solve_ranked`) must take the RANKED candidate; warm `eval_cached()` (plain
/// `.solve()`, unconditionally, per design decision #4) must take the SOLVE
/// candidate. This is documented, accepted behavior, not a regression --
/// see the "Documented, PERMITTED cold/warm divergence" comment cross-referenced
/// above.
#[test]
fn eval_vs_eval_cached_merged_cluster_objective_cluster_may_diverge_by_solve_entrypoint() {
    let module = spanning_objective_cluster_module();

    let parent_total = ValueCellId::new("Parent", "total");
    let child_a_cost = ValueCellId::new("ChildA", "cost");
    let child_b_cost = ValueCellId::new("ChildB", "cost");

    let mut solve_values = HashMap::new();
    solve_values.insert(parent_total.clone(), mm(1.0));
    solve_values.insert(child_a_cost.clone(), mm(2.0));
    solve_values.insert(child_b_cost.clone(), mm(3.0));

    // A DIFFERENT (better-basin) candidate that only `solve_ranked` finds.
    let mut ranked_values = HashMap::new();
    ranked_values.insert(parent_total.clone(), mm(0.5));
    ranked_values.insert(child_a_cost.clone(), mm(0.2));
    ranked_values.insert(child_b_cost.clone(), mm(0.3));

    // Cold eval(): objective-bearing merged cluster -> solve_ranked() -> the
    // RANKED (multistart-best) candidate.
    let solver_a = DivergentRankedSolver {
        solve_values: solve_values.clone(),
        ranked_values: ranked_values.clone(),
    };
    let mut engine_a =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver_a));
    let result_a = engine_a.eval(&module);

    // Warm eval_cached(): merged dispatch always calls plain `.solve()` -> the
    // single-start SOLVE candidate (design decision #4, task #5118 plan.json).
    let solver_b = DivergentRankedSolver {
        solve_values: solve_values.clone(),
        ranked_values: ranked_values.clone(),
    };
    let mut engine_b =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver_b));
    let result_b = engine_b.eval_cached(&module, VersionId(1));

    for (id, cold_want, warm_want) in [
        (&parent_total, mm(0.5), mm(1.0)),
        (&child_a_cost, mm(0.2), mm(2.0)),
        (&child_b_cost, mm(0.3), mm(3.0)),
    ] {
        assert_eq!(
            result_a.values.get(id),
            Some(&cold_want),
            "{id:?}: cold eval() must take solve_ranked()'s top-ranked \
             candidate for an objective-bearing merged cluster",
        );
        assert_eq!(
            result_b.eval_result.values.get(id),
            Some(&warm_want),
            "{id:?}: warm eval_cached() must take plain .solve()'s result -- \
             this is the documented, permitted cold/warm divergence for \
             objective-bearing clusters (reviewer_comprehensive, task #5118 \
             amendment), not a regression",
        );
        assert_ne!(
            cold_want, warm_want,
            "test setup sanity: cold_want/warm_want must actually differ, or \
             this test pins nothing",
        );
    }
}

// ---------------------------------------------------------------------------
// amendment (review round N): objective `combination` is preserved, not
// hardcoded to WeightedSum (reviewer_comprehensive, engine_eval.rs:1710-1716).
// ---------------------------------------------------------------------------

/// Same spanning-cluster shape as `spanning_objective_cluster_module`
/// (Parent(idx0) declares `minimize (ChildA.cost + ChildB.cost)`, ChildA/ChildB
/// own no objective => one MergedSolve cluster [0, 1, 2]), except Parent's
/// objective's own `combination` is `Lexicographic` instead of `WeightedSum`.
fn spanning_objective_cluster_module_lexicographic() -> CompiledModule {
    let parent = TopologyTemplateBuilder::new("Parent")
        .auto_param("Parent", "total", Type::length())
        .objective(ObjectiveSet {
            terms: vec![ObjectiveTerm::new(
                ObjectiveSense::Minimize,
                binop(
                    BinOp::Add,
                    value_ref("ChildA", "cost"),
                    value_ref("ChildB", "cost"),
                ),
            )],
            combination: ObjectiveCombination::Lexicographic,
            cost_robustness_lambda: None,
        })
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

/// The merged `ResolutionProblem` for a cluster with exactly ONE distinct
/// contributing objective must carry that owner's `combination` through
/// VERBATIM -- not silently flatten a `Lexicographic` combination to
/// `WeightedSum`.
///
/// Before this amendment, `build_merged_solver_problem` unconditionally
/// hardcoded `combination: ObjectiveCombination::WeightedSum`, which would
/// have reinterpreted Parent's declared priority ordering ("earlier terms
/// dominate later ones") as a weighted trade-off with no diagnostic -- a
/// topology-dependent change to the optimum (reviewer_comprehensive,
/// engine_eval.rs:1710-1716).
#[test]
fn merged_cluster_preserves_lexicographic_combination_from_single_owner() {
    let module = spanning_objective_cluster_module_lexicographic();

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

    let objective = problem
        .objective
        .expect("merged problem must carry the spanning objective (Parent's) -- got None");
    assert_eq!(
        objective.combination,
        ObjectiveCombination::Lexicographic,
        "merged objective must preserve Parent's OWN `Lexicographic` \
         combination verbatim, not silently flatten it to WeightedSum; got {:?}",
        objective.combination,
    );
    assert_eq!(
        objective.terms.len(),
        1,
        "Parent's single term must still be folded exactly once regardless of \
         combination; got {} term(s)",
        objective.terms.len(),
    );
}

/// Same {A, B} 2-cycle SCC (guarantees ONE `MergedSolve` cluster spanning both
/// scopes, independent of objectives), but A and B EACH declare their OWN
/// objective with a DIFFERENT `combination`.
fn two_cycle_cluster_with_differing_combinations_module() -> CompiledModule {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .objective(ObjectiveSet {
            terms: vec![ObjectiveTerm::new(
                ObjectiveSense::Minimize,
                value_ref("A", "k"),
            )],
            combination: ObjectiveCombination::WeightedSum,
            cost_robustness_lambda: None,
        })
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .objective(ObjectiveSet {
            terms: vec![ObjectiveTerm::new(
                ObjectiveSense::Minimize,
                value_ref("B", "m"),
            )],
            combination: ObjectiveCombination::Lexicographic,
            cost_robustness_lambda: None,
        })
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build()
}

/// When two distinct cluster members declare DIFFERING `combination`s, the
/// merged objective must keep the FIRST-FOUND value (`cluster.scopes` order --
/// A before B) and push a warning `Diagnostic` about the divergence, mirroring
/// `merged_cluster_first_found_lambda_wins_with_divergence_warning`'s
/// first-found-wins contract for `cost_robustness_lambda` (§5.2, amendment
/// task #5014).
#[test]
fn merged_cluster_combination_divergence_warns_and_keeps_first_found() {
    let module = two_cycle_cluster_with_differing_combinations_module();

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
        objective.combination,
        ObjectiveCombination::WeightedSum,
        "merged objective must keep A's (first-found, cluster.scopes order) \
         WeightedSum combination -- not B's later-declared Lexicographic; got \
         {:?}",
        objective.combination,
    );
    assert_eq!(
        objective.terms.len(),
        2,
        "both A's and B's own objective terms must still be folded in -- only \
         the COMBINATION field keeps first-found, term concatenation is \
         unaffected; got {} term(s)",
        objective.terms.len(),
    );

    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning && d.message.contains("combination"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a warning Diagnostic about the combination divergence \
         between cluster members; got: {:#?}",
        result.diagnostics,
    );
    assert!(
        warnings[0].message.contains("Lexicographic")
            && warnings[0].message.contains("WeightedSum"),
        "divergence warning must name both the differing (Lexicographic) and \
         the already-governing (WeightedSum) combination; got: {}",
        warnings[0].message,
    );
}

/// INV-EVAL-1/2 enforcement for the warm merged N-scope commit (task #5118
/// steps 9-10; needed because #5053 γ landed on main, migrating `eval_cached`'s
/// SIBLING per-template + wave-2 arms onto `commit_cell_result`). The co-solved
/// cluster-member autos written by `dispatch_merged_cluster_solve_cached` must
/// route through `commit_cell_result` -- the primitive that writes the
/// `self.journal` leg (a `Started` event carrying an `EventPayload::Custom`
/// trace-source slug) -- NOT the hand-rolled `self.cache.record_evaluation`,
/// which writes ONLY the cache leg and emits no journal `Started`. Reuses THIS
/// file's `two_cycle_cluster_module` + spy solver (no twin harness), mirroring
/// #5053's own `eval_cached_let_miss_provenance_and_determinacy` pattern.
///
/// PRECONDITION (esc-5118-2): a prior cold `eval()` seeds `self.eval_state` +
/// the engine snapshot, the same warm precondition every warm test in this
/// file establishes -- see
/// `eval_cached_merged_cluster_let_surfaces_co_solved_cross_scope_auto`'s doc
/// for the LSP `is_engine_initialized()` rationale. Cold `eval()`'s OWN merged
/// dispatch (`dispatch_merged_cluster_solve`) ALSO journals a `Started` event
/// for A.k/B.m, but with `payload: None`; this test therefore discriminates on
/// the `EventPayload::Custom` provenance slug that ONLY `commit_cell_result`
/// emits, so cold's manual `Started` cannot spuriously satisfy assertion (a).
///
/// RED on the current branch: the Solved arm of
/// `dispatch_merged_cluster_solve_cached` still hand-rolls
/// `self.cache.record_evaluation(...)` (cache leg only, no journal `Started`).
/// GREEN at step 10, once that arm routes through `commit_cell_result`.
#[test]
fn eval_cached_merged_cluster_co_solve_records_commit_provenance() {
    let module = two_cycle_cluster_module();

    let a_k = ValueCellId::new("A", "k");
    let b_m = ValueCellId::new("B", "m");

    let mut combined = HashMap::new();
    combined.insert(a_k.clone(), mm(3.0));
    combined.insert(b_m.clone(), mm(7.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: combined,
        unique: true,
    }]);

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Establish the warm precondition: a prior cold eval() seeds self.eval_state
    // + the engine snapshot (see the step-5 test's doc for the LSP
    // is_engine_initialized() rationale). This ALSO journals cold `Started`
    // events for A.k/B.m with `payload: None` -- the assertions below
    // discriminate on the `Custom` payload so those cannot mask the RED.
    engine.eval(&module);
    // The warm keystroke path under test: co-solves the {A, B} cluster and
    // writes back A.k/B.m. VersionId(2) mirrors #5053's warm-provenance test.
    engine.eval_cached(&module, VersionId(2));

    let snapshot = engine
        .snapshot()
        .expect("engine must have a snapshot after eval()/eval_cached()");

    for id in [&a_k, &b_m] {
        // (a) A `Started` event carrying an `EventPayload::Custom` provenance
        // slug is emitted ONLY by `commit_cell_result` (cell_commit.rs); cold
        // eval()'s merged dispatch emits `Started { payload: None }`, and the
        // current warm `record_evaluation` emits no `Started` at all -- so this
        // is exactly the journal leg the INV-EVAL-1/2 migration (step 10) adds.
        let events = engine.journal().events_for_node(&NodeId::Value(id.clone()));
        assert!(
            events.iter().any(|ev| matches!(ev.kind, EventKind::Started)
                && matches!(ev.payload, Some(EventPayload::Custom(_)))),
            "co-solved cluster-member auto {:?} must have a journal `Started` \
             event with an `EventPayload::Custom` provenance slug -- proving the \
             warm merged write-back routed through `commit_cell_result` \
             (INV-EVAL-1/2), not the hand-rolled `cache.record_evaluation` \
             (cache leg only, no journal `Started`); got {} event(s): {:?}",
            id,
            events.len(),
            events
                .iter()
                .map(|ev| (&ev.kind, &ev.payload))
                .collect::<Vec<_>>(),
        );

        // (b) The co-solved cell is committed `Determined`.
        let (_val, det) = snapshot
            .values
            .get(id)
            .unwrap_or_else(|| panic!("{:?} missing from the final snapshot", id));
        assert_eq!(
            *det,
            DeterminacyState::Determined,
            "co-solved cluster-member auto {:?} must be `Determined`",
            id,
        );
    }
}
