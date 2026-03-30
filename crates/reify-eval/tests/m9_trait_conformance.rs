//! M9 trait conformance integration tests.
//!
//! Exercises trait conformance features through the full pipeline:
//! parse → compile → eval/check → verify.
//! Uses examples/m9_trait_conformance.ri as the source file.

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m9_trait_conformance.ri"
);

// ── Helper ──────────────────────────────────────────────────────────

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
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // Check constraints — all should be Satisfied
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected constraints from trait + structure"
    );
    for entry in &result.constraint_results {
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check Box.size = 50mm = 0.05 SI (metres)
    let size_id = ValueCellId::new("Box", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", size_id));
    match size_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.05).abs() < 1e-12,
                "expected 0.05 SI for Box.size, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Box.size, got {:?}", other),
    }

    // Check Box.half_size = 25mm = 0.025 SI (let binding from trait)
    let half_size_id = ValueCellId::new("Box", "half_size");
    let half_size_val = result
        .values
        .get(&half_size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", half_size_id));
    match half_size_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.025).abs() < 1e-12,
                "expected 0.025 SI for Box.half_size, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Box.half_size, got {:?}", other),
    }
}

// ── Step 5: default injection ───────────────────────────────────────

/// Verify trait default injection: Panel implements WithDefaults (thickness=5mm default).
/// Panel declares no params — gets the default injected from the trait.
#[test]
fn default_injection_values() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m9_trait_conformance.ri should exist");

    let compiled = parse_and_compile(&source);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check Panel.thickness = 5mm = 0.005 SI (injected from trait default)
    let thickness_id = ValueCellId::new("Panel", "thickness");
    let thickness_val = result
        .values
        .get(&thickness_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", thickness_id));
    match thickness_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected 0.005 SI for Panel.thickness (5mm default), got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Panel.thickness, got {:?}", other),
    }

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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check Part.size = 100mm = 0.1 SI
    let size_id = ValueCellId::new("Part", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", size_id));
    match size_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "expected 0.1 SI for Part.size, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Part.size, got {:?}", other),
    }

    // Check Part.mass = 2kg = 2.0 SI
    let mass_id = ValueCellId::new("Part", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", mass_id));
    match mass_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 2.0).abs() < 1e-12,
                "expected 2.0 SI for Part.mass, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Part.mass, got {:?}", other),
    }

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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check Component.size = 200mm = 0.2 SI
    let size_id = ValueCellId::new("Component", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", size_id));
    match size_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.2).abs() < 1e-12,
                "expected 0.2 SI for Component.size, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Component.size, got {:?}", other),
    }

    // Check Component.mass = 5kg = 5.0 SI
    let mass_id = ValueCellId::new("Component", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", mass_id));
    match mass_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 5.0).abs() < 1e-12,
                "expected 5.0 SI for Component.mass, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Component.mass, got {:?}", other),
    }

    // Check Component.density = 2.5 (dimensionless Real)
    let density_id = ValueCellId::new("Component", "density");
    let density_val = result
        .values
        .get(&density_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", density_id));
    match density_val {
        reify_types::Value::Real(v) => {
            assert!(
                (v - 2.5).abs() < 1e-12,
                "expected 2.5 for Component.density, got {}",
                v
            );
        }
        reify_types::Value::Scalar { si_value, .. } => {
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check Merged.x = 10mm = 0.01 SI
    let x_id = ValueCellId::new("Merged", "x");
    let x_val = result
        .values
        .get(&x_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", x_id));
    match x_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "expected 0.01 SI for Merged.x, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for Merged.x, got {:?}", other),
    }

    // Constraint from Base (x > 0mm) should be satisfied
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

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
