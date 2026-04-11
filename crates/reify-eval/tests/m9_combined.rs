//! M9 combined integration tests.
//!
//! Exercises all M9 milestone features in a single cohesive example:
//! trait conformance with defaults, constraint definitions, determinacy predicates,
//! recursive structures, custom unit declarations, meta block access, and doc comments.
//! Uses examples/m9_combined.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::parse_and_compile;
use reify_types::{ModulePath, Satisfaction, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_combined.ri"
);

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read and return the contents of the m9_combined.ri example file.
fn source() -> String {
    std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist")
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

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m9_combined.ri and verify it parses without errors.
#[test]
fn m9_combined_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
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
    let compiled = parse_and_compile(&source());

    // Must have at least one template (structure)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: file produces constraint results and none are Violated.
/// The strict all-Satisfied invariant is covered by `total_constraint_count`.
#[test]
fn all_constraints_satisfied() {
    let check_result = check_source(&source());

    // Must have at least some constraint results (file has active constraints)
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
    );

    // No constraint should be Violated
    for entry in &check_result.constraint_results {
        assert_ne!(
            entry.satisfaction,
            Satisfaction::Violated,
            "constraint {} should not be Violated, got {:?}",
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
    let result = eval_source(&source());

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
    let result = eval_source(&source());

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

/// Verify Bracket produces exactly 4 constraints from two `InRange` invocations,
/// with labels distributed as 2×`InRange[0]` and 2×`InRange[1]` (per-invocation
/// pred_idx reset), all Satisfied. This guards against silent drift if label
/// indexing ever changes.
#[test]
fn constraint_def_labels() {
    let check_result = check_source(&source());

    // Collect all Bracket InRange constraints
    let inrange_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Bracket"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("InRange["))
        })
        .collect();

    // Bracket has 2 InRange invocations × 2 predicates each = 4 total
    assert_eq!(
        inrange_constraints.len(),
        4,
        "expected exactly 4 Bracket InRange constraints (2 invocations × 2 predicates), got {}",
        inrange_constraints.len()
    );

    // Each invocation resets pred_idx to 0, so both emit InRange[0] and InRange[1]
    let count_label = |label: &str| -> usize {
        inrange_constraints
            .iter()
            .filter(|e| e.label.as_deref() == Some(label))
            .count()
    };

    assert_eq!(
        count_label("InRange[0]"),
        2,
        "expected exactly 2 Bracket constraints with label 'InRange[0]' (one per invocation)"
    );
    assert_eq!(
        count_label("InRange[1]"),
        2,
        "expected exactly 2 Bracket constraints with label 'InRange[1]' (one per invocation)"
    );

    // All 4 must be Satisfied
    for entry in &inrange_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Bracket InRange constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── Test 7: custom unit value ────────────────────────────────────────────────

/// Verify Bracket.clearance SI value is approximately 0.0127 (500 * 0.0000254).
/// Confirms the custom unit mil resolves correctly in param defaults.
#[test]
fn custom_unit_value() {
    let result = eval_source(&source());

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
    let result = eval_source(&source());

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
    let result = eval_source(&source());

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
    let check_result = check_source(&source());

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

// ── Test 11: violation detection ─────────────────────────────────────────────────────────────────────────────

/// Modify the source so clearance (100mm) exceeds width (50mm), violating the
/// `clearance < width` constraint. Assert at least one result has
/// `Satisfaction::Violated` — guards against silent false-Satisfied regressions
/// in the checker.
#[test]
fn violated_constraint_detected() {
    // Raise clearance from 500mil (~12.7mm) to 100mm, which exceeds width (50mm).
    // This causes `clearance < width` to be false (VIOLATED) while leaving
    // `clearance > 0mm` and all other Bracket constraints Satisfied.
    let violating = source().replace(
        "param clearance : Length = 500mil",
        "param clearance : Length = 100mm",
    );

    let check_result = check_source(&violating);

    // Full check must still run (not short-circuited by a compile error)
    assert!(
        check_result.constraint_results.len() >= 15,
        "expected >= 15 constraint results even for violating source, got {}",
        check_result.constraint_results.len()
    );

    // At least one constraint must be Violated
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();

    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint after raising clearance above width, \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}
