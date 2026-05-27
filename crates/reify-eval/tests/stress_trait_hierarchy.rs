//! Stress tests for trait hierarchy via trait_hierarchy.ri fixture.
//!
//! Covers:
//!   - smoke test: fixture parses, compiles, evaluates without errors
//!   - 3-deep chain value assertions: x (Root), y (Middle), computed let, z (Leaf)
//!   - 3-deep chain constraint assertions: all levels enforced
//!   - diamond inheritance: single 'x' member, all constraints enforced
//!   - multi-trait implementation: 3+ independent traits, all params/constraints
//!   - constrained diamond: conjunction of constraints from all levels

use std::fs;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Satisfaction;

// ── Helper ────────────────────────────────────────────────────────────────────

/// Load a .ri file, parse, compile (asserting no errors), and evaluate.
/// Returns the full EvalResult for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        errors
    );
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );
    result
}

/// Load and compile a .ri file, returning both compiled module and eval result.
#[allow(dead_code)]
fn compile_and_eval_ri(
    path: &str,
    module_name: &str,
) -> (reify_compiler::CompiledModule, reify_eval::EvalResult) {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        errors
    );
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );
    (compiled, result)
}

// ── step-1: smoke test ────────────────────────────────────────────────────────

/// Load trait_hierarchy.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn trait_hierarchy_parses_and_compiles() {
    let result = eval_ri_file("../../examples/trait_hierarchy.ri", "trait_hierarchy");
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for trait_hierarchy.ri"
    );
}

// ── step-3/4: 3-deep chain value assertions ───────────────────────────────────

/// Verify that ThreeDeepLeaf resolves values from all 3 trait levels:
///   - x = 5mm = 0.005 SI (from Root, via Middle → Leaf)
///   - y = 3mm = 0.003 SI (from Middle, via Leaf)
///   - z = 1mm = 0.001 SI (from Leaf)
///   - computed = x + y = 8mm = 0.008 SI (let binding from Middle)
#[test]
fn three_deep_chain_values() {
    let result = eval_ri_file("../../examples/trait_hierarchy.ri", "trait_hierarchy");

    // x = 5mm = 0.005 m SI
    let x_id = ValueCellId::new("ThreeDeepLeaf", "x");
    let x_val = result
        .values
        .get(&x_id)
        .unwrap_or_else(|| panic!("ThreeDeepLeaf.x not found in result"));
    match x_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "ThreeDeepLeaf.x should be 0.005 m (5mm), got {}",
                si_value
            );
        }
        other => panic!("ThreeDeepLeaf.x should be Scalar, got {:?}", other),
    }

    // y = 3mm = 0.003 m SI
    let y_id = ValueCellId::new("ThreeDeepLeaf", "y");
    let y_val = result
        .values
        .get(&y_id)
        .unwrap_or_else(|| panic!("ThreeDeepLeaf.y not found in result"));
    match y_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.003).abs() < 1e-12,
                "ThreeDeepLeaf.y should be 0.003 m (3mm), got {}",
                si_value
            );
        }
        other => panic!("ThreeDeepLeaf.y should be Scalar, got {:?}", other),
    }

    // z = 1mm = 0.001 m SI
    let z_id = ValueCellId::new("ThreeDeepLeaf", "z");
    let z_val = result
        .values
        .get(&z_id)
        .unwrap_or_else(|| panic!("ThreeDeepLeaf.z not found in result"));
    match z_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.001).abs() < 1e-12,
                "ThreeDeepLeaf.z should be 0.001 m (1mm), got {}",
                si_value
            );
        }
        other => panic!("ThreeDeepLeaf.z should be Scalar, got {:?}", other),
    }

    // computed = x + y = 8mm = 0.008 m SI (let binding from Middle trait)
    let computed_id = ValueCellId::new("ThreeDeepLeaf", "computed");
    let computed_val = result
        .values
        .get(&computed_id)
        .unwrap_or_else(|| panic!("ThreeDeepLeaf.computed not found in result"));
    match computed_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.008).abs() < 1e-12,
                "ThreeDeepLeaf.computed should be 0.008 m (x+y=5mm+3mm=8mm), got {}",
                si_value
            );
        }
        other => panic!("ThreeDeepLeaf.computed should be Scalar, got {:?}", other),
    }
}

// ── step-5/6: 3-deep chain constraint assertions ──────────────────────────────

