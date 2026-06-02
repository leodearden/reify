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
use std::sync::OnceLock;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::{Satisfaction, Value};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_corner_cases.ri"
);

// ── Cached helpers ────────────────────────────────────────────────────────────

/// Read the integration_corner_cases.ri source file, cached across test runs.
fn source() -> String {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        fs::read_to_string(EXAMPLE_PATH)
            .unwrap_or_else(|e| panic!("{} should exist: {}", EXAMPLE_PATH, e))
    })
    .clone()
}

/// Parse and compile integration_corner_cases.ri, cached across test runs.
/// Panics if there are parse or compile errors.
fn compiled() -> reify_compiler::CompiledModule {
    static C: OnceLock<reify_compiler::CompiledModule> = OnceLock::new();
    C.get_or_init(|| {
        let src = source();
        let parsed = reify_syntax::parse(&src, ModulePath::single("integration_corner_cases"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors in integration_corner_cases.ri: {:?}",
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
            "compile errors in integration_corner_cases.ri: {:?}",
            errors
        );
        compiled
    })
    .clone()
}

/// Evaluate the cached compiled module with a fresh engine.
/// Returns the full EvalResult for per-test value assertions.
fn eval_ri_file() -> reify_eval::EvalResult {
    let compiled = compiled();
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
        "eval errors in integration_corner_cases.ri: {:?}",
        eval_errors
    );
    result
}

/// Evaluate the cached compiled module and run constraint checking.
/// Returns the CheckResult for satisfaction assertions.
/// Engine::check() calls eval() internally; its CheckResult.diagnostics already
/// contains all eval diagnostics, so no separate eval() call is needed.
fn eval_and_check_ri() -> reify_eval::CheckResult {
    let compiled = compiled();
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);
    assert_no_errors(
        &result.diagnostics,
        "integration_corner_cases.ri eval/check",
    );
    result
}

/// Assert that a diagnostics slice contains no entries with [`Severity::Error`].
/// Panics with the offending diagnostics and `context` label on failure.
fn assert_no_errors(diagnostics: &[Diagnostic], context: &str) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "errors in {}: {:?}", context, errors);
}

// ── step-1: smoke test ────────────────────────────────────────────────────────

/// Load integration_corner_cases.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn corner_cases_parses_and_compiles() {
    let result = eval_ri_file();
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for integration_corner_cases.ri"
    );
}

// ── step-3: type alias, trait, vacuous constraint def ────────────────────────

/// Verify JerkDemo.j has the correct Jerk dimension (Length·Time⁻³) AND SI
/// value (0.001 = 1mm/s³). This confirms the 3-deep alias chain
/// (Velocity → Acceleration → Jerk) resolves correctly, and pins the value the
/// step-9 compound-literal migration (`1mm / (1s*1s*1s)` -> `1mm/s^3`) must
/// preserve — the prior version pinned the dimension only.
#[test]
fn type_alias_three_deep_resolves() {
    let result = eval_ri_file();
    let j_id = ValueCellId::new("JerkDemo", "j");
    let j_val = result
        .values
        .get(&j_id)
        .unwrap_or_else(|| panic!("JerkDemo.j not found — 3-deep alias should resolve"));

    // Jerk = Length / Time³  →  DimensionVector: L¹·T⁻³
    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME.pow(3));
    match j_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension, &expected_dim,
                "JerkDemo.j dimension should be Length/Time^3 (Jerk), got: {:?}",
                dimension
            );
            // Value pin: 1mm/s³ = 0.001 m/s³ in SI. Guards the step-9 migration
            // against a value drift, not just a dimension drift.
            assert!(
                (si_value - 0.001).abs() < 1e-9,
                "JerkDemo.j SI value should be 0.001 (1mm/s³), got: {}",
                si_value
            );
        }
        other => panic!(
            "JerkDemo.j should be a Scalar (Jerk = Length/Time^3), got: {:?}",
            other
        ),
    }
}

