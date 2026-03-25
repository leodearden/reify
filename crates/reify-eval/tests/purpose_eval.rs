//! Purpose activation/deactivation eval tests.
//!
//! Tests for Engine::activate_purpose and Engine::deactivate_purpose.

use reify_eval::cache::NodeId;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, CompiledPurposeBuilder, TopologyTemplateBuilder};
use reify_test_support::builders::{gt, literal, value_ref};
use reify_test_support::values::mm;
use reify_types::{ConstraintNodeId, ModulePath, Severity, Type, ValueCellId};

// ── Helper ──────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    compiled
}

// ── Step 13: activate_purpose injects constraints ─────────────────

#[test]
fn activate_purpose_injects_constraints() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // Initial eval — establishes the evaluation state
    let _result = engine.eval(&compiled);

    // Before activation: count constraints in the evaluation graph
    let snapshot = engine.snapshot().expect("should have snapshot after eval");
    let constraints_before = snapshot.graph.constraints.len();

    // Activate the purpose
    engine.activate_purpose("mfg_ready", "Bracket");

    // After activation: should have more constraints
    let snapshot = engine.snapshot().expect("should have snapshot after activate");
    let constraints_after = snapshot.graph.constraints.len();
    assert!(
        constraints_after > constraints_before,
        "activating purpose should inject constraints: before={}, after={}",
        constraints_before,
        constraints_after
    );

    // Deactivate the purpose
    engine.deactivate_purpose("mfg_ready");

    // After deactivation: constraints should be back to original count
    let snapshot = engine.snapshot().expect("should have snapshot after deactivate");
    let constraints_final = snapshot.graph.constraints.len();
    assert_eq!(
        constraints_final, constraints_before,
        "deactivating purpose should remove injected constraints: before={}, final={}",
        constraints_before, constraints_final
    );
}

// ── Step 15: activate purpose with optimization objective ──────────

#[test]
fn activate_purpose_with_minimize_objective() {
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
}

purpose lightweight(subject : Structure) {
    minimize subject + subject
}
"#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // Initial eval
    let _result = engine.eval(&compiled);

    // Before activation: no active objectives
    assert!(
        engine.active_objectives().is_empty(),
        "should have no active objectives before activation"
    );

    // Activate the purpose
    engine.activate_purpose("lightweight", "Bracket");

    // After activation: should have one active objective
    let objectives = engine.active_objectives();
    assert_eq!(
        objectives.len(),
        1,
        "activating purpose with minimize should add one objective"
    );
    assert!(engine.is_purpose_active("lightweight"));

    // Deactivate the purpose
    engine.deactivate_purpose("lightweight");

    // After deactivation: no active objectives
    assert!(
        engine.active_objectives().is_empty(),
        "deactivating purpose should remove the objective"
    );
    assert!(!engine.is_purpose_active("lightweight"));
}

// ── Step 17: full pipeline integration test ────────────────────────

#[test]
fn purpose_full_pipeline_integration() {
    // Full pipeline: parse → compile → eval → activate purpose → verify
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
    constraint height > 0mm
}

purpose manufacturing_ready(subject : Structure) {
    constraint 80 > 10
    constraint 60 > 5
}
"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Verify purpose was parsed
    let purpose_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, reify_syntax::Declaration::Purpose(_)))
        .count();
    assert_eq!(purpose_count, 1, "should parse one purpose declaration");

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Verify purpose was compiled
    assert_eq!(
        compiled.compiled_purposes.len(),
        1,
        "should compile one purpose"
    );
    assert_eq!(compiled.compiled_purposes[0].name, "manufacturing_ready");
    assert_eq!(compiled.compiled_purposes[0].constraints.len(), 2);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    assert!(
        result.diagnostics.is_empty(),
        "eval diagnostics: {:?}",
        result.diagnostics
    );

    // Count initial constraints
    let snapshot = engine.snapshot().expect("should have snapshot");
    let initial_constraints = snapshot.graph.constraints.len();
    assert!(initial_constraints >= 2, "Bracket has at least 2 constraints");

    // Activate purpose
    engine.activate_purpose("manufacturing_ready", "Bracket");
    assert!(engine.is_purpose_active("manufacturing_ready"));

    // Purpose constraints injected
    let snapshot = engine.snapshot().expect("snapshot after activate");
    assert_eq!(
        snapshot.graph.constraints.len(),
        initial_constraints + 2,
        "should inject 2 purpose constraints"
    );

    // Deactivate purpose
    engine.deactivate_purpose("manufacturing_ready");
    assert!(!engine.is_purpose_active("manufacturing_ready"));

    // Constraints restored
    let snapshot = engine.snapshot().expect("snapshot after deactivate");
    assert_eq!(
        snapshot.graph.constraints.len(),
        initial_constraints,
        "constraints should be restored after deactivation"
    );

    // Re-activation should work
    engine.activate_purpose("manufacturing_ready", "Bracket");
    assert!(engine.is_purpose_active("manufacturing_ready"));
    let snapshot = engine.snapshot().expect("snapshot after re-activate");
    assert_eq!(
        snapshot.graph.constraints.len(),
        initial_constraints + 2,
        "re-activation should inject constraints again"
    );
}

