//! Compile-breadth tests for the polymorphic-zero comparison coercion (task 4485/β, §7.2).
//!
//! Tests verify that comparison expressions `member > 0` / `0 < member` produce
//! NO error diagnostics for every dimension family touched by the stdlib migration:
//! - Base dimensions: Length, Mass
//! - Compound-product: MomentOfInertia (kg·m²)
//! - Compound-quotient: Stiffness (N/m), Velocity (m/s)
//!
//! The `no error diagnostics` assertions below are NOT vacuous: comparisons DO
//! dimension-check. `emit_comparison_operand_diagnostics` (expr.rs — dimension
//! arm added by task-4490 step-6, widened by task-4629 W5) emits
//! `DiagnosticCode::DimensionMismatch` for a Scalar-vs-Scalar comparison whose
//! dimensions differ, and an error for a dimensioned Scalar against a NON-ZERO
//! Int. The paired negatives live in `comparison_operand_guard_tests.rs`
//! (`scalar_different_dimensions_comparison_emits_dimension_mismatch`,
//! `dimensioned_scalar_gt_nonzero_int_emits_error`). So a clean compile here
//! means the zero really WAS coerced to the sibling's dimension, not that the
//! comparison went unchecked.
//! The eval signal (polymorphic_zero_eval.rs) proves the coercion fires at runtime
//! and produces Satisfaction::Satisfied, including for compound dimensions (Stiffness).
//!
//! TRAIT-BODY shapes ARE reachable from this file, provided a CONFORMER is
//! declared. The vacuity condition is narrower than "trait body": a trait
//! compiled with NO conformer is not dimension-checked at all (measured — see
//! `trait_body_without_conformer_is_not_dimension_checked`), so only a
//! conformer-less probe is vacuous. Add a conformer and the same body emits
//! `DiagnosticCode::DimensionMismatch` on a mismatch, so the trait-body pins
//! below carry real compile signal. `polymorphic_zero_trait_eval.rs` in
//! reify-eval complements them with the RUNTIME satisfaction signal
//! (Satisfied vs Violated), which no compile pin can express.
//!
//! Step-5 tests (additive position + edge/negative cases) are added in the same
//! file: the additive tests confirm the coercion fires before the Add/Sub dimension
//! guard, so `dimensioned ± 0` compiles without error.
//!
//! Mechanism digest (what these tests are pinning):
//! `docs/notes/dimensioned-zero-coercion.md`.

use reify_core::DiagnosticCode;
use reify_test_support::{assert_no_error_diagnostics, collect_errors, compile_source_with_stdlib};

/// Assert at least one error carries `code`, quoting `context` on failure.
///
/// Asserting the CODE, not merely "some error", matters: an unrelated future
/// diagnostic on an unrelated line would keep a bare non-empty check green
/// while the dimension guard rotted away. Mirrors the `assert_has_code` idiom
/// in `comparison_operand_guard_tests.rs` (promoting the shared copy into
/// `reify-test-support` is out of this task's file scope).
fn assert_has_code(errors: &[&reify_core::Diagnostic], code: DiagnosticCode, context: &str) {
    assert!(
        errors.iter().any(|d| d.code == Some(code)),
        "{context}: expected DiagnosticCode::{code:?}; got errors: {errors:#?}"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Step-3 (b): comparison-position breadth
// ────────────────────────────────────────────────────────────────────────────

/// `member > 0` — base dimension Length (right-is-zero form).
#[test]
fn length_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param len : Length = 1m
    constraint len > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "length > 0 comparison");
}

/// `0 < member` — base dimension Length (left-is-zero form).
#[test]
fn zero_lt_length_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param len : Length = 1m
    constraint 0 < len
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 < length comparison");
}

/// `member > 0` — base dimension Mass.
#[test]
fn mass_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > 0 comparison");
}

/// `member > 0` — compound-product dimension MomentOfInertia (kg·m²).
#[test]
fn moment_of_inertia_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param moi : MomentOfInertia = 1kg * 1m * 1m
    constraint moi > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "moment_of_inertia > 0 comparison");
}

/// `member > 0` — compound-quotient dimension Stiffness (N/m).
#[test]
fn stiffness_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param k : Stiffness = 1N / 1m
    constraint k > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "stiffness > 0 comparison");
}

