//! M9 trait conformance integration tests.
//!
//! Exercises trait conformance features through the full pipeline:
//! parse → compile → eval/check → verify.
//! Uses examples/m9_trait_conformance.ri as the source file.

use reify_test_support::{assert_no_eval_errors, make_simple_engine, parse_and_compile};
use reify_core::{ModulePath, ValueCellId};
use reify_ir::Satisfaction;

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_trait_conformance.ri"
);

// ── Local helper ────────────────────────────────────────────────────
//
// Scalar value lookup + SI assertion. Extracted from the repeated
// `get → unwrap_or_else → match Scalar → assert abs diff` pattern
// that appears for every dimensional field in this test file.
//
// Component.density uses a Real/Scalar union and is left inline.

#[track_caller]
fn assert_scalar_si(result: &reify_eval::EvalResult, entity: &str, field: &str, expected_si: f64) {
    let id = ValueCellId::new(entity, field);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", id));
    match val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - expected_si).abs() < 1e-12,
                "expected {} SI for {}.{}, got {}",
                expected_si,
                entity,
                field,
                si_value
            );
        }
        other => panic!("expected Scalar for {}.{}, got {:?}", entity, field, other),
    }
}

// ── Step 1: parse succeeds ──────────────────────────────────────────

/// Read m9_trait_conformance.ri and verify it parses without errors.
#[test]
fn trait_conformance_ri_parses() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
}

// ── Step 3: compile + eval + check ──────────────────────────────────

/// Compile the .ri file, eval, and verify all constraints are satisfied.
#[test]
fn trait_conformance_compiles_and_evals() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have at least 1 template
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

    // Eval
    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check constraints — all should be Satisfied
    let check_result = engine.check(&compiled);
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected constraints from trait + structure"
    );
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

/// Verify basic trait values: Box.size=0.05 SI (50mm), Box.half_size=0.025 SI.
#[test]
fn basic_trait_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check Box.size = 50mm = 0.05 SI (metres)
    assert_scalar_si(&eval_result, "Box", "size", 0.05);

    // Check Box.half_size = 25mm = 0.025 SI (let binding from trait)
    assert_scalar_si(&eval_result, "Box", "half_size", 0.025);
}

// ── Step 5: default injection ───────────────────────────────────────

/// Verify trait default injection: Panel implements WithDefaults (thickness=5mm default).
/// Panel declares no params — gets the default injected from the trait.
#[test]
fn default_injection_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check Panel.thickness = 5mm = 0.005 SI (injected from trait default)
    assert_scalar_si(&eval_result, "Panel", "thickness", 0.005);

    // Constraint from trait (thickness > 0mm) should be satisfied
    let check_result = engine.check(&compiled);
    let panel_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Panel")
        .collect();
    assert!(
        !panel_constraints.is_empty(),
        "expected at least one constraint for Panel"
    );
    for entry in &panel_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Panel constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── Step 7: multi-trait implementation ───────────────────────────────

/// Verify multi-trait: Part : Measurable + Weighable provides both traits' params.
#[test]
fn multi_trait_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check Part.size = 100mm = 0.1 SI
    assert_scalar_si(&eval_result, "Part", "size", 0.1);

    // Check Part.mass = 2kg = 2.0 SI
    assert_scalar_si(&eval_result, "Part", "mass", 2.0);

    // Check Part.half_size = 50mm = 0.05 SI (inherited let from Measurable: half_size = size / 2)
    assert_scalar_si(&eval_result, "Part", "half_size", 0.05);

    // All constraints from both traits + structure should be satisfied
    let check_result = engine.check(&compiled);
    let part_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Part")
        .collect();
    assert!(
        part_constraints.len() >= 3,
        "expected >=3 constraints for Part (size>0mm, mass>0kg, mass<100kg), got {}",
        part_constraints.len()
    );
    for entry in &part_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Part constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── Step 9: refinement chain ────────────────────────────────────────

