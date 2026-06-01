//! β-lowering tests for ObjectiveSet (PRD §3.3 + §6.1).
//!
//! Asserts that the compiler lowers `minimize`/`maximize` declarations into a
//! `WeightedSum` `ObjectiveSet` with correct per-term metadata.
//!
//! GREEN phase: compiler src migrated to `ObjectiveSet` in Step-2.

use reify_core::ModulePath;
use reify_ir::{ObjectiveCombination, ObjectiveSense};

// ── helpers ──────────────────────────────────────────────────────────────────

fn compile_ok(src: &str, module_name: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(src, ModulePath::single(module_name));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        compiled.diagnostics.is_empty(),
        "compile diagnostics: {:?}",
        compiled.diagnostics
    );
    compiled
}

// ── I2: single minimize ───────────────────────────────────────────────────────

/// [I2] One `minimize <expr>` declaration → the template's objective is a
/// single-term `WeightedSum` `ObjectiveSet` whose term carries:
///   • sense == `Minimize`
///   • weight == 1.0
///   • priority == 0
#[test]
fn single_minimize_lowers_to_one_term_weighted_sum() {
    let module = compile_ok(
        r#"structure S {
    param x: Scalar = auto
    minimize x
}"#,
        "test_single_min",
    );

    let template = &module.templates[0];
    let obj = template
        .objective
        .as_ref()
        .expect("template should have an objective");

    // combination
    assert_eq!(
        obj.combination,
        ObjectiveCombination::WeightedSum,
        "combination must be WeightedSum, got {:?}",
        obj.combination
    );

    // exactly 1 term
    assert_eq!(obj.terms.len(), 1, "expected 1 term, got {}", obj.terms.len());

    let term = &obj.terms[0];
    assert_eq!(term.sense, ObjectiveSense::Minimize, "term.sense must be Minimize");
    assert_eq!(term.weight, 1.0, "term.weight must default to 1.0");
    assert_eq!(term.priority, 0, "term.priority must default to 0");

    // expression: ValueRef to S.x
    match &term.expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S", "ValueRef entity must be 'S'");
            assert_eq!(id.member, "x", "ValueRef member must be 'x'");
        }
        other => panic!("expected ValueRef for minimize expr, got {:?}", other),
    }
}

// ── I2: single maximize ───────────────────────────────────────────────────────

/// [I2] One `maximize <expr>` declaration → a 1-term `WeightedSum`
/// `ObjectiveSet` with `sense == Maximize`, `weight == 1.0`, `priority == 0`.
#[test]
fn single_maximize_lowers_to_one_term_weighted_sum() {
    let module = compile_ok(
        r#"structure S {
    param w: Scalar = 10mm
    param h: Scalar = 20mm
    let area = w * h
    maximize area
}"#,
        "test_single_max",
    );

    let template = &module.templates[0];
    let obj = template
        .objective
        .as_ref()
        .expect("template should have an objective");

    assert_eq!(obj.combination, ObjectiveCombination::WeightedSum);
    assert_eq!(obj.terms.len(), 1, "expected 1 term");

    let term = &obj.terms[0];
    assert_eq!(term.sense, ObjectiveSense::Maximize, "term.sense must be Maximize");
    assert_eq!(term.weight, 1.0);
    assert_eq!(term.priority, 0);

    match &term.expr.kind {
        reify_ir::CompiledExprKind::ValueRef(id) => {
            assert_eq!(id.entity, "S");
            assert_eq!(id.member, "area");
        }
        other => panic!("expected ValueRef for maximize expr, got {:?}", other),
    }
}

// ── B2: multi-term WeightedSum (PRD §8) ───────────────────────────────────────

/// [B2] Two `minimize` declarations in the same scope → a 2-term `WeightedSum`
/// `ObjectiveSet`.  Both terms carry default weight 1.0 and priority 0.
/// No `E_OBJECTIVE_CONFLICT` diagnostic is emitted (that is task ζ/4010).
#[test]
fn two_minimize_decls_lower_to_two_term_weighted_sum_no_conflict() {
    let module = compile_ok(
        r#"structure S {
    param mass: Scalar = auto
    param cost: Scalar = auto
    minimize mass
    minimize cost
}"#,
        "test_two_min",
    );

    let template = &module.templates[0];
    let obj = template
        .objective
        .as_ref()
        .expect("template should have an objective for two minimize decls");

    assert_eq!(
        obj.combination,
        ObjectiveCombination::WeightedSum,
        "multi-decl must also be WeightedSum"
    );
    assert_eq!(
        obj.terms.len(),
        2,
        "expected exactly 2 terms, got {}",
        obj.terms.len()
    );

    for (i, term) in obj.terms.iter().enumerate() {
        assert_eq!(
            term.sense,
            ObjectiveSense::Minimize,
            "term[{i}].sense must be Minimize"
        );
        assert_eq!(term.weight, 1.0, "term[{i}].weight must default to 1.0");
        assert_eq!(term.priority, 0, "term[{i}].priority must default to 0");
    }

    // compile_ok() already asserts compiled.diagnostics.is_empty(), which
    // guarantees NO E_OBJECTIVE_CONFLICT diagnostic was emitted (task ζ/4010
    // is the future owner of that diagnostic).
}

// ── no objective ──────────────────────────────────────────────────────────────

/// A structure with no `minimize`/`maximize` declaration → `objective` is `None`.
#[test]
fn no_objective_when_absent() {
    let module = compile_ok(
        r#"structure S {
    param x: Scalar = 5mm
}"#,
        "test_no_obj",
    );

    let template = &module.templates[0];
    assert!(template.objective.is_none(), "expected objective == None");
}
