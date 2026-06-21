// Integration tests for W_UNDERDETERMINED detection (task κ #4019).
//
// These tests exercise `detect_underdetermined` (wired in Engine::eval) through
// the engine-level builder harness — the same approach as scope_coupling.rs.
// Engine-level tests are preferred because the builder controls ValueCellIds
// exactly; `engine.check()` is the literal `reify check` entry point.

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
// Test A — positive: single unconstrained auto param emits W_UNDERDETERMINED
// (step-3 RED — no detection pass exists yet)
// ---------------------------------------------------------------------------

/// A single-template module with one `auto` param "FreeBar.gap" and NO
/// constraint or objective referencing it.  With no solver attached,
/// `engine.eval` must emit exactly one `W_UNDERDETERMINED` diagnostic naming
/// the free param ("FreeBar.gap") and indicating no touching constraints.
///
/// RED until step-4 adds `detect_underdetermined` to Engine::eval.
#[test]
fn eval_emits_underdetermined_for_unconstrained_auto_param() {
    let template = TopologyTemplateBuilder::new("FreeBar")
        .auto_param("FreeBar", "gap", Type::length())
        // No constraints — gap is entirely unconstrained.
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let under_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .collect();

    assert_eq!(
        under_diags.len(),
        1,
        "expected exactly 1 W_UNDERDETERMINED diagnostic for unconstrained auto param, \
         got {}: {:?}",
        under_diags.len(),
        result.diagnostics,
    );

    let msg = &under_diags[0].message;
    assert!(
        msg.contains("W_UNDERDETERMINED"),
        "diagnostic message should contain 'W_UNDERDETERMINED'; got: {msg}"
    );
    assert!(
        msg.contains("FreeBar.gap"),
        "diagnostic message should name the free cell 'FreeBar.gap'; got: {msg}"
    );
    assert!(
        msg.contains("touching constraints: none")
            || msg.contains("none")
                && msg.contains("touching"),
        "diagnostic message should mention empty touching constraints; got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// Guard tests (step-5 RED)
// ---------------------------------------------------------------------------

/// (a) An auto param referenced ONLY by an objective term (no constraint) must
/// NOT emit W_UNDERDETERMINED.  An objective determines the cell's value, so
/// flagging it would be a false positive on objective-driven designs.
///
/// RED until step-6 extends detect_underdetermined to scan objective reads.
#[test]
fn no_underdetermined_for_objective_only_auto_param() {
    let template = TopologyTemplateBuilder::new("ObjOnly")
        .auto_param("ObjOnly", "x", Type::length())
        // No constraint on x — only referenced by an objective term.
        .objective(ObjectiveSet::single(
            ObjectiveSense::Minimize,
            value_ref("ObjOnly", "x"),
        ))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .count();
    assert_eq!(
        count, 0,
        "objective-pinned auto param must not trigger W_UNDERDETERMINED; \
         got: {:?}",
        result.diagnostics,
    );
}

/// (b) An auto param referenced by a self-constraint must NOT emit
/// W_UNDERDETERMINED — it IS in the global constraint read-set.
#[test]
fn no_underdetermined_for_constrained_auto_param() {
    let template = TopologyTemplateBuilder::new("Bounded")
        .auto_param("Bounded", "width", Type::length())
        .constraint(
            "Bounded",
            0,
            None,
            gt(value_ref("Bounded", "width"), literal(mm(1.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .count();
    assert_eq!(
        count, 0,
        "constrained auto param must not trigger W_UNDERDETERMINED; got: {:?}",
        result.diagnostics,
    );
}

/// (c) Cross-scope guard: two templates where `Leaf.k` (auto, no self-constraint)
/// is read by sibling `Later`'s constraint.  The GLOBAL scan pins `Leaf.k`
/// (via `Later`'s constraint read-set), so it must NOT be flagged.
#[test]
fn no_underdetermined_for_cross_scope_read_auto_param() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        // No self-constraint on Leaf.k — it is only pinned by Later's constraint.
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            0,
            None,
            // Later reads Leaf.k → Leaf.k enters the global read-set.
            gt(value_ref("Later", "y"), value_ref("Leaf", "k")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    // Leaf.k is in the global read-set → must not be flagged.
    // Later.y is also read by the constraint (in the constraint expression itself
    // as the LHS of `gt`) → also in the read-set → must not be flagged.
    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .count();
    assert_eq!(
        count, 0,
        "cross-scope-pinned auto param must not trigger W_UNDERDETERMINED; got: {:?}",
        result.diagnostics,
    );
}

/// (d) A non-auto `Param` (not `auto`) must NOT emit W_UNDERDETERMINED
/// regardless of whether it appears in any constraint.
#[test]
fn no_underdetermined_for_non_auto_param() {
    let template = TopologyTemplateBuilder::new("Regular")
        .param("Regular", "size", Type::length(), None) // non-auto Param
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .count();
    assert_eq!(
        count, 0,
        "non-auto param must not trigger W_UNDERDETERMINED; got: {:?}",
        result.diagnostics,
    );
}

/// (e) `engine.check()` — the literal `reify check` entry point — propagates
/// the W_UNDERDETERMINED diagnostic from `eval()` for a free-param module.
#[test]
fn check_propagates_underdetermined_diagnostic() {
    let template = TopologyTemplateBuilder::new("FreeBar")
        .auto_param("FreeBar", "gap", Type::length())
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = no_solver_engine();
    let check_result = engine.check(&module);

    let count = check_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::Underdetermined))
        .count();
    assert!(
        count > 0,
        "engine.check() should propagate W_UNDERDETERMINED from eval(); got diagnostics: {:?}",
        check_result.diagnostics,
    );
}
