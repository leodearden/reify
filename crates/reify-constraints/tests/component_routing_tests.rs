//! Domain routing and connectivity of decomposed components, for autos reached
//! THROUGH dependent cells or the objective, and for constraints reading cells
//! that no stored-order fold can derive. Exercised through the public API only.
//!
//! Dependent cells here are hand-built IR, bypassing reify-eval's
//! `build_dependent_cells`, which pre-drops cycles; that is what makes a cyclic
//! cell reachable at all.

use reify_constraints::{
    ObjectiveConsumption, SolverRegistry, decompose_into_components, objective_consumption,
};
use reify_core::{Type, ValueCellId};
use reify_ir::{
    AutoParam, BinOp, CompiledExpr, ConstraintDomain, ConstraintSolver, ObjectiveSense,
    ObjectiveSet, ResolutionProblem, SolveResult, ValueMap,
};
use reify_test_support::*;
use std::collections::{HashMap, HashSet};

const E: &str = "Cyc";

fn id(member: &str) -> ValueCellId {
    vcid(E, member)
}

fn ids<const N: usize>(members: [&str; N]) -> HashSet<ValueCellId> {
    members.into_iter().map(id).collect()
}

/// A length-typed `ValueRef`.
fn len_ref(member: &str) -> CompiledExpr {
    value_ref(E, member)
}

fn add(l: CompiledExpr, r: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, l, r, Type::length())
}

fn length_auto(member: &str) -> AutoParam {
    single_auto_param(id(member))
}

/// `x = y + a`, `y = x + b`: each cell closes a back edge, so no stored-order
/// fold can derive either, and both transitively read `{a, b}`.
fn cyclic_cells() -> Vec<(ValueCellId, CompiledExpr)> {
    vec![
        (id("x"), add(len_ref("y"), len_ref("a"))),
        (id("y"), add(len_ref("x"), len_ref("b"))),
    ]
}

fn problem(
    auto_params: Vec<AutoParam>,
    constraints: Vec<(reify_core::ConstraintNodeId, CompiledExpr)>,
    dependent_cells: Vec<(ValueCellId, CompiledExpr)>,
    objective: Option<ObjectiveSet>,
) -> ResolutionProblem {
    ResolutionProblem {
        auto_params,
        constraints,
        dependent_cells,
        current_values: ValueMap::new(),
        objective,
        functions: vec![].into(),
    }
}

// ---------------------------------------------------------------------------
// A constraint reading an UNFOLDABLE (cyclic) cell must be coupled to every
// auto that cell reaches and routed to the fallback slot, never dropped
// (#6514). A dropped constraint can empty the decomposition, and the registry
// then reports `Solved { unique: true }` with every auto at its default.
// ---------------------------------------------------------------------------

#[test]
fn a_constraint_reading_only_a_cyclic_cell_still_forms_a_component() {
    let autos = vec![length_auto("a"), length_auto("b")];
    let constraints = vec![(cnid(E, 0), eq(len_ref("x"), literal(mm(5.0))))];

    let components = decompose_into_components(&autos, &constraints, None, &cyclic_cells());

    assert_eq!(
        components.len(),
        1,
        "`x == 5mm` reads the cyclic cell `x`, which transitively reads BOTH \
         autos. An EMPTY decomposition means the constraint was silently \
         dropped, and the registry reads that as `Solved {{ unique: true }}` \
         with every auto at its default; got {components:?}",
    );
    let c = &components[0];
    assert_eq!(
        c.auto_params,
        ids(["a", "b"]),
        "the component must hold every auto the cyclic cell reaches",
    );
    assert_eq!(
        c.constraints
            .iter()
            .map(|(cid, _)| cid.clone())
            .collect::<Vec<_>>(),
        vec![cnid(E, 0)],
        "the constraint reading the cyclic cell must reach the sub-problem",
    );
    assert_eq!(
        c.domain,
        ConstraintDomain::CrossDomain,
        "no specialized solver can derive a value the per-component fold \
         filter refuses to fold, so the component must route to the FALLBACK \
         slot; `Dimensional` hands it to a solver that evaluates `x` stale",
    );
}

#[test]
fn a_constraint_reading_a_cell_downstream_of_a_cycle_still_forms_a_component() {
    let autos = vec![length_auto("a"), length_auto("b")];
    let mut cells = cyclic_cells();
    cells.push((id("z"), add(len_ref("x"), literal(mm(1.0)))));
    let constraints = vec![(cnid(E, 0), eq(len_ref("z"), literal(mm(5.0))))];

    let components = decompose_into_components(&autos, &constraints, None, &cells);

    assert_eq!(
        components.len(),
        1,
        "`z = x + 1mm` inherits the cycle's unknown value, and `z == 5mm` \
         transitively reads both autos. An EMPTY decomposition means the \
         constraint was silently dropped; got {components:?}",
    );
    let c = &components[0];
    assert_eq!(
        c.auto_params,
        ids(["a", "b"]),
        "the component must hold every auto reached through `z` and the cycle",
    );
    assert_eq!(
        c.constraints.len(),
        1,
        "the constraint reading `z` must reach the sub-problem",
    );
    assert_eq!(
        c.domain,
        ConstraintDomain::CrossDomain,
        "a cell downstream of a cycle is unfoldable too, so the component \
         must route to the FALLBACK slot",
    );
}