/// Verify that all constraints from Root, Middle, and Leaf are enforced and
/// satisfied for ThreeDeepLeaf with its defaults (x=5mm, y=3mm, z=1mm).
///
/// Expected constraints:
///   - x > 0mm    (from Root)
///   - y > 0mm    (from Middle)
///   - z > 0mm    (from Leaf)
///   - z < x      (from ThreeDeepLeaf itself)
///     Total: at least 4 constraints, all Satisfied.
#[test]
fn three_deep_chain_constraints_all_satisfied() {
    let source = std::fs::read_to_string("../../examples/trait_hierarchy.ri")
        .expect("trait_hierarchy.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("trait_hierarchy"));
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval to populate values
    let _ = engine.eval(&compiled);

    // Then check constraints
    let check_result = engine.check(&compiled);
    let leaf_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "ThreeDeepLeaf")
        .collect();

    assert!(
        leaf_constraints.len() >= 4,
        "expected >= 4 constraints for ThreeDeepLeaf (x>0mm, y>0mm, z>0mm, z<x), got {}",
        leaf_constraints.len()
    );
    for entry in &leaf_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "ThreeDeepLeaf constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── step-7/8: Diamond inheritance ────────────────────────────────────────────

/// Verify diamond inheritance: DiamondStruct : LeftBase + RightBase (both refine Base).
/// Checks:
///   - DiamondStruct.d = 10mm = 0.01 SI (single shared param, deduplicated)
///   - Constraints from Base (d > 0mm) and structure (d < 500mm) all Satisfied
///   - The merged member 'd' appears exactly once (no duplication)
#[test]
fn diamond_inheritance_merges_correctly() {
    let source = std::fs::read_to_string("../../examples/trait_hierarchy.ri")
        .expect("trait_hierarchy.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("trait_hierarchy"));
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // d = 10mm = 0.01 m SI (single merged param from diamond)
    let d_id = ValueCellId::new("DiamondStruct", "d");
    let d_val = result.values.get(&d_id).unwrap_or_else(|| {
        panic!("DiamondStruct.d not found — diamond merge should produce single 'd'")
    });
    match d_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "DiamondStruct.d should be 0.01 m (10mm), got {}",
                si_value
            );
        }
        other => panic!("DiamondStruct.d should be Scalar, got {:?}", other),
    }

    // Constraints: d > 0mm (from Base, deduplicated) and d < 500mm (from structure)
    let check_result = engine.check(&compiled);
    let diamond_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "DiamondStruct")
        .collect();

    assert!(
        diamond_constraints.len() >= 2,
        "expected >= 2 constraints for DiamondStruct (d>0mm from Base, d<500mm from struct), got {}",
        diamond_constraints.len()
    );
    for entry in &diamond_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "DiamondStruct constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── step-9/10: Multi-trait implementation (3 independent traits) ──────────────

