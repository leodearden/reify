// Integration tests for W_OBJECTIVE_INHERIT_AMBIGUOUS detection (task δ #4825;
// PRD docs/prds/v0_6/objective-scope-inheritance.md §3.4/§6.4, BT8).
//
// ## What fires W_OBJECTIVE_INHERIT_AMBIGUOUS
//
// An objective-less structure C is a sub-component of two or more containers
// (A, B) that each carry DISTINCT objectives.  α's `nearest_container_objective`
// returns `ContainerObjective::Ambiguous { containers }` for C.  No objective is
// inherited; the scope falls to centrality/feasibility (γ's existing behavior).
// δ emits the loud diagnostic so the fall-through is not silent (INV-6).
//
// ## Test harness
//
// Engine-level builder tests, same approach as scope_coupling.rs and
// underdetermined.rs.  `engine.check()` is the literal `reify check` entry
// point; no solver is attached (`no_solver_engine()`).  The builder controls
// sub_component/objective exactly so the containment index sees the correct graph.

use reify_core::{DiagnosticCode, ModulePath, Type};
use reify_eval::Engine;
use reify_ir::{ObjectiveSense, ObjectiveSet};
use reify_test_support::{
    CompiledModuleBuilder, MockConstraintChecker, TopologyTemplateBuilder, gt, literal, mm,
    value_ref,
};

// ---------------------------------------------------------------------------
// Helper: build a no-solver engine (mirrors `reify check` entry point).
// ---------------------------------------------------------------------------
fn no_solver_engine() -> Engine {
    Engine::new(Box::new(MockConstraintChecker::new()), None)
}

// ---------------------------------------------------------------------------
// Test 1 (BT8 positive) — multi-container reuse with distinct objectives:
// C is sub of A(minimize) and B(maximize); assert W_OBJECTIVE_INHERIT_AMBIGUOUS.
//
// RED until step-4 adds `detect_ambiguous_inherited_objectives` to Engine::eval.
// ---------------------------------------------------------------------------

/// (BT8-eval) `engine.eval` on a module where C is a sub of both A(minimize)
/// and B(maximize) — distinct-objective containers — must yield at least one
/// `W_OBJECTIVE_INHERIT_AMBIGUOUS` diagnostic.  The diagnostic message must name
/// BOTH containers ("A" and "B") and the structure ("C").
///
/// RED until step-4 wires the detector.
#[test]
fn eval_emits_objective_inherit_ambiguous_for_multi_container_reuse() {
    // A: minimize(A.x), sub c1:C
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "x", Type::length())
        .constraint("A", 0, None, gt(value_ref("A", "x"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("A", "x"),
        ))
        .sub_component("c1", "C", vec![])
        .build();

    // B: maximize(B.y), sub c2:C — distinct objective from A
    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "y", Type::length())
        .constraint("B", 0, None, gt(value_ref("B", "y"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Maximize,
            value_ref("B", "y"),
        ))
        .sub_component("c2", "C", vec![])
        .build();

    // C: objective-less leaf (one auto_param so it has content)
    let c = TopologyTemplateBuilder::new("C")
        .auto_param("C", "k", Type::length())
        .constraint("C", 0, None, gt(value_ref("C", "k"), literal(mm(0.0))))
        .build();

    // Template order: A(0), B(1), C(2) — ContainmentIndex sorts by slice index,
    // so Ambiguous.containers = ["A", "B"] (index 0 < 1).
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .template(c)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let ambig_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ObjectiveInheritAmbiguous))
        .collect();

    assert!(
        !ambig_diags.is_empty(),
        "eval: expected ≥1 W_OBJECTIVE_INHERIT_AMBIGUOUS diagnostic for C under A+B, \
         got 0. All diagnostics: {:?}",
        result.diagnostics,
    );

    // Message must name the structure and BOTH containers.
    let msg = &ambig_diags[0].message;
    assert!(
        msg.contains("W_OBJECTIVE_INHERIT_AMBIGUOUS"),
        "eval: diagnostic message must contain 'W_OBJECTIVE_INHERIT_AMBIGUOUS'; got: {msg}"
    );
    assert!(
        msg.contains('C') || msg.contains("'C'"),
        "eval: diagnostic message must name structure 'C'; got: {msg}"
    );
    assert!(
        msg.contains('A') || msg.contains("'A'"),
        "eval: diagnostic message must name container 'A'; got: {msg}"
    );
    assert!(
        msg.contains('B') || msg.contains("'B'"),
        "eval: diagnostic message must name container 'B'; got: {msg}"
    );
}

/// (BT8-check) Same module through `engine.check()` — must also emit
/// `W_OBJECTIVE_INHERIT_AMBIGUOUS`.
///
/// RED until step-4.
#[test]
fn check_emits_objective_inherit_ambiguous_for_multi_container_reuse() {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "x", Type::length())
        .constraint("A", 0, None, gt(value_ref("A", "x"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("A", "x"),
        ))
        .sub_component("c1", "C", vec![])
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "y", Type::length())
        .constraint("B", 0, None, gt(value_ref("B", "y"), literal(mm(0.0))))
        .objective(ObjectiveSet::single(
            ObjectiveSense::Maximize,
            value_ref("B", "y"),
        ))
        .sub_component("c2", "C", vec![])
        .build();

    let c = TopologyTemplateBuilder::new("C")
        .auto_param("C", "k", Type::length())
        .constraint("C", 0, None, gt(value_ref("C", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .template(c)
        .build();

    let mut engine = no_solver_engine();
    let check_result = engine.check(&module);

    let ambig_diags: Vec<_> = check_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ObjectiveInheritAmbiguous))
        .collect();

    assert!(
        !ambig_diags.is_empty(),
        "check: expected ≥1 W_OBJECTIVE_INHERIT_AMBIGUOUS diagnostic for C under A+B, \
         got 0. All diagnostics: {:?}",
        check_result.diagnostics,
    );

    let msg = &ambig_diags[0].message;
    assert!(
        msg.contains("W_OBJECTIVE_INHERIT_AMBIGUOUS"),
        "check: diagnostic message must contain 'W_OBJECTIVE_INHERIT_AMBIGUOUS'; got: {msg}"
    );
    assert!(
        msg.contains('A') || msg.contains("'A'"),
        "check: diagnostic message must name container 'A'; got: {msg}"
    );
    assert!(
        msg.contains('B') || msg.contains("'B'"),
        "check: diagnostic message must name container 'B'; got: {msg}"
    );
}
