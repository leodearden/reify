// Integration tests for W_SCOPE_COUPLING detection (task 4020).
//
// These tests exercise `detect_scope_coupling` (wired in Engine::eval) through
// the engine-level builder harness — exactly the same approach as resolution.rs
// for the solver wiring.  Engine-level tests are used rather than a CLI fixture
// because cross-sub references compile to instance-scoped ids (e.g.
// `Parent.c.x`), distinct from a child template's own auto cell (`Child.x`),
// so a hand-written parent/child `.ri` is not guaranteed to produce a
// template-level cross-scope auto read.  The builder controls ValueCellIds
// exactly and `engine.check()` is the literal `reify check` entry point.

use reify_core::{DiagnosticCode, ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_ir::{ObjectiveSet, ObjectiveSense};
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
// Test A — positive: constraint-sourced coupling (step-3 RED)
// ---------------------------------------------------------------------------

/// Two-template module where "Later" has a constraint that reads "Leaf"'s
/// frozen auto cell `Leaf.k`.  With NO solver attached, `engine.eval` must
/// emit exactly one W_SCOPE_COUPLING diagnostic naming "Leaf", "Later", and
/// the crossing cell.
#[test]
fn eval_emits_scope_coupling_for_constraint_crossing() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        // self-constraint: Leaf.k > 1mm
        .constraint("Leaf", 0, None, gt(value_ref("Leaf", "k"), literal(mm(1.0))))
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        // crossing constraint: Later.y > Leaf.k  (reads frozen Leaf.k)
        .constraint(
            "Later",
            1,
            None,
            gt(value_ref("Later", "y"), value_ref("Leaf", "k")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let coupling_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .collect();

    assert_eq!(
        coupling_diags.len(),
        1,
        "expected exactly 1 W_SCOPE_COUPLING diagnostic, got {}: {:?}",
        coupling_diags.len(),
        result.diagnostics,
    );

    let msg = &coupling_diags[0].message;
    assert!(
        msg.contains("Leaf"),
        "diagnostic message should name the frozen scope 'Leaf'; got: {msg}"
    );
    assert!(
        msg.contains("Later"),
        "diagnostic message should name the later scope 'Later'; got: {msg}"
    );
    // The crossing cell is Leaf.k — check for entity and member in message.
    assert!(
        msg.contains("Leaf") && (msg.contains(".k") || msg.contains("k")),
        "diagnostic message should reference the crossing cell 'Leaf.k'; got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Test B — reify-check reachability (step-3 RED)
// ---------------------------------------------------------------------------

/// Same module through `engine.check()` — the literal `reify check` entry
/// point.  The W_SCOPE_COUPLING diagnostic from `eval()` must propagate into
/// `CheckResult.diagnostics`.
#[test]
fn check_propagates_scope_coupling_diagnostic() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint("Leaf", 0, None, gt(value_ref("Leaf", "k"), literal(mm(1.0))))
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            1,
            None,
            gt(value_ref("Later", "y"), value_ref("Leaf", "k")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let check_result = engine.check(&module);

    let coupling_diags: Vec<_> = check_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .collect();

    assert!(
        !coupling_diags.is_empty(),
        "engine.check() should propagate W_SCOPE_COUPLING from eval(); got diagnostics: {:?}",
        check_result.diagnostics,
    );
}

// ---------------------------------------------------------------------------
// Test C — positive: objective-sourced coupling (step-5 RED)
// ---------------------------------------------------------------------------

/// Two-template module where "Later" has an *objective* that reads the frozen
/// auto cell `Leaf.k` (not a constraint).  The PRD says coupling is detected
/// from "a constraint OR objective".  Must emit exactly one W_SCOPE_COUPLING.
///
/// RED until step-6 extends detect_scope_coupling to scan objective terms.
#[test]
fn eval_emits_scope_coupling_for_objective_crossing() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        // self-constraint keeps the template non-trivial
        .constraint("Leaf", 0, None, gt(value_ref("Leaf", "k"), literal(mm(1.0))))
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        // self-constraint on Later.y (no crossing here)
        .constraint("Later", 1, None, gt(value_ref("Later", "y"), literal(mm(0.5))))
        // objective reads frozen Leaf.k — the coupling source
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("Leaf", "k"),
        ))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let coupling_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .collect();

    assert_eq!(
        coupling_diags.len(),
        1,
        "expected exactly 1 W_SCOPE_COUPLING from objective crossing, got {}: {:?}",
        coupling_diags.len(),
        result.diagnostics,
    );

    let msg = &coupling_diags[0].message;
    assert!(
        msg.contains("Leaf"),
        "diagnostic message should name frozen scope 'Leaf'; got: {msg}"
    );
    assert!(
        msg.contains("Later"),
        "diagnostic message should name later scope 'Later'; got: {msg}"
    );
}
