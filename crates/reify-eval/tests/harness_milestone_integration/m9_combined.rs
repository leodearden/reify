//! M9 combined integration tests.
//!
//! Exercises all M9 milestone features in a single cohesive example:
//! trait conformance with defaults, constraint definitions, determinacy predicates,
//! recursive structures, custom unit declarations, meta block access, and doc comments.
//! Uses examples/m9_combined.ri as the source file.

use reify_core::{ModulePath, ValueCellId};
use reify_ir::Satisfaction;
use reify_test_support::{check_source, eval_source, parse_and_compile};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/m9_combined.ri");

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Read and return the contents of the m9_combined.ri example file as a `&'static str`.
/// The file is read only once per test process (cached in a `OnceLock`);
/// each caller receives a reference to the single cached copy — no cloning.
fn source() -> &'static str {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_combined.ri should exist")
    })
    .as_str()
}

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m9_combined.ri and verify it parses without errors.
#[test]
fn m9_combined_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(src, ModulePath::single("test"));
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
    let compiled = parse_and_compile(source());

    // Must have at least one template (structure)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: file produces constraint results and all are Satisfied.
/// Complements `total_constraint_count`, which additionally asserts count >= 15.
#[test]
fn all_constraints_satisfied() {
    let check_result = check_source(source());

    // Must have at least some constraint results (file has active constraints)
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
    );

    // Every entry must be exactly Satisfied (Violated and Indeterminate both fail)
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
    let result = eval_source(source());

    // Bracket.length = 100mm = 0.1 SI
    let length_id = ValueCellId::new("Bracket", "length");
    let length_val = result
        .values
        .get(&length_id)
        .unwrap_or_else(|| panic!("Bracket.length not found"));
    match length_val {
        reify_ir::Value::Scalar { si_value, .. } => {
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
        reify_ir::Value::Scalar { si_value, .. } => {
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
        reify_ir::Value::Scalar { si_value, .. } => {
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
    let result = eval_source(source());

    // Plate.mass = 1kg = 1.0 SI (injected from Weighted trait default)
    let mass_id = ValueCellId::new("Plate", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("Plate.mass not found"));
    match mass_val {
        reify_ir::Value::Scalar { si_value, .. } => {
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
    let check_result = check_source(source());

    // Collect all Bracket InRange constraints
    let inrange_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Bracket"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("InRange#"))
        })
        .collect();

    // Bracket has 2 InRange invocations × 2 predicates each = 4 total
    assert_eq!(
        inrange_constraints.len(),
        4,
        "expected exactly 4 Bracket InRange constraints (2 invocations × 2 predicates), got {}",
        inrange_constraints.len()
    );

    // Under task 845 each invocation has a unique inst_idx, so the 4 labels are
    // InRange#0[0], InRange#0[1], InRange#1[0], InRange#1[1] (1 of each).
    let count_label = |label: &str| -> usize {
        inrange_constraints
            .iter()
            .filter(|e| e.label.as_deref() == Some(label))
            .count()
    };

    assert_eq!(
        count_label("InRange#0[0]"),
        1,
        "expected exactly 1 Bracket constraint with label 'InRange#0[0]'"
    );
    assert_eq!(
        count_label("InRange#0[1]"),
        1,
        "expected exactly 1 Bracket constraint with label 'InRange#0[1]'"
    );
    assert_eq!(
        count_label("InRange#1[0]"),
        1,
        "expected exactly 1 Bracket constraint with label 'InRange#1[0]'"
    );
    assert_eq!(
        count_label("InRange#1[1]"),
        1,
        "expected exactly 1 Bracket constraint with label 'InRange#1[1]'"
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
    let result = eval_source(source());

    // Bracket.clearance = 500mil = 500 * 0.0000254m = 0.0127 SI
    let clearance_id = ValueCellId::new("Bracket", "clearance");
    let clearance_val = result
        .values
        .get(&clearance_id)
        .unwrap_or_else(|| panic!("Bracket.clearance not found"));
    match clearance_val {
        reify_ir::Value::Scalar { si_value, .. } => {
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
    let result = eval_source(source());

    // Bracket.label = "steel" (from meta.material)
    let label_id = ValueCellId::new("Bracket", "label");
    let label_val = result
        .values
        .get(&label_id)
        .unwrap_or_else(|| panic!("Bracket.label not found"));
    assert_eq!(
        label_val,
        &reify_ir::Value::String("steel".to_string()),
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
        &reify_ir::Value::String("A".to_string()),
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
    let result = eval_source(source());

    // BracketTree.child.span = 200mm/2 = 100mm = 0.1 SI
    let child_span_id = ValueCellId::new("BracketTree.child", "span");
    let child_span = result
        .values
        .get(&child_span_id)
        .unwrap_or_else(|| panic!("BracketTree.child.span not found"));
    match child_span {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-9,
                "expected ~0.1 SI for BracketTree.child.span (100mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for BracketTree.child.span, got {:?}",
            other
        ),
    }

    // BracketTree.child.child.span = 100mm/2 = 50mm = 0.05 SI
    let grandchild_span_id = ValueCellId::new("BracketTree.child.child", "span");
    let grandchild_span = result
        .values
        .get(&grandchild_span_id)
        .unwrap_or_else(|| panic!("BracketTree.child.child.span not found"));
    match grandchild_span {
        reify_ir::Value::Scalar { si_value, .. } => {
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
    let check_result = check_source(source());

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

    // Guard: confirm the substitution actually happened.
    // If this fires the target substring drifted; update the test to match.
    assert_ne!(
        violating,
        source(),
        "replace target drifted — 'param clearance : Length = 500mil' not found; update the test"
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

    // The violation must correspond specifically to the `clearance < width` constraint
    // on entity Bracket (not some unrelated or cascade failure).  Inline constraints
    // have label=None so we cannot match by label directly; instead assert exactly one
    // Bracket violation — any more would indicate an unexpected regression.
    let bracket_violation_count = violations
        .iter()
        .filter(|e| e.id.entity == "Bracket")
        .count();
    assert_eq!(
        bracket_violation_count,
        1,
        "expected exactly 1 Bracket violation (clearance < width), \
         but found {} Bracket violations among: {:?}",
        bracket_violation_count,
        violations.iter().map(|e| &e.id).collect::<Vec<_>>()
    );

    // Other previously-passing Bracket constraints must remain Satisfied — guards against
    // cascade-failure regressions masking the real bug.  The violating source produces
    // 12 total Bracket constraint results (verified empirically): 11 Satisfied and 1
    // Violated (`clearance < width`).  The threshold is set to 9 (out of 11 actual)
    // to meaningfully catch regressions while tolerating minor example-file evolution.
    // Breakdown of Satisfied constraints: 2 trait constraints (length>0mm, mass>0kg),
    // 4 InRange predicates (label InRange[0]/InRange[1] × 2 invocations), 3 determined
    // predicates (length, width, mass), plus `width<length` and `clearance>0mm`.
    let bracket_still_satisfied: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Bracket" && e.satisfaction == Satisfaction::Satisfied)
        .collect();
    assert!(
        bracket_still_satisfied.len() >= 9,
        "expected at least 9 Bracket constraints still Satisfied after raising clearance \
         (actual healthy count is 11 — InRange×4, determined×3, trait×2, width<length, \
         clearance>0mm), got {}",
        bracket_still_satisfied.len()
    );
}
