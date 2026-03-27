//! Purpose activation/deactivation eval tests.
//!
//! Tests for Engine::activate_purpose and Engine::deactivate_purpose.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity};

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
    let snapshot = engine
        .snapshot()
        .expect("should have snapshot after activate");
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
    let snapshot = engine
        .snapshot()
        .expect("should have snapshot after deactivate");
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
    assert!(
        initial_constraints >= 2,
        "Bracket has at least 2 constraints"
    );

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