/// Verify refinement chain: Physical : Measurable + Weighable, Component : Physical.
/// Component satisfies all transitive requirements.
#[test]
fn refinement_chain_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check Component.size = 200mm = 0.2 SI
    assert_scalar_si(&eval_result, "Component", "size", 0.2);

    // Check Component.mass = 5kg = 5.0 SI
    assert_scalar_si(&eval_result, "Component", "mass", 5.0);

    // Check Component.density = 2.5 (dimensionless Real — not SI-typed, so left inline)
    let density_id = ValueCellId::new("Component", "density");
    let density_val = eval_result
        .values
        .get(&density_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", density_id));
    match density_val {
        reify_ir::Value::Real(v) => {
            assert!(
                (v - 2.5).abs() < 1e-12,
                "expected 2.5 for Component.density, got {}",
                v
            );
        }
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 2.5).abs() < 1e-12,
                "expected 2.5 for Component.density, got {}",
                si_value
            );
        }
        other => panic!(
            "expected Real or Scalar for Component.density, got {:?}",
            other
        ),
    }

    // Check Component.half_size = size/2 = 200mm/2 = 100mm = 0.1 SI
    // (inherited let binding from Measurable trait: half_size = size / 2)
    assert_scalar_si(&eval_result, "Component", "half_size", 0.1);

    // All constraints from the full chain should be satisfied
    let check_result = engine.check(&compiled);
    let comp_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Component")
        .collect();
    assert!(
        comp_constraints.len() >= 4,
        "expected >=4 constraints for Component (size>0mm, mass>0kg, density>0, density<100), got {}",
        comp_constraints.len()
    );
    for entry in &comp_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Component constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── Step 11: diamond merging ────────────────────────────────────────

/// Verify diamond merging: Left + Right both refine Base.
/// Merged : Left + Right should have shared param x deduplicated.
#[test]
fn diamond_merging_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    // Check Merged.x = 10mm = 0.01 SI
    assert_scalar_si(&eval_result, "Merged", "x", 0.01);

    // Constraint deduplication: Left and Right both refine Base, so the `x > 0mm`
    // constraint from Base appears exactly once (diamond merging deduplicates it).
    // The second constraint is `x < 500mm` declared on the Merged structure itself.
    // Expected 2 constraints total: (1) x > 0mm from Base (deduplicated), (2) x < 500mm from Merged.
    let check_result = engine.check(&compiled);
    let merged_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Merged")
        .collect();
    assert_eq!(
        merged_constraints.len(),
        2,
        "expected exactly 2 constraints for Merged (x>0mm from Base once, x<500mm from structure), got {}",
        merged_constraints.len()
    );
    for entry in &merged_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Merged constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── Step 13: total constraint count + qualified access fallback ─────

/// Verify the .ri file produces >= 12 total constraint results across all structures.
/// This acts as the comprehensive assertion that all trait features contribute constraints.
#[test]
fn total_constraint_count() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    let check_result = engine.check(&compiled);
    assert!(
        check_result.constraint_results.len() >= 12,
        "expected >= 12 total constraints across all structures, got {}",
        check_result.constraint_results.len()
    );

    // All constraints should be satisfied
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

/// Verify qualified access disambiguation (fallback: qualified access not available in branch).
/// Instead, verify all feature areas produce correct constraint counts per structure,
/// including the Qualified structure with distinct params from Alpha + Beta traits.
#[test]
fn qualified_access_disambiguation() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let mut engine = make_simple_engine();
    let eval_result = engine.eval(&compiled);
    assert_no_eval_errors(&eval_result);

    let check_result = engine.check(&compiled);

    // Verify per-structure constraint counts
    let count_for = |entity: &str| -> usize {
        check_result
            .constraint_results
            .iter()
            .filter(|e| e.id.entity == entity)
            .count()
    };

    // Box: size>0mm (trait), height>0mm, size<1000mm = 3
    assert!(
        count_for("Box") >= 3,
        "expected >= 3 constraints for Box, got {}",
        count_for("Box")
    );

    // Panel: thickness>0mm (trait default) = 1
    assert!(
        count_for("Panel") >= 1,
        "expected >= 1 constraint for Panel, got {}",
        count_for("Panel")
    );

    // Part: size>0mm (Measurable), mass>0kg (Weighable), mass<100kg = 3
    assert!(
        count_for("Part") >= 3,
        "expected >= 3 constraints for Part, got {}",
        count_for("Part")
    );

    // Component: size>0mm, mass>0kg (chain), density>0 (Physical), density<100 = 4
    assert!(
        count_for("Component") >= 4,
        "expected >= 4 constraints for Component, got {}",
        count_for("Component")
    );

    // Merged: x>0mm (Base), x<500mm = 2
    assert!(
        count_for("Merged") >= 2,
        "expected >= 2 constraints for Merged, got {}",
        count_for("Merged")
    );

    // Qualified: a_val>0 (Alpha), b_val>0 (Beta), a_val<100, b_val<100, sum>1 = 5
    assert!(
        count_for("Qualified") >= 5,
        "expected >= 5 constraints for Qualified, got {}",
        count_for("Qualified")
    );
    // All Qualified constraints should be satisfied
    for entry in check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "Qualified")
    {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Qualified constraint {} should be satisfied",
            entry.id
        );
    }
}