/// `member > 0` — compound-quotient dimension Velocity (m/s).
#[test]
fn velocity_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param v : Velocity = 1m / 1s
    constraint v > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "velocity > 0 comparison");
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5: additive-position + edge/negative cases
// ────────────────────────────────────────────────────────────────────────────

/// `mass + 0` — additive-position zero coercion (right-is-zero form).
///
/// The compile-time rewrite promotes `0` to `Scalar<Mass>(0.0)` before the
/// Add/Sub dimension guard runs, so no "incompatible types" error is emitted.
#[test]
fn mass_add_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass + 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass + 0 additive coercion");
}

/// `mass - 0` — additive-position zero coercion (subtract zero form).
///
/// Zero-coercion promotes `0` to `Scalar<Mass>(0.0)`; the dimension guard sees
/// matching types on both sides and emits no error.
#[test]
fn mass_sub_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 3kg
    let m : Mass = mass - 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass - 0 subtractive coercion");
}

/// `constraint mass > -0` — unary-neg zero form in comparison (right-is-zero via UnOp{"-"}).
///
/// `is_syntactic_zero_literal` recurses through the `UnOp{"-"}` wrapper, treating
/// `-0` as a syntactic zero; the coercion adopts the Mass dimension and no error
/// diagnostic is emitted.
#[test]
fn mass_gt_neg_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > -0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > -0 unary-neg zero comparison");
}

/// `constraint mass > -0.0` — unary-neg real zero form.
#[test]
fn mass_gt_neg_zero_real_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > -0.0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass > -0.0 unary-neg real zero");
}

// ────────────────────────────────────────────────────────────────────────────
// Step-5 negative/no-op cases (must NOT change current behaviour)
// ────────────────────────────────────────────────────────────────────────────

/// `0 > 0` — both-zero: no coercion; compiles without error, both operands stay Int.
///
/// The gating predicate requires the OTHER operand to be Scalar<D> with
/// !D.is_dimensionless(). When both are zero/dimensionless, no adoption occurs.
#[test]
fn both_zero_comparison_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    constraint 0 > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 > 0 both-zero should compile");
}

/// `0 > 1.0` — dimensionless sibling: no coercion; compiles without error.
///
/// The sibling is dimensionless_scalar (D.is_dimensionless()), so no adoption.
#[test]
fn dimensionless_sibling_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    constraint 0 > 1.0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "0 > 1.0 dimensionless sibling");
}

/// `mass > 1 - 1` — constant-folded zero: not a *syntactic* literal, but a
/// dimensionless constant expression that folds to exactly `0`.
///
/// `1 - 1` is `ExprKind::BinOp`, not `NumberLiteral`, so `is_syntactic_zero_literal`
/// returns false — but `coerce_zero_operand` also recognizes constant-folded zeros
/// (via `const_numeric_value`, task 4490): a dimensionless folded zero adopts the
/// dimensioned sibling (`Mass`) and the comparison compiles clean. (A *dimensioned*
/// folded zero like `mass > 1m - 1m` is NOT coerced and still errors.)
#[test]
fn constant_folded_zero_not_coerced_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass > 1 - 1
}
"#,
    );
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "mass > 1-1 constant-folded zero no extra error",
    );
}

// ────────────────────────────────────────────────────────────────────────────
// task 6038 — operand shapes asserted by the corrected stdlib esc-3115-112
// comments
//
// The stdlib comment sweep restates several `> 0N` / `> 0Hz` / `>= 0kg` /
// `> 0 * 1N * 1s` / `> 0.0V/m` constraint literals as a readability CONVENTION
// rather than a requirement, on the grounds that a bare `0` would compile too.
//
// `coerce_zero_operand` gates on the sibling being `Type::Scalar{dimension}`
// with `!dimension.is_dimensionless()`, so it is entirely dimension-family-
// AGNOSTIC: one `#[test]` per stdlib dimension family would add no
// discriminating power over the comparison-breadth cases above (no mutation of
// the coercion could fail Force-but-not-Length). Family breadth is therefore
// kept as ONE cheap table-driven case; the axes that are genuinely new get a
// test each:
//   - `>=`, a distinct arm of `compile_binop`'s operator list,
//   - the MEMBER-ACCESS operand shape (plus its non-vacuity guard),
//   - the bare non-negated REAL literal `0.0` (only `-0.0` existed above).
// ────────────────────────────────────────────────────────────────────────────

