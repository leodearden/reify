//! M9 constraint def integration tests.
//!
//! Exercises constraint definition features through the full pipeline:
//! parse → compile → eval/check → verify.
//! Uses examples/m9_constraint_def.ri as the source file.

use reify_core::{ModulePath, ValueCellId};
use reify_ir::Satisfaction;
use reify_test_support::{check_source, make_simple_engine, parse_and_compile};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_constraint_def.ri"
);

// ── Test 1: file parses without errors ──────────────────────────────────────

/// Read m9_constraint_def.ri and verify it parses without errors.
#[test]
fn constraint_def_ri_parses() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

// ── Test 2: compiles without error diagnostics ───────────────────────────────

/// Compile the .ri file and verify no error diagnostics.
/// Also confirms at least one template exists (structures are present).
#[test]
fn constraint_def_ri_compiles_no_errors() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);

    // Must have at least one template (structure)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Compile, eval and check — all constraint results must be Satisfied.
#[test]
fn constraint_def_all_constraints_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let check_result = check_source(&source);

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

// ── Test 4: single-predicate def values ──────────────────────────────────────

/// Wall.thickness = 5mm = 0.005 SI; constraint carries label MinThickness[0].
#[test]
fn single_predicate_values() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let check_result = check_source(&source);

    // Wall.thickness = 5mm = 0.005 m (SI)
    let thickness_id = ValueCellId::new("Wall", "thickness");
    let thickness_val = check_result
        .values
        .get(&thickness_id)
        .unwrap_or_else(|| panic!("Wall.thickness not found in values"));
    match thickness_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected 0.005 SI for Wall.thickness (5mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Wall.thickness, got {:?}", other),
    }

    // Wall should have exactly 1 constraint result labeled MinThickness[0]
    let wall_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Wall")
        .collect();
    assert_eq!(
        wall_constraints.len(),
        1,
        "expected 1 constraint for Wall, got {}",
        wall_constraints.len()
    );
    assert_eq!(
        wall_constraints[0].label,
        Some("MinThickness#0[0]".to_string()),
        "expected label MinThickness[0], got {:?}",
        wall_constraints[0].label
    );
    assert_eq!(
        wall_constraints[0].satisfaction,
        Satisfaction::Satisfied,
        "Wall MinThickness[0] should be Satisfied"
    );
}

// ── Test 5: multi-param Bounded values ───────────────────────────────────────

/// Pipe.diameter = 20mm; Bounded[0] (x>=lo) and Bounded[1] (x<=hi) both Satisfied.
#[test]
fn multi_param_bounded_values() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let check_result = check_source(&source);

    // Pipe.diameter = 20mm = 0.020 m (SI)
    let diameter_id = ValueCellId::new("Pipe", "diameter");
    let diameter_val = check_result
        .values
        .get(&diameter_id)
        .unwrap_or_else(|| panic!("Pipe.diameter not found in values"));
    match diameter_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.020).abs() < 1e-12,
                "expected 0.020 SI for Pipe.diameter (20mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Pipe.diameter, got {:?}", other),
    }

    // Pipe should have Bounded[0] and Bounded[1]
    let find_pipe = |label: &str| {
        check_result
            .constraint_results
            .iter()
            .find(|e| e.id.entity == "Pipe" && e.label == Some(label.to_string()))
            .unwrap_or_else(|| panic!("expected Pipe constraint with label '{label}'"))
    };
    let bounded0 = find_pipe("Bounded#0[0]");
    let bounded1 = find_pipe("Bounded#0[1]");
    assert_eq!(
        bounded0.satisfaction,
        Satisfaction::Satisfied,
        "Pipe Bounded[0] should be Satisfied"
    );
    assert_eq!(
        bounded1.satisfaction,
        Satisfaction::Satisfied,
        "Pipe Bounded[1] should be Satisfied"
    );
}

// ── Test 6: SafeRatio conjunction predicate labels ───────────────────────────

/// Beam uses SafeRatio; SafeRatio[0] (a/b > 0.5) and SafeRatio[1] (a/b < 2.0) both Satisfied.
#[test]
fn conjunction_predicate_labels() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // Beam must be in the compiled templates
    let has_beam = compiled.templates.iter().any(|t| t.name == "Beam");
    assert!(has_beam, "expected Beam template in compiled module");

    let find_beam = |label: &str| {
        check_result
            .constraint_results
            .iter()
            .find(|e| e.id.entity == "Beam" && e.label == Some(label.to_string()))
            .unwrap_or_else(|| panic!("expected Beam constraint with label '{label}'"))
    };
    let ratio0 = find_beam("SafeRatio#0[0]");
    let ratio1 = find_beam("SafeRatio#0[1]");
    assert_eq!(
        ratio0.satisfaction,
        Satisfaction::Satisfied,
        "Beam SafeRatio[0] should be Satisfied"
    );
    assert_eq!(
        ratio1.satisfaction,
        Satisfaction::Satisfied,
        "Beam SafeRatio[1] should be Satisfied"
    );
}

