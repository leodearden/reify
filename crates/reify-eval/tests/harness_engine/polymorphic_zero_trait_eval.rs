//! Eval-signal pins for the polymorphic-zero coercion in a TRAIT body
//! (task 4485/β, §7.2 — companion to `polymorphic_zero_eval.rs`).
//!
//! WHY A SEPARATE SHAPE. The compile-breadth pins in
//! `reify-compiler/tests/polymorphic_zero_tests.rs` are all written as a
//! `structure`, deliberately: a trait compiled with NO conformer does not
//! dimension-check its body, so a "no error diagnostics" probe written against
//! a bare trait passes no matter what the coercion does (recorded in that
//! file's `member_access_mismatched_non_zero_still_errors`). Two of the stdlib
//! sites whose comments assert the coercion nevertheless live in trait bodies:
//!
//!   - `structural_physical.ri` — `trait Physical { constraint material.density
//!     > 0kg/m^3 }` (MEMBER-ACCESS operand).
//!   - `materials_electrical.ri` — `trait Insulating { constraint
//!     dielectric_strength > 0.0V/m }`, where the param is INHERITED from
//!     `ElectricallyCharacterized`.
//!
//! The reachable signal for those shapes is the RUNTIME one: compile a trait
//! whose body uses a bare zero, conform a structure to it, and assert the
//! injected constraint reports `Satisfaction::Satisfied`. If the zero ever
//! stopped being coerced, the failure would be SILENT at compile time —
//! `eval_cmp` would see Density-vs-Real, and the constraint would degrade to
//! `Satisfaction::Indeterminate` plus a ConstraintIndeterminate warning rather
//! than erroring. These pins convert that silent degradation into a test
//! failure.

use reify_ir::Satisfaction;
use reify_test_support::check_source_with_stdlib;

/// Assert every constraint result is `Satisfied`, with a readable failure.
fn assert_all_satisfied(result: &reify_eval::CheckResult, what: &str) {
    assert!(
        !result.constraint_results.is_empty(),
        "{what}: expected at least one constraint result (the trait body's \
         constraint must be injected into the conformer); got none"
    );
    for cr in &result.constraint_results {
        assert_eq!(
            cr.satisfaction,
            Satisfaction::Satisfied,
            "{what}: constraint {cr:?} should be Satisfied. Indeterminate here \
             means the bare zero was NOT coerced to the sibling's dimension, so \
             eval_cmp saw a dimensioned Scalar against a dimensionless Real."
        );
    }
}

/// TRAIT BODY + MEMBER ACCESS — the `structural_physical.ri` `trait Physical`
/// shape: `constraint material.density > 0` where `material : Material` is a
/// trait param and `density` is a struct field typed `Density`.
///
/// Backs the "a bare `0` compiles too, MEMBER-ACCESS operand included" claim in
/// that file's `trait Physical` note. The stdlib keeps `0kg/m^3` by convention;
/// this pin proves the bare form is genuinely equivalent at RUNTIME, which the
/// structure-only compile pins cannot reach for a trait body.
#[test]
fn trait_body_member_access_gt_bare_zero_satisfied() {
    let result = check_source_with_stdlib(
        r#"
trait HasBody {
    param material : Material
    constraint material.density > 0
}

structure Widget : HasBody {
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}
"#,
    );
    assert_all_satisfied(&result, "trait-body `material.density > 0`");
}

/// TRAIT BODY + INHERITED PARAM — the `materials_electrical.ri` `trait
/// Insulating` shape: the constraint lives in a refining trait while the param
/// it names is declared by the parent trait.
///
/// Backs the "`0.0` would compile too" claim in the Insulating note. Uses the
/// plain `0.0` Real literal (not `0`), matching the stdlib site's `0.0V/m`.
#[test]
fn trait_body_inherited_param_gt_bare_zero_satisfied() {
    let result = check_source_with_stdlib(
        r#"
trait Charged {
    param dielectric_strength : DielectricStrength
}

trait NonDegenerate : Charged {
    constraint dielectric_strength > 0.0
}

structure Insulator : NonDegenerate {
    param dielectric_strength : DielectricStrength = 20000000.0V/m
}
"#,
    );
    assert_all_satisfied(&result, "trait-body inherited `dielectric_strength > 0.0`");
}

/// NON-VACUITY GUARD for the two pins above.
///
/// The same trait/conformer shape with a value that genuinely fails the bound
/// must report `Violated` — not `Satisfied`, and not `Indeterminate`. A checker
/// that reported `Satisfied` unconditionally, or a coercion that silently
/// produced `Undef`, would be caught here.
#[test]
fn trait_body_member_access_gt_bare_zero_violated_when_zero() {
    let result = check_source_with_stdlib(
        r#"
trait HasBody {
    param material : Material
    constraint material.density > 0
}

structure Widget : HasBody {
    param material : Material = Material(name: "void", density: 0kg/m^3, youngs_modulus: 200GPa)
}
"#,
    );
    assert_eq!(
        result.constraint_results.len(),
        1,
        "expected exactly 1 constraint result, got {}",
        result.constraint_results.len()
    );
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "density (0kg/m^3) > 0 should be Violated (0 > 0 is false), got {:?}. \
         Indeterminate here would mean the zero was not coerced.",
        result.constraint_results[0].satisfaction
    );
}