/// Verify FullTraitImpl covers all trait member kinds:
///   - param:      `size` evaluates as a Scalar value
///   - let:        `doubled` evaluates as a Scalar value
///   - port:       FullTraitImpl declares conformance to FullTrait (which owns the port);
///     trait ports live in the trait definition, not in the implementing structure's
///     template, so we verify the trait_bounds relationship
///   - constraint: inherited `size > 0mm` is Satisfied (no Violated constraints)
///
/// Uses a single engine + single check() call: CheckResult.values serves param/let
/// assertions and CheckResult.constraint_results serves the constraint assertion.
/// This avoids the two-engine / three-eval pattern of the previous version.
#[test]
fn trait_all_member_kinds() {
    let compiled = compiled();

    // ── port: trait_bounds relationship (compiled module, no eval needed) ──
    // Ports declared in a trait live in the trait definition, not in the implementing
    // structure's TopologyTemplate.ports. We verify via trait_bounds that FullTraitImpl
    // declares conformance to FullTrait (which has `port output : out FullTrait`).
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "FullTraitImpl")
        .expect("FullTraitImpl template must exist in compiled module");
    assert!(
        template.trait_bounds.iter().any(|b| b == "FullTrait"),
        "FullTraitImpl should declare conformance to FullTrait (which has the 'output' port), \
         found trait_bounds: {:?}",
        template.trait_bounds
    );

    // ── single engine: check() gives values + constraint_results in one eval pass ──
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

    // Assert no eval-severity errors from the single CheckResult.
    assert_no_errors(&check_result.diagnostics, "trait_all_member_kinds");

    // ── param + let: value assertions from CheckResult.values ──
    let size_id = ValueCellId::new("FullTraitImpl", "size");
    let size_val = check_result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("FullTraitImpl.size not found"));
    assert!(
        matches!(size_val, Value::Scalar { .. }),
        "FullTraitImpl.size should be Scalar, got: {:?}",
        size_val
    );

    let doubled_id = ValueCellId::new("FullTraitImpl", "doubled");
    let doubled_val = check_result
        .values
        .get(&doubled_id)
        .unwrap_or_else(|| panic!("FullTraitImpl.doubled not found"));
    assert!(
        matches!(doubled_val, Value::Scalar { .. }),
        "FullTraitImpl.doubled should be Scalar, got: {:?}",
        doubled_val
    );

    // ── constraint: verify no Violated constraints on FullTraitImpl ──
    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "FullTraitImpl" && e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "FullTraitImpl should have no Violated constraints (e.g. size > 0mm should be Satisfied), \
         violated: {:?}",
        violated.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

/// Verify VacuousUser compiles and has a determined w value (vacuous constraint def = 0 predicates).
#[test]
fn vacuous_constraint_def_compiles() {
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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

/// Verify OptionEdgeCases.s evaluates to Value::Option(Some(Undef)).
/// some(u) where u is an unset param (Undef) wraps Undef in an Option;
/// the inner value must be Undef — not just Some(_).
#[test]
fn option_some_undef_is_determined() {
    let result = eval_ri_file();
    let s_id = ValueCellId::new("OptionEdgeCases", "s");
    let s_val = result
        .values
        .get(&s_id)
        .unwrap_or_else(|| panic!("OptionEdgeCases.s not found"));
    match s_val {
        Value::Option(Some(inner)) => {
            assert_eq!(
                **inner,
                Value::Undef,
                "some(undef) inner value should be Undef, got: {:?}",
                inner
            );
        }
        other => panic!(
            "some(undef) should produce Value::Option(Some(Undef)), got: {:?}",
            other
        ),
    }
}

// ── step-11: recursive depth=0 no child ──────────────────────────────────────

/// Verify RecTree.depth evaluates to Value::Int(0).
/// With depth=0 the where guard prevents child from being unfolded.
#[test]
fn recursive_depth_zero_no_child() {
    let result = eval_ri_file();

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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
    let result = eval_ri_file();
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
            assert!(
                *lower_inclusive,
                "EqualRange.r lower_inclusive should be true for `5mm..5mm` (non-exclusive `..`)"
            );
            assert!(
                *upper_inclusive,
                "EqualRange.r upper_inclusive should be true for `5mm..5mm` (non-exclusive `..`)"
            );
        }
        other => panic!("EqualRange.r should be Value::Range, got: {:?}", other),
    }
}

/// Verify AutoFreeMulti.x and AutoFreeMulti.y exist as cells.
/// constrained(x) and constrained(y) should be Satisfied (auto = solver variables).
#[test]
fn auto_free_multiple_solutions() {
    let check_result = eval_and_check_ri();

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
        auto_constraints
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}

// ── step-18: all constraints no violations, count >= 25 ──────────────────────

/// Run check() on the full .ri file, verify no Violated constraints.
#[test]
fn all_non_auto_constraints_satisfied() {
    let check_result = eval_and_check_ri();

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
    let check_result = eval_and_check_ri();

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