// ── Step 21: eval() should clear stale purpose state ───────────────

#[test]
fn eval_clears_stale_purpose_state() {
    // Calling eval() a second time builds a fresh snapshot. Active purpose
    // state must be cleared so that is_purpose_active() doesn't return
    // stale results and activate/deactivate work correctly.
    let source = r#"
structure Bracket {
    param width : Length = 80mm
    param height : Length = 60mm
    constraint width > 0mm
}

purpose mfg_ready(subject : Structure) {
    constraint 1 > 0
}
"#;

    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval and activate
    let _result = engine.eval(&compiled);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after activation"
    );

    // Second eval — fresh snapshot; purpose state should be cleared
    let _result = engine.eval(&compiled);
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should NOT be active after a fresh eval() call"
    );

    // Re-activation should work (not blocked by stale 'already active' guard)
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be re-activatable after fresh eval()"
    );
}

// ── Helpers for demand/eval-set tests ─────────────────────────────

/// Build a module with a Bracket template (width param) and a purpose
/// `mfg_ready` whose constraint references `mfg_ready.width` — after
/// remap_entity("mfg_ready", "Bracket") this becomes Bracket.width.
fn bracket_with_purpose_module() -> reify_compiler::CompiledModule {
    let template = TopologyTemplateBuilder::new("Bracket")
        .param("Bracket", "width", Type::length(), Some(literal(mm(80.0))))
        .build();

    // Purpose constraint: mfg_ready.width > 50mm
    // After remap_entity("mfg_ready", "Bracket") → Bracket.width > 50mm
    let purpose = CompiledPurposeBuilder::new("mfg_ready")
        .param("subject", "Structure")
        .constraint(
            "mfg_ready",
            0,
            None,
            gt(value_ref("mfg_ready", "width"), literal(mm(50.0))),
        )
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .compiled_purpose(purpose)
        .build()
}

// ── Step 1: purpose constraint appears in eval_set after activate+edit ─

/// After activating a purpose whose constraint references an entity param,
/// editing that param should include the purpose constraint in last_eval_set.
///
/// This test FAILS before step-2 because activate_purpose does not update
/// demand/reverse_index/trace_map after injecting constraints.
#[test]
fn purpose_constraint_in_eval_set_after_activate_and_edit() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // Cold-start eval establishes baseline state
    engine.eval(&module);

    // Activate purpose: injects constraint with entity "purpose:mfg_ready@Bracket"
    engine.activate_purpose("mfg_ready", "Bracket");

    // Edit Bracket.width — this should dirty the purpose constraint
    let width_id = ValueCellId::new("Bracket", "width");
    engine.edit_param(width_id, mm(100.0)).expect("edit_param should succeed");

    let eval_set = engine.last_eval_set();

    // The purpose constraint NodeId
    let purpose_constraint = NodeId::Constraint(
        ConstraintNodeId::new("purpose:mfg_ready@Bracket", 0),
    );

    assert!(
        eval_set.contains(&purpose_constraint),
        "purpose constraint should be in eval_set after activate+edit_param; \
        eval_set = {:?}",
        eval_set
    );
}

// ── Step 3: purpose constraint removed from eval_set after deactivate ─

/// After deactivating a purpose, editing a param the purpose constraint
/// depended on should NOT include the purpose constraint in last_eval_set.
///
/// This test FAILS before step-4 because deactivate_purpose does not update
/// demand/reverse_index/trace_map after removing constraints.
#[test]
fn purpose_constraint_removed_from_eval_set_after_deactivate() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    let width_id = ValueCellId::new("Bracket", "width");
    let purpose_constraint = NodeId::Constraint(
        ConstraintNodeId::new("purpose:mfg_ready@Bracket", 0),
    );

    // eval → activate → edit: verify constraint IS in eval_set
    engine.eval(&module);
    engine.activate_purpose("mfg_ready", "Bracket");
    engine.edit_param(width_id.clone(), mm(100.0)).expect("edit_param should succeed");
    assert!(
        engine.last_eval_set().contains(&purpose_constraint),
        "purpose constraint should be in eval_set after activation"
    );

    // deactivate → edit: verify constraint is NOT in eval_set
    engine.deactivate_purpose("mfg_ready");
    engine.edit_param(width_id.clone(), mm(90.0)).expect("edit_param should succeed after deactivate");
    assert!(
        !engine.last_eval_set().contains(&purpose_constraint),
        "purpose constraint should NOT be in eval_set after deactivation; \
        eval_set = {:?}",
        engine.last_eval_set()
    );
}

