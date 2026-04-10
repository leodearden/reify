//! M9 combined integration tests.
//!
//! Exercises all M9 milestone features in a single cohesive example:
//! trait conformance with defaults, constraint definitions, determinacy predicates,
//! recursive structures, custom unit declarations, meta block access, and doc comments.
//! Uses examples/m9_combined.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_combined.ri"
);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
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

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m9_combined.ri and verify it parses without errors.
#[test]
fn m9_combined_ri_parses() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

// ── Test 2: file compiles without error diagnostics ─────────────────────────

/// Compile m9_combined.ri and verify no error-severity diagnostics.
/// Also confirms at least one template exists (structures are present).
#[test]
fn m9_combined_compiles_no_errors() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);

    // Must have at least one template (structure)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Compile, eval with SimpleConstraintChecker, check() — all constraint results
/// must be Satisfied and the results list must be non-empty.
#[test]
fn all_constraints_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    // Must have at least some constraint results (file has active constraints)
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
    );

    // All must be Satisfied — no violations
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Test 4: trait values ─────────────────────────────────────────────────────

/// Verify multi-trait values: Bracket.length=0.1 SI (100mm), Bracket.half_length=0.05 SI
/// (50mm from Dimensional trait let binding), Bracket.mass=2.0 SI (2kg from Weighted trait).
/// Confirms multi-trait inheritance and trait let binding propagation.
#[test]
fn trait_values() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Bracket.length = 100mm = 0.1 SI
    let length_id = ValueCellId::new("Bracket", "length");
    let length_val = result
        .values
        .get(&length_id)
        .unwrap_or_else(|| panic!("Bracket.length not found"));
    match length_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "expected 0.1 SI for Bracket.length (100mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Bracket.length, got {:?}", other),
    }

    // Bracket.half_length = 50mm = 0.05 SI (from Dimensional trait let binding)
    let half_id = ValueCellId::new("Bracket", "half_length");
    let half_val = result
        .values
        .get(&half_id)
        .unwrap_or_else(|| panic!("Bracket.half_length not found"));
    match half_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected 0.05 SI for Bracket.half_length (50mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Bracket.half_length, got {:?}", other),
    }

    // Bracket.mass = 2kg = 2.0 SI (from Weighted trait param, overridden in structure)
    let mass_id = ValueCellId::new("Bracket", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("Bracket.mass not found"));
    match mass_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 2.0).abs() < 1e-12,
                "expected 2.0 SI for Bracket.mass (2kg), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Bracket.mass, got {:?}", other),
    }
}

// ── Test 5: default injection ────────────────────────────────────────────────

/// Verify Plate.mass=1.0 SI (1kg default injected from Weighted trait).
/// Confirms empty-body structure receives trait defaults.
#[test]
fn default_injection() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Plate.mass = 1kg = 1.0 SI (injected from Weighted trait default)
    let mass_id = ValueCellId::new("Plate", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("Plate.mass not found"));
    match mass_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 1.0).abs() < 1e-12,
                "expected 1.0 SI for Plate.mass (1kg default from Weighted), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Plate.mass, got {:?}", other),
    }
}

// ── Test 6: constraint def labels ────────────────────────────────────────────

/// Verify Bracket has InRange[0] and InRange[1] constraints for the first
/// InRange invocation (length bounds), both Satisfied.
/// Confirms constraint def label generation.
#[test]
fn constraint_def_labels() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    // Find InRange[0] and InRange[1] for Bracket (first invocation: length bounds)
    let find_bracket = |label: &str| {
        check_result
            .constraint_results
            .iter()
            .find(|e| e.id.entity == "Bracket" && e.label == Some(label.to_string()))
            .unwrap_or_else(|| panic!("expected Bracket constraint with label '{label}'"))
    };

    let inrange0 = find_bracket("InRange[0]");
    let inrange1 = find_bracket("InRange[1]");

    assert_eq!(
        inrange0.satisfaction,
        Satisfaction::Satisfied,
        "Bracket InRange[0] should be Satisfied"
    );
    assert_eq!(
        inrange1.satisfaction,
        Satisfaction::Satisfied,
        "Bracket InRange[1] should be Satisfied"
    );
}

