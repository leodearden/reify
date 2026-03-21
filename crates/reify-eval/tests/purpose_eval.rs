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
    constraint subject.width > 10mm
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
