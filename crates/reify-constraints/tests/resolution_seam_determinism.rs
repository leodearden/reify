//! I4 / T1 of `docs/prds/v0_6/solver-driver-parity.md` (§4.4, §4.7): no hash
//! iteration order may reach a `ResolutionProblem` or the result built from
//! one. Driven only through the public API: `decompose_into_components`,
//! `SolverRegistry` and `objective_consumption`.
//!
//! The permutation source is the HASH SEED. Every run happens on a fresh
//! thread, which draws fresh `RandomState` keys, so each run sees a different
//! iteration order for every std map built inside the seam. That seed is the
//! only thing PRD §2.4's axis-order mechanism ever permuted. The CLI twin, two
//! separate `reify eval` processes, is
//! `crates/reify-cli/tests/harness_cli/cli_eval_registry_determinism.rs`.

use reify_constraints::{
    DimensionalSolver, ObjectiveConsumption, SolverRegistry, decompose_into_components,
    objective_consumption,
};
use reify_core::{ConstraintNodeId, Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintSolver, ObjectiveSense, ObjectiveSet,
    RankedSolveResult, ResolutionProblem, SolveResult, Value, ValueMap,
};
use reify_test_support::*;
use std::collections::{BTreeMap, HashMap};

/// Runs per test. A two-way random order passes all of them by chance with
/// probability 2^-15.
const RUNS: usize = 16;

const E: &str = "Part";

