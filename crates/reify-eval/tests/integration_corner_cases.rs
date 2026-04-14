//! Integration corner-cases tests.
//!
//! Exercises edge cases in the parse → compile → eval pipeline:
//!   - 3-deep type alias chain (Velocity → Acceleration → Jerk)
//!   - Trait with all member kinds (param, let, constraint, port)
//!   - Vacuous constraint def with 0 predicates
//!   - Empty list .count (EmptyListOps)
//!   - Undef propagation through arithmetic, comparison, negation
//!   - Option edge cases: none vs some(undef)
//!   - Recursive structure with depth=0 guard (no child unfolded)
//!   - 4-element chained comparison
//!   - Kleene three-valued logic: false∧Undef=false, true∨Undef=true
//!   - Range with equal bounds (degenerate range)
//!   - auto(free) multiple solver variables
//!
//! Uses examples/integration_corner_cases.ri as the source file.

use std::fs;

use reify_constraints::SimpleConstraintChecker;
use reify_types::{ModulePath, Satisfaction, Severity, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_corner_cases.ri"
);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Load a .ri file, parse, compile (asserting no errors), and evaluate.
/// Returns the full EvalResult for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
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
    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
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

/// Load integration_corner_cases.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn corner_cases_parses_and_compiles() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for integration_corner_cases.ri"
    );
}

// ── step-3: type alias, trait, vacuous constraint def ────────────────────────

/// Verify JerkDemo.j is a Scalar value (Jerk alias resolves to Length/Time^3 dimension).
#[test]
fn type_alias_three_deep_resolves() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let j_id = ValueCellId::new("JerkDemo", "j");
    let j_val = result
        .values
        .get(&j_id)
        .unwrap_or_else(|| panic!("JerkDemo.j not found — 3-deep alias should resolve"));
    assert!(
        matches!(j_val, Value::Scalar { .. }),
        "JerkDemo.j should be Scalar (Jerk = Length/Time^3), got: {:?}",
        j_val
    );
}

/// Verify FullTraitImpl has value cells for size and doubled.
#[test]
fn trait_all_member_kinds() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");

    let size_id = ValueCellId::new("FullTraitImpl", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("FullTraitImpl.size not found"));
    assert!(
        matches!(size_val, Value::Scalar { .. }),
        "FullTraitImpl.size should be Scalar, got: {:?}",
        size_val
    );

    let doubled_id = ValueCellId::new("FullTraitImpl", "doubled");
    let doubled_val = result
        .values
        .get(&doubled_id)
        .unwrap_or_else(|| panic!("FullTraitImpl.doubled not found"));
    assert!(
        matches!(doubled_val, Value::Scalar { .. }),
        "FullTraitImpl.doubled should be Scalar, got: {:?}",
        doubled_val
    );
}

/// Verify VacuousUser compiles and has a determined w value (vacuous constraint def = 0 predicates).
#[test]
fn vacuous_constraint_def_compiles() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let w_id = ValueCellId::new("VacuousUser", "w");
    let w_val = result
        .values
        .get(&w_id)
        .unwrap_or_else(|| panic!("VacuousUser.w not found"));
    assert!(
        matches!(w_val, Value::Scalar { .. }),
        "VacuousUser.w should be Scalar (determined), got: {:?}",
        w_val
    );
}

// ── step-5: empty list count ──────────────────────────────────────────────────

/// Verify EmptyListOps.n evaluates to Value::Int(0) (empty list .count = 0).
#[test]
fn empty_list_count_is_zero() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let n_id = ValueCellId::new("EmptyListOps", "n");
    let n_val = result
        .values
        .get(&n_id)
        .unwrap_or_else(|| panic!("EmptyListOps.n not found"));
    assert_eq!(
        n_val,
        &Value::Int(0),
        "EmptyListOps.n should be Int(0) for empty list .count, got: {:?}",
        n_val
    );
}

// ── step-7: undef propagation ─────────────────────────────────────────────────

/// Verify UndefPropagation.arith is Value::Undef (Undef + 1 = Undef).
#[test]
fn undef_propagation_arithmetic() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let arith_id = ValueCellId::new("UndefPropagation", "arith");
    let arith_val = result
        .values
        .get(&arith_id)
        .unwrap_or_else(|| panic!("UndefPropagation.arith not found"));
    assert_eq!(
        arith_val,
        &Value::Undef,
        "Undef + 1 should propagate to Undef, got: {:?}",
        arith_val
    );
}