// ── Step 5: full activate/deactivate/reactivate cycle ─────────────

/// Full cycle: activate → edit (in set) → deactivate → edit (not in set)
/// → reactivate → edit (in set again).
///
/// Should pass once steps 2+4 are implemented.
#[test]
fn activate_deactivate_reactivate_eval_set_cycle() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    let width_id = ValueCellId::new("Bracket", "width");
    let purpose_constraint = NodeId::Constraint(
        ConstraintNodeId::new("purpose:mfg_ready@Bracket", 0),
    );

    engine.eval(&module);

    // Round 1: activate
    engine.activate_purpose("mfg_ready", "Bracket");
    engine.edit_param(width_id.clone(), mm(100.0)).unwrap();
    assert!(
        engine.last_eval_set().contains(&purpose_constraint),
        "cycle round 1: constraint should be in eval_set after activation"
    );

    // Round 2: deactivate
    engine.deactivate_purpose("mfg_ready");
    engine.edit_param(width_id.clone(), mm(90.0)).unwrap();
    assert!(
        !engine.last_eval_set().contains(&purpose_constraint),
        "cycle round 2: constraint should NOT be in eval_set after deactivation"
    );

    // Round 3: reactivate
    engine.activate_purpose("mfg_ready", "Bracket");
    engine.edit_param(width_id.clone(), mm(80.0)).unwrap();
    assert!(
        engine.last_eval_set().contains(&purpose_constraint),
        "cycle round 3: constraint should be in eval_set after re-activation"
    );
}

// ── Step 7: activate_purpose before eval is safe ──────────────────

/// Activating a purpose before any eval() call should be a no-op,
/// not a panic. The existing early-return guard on eval_state == None
/// should handle this.
#[test]
fn activate_purpose_before_eval_is_safe() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // Must store module to ensure purposes are known — but engine has no
    // compiled_purposes until eval() is called. Activate before eval.
    // This should not panic (early-return on eval_state == None).
    engine.activate_purpose("mfg_ready", "Bracket");

    // No eval state: snapshot is None
    assert!(
        engine.snapshot().is_none(),
        "snapshot should be None before eval()"
    );

    // Purpose is not active (no eval state, activation was silently skipped)
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "purpose should not be active before eval()"
    );

    // After eval, activation should work normally
    engine.eval(&module);
    engine.activate_purpose("mfg_ready", "Bracket");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "purpose should be active after eval() + activate()"
    );
}

// ── Step 8: activate unknown purpose is noop ─────────────────────

/// Activating a purpose whose name doesn't exist in compiled_purposes
/// should be a silent no-op.
#[test]
fn activate_unknown_purpose_is_noop() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let snapshot_before = engine.snapshot().expect("snapshot should exist after eval");
    let count_before = snapshot_before.graph.constraints.len();

    // Activate with an unknown purpose name
    engine.activate_purpose("nonexistent_purpose", "Bracket");

    // Constraint count should be unchanged
    let snapshot_after = engine.snapshot().expect("snapshot should exist after noop activate");
    let count_after = snapshot_after.graph.constraints.len();
    assert_eq!(
        count_before, count_after,
        "activating unknown purpose should not change constraint count"
    );

    // Purpose should not be marked active
    assert!(
        !engine.is_purpose_active("nonexistent_purpose"),
        "unknown purpose should not be marked active"
    );
}

// ── Step 9: deactivate inactive purpose is noop ───────────────────

/// Deactivating a purpose that was never activated should be a silent
/// no-op (no panic, constraint count unchanged, no active objectives).
#[test]
fn deactivate_inactive_purpose_is_noop() {
    let module = bracket_with_purpose_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let snapshot_before = engine.snapshot().expect("snapshot should exist after eval");
    let count_before = snapshot_before.graph.constraints.len();

    // Deactivate a purpose that was never activated — should be a no-op
    engine.deactivate_purpose("mfg_ready");

    // Constraint count should be unchanged
    let snapshot_after = engine.snapshot().expect("snapshot should exist after noop deactivate");
    let count_after = snapshot_after.graph.constraints.len();
    assert_eq!(
        count_before, count_after,
        "deactivating inactive purpose should not change constraint count"
    );

    // No active objectives
    assert!(
        engine.active_objectives().is_empty(),
        "no objectives should be active after noop deactivate"
    );
}