/// Guards against a fix that only rescues constraints whose referenced set
/// came out EMPTY: here the direct auto `c` keeps the constraint in today's
/// decomposition, but without the cycle's autos.
#[test]
fn a_constraint_reading_a_cyclic_cell_and_a_direct_auto_couples_all_three_autos() {
    let autos = vec![length_auto("a"), length_auto("b"), length_auto("c")];
    let constraints = vec![(
        cnid(E, 0),
        ge(add(len_ref("x"), len_ref("c")), literal(mm(1.0))),
    )];

    let components = decompose_into_components(&autos, &constraints, None, &cyclic_cells());

    assert_eq!(
        components.len(),
        1,
        "one constraint, one component; got {components:?}"
    );
    let c = &components[0];
    assert_eq!(
        c.auto_params,
        ids(["a", "b", "c"]),
        "`x + c >= 1mm` reads `c` directly and `a`, `b` through the cyclic \
         cell. `{{c}}` alone means the cycle's reach was ignored whenever the \
         constraint had some other auto to hold on to",
    );
    assert_eq!(
        c.domain,
        ConstraintDomain::CrossDomain,
        "`Dimensional` means reading the unfoldable cell did not force the \
         fallback slot",
    );
}

#[test]
fn a_cyclic_cell_constraint_reaches_the_fallback_solver_without_the_cyclic_cells() {
    let solved = || SolveResult::Solved {
        values: HashMap::new(),
        unique: true,
    };
    let dim_spy = MultiCallSpyConstraintSolver::new(vec![solved()]);
    let fallback_spy = MultiCallSpyConstraintSolver::new(vec![solved()]);
    let dim_calls = dim_spy.captured_problems();
    let fallback_calls = fallback_spy.captured_problems();
    let registry =
        SolverRegistry::with_solvers(Box::new(dim_spy), None, None, Some(Box::new(fallback_spy)));

    let _ = registry.solve(&problem(
        vec![length_auto("a"), length_auto("b")],
        vec![(cnid(E, 0), eq(len_ref("x"), literal(mm(5.0))))],
        cyclic_cells(),
        None,
    ));

    let fallback = fallback_calls.lock().unwrap();
    assert_eq!(
        fallback.len(),
        1,
        "the component reading the cyclic cell must be dispatched to the \
         FALLBACK solver exactly once. Zero calls means it never became a \
         component and the registry answered `Solved` with every auto at its \
         default",
    );
    let sub = &fallback[0];
    let sub_autos: HashSet<ValueCellId> = sub.auto_params.iter().map(|p| p.id.clone()).collect();
    assert_eq!(
        sub_autos,
        ids(["a", "b"]),
        "the fallback sub-problem must own every auto the cyclic cell reaches",
    );
    let sub_cells: Vec<&ValueCellId> = sub.dependent_cells.iter().map(|(cell, _)| cell).collect();
    assert!(
        !sub_cells.contains(&&id("x")) && !sub_cells.contains(&&id("y")),
        "the drop-side filter must still refuse to fold a cell whose value no \
         stored-order fold can derive; got {sub_cells:?}",
    );
    assert!(
        dim_calls.lock().unwrap().is_empty(),
        "the dimensional solver must not be handed a component reading an \
         unfoldable cell",
    );
}

/// GUARD, and deliberately ASYMMETRIC with the constraint axis above: the
/// objective gets NO reach through an unfoldable cell. The cell is never
/// folded, so an objective reading only it governs nothing, and
/// `E_OBJECTIVE_UNCONSUMED` must stay loud.
#[test]
fn an_objective_reading_only_a_cyclic_cell_stays_fallback_component_zero() {
    let p = problem(
        vec![length_auto("a")],
        vec![(cnid(E, 0), ge(len_ref("a"), literal(mm(1.0))))],
        vec![
            (id("x"), add(len_ref("y"), len_ref("a"))),
            (id("y"), len_ref("x")),
        ],
        Some(ObjectiveSet::single(ObjectiveSense::Minimize, len_ref("x"))),
    );

    assert_eq!(
        objective_consumption(&p),
        ObjectiveConsumption::FallbackComponentZero,
        "`minimize x` reads only a cyclic cell no fold can derive. `Consumed` \
         means the objective's reach coupled through that cell, which would \
         silence E_OBJECTIVE_UNCONSUMED for an objective that governs nothing",
    );
}