// ── Test 7: same def reused by two structures ─────────────────────────────────

/// ThinPlate and ThickPlate both use MinThickness with different arg values.
/// Both produce MinThickness[0] that is Satisfied.
#[test]
fn reused_def_both_structures() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // Both structures must exist
    let has_thin = compiled.templates.iter().any(|t| t.name == "ThinPlate");
    let has_thick = compiled.templates.iter().any(|t| t.name == "ThickPlate");
    assert!(has_thin, "expected ThinPlate template");
    assert!(has_thick, "expected ThickPlate template");

    for entity in &["ThinPlate", "ThickPlate"] {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|e| {
                &e.id.entity.as_str() == entity && e.label == Some("MinThickness#0[0]".to_string())
            })
            .unwrap_or_else(|| panic!("expected {entity} MinThickness[0]"));
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "{entity} MinThickness[0] should be Satisfied"
        );
    }
}

// ── Test 8: named args in non-declaration order ───────────────────────────────

/// FlippedPipe uses Bounded with args in reverse order (hi, lo, x) instead of (x, lo, hi).
/// Substitution must be correct regardless of arg order.
#[test]
fn named_args_order_independent() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // FlippedPipe must be present
    let has_flipped = compiled.templates.iter().any(|t| t.name == "FlippedPipe");
    assert!(has_flipped, "expected FlippedPipe template");

    // FlippedPipe should have Bounded[0] and Bounded[1] both Satisfied
    let flipped_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "FlippedPipe")
        .collect();
    assert_eq!(
        flipped_constraints.len(),
        2,
        "expected 2 constraints for FlippedPipe, got {}",
        flipped_constraints.len()
    );
    for entry in &flipped_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "FlippedPipe {} should be Satisfied",
            entry.label.as_deref().unwrap_or("(no label)")
        );
    }
}

// ── Test 9: total constraint count ───────────────────────────────────────────

/// The example file should produce >= 8 total active constraint results.
#[test]
fn total_constraint_count() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let check_result = check_source(&source);

    assert!(
        check_result.constraint_results.len() >= 8,
        "expected >= 8 total constraint results, got {}",
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

// ── Test 10: where-guarded constraint inactive when guard = false ─────────────

/// InactiveWall has a where-guarded constraint with guard=false.
/// No constraint results should be emitted for it.
#[test]
fn guarded_constraint_inactive() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // InactiveWall must exist as a template
    let has_inactive = compiled.templates.iter().any(|t| t.name == "InactiveWall");
    assert!(has_inactive, "expected InactiveWall template");

    // But it should produce 0 constraint results (guard is false)
    let inactive_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "InactiveWall")
        .collect();
    assert!(
        inactive_constraints.is_empty(),
        "expected no constraint results for InactiveWall (guard=false), got: {:?}",
        inactive_constraints
    );
}

// ── Test 11: where-guarded active constraint is checked and satisfied ──────────

/// ActiveWall has a where-guarded constraint with guard=true (enabled=true).
/// Exactly one Satisfied constraint result for MinThickness[0] should be present.
///
/// Covers task 878 subtest "active guard": verifies that the `where enabled` guard
/// with `enabled=true` flows through end-to-end and the constraint is both applied
/// and satisfied.
#[test]
fn guarded_constraint_active_is_checked_and_satisfied() {
    let source =
        std::fs::read_to_string(EXAMPLE_PATH).expect("examples/m9_constraint_def.ri should exist");

    let compiled = parse_and_compile(&source);
    let mut engine = make_simple_engine();
    let check_result = engine.check(&compiled);

    // ActiveWall must exist as a template
    let has_active = compiled.templates.iter().any(|t| t.name == "ActiveWall");
    assert!(has_active, "expected ActiveWall template");

    // Filter constraint results to just ActiveWall
    let active_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "ActiveWall")
        .collect();

    // Should have exactly 1 constraint result for ActiveWall (guard=true → constraint fires)
    assert_eq!(
        active_constraints.len(),
        1,
        "expected exactly 1 constraint result for ActiveWall (enabled=true), got {}",
        active_constraints.len()
    );

    // The result should be MinThickness[0] and should be Satisfied
    // (thickness=5mm satisfies MinThickness: t > 1mm)
    let result = &active_constraints[0];
    let label = result.label.as_deref().unwrap_or("(no label)");
    assert!(
        label.starts_with("MinThickness"),
        "expected label starting with 'MinThickness', got '{label}'"
    );
    assert_eq!(
        result.satisfaction,
        Satisfaction::Satisfied,
        "ActiveWall MinThickness[0] should be Satisfied (thickness=5mm > 1mm), got {:?}",
        result.satisfaction
    );
}
