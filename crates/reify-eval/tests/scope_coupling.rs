// Integration tests for W_SCOPE_COUPLING detection (task 4020; updated for β #4822).
//
// ## β #4822 semantics change
//
// Before β: W_SCOPE_COUPLING fired for ACYCLIC cross-scope auto reads (scope B
// reads a "frozen" auto cell owned by earlier-resolved scope A).
// After β:  W_SCOPE_COUPLING fires ONLY for irreducible read-cycles (SCCs of size
// ≥ 2).  Acyclic crossings are resolved by the dependency-ordered solve and do
// NOT warn — ordering is the fix.
//
// Tests updated for acyclic cases (A→D below) now assert ZERO diagnostics.
// The guard tests (E–G) still hold (reasons below).  A new test (H) covers the
// cycle case that must still fire.
//
// ## Test harness
//
// Engine-level builder tests rather than CLI fixtures, because cross-sub
// references compile to instance-scoped ids (`Parent.c.x`, not `Child.x`) and
// the builder controls ValueCellIds exactly.  `engine.check()` is the literal
// `reify check` entry point.

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
// Tests A–D — acyclic crossings: assert ZERO W_SCOPE_COUPLING (β §6.4)
//
// After β, the resolve_order reorders Leaf before Later, so the crossing is
// resolved by ordering — no warning is appropriate for an acyclic read.
// ---------------------------------------------------------------------------

/// (A) Acyclic constraint crossing [Leaf, Later] where Later reads Leaf.k.
///
/// β resolves the ordering so Leaf is solved first; acyclic crossings emit no
/// W_SCOPE_COUPLING.  Assert zero.
#[test]
fn eval_emits_scope_coupling_for_constraint_crossing() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint(
            "Leaf",
            0,
            None,
            gt(value_ref("Leaf", "k"), literal(mm(1.0))),
        )
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        // crossing constraint: Later.y > Leaf.k  (acyclic read)
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

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "acyclic crossing resolved by ordering — must emit 0 W_SCOPE_COUPLING; got {}: {:?}",
        count, result.diagnostics,
    );
}

/// (B) Same acyclic module through `engine.check()` — must also emit zero.
#[test]
fn check_propagates_scope_coupling_diagnostic() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint(
            "Leaf",
            0,
            None,
            gt(value_ref("Leaf", "k"), literal(mm(1.0))),
        )
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

    let count = check_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "acyclic crossing via engine.check() — must emit 0 W_SCOPE_COUPLING; got {}: {:?}",
        count, check_result.diagnostics,
    );
}

/// (C) Acyclic objective crossing: Later has an *objective* that reads Leaf.k.
///
/// β resolves the ordering (Leaf before Later), so no warning.  Assert zero.
#[test]
fn eval_emits_scope_coupling_for_objective_crossing() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint(
            "Leaf",
            0,
            None,
            gt(value_ref("Leaf", "k"), literal(mm(1.0))),
        )
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            1,
            None,
            gt(value_ref("Later", "y"), literal(mm(0.5))),
        )
        // objective reads Leaf.k — acyclic read
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

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "acyclic objective crossing resolved by ordering — must emit 0 W_SCOPE_COUPLING; got {}: {:?}",
        count, result.diagnostics,
    );
}

/// (D) Acyclic crossing with duplicate reads: Later has TWO constraints both
/// reading Leaf.k.  β resolves the ordering; no warning is emitted.  Assert zero.
///
/// (The old dedup test asserted == 1; after β the correct count is 0.)
#[test]
fn coupling_dedup_two_constraints_same_crossing_cell() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            0,
            None,
            gt(value_ref("Later", "y"), value_ref("Leaf", "k")),
        )
        .constraint(
            "Later",
            1,
            None,
            gt(literal(mm(10.0)), value_ref("Leaf", "k")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "acyclic crossing (duplicate reads) resolved by ordering — must emit 0; got {}: {:?}",
        count, result.diagnostics,
    );
}

// ---------------------------------------------------------------------------
// Guard tests E–G — pin absence, auto-only, own-scope: still assert 0
// ---------------------------------------------------------------------------

/// (E) Same-scope self-read: single template reading its OWN auto cell.
/// Never emits W_SCOPE_COUPLING — no change.
#[test]
fn no_coupling_for_same_scope_self_read() {
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "thickness", Type::length())
        .constraint(
            "S",
            0,
            None,
            gt(value_ref("S", "thickness"), literal(mm(1.0))),
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
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "self-read must not trigger W_SCOPE_COUPLING; got: {:?}",
        result.diagnostics
    );
}

