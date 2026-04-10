//! M9 pipeline integration tests.
//!
//! Exercises cross-feature composition combining all three M9 milestone features:
//! constraint def instantiation, trait conformance, and determinacy predicates.
//!
//! Cross-cutting scenarios tested:
//!   1. Constraint defs whose predicates use determinacy predicates internally
//!   2. Traits with determinacy constraints injected into implementing structures
//!   3. Recursive structures whose sub guards use determinacy predicates
//!   4. Multi-trait structures combining constraint defs, trait defaults, and determinacy
//!
//! Uses `examples/m9_integration.ri` as the capstone source file and inline source
//! strings for focused per-scenario assertions.

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the integration example file, resolved at compile time from crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_integration.ri"
);

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module ready for eval or check.
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

/// Parse, compile, eval with SimpleConstraintChecker, return EvalResult.
/// Use when asserting on values (SI scalars, strings, booleans).
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

/// Parse, compile, check with SimpleConstraintChecker, return CheckResult.
/// Use when asserting on constraint satisfaction, labels, and counts.
fn check_source(source: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile(source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.check(&compiled)
}

// ── Step 1: .ri file parses and compiles ─────────────────────────────────────

/// Read examples/m9_integration.ri, parse it, assert no parse errors, compile,
/// assert no error-severity diagnostics, assert at least one template exists.
/// This is the baseline test confirming the capstone example file is valid.
#[test]
fn ri_file_parses_and_compiles() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_integration.ri should exist");

    // Step A: parse
    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in m9_integration.ri: {:?}",
        parsed.errors
    );

    // Step B: compile — no error diagnostics
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in m9_integration.ri: {:?}",
        errors
    );

    // Step C: at least one template (structures are present)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in m9_integration.ri, got none"
    );
}

// ── Step 3: constraint def with determinacy — satisfied case ─────────────────

/// Cross-feature: a constraint def whose sole predicate is a determinacy predicate.
/// When the invoked param has a concrete default (size=10mm), determined(v) is true,
/// so RequireDetermined[0] should be Satisfied.
#[test]
fn constraint_def_with_determinacy_satisfied() {
    let source = r#"
constraint def RequireDetermined {
    param v : Length
    determined(v)
}
structure S {
    param size : Length = 10mm
    constraint RequireDetermined(v: size)
}
"#;
    let result = check_source(source);

    // Exactly one constraint result (one invocation, one predicate)
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.label,
        Some("RequireDetermined[0]".to_string()),
        "expected label Some(\"RequireDetermined[0]\"), got: {:?}",
        entry.label
    );
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "RequireDetermined[0] should be Satisfied when param has default, got: {:?}",
        entry.satisfaction
    );
}

// ── Step 5: constraint def with determinacy — violated case ──────────────────

/// Cross-feature: when the invoked param has no default (size : Length, Undetermined),
/// determined(v) evaluates to false, so RequireDetermined[0] should be Violated.
#[test]
fn constraint_def_with_determinacy_violated() {
    let source = r#"
constraint def RequireDetermined {
    param v : Length
    determined(v)
}
structure S {
    param size : Length
    constraint RequireDetermined(v: size)
}
"#;
    let result = check_source(source);

    // Exactly one constraint result
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected 1 constraint result, got: {:?}",
        result.constraint_results
    );

    let entry = &result.constraint_results[0];
    assert_eq!(
        entry.label,
        Some("RequireDetermined[0]".to_string()),
        "expected label Some(\"RequireDetermined[0]\"), got: {:?}",
        entry.label
    );
    // determined(size) evaluates to Bool(false) when size is Undetermined → Violated
    assert_ne!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "RequireDetermined[0] should NOT be Satisfied when param is undetermined, got: {:?}",
        entry.satisfaction
    );
}

// ── Step 7: multi-predicate constraint def with determinacy + value range ─────

/// Cross-feature: DeterminedInRange has 3 predicates — determined(v), v >= lo, v <= hi.
/// With v=50mm, lo=10mm, hi=100mm, all three predicates are satisfied.
/// Verifies DeterminedInRange[0], [1], [2] all Satisfied.
#[test]
fn constraint_def_multi_predicate_determinacy_plus_value() {
    let source = r#"
constraint def DeterminedInRange {
    param v  : Length
    param lo : Length
    param hi : Length
    determined(v)
    v >= lo
    v <= hi
}
structure S {
    param v  : Length = 50mm
    param lo : Length = 10mm
    param hi : Length = 100mm
    constraint DeterminedInRange(v: v, lo: lo, hi: hi)
}
"#;
    let result = check_source(source);

    // Exactly 3 constraint results (one per predicate in the def)
    assert_eq!(
        result.constraint_results.len(),
        3,
        "expected 3 constraint results (one per predicate), got: {:?}",
        result.constraint_results
    );

    // DeterminedInRange[0] = determined(v): v=50mm has default → Satisfied
    let entry0 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange[0]".to_string()))
        .expect("expected DeterminedInRange[0]");
    assert_eq!(
        entry0.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[0] (determined(v)) should be Satisfied"
    );

    // DeterminedInRange[1] = v >= lo: 50mm >= 10mm → Satisfied
    let entry1 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange[1]".to_string()))
        .expect("expected DeterminedInRange[1]");
    assert_eq!(
        entry1.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[1] (v >= lo) should be Satisfied (50mm >= 10mm)"
    );

    // DeterminedInRange[2] = v <= hi: 50mm <= 100mm → Satisfied
    let entry2 = result
        .constraint_results
        .iter()
        .find(|e| e.label == Some("DeterminedInRange[2]".to_string()))
        .expect("expected DeterminedInRange[2]");
    assert_eq!(
        entry2.satisfaction,
        Satisfaction::Satisfied,
        "DeterminedInRange[2] (v <= hi) should be Satisfied (50mm <= 100mm)"
    );
}

// ── Step 9: trait with constraint def invocation ──────────────────────────────

/// Cross-feature: a trait body invokes a constraint def. The constraint def invocation
/// should be injected into the implementing structure as if it were declared there.
/// Widget : Sized { size = 50mm } should have Positive[0] = Satisfied and
/// Widget.size should evaluate to 0.05 SI (50mm).
#[test]
fn trait_with_constraint_def_invocation() {
    let source = r#"
constraint def Positive {
    param v : Length
    v > 0mm
}
trait Sized {
    param size : Length
    constraint Positive(v: size)
}
structure Widget : Sized {
    param size : Length = 50mm
}
"#;
    let check_result = check_source(source);
    let eval_result = eval_source(source);

    // Widget should have Positive[0] = Satisfied
    let entry = check_result
        .constraint_results
        .iter()
        .find(|e| e.id.entity == "Widget" && e.label == Some("Positive[0]".to_string()))
        .expect("expected Widget to have constraint with label 'Positive[0]'");
    assert_eq!(
        entry.satisfaction,
        Satisfaction::Satisfied,
        "Positive[0] should be Satisfied for Widget.size=50mm > 0mm"
    );

    // Widget.size = 50mm = 0.05 SI
    let size_id = ValueCellId::new("Widget", "size");
    let size_val = eval_result
        .values
        .get(&size_id)
        .expect("Widget.size should exist in eval result");
    match size_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected 0.05 SI for Widget.size (50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Widget.size, got {:?}", other),
    }
}