/// Run `f` once per run, each on its own thread, joined in order.
fn across_fresh_threads<T: Send>(runs: usize, f: impl Fn() -> T + Sync) -> Vec<T> {
    (0..runs)
        .map(|_| {
            std::thread::scope(|s| {
                s.spawn(&f)
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
        })
        .collect()
}

fn assert_every_run_equals<T: PartialEq + std::fmt::Debug>(runs: &[T], expected: &T, what: &str) {
    for (run, got) in runs.iter().enumerate() {
        assert_eq!(
            got,
            expected,
            "{what}: run {run} of {} diverged",
            runs.len()
        );
    }
}

fn id(member: &str) -> ValueCellId {
    vcid(E, member)
}

fn ids(members: &[&str]) -> Vec<ValueCellId> {
    members.iter().map(|m| id(m)).collect()
}

fn len_ref(member: &str) -> CompiledExpr {
    value_ref(E, member)
}

fn at_least(member: &str, millimetres: f64) -> CompiledExpr {
    ge(len_ref(member), literal(mm(millimetres)))
}

fn numbered(exprs: Vec<CompiledExpr>) -> Vec<(ConstraintNodeId, CompiledExpr)> {
    exprs
        .into_iter()
        .enumerate()
        .map(|(i, e)| (cnid(E, i as u32), e))
        .collect()
}

fn problem(
    autos: &[&str],
    constraints: Vec<CompiledExpr>,
    objective: Option<ObjectiveSet>,
) -> ResolutionProblem {
    ResolutionProblem {
        auto_params: autos.iter().map(|m| single_auto_param(id(m))).collect(),
        constraints: numbered(constraints),
        dependent_cells: Vec::new(),
        current_values: ValueMap::new(),
        objective,
        functions: vec![].into(),
    }
}

fn always_solved_spy() -> MultiCallSpyConstraintSolver {
    MultiCallSpyConstraintSolver::new(vec![SolveResult::Solved {
        values: HashMap::new(),
        unique: true,
    }])
}

/// Solve `p` through a spy-backed registry and return every sub-problem the
/// spy was handed, in CALL order.
fn dispatched_sub_problems(p: &ResolutionProblem) -> Vec<ResolutionProblem> {
    let spy = always_solved_spy();
    let captured = spy.captured_problems();
    let _ = SolverRegistry::new(Box::new(spy)).solve(p);
    let calls = captured.lock().unwrap().clone();
    calls
}

fn axis_order(p: &ResolutionProblem) -> Vec<ValueCellId> {
    p.auto_params.iter().map(|ap| ap.id.clone()).collect()
}

/// Autos declared [a, b, c]; constraints declared [c, a, b], one per component.
fn three_singleton_components() -> ResolutionProblem {
    problem(
        &["a", "b", "c"],
        vec![at_least("c", 1.0), at_least("a", 1.0), at_least("b", 1.0)],
        None,
    )
}

#[test]
fn decompose_returns_components_in_first_constraint_order() {
    let runs = across_fresh_threads(RUNS, || {
        let p = three_singleton_components();
        decompose_into_components(&p.auto_params, &p.constraints, None, &p.dependent_cells)
            .iter()
            .map(|c| {
                let autos = c.auto_params.iter().cloned().collect::<Vec<_>>();
                let constraints = c
                    .constraints
                    .iter()
                    .map(|(cid, _)| cid.clone())
                    .collect::<Vec<_>>();
                (autos, constraints)
            })
            .collect::<Vec<_>>()
    });

    let expected = vec![
        (ids(&["c"]), vec![cnid(E, 0)]),
        (ids(&["a"]), vec![cnid(E, 1)]),
        (ids(&["b"]), vec![cnid(E, 2)]),
    ];
    assert_every_run_equals(
        &runs,
        &expected,
        "components must come back in first-constraint declaration order",
    );
}

#[test]
fn registry_dispatches_components_in_first_constraint_order() {
    let runs = across_fresh_threads(RUNS, || {
        dispatched_sub_problems(&three_singleton_components())
            .iter()
            .map(axis_order)
            .collect::<Vec<_>>()
    });

    assert_every_run_equals(
        &runs,
        &vec![ids(&["c"]), ids(&["a"]), ids(&["b"])],
        "the registry must dispatch components in first-constraint declaration order",
    );
}

fn eq_mm(member: &str, millimetres: f64) -> CompiledExpr {
    eq(len_ref(member), literal(mm(millimetres)))
}

#[test]
fn first_failing_component_in_declaration_order_is_the_failure_reported() {
    let a_only = problem(&["a"], vec![eq_mm("a", 10.0), eq_mm("a", 20.0)], None);
    let expected = DimensionalSolver.solve(&a_only);
    assert!(
        matches!(expected, SolveResult::Infeasible { .. }),
        "fixture premise: `a == 10mm ∧ a == 20mm` alone is Infeasible, got {expected:?}",
    );
    let expected = format!("{expected:?}");

    let runs = across_fresh_threads(RUNS, || {
        let p = problem(
            &["a", "b"],
            vec![
                eq_mm("a", 10.0),
                eq_mm("a", 20.0),
                eq_mm("b", 1.0),
                eq_mm("b", 2.0),
            ],
            None,
        );
        format!("{:?}", SolverRegistry::production().solve(&p))
    });

    assert_every_run_equals(
        &runs,
        &expected,
        "the reported failure must be the earliest-declared contradiction, a's",
    );
}

#[test]
fn fallback_objective_attaches_to_the_first_declared_component() {
    let fixture = || {
        problem(
            &["a", "b"],
            vec![at_least("b", 1.0), at_least("a", 1.0)],
            Some(ObjectiveSet::single(ObjectiveSense::Minimize, len_ref("k"))),
        )
    };
    assert_eq!(
        objective_consumption(&fixture()),
        ObjectiveConsumption::FallbackComponentZero,
        "fixture premise: `minimize Part.k` reaches no auto",
    );

    let runs = across_fresh_threads(RUNS, || {
        dispatched_sub_problems(&fixture())
            .iter()
            .filter(|sub| sub.objective.is_some())
            .map(axis_order)
            .collect::<Vec<_>>()
    });

    assert_every_run_equals(
        &runs,
        &vec![ids(&["b"])],
        "the unconsumed objective must ride on exactly one component, the one \
         holding the first-declared constraint (b's)",
    );
}

fn bounded_free_auto(member: &str) -> AutoParam {
    AutoParam {
        id: id(member),
        param_type: Type::length(),
        bounds: Some((0.001, 0.1)),
        free: true,
    }
}

/// A dim-2 interior quadratic objective over x, y (minimum at 30mm, 70mm)
/// plus an independent bounded z. Mirrors registry_tests.rs's private
/// `two_param_objective_plus_independent_component`.
fn quadratic_plus_independent_component() -> ResolutionProblem {
    let offset = |member: &str, millimetres: f64| {
        let d = binop(BinOp::Sub, len_ref(member), literal(mm(millimetres)));
        binop(BinOp::Mul, d.clone(), d)
    };
    let objective = ObjectiveSet::single(
        ObjectiveSense::Minimize,
        binop(BinOp::Add, offset("x", 30.0), offset("y", 70.0)),
    );

    let mut current_values = ValueMap::new();
    current_values.insert(id("x"), mm(10.0));
    current_values.insert(id("y"), mm(90.0));
    current_values.insert(id("z"), mm(20.0));

    ResolutionProblem {
        auto_params: vec![
            bounded_free_auto("x"),
            bounded_free_auto("y"),
            bounded_free_auto("z"),
        ],
        constraints: numbered(vec![
            gt(len_ref("x"), literal(mm(1.0))),
            lt(len_ref("x"), literal(mm(100.0))),
            gt(len_ref("y"), literal(mm(1.0))),
            lt(len_ref("y"), literal(mm(100.0))),
            at_least("z", 3.0),
            le(len_ref("z"), literal(mm(4.0))),
        ]),
        dependent_cells: Vec::new(),
        current_values,
        objective: Some(objective),
        functions: vec![].into(),
    }
}

/// `{:?}` of f64 is round-trip exact, so equal text is equal bits.
fn values_fingerprint(values: &HashMap<ValueCellId, Value>) -> String {
    format!("{:?}", values.iter().collect::<BTreeMap<_, _>>())
}

fn solve_fingerprint(result: &SolveResult) -> String {
    match result {
        SolveResult::Solved { values, unique } => {
            format!(
                "Solved {{ values: {}, unique: {unique} }}",
                values_fingerprint(values)
            )
        }
        other => format!("{other:?}"),
    }
}

/// The ranked fingerprint and the candidate count, so the caller can check
/// that candidate ORDER is actually discriminated.
fn ranked_fingerprint(result: &RankedSolveResult) -> (String, usize) {
    match result {
        RankedSolveResult::Ranked {
            candidates,
            optimality,
            completeness,
        } => {
            let candidates_fp = candidates
                .iter()
                .map(|c| {
                    format!(
                        "({}, {:?}, {})",
                        values_fingerprint(&c.values),
                        c.objective_score.map(f64::to_bits),
                        c.unique
                    )
                })
                .collect::<Vec<_>>();
            (
                format!("Ranked {{ {candidates_fp:?}, {optimality:?}, {completeness:?} }}"),
                candidates.len(),
            )
        }
        other => (format!("{other:?}"), 0),
    }
}

#[test]
fn production_registry_is_bit_identical_across_hash_seeds() {
    let runs = across_fresh_threads(RUNS, || {
        let p = quadratic_plus_independent_component();
        let registry = SolverRegistry::production();
        (
            solve_fingerprint(&registry.solve(&p)),
            ranked_fingerprint(&registry.solve_ranked(&p)),
        )
    });

    let (solved, (ranked, candidate_count)) = &runs[0];
    assert!(
        solved.starts_with("Solved"),
        "fixture premise: the model solves, got {solved}",
    );
    assert!(
        *candidate_count >= 2,
        "fixture premise: >=2 ranked candidates, so candidate ORDER is \
         discriminated; got {candidate_count}: {ranked}",
    );
    assert_every_run_equals(
        &runs,
        &runs[0],
        "SolverRegistry::production() must give bit-identical solve and \
         solve_ranked results whatever the hash seed",
    );
}