/// Family breadth for the dimension aliases the swept stdlib sites use:
/// Force (`StepForce.magnitude > 0N`, `HarmonicForce.amplitude > 0N`,
/// `JointLimit.max_force`), Frequency (`HarmonicForce.frequency > 0Hz` and the
/// three shaper `target_frequency > 0Hz`), and Impulse
/// (`ImpulseForce.impulse > 0 * 1N * 1s`).
///
/// Table-driven on purpose: the coercion is dimension-agnostic, so these pin
/// that the ALIASES resolve to non-dimensionless `Scalar` (which is what puts
/// them on the coercion's path at all) without three near-identical `#[test]`
/// bodies that no mutation could separate.
#[test]
fn stdlib_dimension_families_gt_zero_no_error() {
    for (ty, init) in [
        ("Force", "1N"),
        ("Frequency", "1Hz"),
        ("Impulse", "1N * 1s"),
    ] {
        let compiled = compile_source_with_stdlib(&format!(
            r#"
structure S {{
    param x : {ty} = {init}
    constraint x > 0
}}
"#
        ));
        assert_no_error_diagnostics(&compiled.diagnostics, &format!("{ty} > 0 comparison"));
    }
}

/// `member >= 0` — the `>=` operator, which no case above exercises.
///
/// `coerce_zero_operand` runs in `compile_binop` before `infer_binop_type`, so
/// it is operator-agnostic; this pins that for the one stdlib site that uses a
/// non-strict bound, `MassProperties`' `constraint mass >= 0kg` (dynamics.ri).
#[test]
fn mass_ge_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param mass : Mass = 1kg
    constraint mass >= 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "mass >= 0 comparison");
}

/// `member == 0` / `member != 0` — the EQUALITY operators.
///
/// The `matches!` gate in `compile_binop` that admits an operand pair to
/// `coerce_zero_operand` lists `Eq | Ne` alongside the four relational
/// operators and `Add | Sub`. No other test in the compiler or eval trees
/// exercised the equality pair, so dropping `Eq`/`Ne` from that list would have
/// left the whole suite green while making the "every relational operator"
/// claim in `docs/notes/dimensioned-zero-coercion.md` false. No stdlib site
/// uses this shape today; it is pinned because the note claims it.
#[test]
fn mass_eq_ne_zero_no_error() {
    for op in ["==", "!="] {
        let compiled = compile_source_with_stdlib(&format!(
            r#"
structure S {{
    param mass : Mass = 1kg
    constraint mass {op} 0
}}
"#
        ));
        assert_no_error_diagnostics(&compiled.diagnostics, &format!("mass {op} 0 comparison"));
    }
}

/// NON-VACUITY GUARD for `mass_eq_ne_zero_no_error`.
///
/// The equality operators are dimension-checked exactly like the relational
/// ones, so a mismatched NON-ZERO RHS must error. Without this, the pin above
/// would pass even if `Eq`/`Ne` were exempted from dimension checking entirely
/// (rather than being coerced).
#[test]
fn mass_eq_ne_mismatched_non_zero_still_errors() {
    for op in ["==", "!="] {
        let compiled = compile_source_with_stdlib(&format!(
            r#"
structure S {{
    param mass : Mass = 1kg
    constraint mass {op} 1m
}}
"#
        ));
        let errors = collect_errors(&compiled.diagnostics);
        assert_has_code(
            &errors,
            DiagnosticCode::DimensionMismatch,
            &format!("`mass {op} 1m` (Mass vs Length)"),
        );
    }
}

/// `member > 0.0` — bare, NON-NEGATED real-literal zero.
///
/// `is_syntactic_zero_literal` matches `NumberLiteral { value == 0.0 }`
/// regardless of `is_real`, so `0.0` is coerced exactly like `0`. Every other
/// real-literal case in this file is the NEGATED `-0.0` form, so this is the
/// only pin on the plain `0.0` shape — the one the `dielectric_strength >
/// 0.0V/m` note in materials_electrical.ri rests on.
#[test]
fn real_literal_zero_rhs_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param dielectric_strength : DielectricStrength = 1V/m
    constraint dielectric_strength > 0.0
}
"#,
    );
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "dielectric_strength > 0.0 comparison",
    );
}