/// (F) [Later, Leaf] module where Later reads Leaf.k.
///
/// Before β: "reader resolves before owner" — no warning because Leaf hadn't
/// been frozen yet when Later was processed source-order.
/// After β: resolve_order reorders to [Leaf, Later] (Leaf solved first because
/// Later reads Leaf.k) — acyclic, no warning.  Assert 0 for the same reason.
#[test]
fn no_coupling_when_reader_resolves_before_owner() {
    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            0,
            None,
            gt(value_ref("Later", "y"), value_ref("Leaf", "k")),
        )
        .build();

    let leaf = TopologyTemplateBuilder::new("Leaf")
        .auto_param("Leaf", "k", Type::length())
        .constraint(
            "Leaf",
            1,
            None,
            gt(value_ref("Leaf", "k"), literal(mm(1.0))),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(later)
        .template(leaf)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "acyclic read (reordered by β) must not trigger W_SCOPE_COUPLING; got: {:?}",
        result.diagnostics
    );
}

/// (G) Non-auto crossing: Leaf.p is a Param (not Auto); Later reads it.
/// Non-auto reads create no read-DAG edges and emit no W_SCOPE_COUPLING.
#[test]
fn no_coupling_for_non_auto_crossing() {
    let leaf = TopologyTemplateBuilder::new("Leaf")
        .param("Leaf", "p", Type::length(), None)
        .build();

    let later = TopologyTemplateBuilder::new("Later")
        .auto_param("Later", "y", Type::length())
        .constraint(
            "Later",
            0,
            None,
            gt(value_ref("Later", "y"), value_ref("Leaf", "p")),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(leaf)
        .template(later)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let count = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert_eq!(
        count, 0,
        "non-auto crossing must not trigger W_SCOPE_COUPLING; got: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Test H — irreducible 2-cycle: assert ≥1 W_SCOPE_COUPLING (β §6.4)
//
// A reads B.m AND B reads A.k — they form an irreducible cycle (SCC of size 2).
// resolve_order CANNOT resolve this by ordering; it emits W_SCOPE_COUPLING for
// the intra-SCC crossing.  Must fire via both engine.eval AND engine.check.
// ---------------------------------------------------------------------------

/// (H-eval) True 2-cycle via eval(): must emit ≥1 W_SCOPE_COUPLING naming
/// both scopes + a crossing cell.
#[test]
fn eval_cycle_emits_scope_coupling() {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        // A reads B.m — creates edge B→A in read-DAG
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        // B reads A.k — creates edge A→B in read-DAG → cycle!
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build();

    let mut engine = no_solver_engine();
    let result = engine.eval(&module);

    let coupling_diags: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .collect();

    assert!(
        !coupling_diags.is_empty(),
        "irreducible 2-cycle must emit ≥1 W_SCOPE_COUPLING; got none (diagnostics: {:?})",
        result.diagnostics,
    );

    // At least one diagnostic must name both scopes.
    let any_names_both = coupling_diags.iter().any(|d| {
        let m = &d.message;
        m.contains("A") && m.contains("B")
    });
    assert!(
        any_names_both,
        "at least one W_SCOPE_COUPLING must name both 'A' and 'B'; messages: {:?}",
        coupling_diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );

    // At least one diagnostic must name a crossing cell.
    let any_names_cell = coupling_diags.iter().any(|d| {
        d.message.contains("A.k") || d.message.contains("B.m")
    });
    assert!(
        any_names_cell,
        "at least one W_SCOPE_COUPLING must name a crossing cell (A.k or B.m); messages: {:?}",
        coupling_diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
    );
}

/// (H-check) Same 2-cycle via engine.check() — cycle warning must surface on
/// the no-solver (`reify check`) path, same as it did before β.
#[test]
fn check_cycle_emits_scope_coupling() {
    let a = TopologyTemplateBuilder::new("A")
        .auto_param("A", "k", Type::length())
        .constraint("A", 0, None, gt(value_ref("B", "m"), literal(mm(0.0))))
        .build();

    let b = TopologyTemplateBuilder::new("B")
        .auto_param("B", "m", Type::length())
        .constraint("B", 0, None, gt(value_ref("A", "k"), literal(mm(0.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(a)
        .template(b)
        .build();

    let mut engine = no_solver_engine();
    let check_result = engine.check(&module);

    let count = check_result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ScopeCoupling))
        .count();
    assert!(
        count >= 1,
        "engine.check() must surface cycle W_SCOPE_COUPLING; got 0 (diagnostics: {:?})",
        check_result.diagnostics,
    );
}