// ── Test 7: custom unit value ────────────────────────────────────────────────

/// Verify Bracket.clearance SI value is approximately 0.0127 (500 * 0.0000254).
/// Confirms the custom unit mil resolves correctly in param defaults.
#[test]
fn custom_unit_value() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Bracket.clearance = 500mil = 500 * 0.0000254m = 0.0127 SI
    let clearance_id = ValueCellId::new("Bracket", "clearance");
    let clearance_val = result
        .values
        .get(&clearance_id)
        .unwrap_or_else(|| panic!("Bracket.clearance not found"));
    match clearance_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.0127).abs() < 1e-9,
                "expected ~0.0127 SI for Bracket.clearance (500mil), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Bracket.clearance, got {:?}", other),
    }
}

// ── Test 8: meta access values ───────────────────────────────────────────────

/// Verify Bracket.label=Value::String("steel") and Bracket.rev=Value::String("A").
/// Confirms meta block keys are accessible in let bindings.
#[test]
fn meta_access_values() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Bracket.label = "steel" (from meta.material)
    let label_id = ValueCellId::new("Bracket", "label");
    let label_val = result
        .values
        .get(&label_id)
        .unwrap_or_else(|| panic!("Bracket.label not found"));
    assert_eq!(
        label_val,
        &reify_types::Value::String("steel".to_string()),
        "Bracket.label should be String(\"steel\") via meta.material"
    );

    // Bracket.rev = "A" (from meta.revision)
    let rev_id = ValueCellId::new("Bracket", "rev");
    let rev_val = result
        .values
        .get(&rev_id)
        .unwrap_or_else(|| panic!("Bracket.rev not found"));
    assert_eq!(
        rev_val,
        &reify_types::Value::String("A".to_string()),
        "Bracket.rev should be String(\"A\") via meta.revision"
    );
}

// ── Test 9: recursive unfold depth ──────────────────────────────────────────

/// Verify BracketTree recursive unfolding:
///   depth=2 → child.span ≈ 0.1 SI (100mm), child.child.span ≈ 0.05 SI (50mm),
///   child.child.child.span does NOT exist (guard depth>0 false at depth=0).
/// Confirms recursive unfolding with depth-gate termination.
#[test]
fn recursive_unfold_depth() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // BracketTree.child.span = 200mm/2 = 100mm = 0.1 SI
    let child_span_id = ValueCellId::new("BracketTree.child", "span");
    let child_span = result
        .values
        .get(&child_span_id)
        .unwrap_or_else(|| panic!("BracketTree.child.span not found"));
    match child_span {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-9,
                "expected ~0.1 SI for BracketTree.child.span (100mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for BracketTree.child.span, got {:?}", other),
    }

    // BracketTree.child.child.span = 100mm/2 = 50mm = 0.05 SI
    let grandchild_span_id = ValueCellId::new("BracketTree.child.child", "span");
    let grandchild_span = result
        .values
        .get(&grandchild_span_id)
        .unwrap_or_else(|| panic!("BracketTree.child.child.span not found"));
    match grandchild_span {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "expected ~0.05 SI for BracketTree.child.child.span (50mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for BracketTree.child.child.span, got {:?}",
            other
        ),
    }

    // BracketTree.child.child.child.span must NOT exist (depth=0, guard false)
    let great_grandchild_span_id = ValueCellId::new("BracketTree.child.child.child", "span");
    assert!(
        !result.values.contains(&great_grandchild_span_id),
        "BracketTree.child.child.child.span should not exist (depth=0 stops unfolding)"
    );
}

// ── Test 10: total constraint count ─────────────────────────────────────────

/// Capstone assertion: constraint_results.len() >= 15, all Satisfied.
/// This is the >=15 assertions requirement from the task description.
#[test]
fn total_constraint_count() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist");

    let compiled = parse_and_compile(&source);
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    assert!(
        check_result.constraint_results.len() >= 15,
        "expected >= 15 total constraint results, got {}",
        check_result.constraint_results.len()
    );

    // All must be Satisfied
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied",
            entry.id
        );
    }
}