/// CONTRAST CASE — a NON-ZERO bare-numeric RHS against a dimensioned Scalar is
/// a COMPILE error, not a runtime `Indeterminate`.
///
/// Backs the `trait Conductive` note in materials_electrical.ri, whose
/// `constraint resistivity < 0.0001ohm*m` is the one swept site whose RHS is
/// non-zero. `0.0001` compiles to a DIMENSIONLESS `Type::Scalar` (which
/// `Display`s as `Real`), so `resistivity < 0.0001` lands on the
/// Scalar-vs-Scalar differing-dimension arm of
/// `emit_comparison_operand_diagnostics` and carries
/// `DiagnosticCode::DimensionMismatch`. Task-4629 W5 removed the former
/// `!ld.is_dimensionless() && !rd.is_dimensionless()` suppression that used to
/// let this shape through to eval.
///
/// So the dimensioned RHS at that site really is load-bearing — but for a
/// different reason than the pre-task-4485/β rationale claimed: the bare form
/// is rejected before eval, never degraded to `Indeterminate` at eval. Pairs
/// with `real_literal_zero_rhs_no_error` above: same literal kind, and only the
/// ZERO is coerced.
#[test]
fn nonzero_real_literal_rhs_emits_dimension_mismatch() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param resistivity : ElectricResistivity = 1ohm*m
    constraint resistivity < 0.0001
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert_has_code(
        &errors,
        DiagnosticCode::DimensionMismatch,
        "`resistivity < 0.0001` (ElectricResistivity vs dimensionless Real). If \
         this now passes cleanly, the `trait Conductive` note in \
         materials_electrical.ri needs revisiting",
    );
}

/// `material.density > 0` — MEMBER-ACCESS operand, the shape backing
/// `structural_physical.ri`'s `trait Physical` / `constraint material.density
/// > 0kg/m^3`.
///
/// The coercion reaches a member access: its gate keys on the sibling's
/// COMPILED TYPE being `Type::Scalar{D}` with non-dimensionless D — it is
/// syntax-agnostic about the sibling's expression shape — and
/// `material.density` compiles to `Scalar[kg·m^-3]`. It is asserted rather
/// than inferred from the plain-identifier cases above because the COMPILED
/// shape differs from theirs: a member access lowers to `IndexAccess`, not
/// `ValueRef`. That is the same node the subscript form lowers to, so
/// `index_access_subscript_gt_zero_no_error` below extends this pin rather
/// than contrasting with it — `structural_physical.ri`'s dimensioned
/// `moi_principal[0]` RHS is a call-site choice, not a shape the coercion
/// misses.
///
/// Written as a `structure`; the `trait Physical` shape it mirrors is pinned
/// separately by `trait_body_with_conformer_member_access_gt_zero_no_error`
/// below (a trait body needs a CONFORMER to be dimension-checked at all — see
/// `trait_body_without_conformer_is_not_dimension_checked`).
#[test]
fn member_access_lhs_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param material : Material
    constraint material.density > 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "member-access LHS > 0 comparison");
}

/// NON-VACUITY GUARD for `member_access_lhs_gt_zero_no_error` above.
///
/// A mismatched NON-ZERO literal against the same member access
/// (`Scalar[kg·m^-3]` vs `Scalar[m]`) must be rejected with
/// `DiagnosticCode::DimensionMismatch` — the code
/// `format_dimension_mismatch_diagnostic` attaches on the Scalar-vs-Scalar
/// arm. A non-zero is never coerced, so the sibling test passing means the
/// ZERO was coerced, not that member accesses go unchecked.
///
/// This guard is not ceremonial: the same probe written against a CONFORMER-LESS
/// `trait` body goes silent — see
/// `trait_body_without_conformer_is_not_dimension_checked` below.
#[test]
fn member_access_mismatched_non_zero_still_errors() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param material : Material
    constraint material.density > 1m
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert_has_code(
        &errors,
        DiagnosticCode::DimensionMismatch,
        "`material.density > 1m` (Density vs Length) — the member-access \
         dimension guard is not firing, which would make \
         member_access_lhs_gt_zero_no_error vacuous",
    );
}

