//! Eval-signal pins for the polymorphic-zero coercion in a TRAIT body
//! (task 4485/β, §7.2 — companion to `polymorphic_zero_eval.rs`).
//!
//! WHAT THESE ADD. Two of the stdlib sites whose comments assert the coercion
//! live in trait bodies rather than structure bodies:
//!
//!   - `structural_physical.ri` — `trait Physical { constraint material.density
//!     > 0kg/m^3 }` (MEMBER-ACCESS operand).
//!   - `materials_electrical.ri` — `trait Insulating { constraint
//!     dielectric_strength > 0.0V/m }`, where the param is INHERITED from
//!     `ElectricallyCharacterized`.
//!
//! Those shapes ARE checkable at COMPILE level, and are pinned there — see
//! `trait_body_with_conformer_member_access_gt_zero_no_error` and
//! `trait_body_inherited_param_gt_zero_no_error` in
//! `reify-compiler/tests/polymorphic_zero_tests.rs`. (Only a CONFORMER-LESS
//! trait body goes unchecked; add a conformer and a mismatch emits
//! `DiagnosticCode::DimensionMismatch`.) A regressed coercion would therefore
//! be LOUD, not a silent degradation: `material.density > 0` would land on the
//! `(Type::Scalar{..}, Type::Int)` arm of `emit_comparison_operand_diagnostics`
//! and error at compile time.
//!
//! What this file adds on top is the RUNTIME SATISFACTION signal, which no
//! compile pin can express: that the injected trait-body constraint actually
//! evaluates, and discriminates `Satisfied` from `Violated` — rather than
//! degrading to `Satisfaction::Indeterminate`, which is what an
//! uncoerced dimensioned-vs-Real comparison yields in `eval_cmp` if it ever
//! reaches eval.
//!
//! NOTE on the harness: `check_source_with_stdlib` asserts a clean compile
//! before evaluating, so these tests presuppose the compile pins above; they do
//! not stand in for them.
//!
//! Mechanism digest: `docs/notes/dimensioned-zero-coercion.md`.

use reify_ir::Satisfaction;
use reify_test_support::check_source_with_stdlib;

/// Assert `result` holds exactly `expected` constraint results and every one is
/// `Satisfied`, with a readable failure.
///
/// The count is pinned, not merely checked non-empty: a silently DROPPED
/// trait-body constraint would otherwise stay invisible if some future
/// stdlib/prelude change injected an unrelated always-satisfied constraint into
/// the conformer, and both pins below would go vacuous. Matches the `== 1` pin
/// in `trait_body_member_access_gt_bare_zero_violated_when_zero`.
fn assert_all_satisfied(result: &reify_eval::CheckResult, expected: usize, what: &str) {
    assert_eq!(
        result.constraint_results.len(),
        expected,
        "{what}: expected exactly {expected} constraint result(s) (the trait \
         body's constraint must be injected into the conformer, and nothing \
         else); got {:?}",
        result.constraint_results
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
/// the compile pin lives in `polymorphic_zero_tests.rs`, and this one adds that
/// the coerced constraint genuinely EVALUATES to `Satisfied` at runtime.
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
    assert_all_satisfied(&result, 1, "trait-body `material.density > 0`");
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
    assert_all_satisfied(
        &result,
        1,
        "trait-body inherited `dielectric_strength > 0.0`",
    );
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