/// Verify UndefPropagation.cmp is Value::Undef (Undef > 0 = Undef).
#[test]
fn undef_propagation_comparison() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let cmp_id = ValueCellId::new("UndefPropagation", "cmp");
    let cmp_val = result
        .values
        .get(&cmp_id)
        .unwrap_or_else(|| panic!("UndefPropagation.cmp not found"));
    assert_eq!(
        cmp_val,
        &Value::Undef,
        "Undef > 0 should propagate to Undef, got: {:?}",
        cmp_val
    );
}

/// Verify UndefPropagation.neg is Value::Undef (-Undef = Undef).
#[test]
fn undef_propagation_negation() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let neg_id = ValueCellId::new("UndefPropagation", "neg");
    let neg_val = result
        .values
        .get(&neg_id)
        .unwrap_or_else(|| panic!("UndefPropagation.neg not found"));
    assert_eq!(
        neg_val,
        &Value::Undef,
        "-Undef should propagate to Undef, got: {:?}",
        neg_val
    );
}

// ── step-9: option edge cases ─────────────────────────────────────────────────

/// Verify OptionEdgeCases.n evaluates to Value::Option(None).
#[test]
fn option_none_is_determined() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let n_id = ValueCellId::new("OptionEdgeCases", "n");
    let n_val = result
        .values
        .get(&n_id)
        .unwrap_or_else(|| panic!("OptionEdgeCases.n not found"));
    assert!(
        matches!(n_val, Value::Option(None)),
        "none should produce Value::Option(None), got: {:?}",
        n_val
    );
}

/// Verify OptionEdgeCases.s evaluates to Value::Option(Some(_)).
/// some(Undef) wraps the Undef in an Option — the Option itself is determined.
#[test]
fn option_some_undef_is_determined() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let s_id = ValueCellId::new("OptionEdgeCases", "s");
    let s_val = result
        .values
        .get(&s_id)
        .unwrap_or_else(|| panic!("OptionEdgeCases.s not found"));
    assert!(
        matches!(s_val, Value::Option(Some(_))),
        "some(undef) should produce Value::Option(Some(_)), got: {:?}",
        s_val
    );
}

// ── step-11: recursive depth=0 no child ──────────────────────────────────────

/// Verify RecTree.depth evaluates to Value::Int(0).
/// With depth=0 the where guard prevents child from being unfolded.
#[test]
fn recursive_depth_zero_no_child() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");

    // depth should be 0
    let depth_id = ValueCellId::new("RecTree", "depth");
    let depth_val = result
        .values
        .get(&depth_id)
        .unwrap_or_else(|| panic!("RecTree.depth not found"));
    assert_eq!(
        depth_val,
        &Value::Int(0),
        "RecTree.depth should be Int(0), got: {:?}",
        depth_val
    );

    // child should NOT exist (guard depth > 0 was false at depth=0)
    let child_depth_id = ValueCellId::new("RecTree.child", "depth");
    assert!(
        !result.values.contains(&child_depth_id),
        "RecTree.child.depth should not exist when depth=0 (guard prevented unfolding), but was found"
    );
}

// ── step-13: chained 4-element comparison ────────────────────────────────────

/// Verify ChainedFour.chain evaluates to Value::Bool(true).
/// a=1mm < b=2mm < c=3mm < d=4mm → all pairwise comparisons true → true.
#[test]
fn chained_comparison_four_elements() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let chain_id = ValueCellId::new("ChainedFour", "chain");
    let chain_val = result
        .values
        .get(&chain_id)
        .unwrap_or_else(|| panic!("ChainedFour.chain not found"));
    assert_eq!(
        chain_val,
        &Value::Bool(true),
        "1mm < 2mm < 3mm < 4mm should be true, got: {:?}",
        chain_val
    );
}

// ── step-15: Kleene three-valued logic ───────────────────────────────────────

/// Verify KleeneEdge.and_absorb evaluates to Value::Bool(false).
/// false ∧ Undef = false (Kleene AND absorbing element).
#[test]
fn kleene_and_false_absorbs_undef() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let and_id = ValueCellId::new("KleeneEdge", "and_absorb");
    let and_val = result
        .values
        .get(&and_id)
        .unwrap_or_else(|| panic!("KleeneEdge.and_absorb not found"));
    assert_eq!(
        and_val,
        &Value::Bool(false),
        "false && Undef should be Bool(false) (Kleene AND), got: {:?}",
        and_val
    );
}