/// `moi_principal[0] > 0` — SUBSCRIPT-IndexAccess operand, the shape backing
/// `structural_physical.ri`'s `trait Rigid` / `constraint moi_principal[0] >
/// 0.0 * 1kg * 1m * 1m`.
///
/// WHY THIS PIN EXISTS. `docs/notes/dimensioned-zero-coercion.md` once filed
/// the subscript form under "where the rule does NOT reach" while filing the
/// member-access form as covered. That split is not expressible in the
/// compiler: `material.density` and `moi_principal[0]` lower to the SAME
/// `CompiledExprKind::IndexAccess` (the member access destructures as
/// `IndexAccess { object: ValueRef("material"), index: Literal(String("density")) }`
/// — see `structural_physical_tests.rs`), and `coerce_zero_operand` gates only
/// on the sibling's compiled `result_type`, never on its expression shape. The
/// claim went wrong because nothing measured it; this pair measures it, so a
/// "member access is covered but index access is not" split must not be
/// re-asserted in prose. `structural_physical.ri` keeping the dimensioned RHS
/// there is a CALL-SITE preference, not a compiler limit.
///
/// Written as a `structure`, not a bare `trait`: per
/// `trait_body_without_conformer_is_not_dimension_checked` a conformer-less
/// trait body is not dimension-checked at all, so both halves of this pair
/// would pass vacuously.
#[test]
fn index_access_subscript_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param moi : Tensor<2,3,MomentOfInertia>
    let eig = eigenvalues(moi)
    constraint eig[0] > 0
}
"#,
    );
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "subscript-IndexAccess LHS > 0 comparison",
    );
}

/// NON-VACUITY GUARD for `index_access_subscript_gt_zero_no_error` above.
///
/// The identical subscript operand against a mismatched NON-ZERO literal must
/// be rejected with `DiagnosticCode::DimensionMismatch` (`Scalar[m^2·kg]` vs
/// `Scalar[m]`). That is what makes the positive a measurement rather than an
/// assertion: it proves `eig[0]` really typed as a dimensioned Scalar, so the
/// bare `0` in the sibling case must have been coerced — a dimensioned Scalar
/// against a dimensionless Real is a hard error, already pinned by
/// `nonzero_real_literal_rhs_emits_dimension_mismatch`.
#[test]
fn index_access_subscript_mismatched_non_zero_still_errors() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param moi : Tensor<2,3,MomentOfInertia>
    let eig = eigenvalues(moi)
    constraint eig[0] > 1m
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert_has_code(
        &errors,
        DiagnosticCode::DimensionMismatch,
        "`eig[0] > 1m` (MomentOfInertia vs Length) — the subscript-IndexAccess \
         dimension guard is not firing, which would make \
         index_access_subscript_gt_zero_no_error vacuous",
    );
}

// ────────────────────────────────────────────────────────────────────────────
// TRAIT-BODY shapes (task 6038 amendment): the two swept stdlib sites whose
// constraints live in a trait body rather than a structure body.
// ────────────────────────────────────────────────────────────────────────────

/// SCOPE OF THE VACUITY CARVE-OUT — a trait body compiled with NO conformer is
/// not dimension-checked at all.
///
/// Measured, not assumed: `constraint material.density > 1m` in a conformer-less
/// trait produces ZERO diagnostics, even though the same body in a structure
/// (`member_access_mismatched_non_zero_still_errors` above) errors with
/// `DimensionMismatch`. This is why every trait-body pin below declares a
/// conformer — and it bounds the carve-out to exactly that case, rather than to
/// "trait bodies" generally.
#[test]
fn trait_body_without_conformer_is_not_dimension_checked() {
    let compiled = compile_source_with_stdlib(
        r#"
trait HasBody {
    param material : Material
    constraint material.density > 1m
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "a conformer-less trait body is expected to go unchecked (that is the \
         documented vacuity condition). If this now errors, the carve-out in \
         this file's module doc and in docs/notes/dimensioned-zero-coercion.md \
         is stale and should be deleted. Diagnostics: {errors:#?}"
    );
}

/// TRAIT BODY + MEMBER ACCESS, WITH CONFORMER — the `structural_physical.ri`
/// `trait Physical` / `constraint material.density > 0kg/m^3` shape, compiled
/// at the level where it is actually checked.
///
/// Non-vacuous by construction: `trait_body_with_conformer_mismatch_errors`
/// below shows the identical shape with a mismatched non-zero RHS DOES error,
/// so a clean compile here means the bare zero was coerced.
#[test]
fn trait_body_with_conformer_member_access_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
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
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "conformed trait-body `material.density > 0`",
    );
}