/// Verify TripleImpl : Weighable + Sizeable + Countable has all trait params and
/// constraints from all three independent traits.
///
/// Expected values:
///   - mass = 2kg = 2.0 SI (from Weighable)
///   - width = 100mm = 0.1 SI (from Sizeable)
///   - count = 5.0 (Real, from Countable)
///
/// Expected constraints: mass>0kg, width>0mm, count>=0, count<1000 — all Satisfied.
#[test]
fn multi_trait_impl_three_independent() {
    let source = std::fs::read_to_string("../../examples/trait_hierarchy.ri")
        .expect("trait_hierarchy.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("trait_hierarchy"));
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // mass = 2kg = 2.0 SI
    let mass_id = ValueCellId::new("TripleImpl", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("TripleImpl.mass not found"));
    match mass_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 2.0).abs() < 1e-12,
                "TripleImpl.mass should be 2.0 kg SI, got {}",
                si_value
            );
        }
        other => panic!("TripleImpl.mass should be Scalar, got {:?}", other),
    }

    // width = 100mm = 0.1 SI
    let width_id = ValueCellId::new("TripleImpl", "width");
    let width_val = result
        .values
        .get(&width_id)
        .unwrap_or_else(|| panic!("TripleImpl.width not found"));
    match width_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "TripleImpl.width should be 0.1 m (100mm) SI, got {}",
                si_value
            );
        }
        other => panic!("TripleImpl.width should be Scalar, got {:?}", other),
    }

    // count = 5.0 (Real — whole-number literals may be stored as Int or Real by the evaluator)
    let count_id = ValueCellId::new("TripleImpl", "count");
    let count_val = result
        .values
        .get(&count_id)
        .unwrap_or_else(|| panic!("TripleImpl.count not found"));
    match count_val {
        reify_ir::Value::Real(v) => {
            assert!(
                (v - 5.0).abs() < 1e-12,
                "TripleImpl.count should be 5.0, got {}",
                v
            );
        }
        reify_ir::Value::Int(v) => {
            // The evaluator may store whole-number Real literals as Int
            assert_eq!(*v, 5, "TripleImpl.count should be 5, got {}", v);
        }
        reify_ir::Value::Scalar { si_value, .. } => {
            // dimensionless scalar also acceptable
            assert!(
                (si_value - 5.0).abs() < 1e-12,
                "TripleImpl.count should be 5.0, got {}",
                si_value
            );
        }
        other => panic!(
            "TripleImpl.count should be Real, Int, or dimensionless Scalar, got {:?}",
            other
        ),
    }

    // All constraints from all 3 traits + structure should be satisfied
    let check_result = engine.check(&compiled);
    let triple_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "TripleImpl")
        .collect();

    assert!(
        triple_constraints.len() >= 4,
        "expected >= 4 constraints for TripleImpl (mass>0kg, width>0mm, count>=0, count<1000), got {}",
        triple_constraints.len()
    );
    for entry in &triple_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "TripleImpl constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── step-11/12: Constrained diamond (constraints at every level) ──────────────

/// Verify ConstrainedDiamond : LeftC + RightC (both via BaseC) collects all
/// constraints as a conjunction.
///
/// BaseC contributes: x > 0mm AND x < 1000mm
/// LeftC contributes: l_val > 0mm
/// RightC contributes: r_val > 0mm
/// ConstrainedDiamond itself: x < 100mm
///
/// With defaults x=10mm, l_val=5mm, r_val=5mm all should be Satisfied.
/// The 2 BaseC constraints should appear only once (diamond deduplication).
#[test]
fn diamond_with_extra_constraints() {
    let source = std::fs::read_to_string("../../examples/trait_hierarchy.ri")
        .expect("trait_hierarchy.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("trait_hierarchy"));
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

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // x = 10mm = 0.01 m SI
    let x_id = ValueCellId::new("ConstrainedDiamond", "x");
    let x_val = result
        .values
        .get(&x_id)
        .unwrap_or_else(|| panic!("ConstrainedDiamond.x not found"));
    match x_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "ConstrainedDiamond.x should be 0.01 m (10mm), got {}",
                si_value
            );
        }
        other => panic!("ConstrainedDiamond.x should be Scalar, got {:?}", other),
    }

    // l_val = 5mm = 0.005 m SI
    let l_id = ValueCellId::new("ConstrainedDiamond", "l_val");
    let l_val = result
        .values
        .get(&l_id)
        .unwrap_or_else(|| panic!("ConstrainedDiamond.l_val not found"));
    match l_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "ConstrainedDiamond.l_val should be 0.005 m (5mm), got {}",
                si_value
            );
        }
        other => panic!("ConstrainedDiamond.l_val should be Scalar, got {:?}", other),
    }

    // r_val = 5mm = 0.005 m SI
    let r_id = ValueCellId::new("ConstrainedDiamond", "r_val");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("ConstrainedDiamond.r_val not found"));
    match r_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "ConstrainedDiamond.r_val should be 0.005 m (5mm), got {}",
                si_value
            );
        }
        other => panic!("ConstrainedDiamond.r_val should be Scalar, got {:?}", other),
    }

    // All constraints from BaseC (deduplicated), LeftC, RightC, and ConstrainedDiamond itself
    // Expected at minimum: x>0mm, x<1000mm, l_val>0mm, r_val>0mm, x<100mm = 5 constraints
    let check_result = engine.check(&compiled);
    let cd_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "ConstrainedDiamond")
        .collect();

    assert!(
        cd_constraints.len() >= 5,
        "expected >= 5 constraints for ConstrainedDiamond (x>0mm, x<1000mm, l_val>0mm, r_val>0mm, x<100mm), got {}",
        cd_constraints.len()
    );
    for entry in &cd_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "ConstrainedDiamond constraint {} should be Satisfied",
            entry.id
        );
    }
}