/// Verify KleeneEdge.or_absorb evaluates to Value::Bool(true).
/// true ∨ Undef = true (Kleene OR absorbing element).
#[test]
fn kleene_or_true_absorbs_undef() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let or_id = ValueCellId::new("KleeneEdge", "or_absorb");
    let or_val = result
        .values
        .get(&or_id)
        .unwrap_or_else(|| panic!("KleeneEdge.or_absorb not found"));
    assert_eq!(
        or_val,
        &Value::Bool(true),
        "true || Undef should be Bool(true) (Kleene OR), got: {:?}",
        or_val
    );
}

/// Verify KleeneEdge.implies_vacuous evaluates to Value::Bool(true).
/// !false || Undef = true || Undef = true (vacuous implication pattern).
#[test]
fn kleene_implies_vacuous_true() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let imp_id = ValueCellId::new("KleeneEdge", "implies_vacuous");
    let imp_val = result
        .values
        .get(&imp_id)
        .unwrap_or_else(|| panic!("KleeneEdge.implies_vacuous not found"));
    assert_eq!(
        imp_val,
        &Value::Bool(true),
        "!false || Undef should be Bool(true) (vacuous implication), got: {:?}",
        imp_val
    );
}

// ── step-17: range equal bounds, auto free params ────────────────────────────

/// Verify EqualRange.r is a Value::Range with equal lower/upper bounds.
#[test]
fn range_equal_bounds_value() {
    let result = eval_ri_file(EXAMPLE_PATH, "integration_corner_cases");
    let r_id = ValueCellId::new("EqualRange", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("EqualRange.r not found"));
    match r_val {
        Value::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            assert!(lower.is_some(), "EqualRange.r lower bound should be Some");
            assert!(upper.is_some(), "EqualRange.r upper bound should be Some");
            // Both bounds should be equal (degenerate range)
            assert_eq!(
                lower.as_deref(),
                upper.as_deref(),
                "EqualRange.r lower and upper bounds should be equal (5mm..5mm)"
            );
            let _ = lower_inclusive;
            let _ = upper_inclusive;
        }
        other => panic!("EqualRange.r should be Value::Range, got: {:?}", other),
    }
}

/// Verify AutoFreeMulti.x and AutoFreeMulti.y exist as cells.
/// constrained(x) and constrained(y) should be Satisfied (auto = solver variables).
#[test]
fn auto_free_multiple_solutions() {
    let source = fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("integration_corner_cases"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _result = engine.eval(&compiled);
    let check_result = engine.check(&compiled);

    // constrained(x) and constrained(y) should be Satisfied
    let auto_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "AutoFreeMulti")
        .collect();
    assert!(
        !auto_constraints.is_empty(),
        "AutoFreeMulti should have at least some constraint results"
    );
    // At minimum, constrained(x) and constrained(y) should be Satisfied
    let satisfied_count = auto_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Satisfied)
        .count();
    assert!(
        satisfied_count >= 2,
        "AutoFreeMulti should have at least 2 Satisfied constraints (constrained(x) and constrained(y)), got {} satisfied of {}: {:?}",
        satisfied_count,
        auto_constraints.len(),
        auto_constraints.iter().map(|e| (&e.id, &e.satisfaction)).collect::<Vec<_>>()
    );
}

// ── step-18: all constraints no violations, count >= 25 ──────────────────────

/// Run check() on the full .ri file, verify no Violated constraints.
#[test]
fn all_non_auto_constraints_satisfied() {
    let source = fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("integration_corner_cases"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _ = engine.eval(&compiled);
    let check_result = engine.check(&compiled);

    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "no constraints should be Violated, but found {} violated: {:?}",
        violated.len(),
        violated.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

/// Assert total constraint count (Satisfied + Indeterminate) >= 25.
#[test]
fn assertion_count_at_least_25() {
    let source = fs::read_to_string(EXAMPLE_PATH)
        .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("integration_corner_cases"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let _ = engine.eval(&compiled);
    let check_result = engine.check(&compiled);

    let total = check_result.constraint_results.len();
    assert!(
        total >= 25,
        "expected >= 25 total constraints, got {}. Breakdown: {:?}",
        total,
        check_result
            .constraint_results
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}