/// NON-VACUITY GUARD for the conformed trait-body pins.
///
/// Same trait/conformer shape, mismatched NON-ZERO RHS (`Scalar[kg·m^-3]` vs
/// `Scalar[m]`) — must be rejected with `DiagnosticCode::DimensionMismatch`.
#[test]
fn trait_body_with_conformer_mismatch_errors() {
    let compiled = compile_source_with_stdlib(
        r#"
trait HasBody {
    param material : Material
    constraint material.density > 1m
}

structure Widget : HasBody {
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert_has_code(
        &errors,
        DiagnosticCode::DimensionMismatch,
        "conformed trait-body `material.density > 1m` (Density vs Length) — \
         without this, trait_body_with_conformer_member_access_gt_zero_no_error \
         would be vacuous",
    );
}

/// TRAIT BODY + INHERITED PARAM, WITH CONFORMER — the
/// `materials_electrical.ri` `trait Insulating` / `dielectric_strength > 0.0V/m`
/// shape: the constraint lives in a REFINING trait while the param it names is
/// declared by the parent trait. Uses the plain `0.0` Real literal, matching the
/// stdlib site.
#[test]
fn trait_body_inherited_param_gt_zero_no_error() {
    let compiled = compile_source_with_stdlib(
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
    assert_no_error_diagnostics(
        &compiled.diagnostics,
        "conformed refining-trait `dielectric_strength > 0.0`",
    );
}

/// NON-VACUITY GUARD for `trait_body_inherited_param_gt_zero_no_error`.
///
/// The inherited-param shape with a NON-ZERO dimensionless RHS must error —
/// which is also the `trait Insulating` neighbour `resistivity >
/// 1000000ohm*m`'s rationale for keeping its unit.
#[test]
fn trait_body_inherited_param_mismatch_errors() {
    let compiled = compile_source_with_stdlib(
        r#"
trait Charged {
    param dielectric_strength : DielectricStrength
}

trait NonDegenerate : Charged {
    constraint dielectric_strength > 1.0
}

structure Insulator : NonDegenerate {
    param dielectric_strength : DielectricStrength = 20000000.0V/m
}
"#,
    );
    let errors = collect_errors(&compiled.diagnostics);
    assert_has_code(
        &errors,
        DiagnosticCode::DimensionMismatch,
        "conformed refining-trait `dielectric_strength > 1.0` \
         (DielectricStrength vs dimensionless Real)",
    );
}

/// CONTRAST CASE — a param DEFAULT is not rewritten the way a binop operand is.
///
/// `coerce_zero_operand`'s sole call site is inside `compile_binop`, so the
/// polymorphic-zero rewrite never reaches a param default. The param-default
/// literal guard in `check_param_default_type` merely early-`return`s to
/// SUPPRESS `ParamDefaultTypeMismatch`; it performs no rewrite. Consequence,
/// measured here: `param x : Angle = 0` stores a DIMENSIONLESS default while
/// `= 0deg` stores `Scalar[rad]`.
///
/// This is what the `HarmonicForce.phase : Angle = 0deg` note in
/// modal_analysis.ri records — `0deg` is preferred because it dimensions the
/// stored default, not merely because it reads better.
#[test]
fn param_default_bare_zero_stays_dimensionless() {
    let compiled = compile_source_with_stdlib(
        r#"
structure S {
    param phase_deg : Angle = 0deg
    param phase_bare : Angle = 0
}
"#,
    );
    assert_no_error_diagnostics(&compiled.diagnostics, "Angle param defaults");

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("structure S must compile to a template");
    let default_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| panic!("no value cell named {member}"))
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("{member} has no default expr"))
            .result_type
            .clone()
    };

    let deg = default_type("phase_deg");
    assert!(
        matches!(&deg, reify_core::ty::Type::Scalar { dimension } if !dimension.is_dimensionless()),
        "`= 0deg` must store a dimensioned Angle default; got {deg:?}"
    );

    let bare = default_type("phase_bare");
    assert!(
        !matches!(&bare, reify_core::ty::Type::Scalar { dimension } if !dimension.is_dimensionless()),
        "`= 0` must stay DIMENSIONLESS — the param-default guard suppresses the \
         diagnostic but does not rewrite the value, unlike coerce_zero_operand. \
         Got {bare:?}; if this now fails, the `phase : Angle = 0deg` note in \
         modal_analysis.ri needs revisiting."
    );
}
