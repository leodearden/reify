//! Compile-time operand-kind guard tests for the six relational ops
//! (Eq/Ne/Lt/Le/Gt/Ge) — task 4490, step-3/step-4/step-5/step-6/step-7/step-8;
//! W3 (raw Field/StructureRef guard) and W5 (B2 gradualism) — task #4629.
//!
//! ## Coverage
//!
//! **Kind-error path (RED until step-4):**
//! - `param m : Matrix<3,3,Length>` + `constraint m > 0` → CmpOperandKind, fixit
//! - `param t : Tensor<2,3,MomentOfInertia>` + `constraint t > 0` → CmpOperandKind, fixit
//! - `param v : Vector3<Force>` + `constraint v < 0` → CmpOperandKind (no fixit required)
//! - `param pt : Point3<Length>` + `constraint pt == 0` → CmpOperandKind
//! - `param lst : List<Int>` + `constraint lst == 0` → CmpOperandKind (via list literal)
//! - ORDER op on Enum → CmpOperandKind
//! - ORDER op on String → CmpOperandKind
//!
//! **CRUX regression assertions (must stay GREEN — pin scoping decision):**
//! - Enum equality `where shape == Shape.Round { ... }` → no error
//! - `name == "steel"` (String ==) → no error
//! - `flag == true` (Bool ==) → no error
//!
//! **Live stdlib pattern regression (task 4229, structural_physical.ri:80):**
//! - `eigenvalues(m)[0] > 0.0 * 1m * 1m * 1kg` (scalar result from matrix reduction) → no error
//!
//! **Dimension-mismatch path (RED until step-6):**
//! - `param len : Length` + `param mass : Mass` + `constraint len < mass` → DimensionMismatch
//! - `1m == 1s` → DimensionMismatch
//! - `mass > 5` (dimensioned vs non-zero Int) → Error
//!
//! **Dimension-mismatch regressions (must stay GREEN):**
//! - `mass1 > mass2` (same dimension) → no error
//! - `mass > 0` (β zero-coercion gives 0 the Mass dimension) → no error
//! - `ratio >= 0.5` (dimensionless scalar) → no error
//! - `iterations >= 0` (Int vs Int) → no error
//!
//! **Chained comparison path (RED until step-8):**
//! - `0 < m < 5` where m is Matrix → CmpOperandKind
//! - `0m < x < 5s` (chained dimension mismatch) → Error
//!
//! **Chained comparison regressions (must stay GREEN):**
//! - `0 < poissons_ratio < 0.5` (dimensionless scalar) → no error
//! - `0kg < mass < 5kg` (same-dimension chained) → no error

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{
    assert_no_error_diagnostics, compile_source, compile_source_with_stdlib,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Compile `source` with stdlib and return Error-severity diagnostics.
fn errors_stdlib(source: &str) -> Vec<reify_core::Diagnostic> {
    let module = compile_source_with_stdlib(source);
    module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Compile `source` without stdlib and return Error-severity diagnostics.
fn errors(source: &str) -> Vec<reify_core::Diagnostic> {
    let module = compile_source(source);
    module
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Assert any error has the given code; return the matched diagnostics.
fn assert_has_code(
    errors: &[reify_core::Diagnostic],
    code: DiagnosticCode,
    context: &str,
) {
    let found = errors.iter().any(|d| d.code == Some(code));
    assert!(
        found,
        "{context}: expected DiagnosticCode::{code:?}; got errors: {errors:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// KIND-ERROR TESTS (RED until step-4)
// ══════════════════════════════════════════════════════════════════════════════

/// `param m : Matrix<3,3,Length>` with `constraint m > 0` must produce
/// `DiagnosticCode::CmpOperandKind` with a message mentioning "eigenvalues"
/// AND "trace" (the tensor/matrix-specific fixit).
///
/// RED (step-3): guard not written.
/// GREEN (step-4): emit_comparison_operand_diagnostics fires for Matrix operand.
#[test]
fn matrix_gt_scalar_emits_cmp_operand_kind_with_fixit() {
    let src = r#"
structure def S {
    param m : Matrix<3, 3, Length>
    constraint m > 0
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "Matrix > 0");
    // The fixit must name both canonical reductions.
    let has_eigenvalues = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind) && d.message.contains("eigenvalues"));
    assert!(
        has_eigenvalues,
        "CmpOperandKind for Matrix operand must mention 'eigenvalues'; got: {errs:?}"
    );
    let has_trace = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind) && d.message.contains("trace"));
    assert!(
        has_trace,
        "CmpOperandKind for Matrix operand must mention 'trace'; got: {errs:?}"
    );
}

/// `param t : Tensor<2,3,MomentOfInertia>` with `constraint t > 0` must
/// produce `DiagnosticCode::CmpOperandKind` with the eigenvalues/trace fixit.
///
/// RED (step-3): guard not written.
/// GREEN (step-4): emit_comparison_operand_diagnostics fires for Tensor operand.
#[test]
fn tensor_gt_scalar_emits_cmp_operand_kind_with_fixit() {
    let src = r#"
structure def S {
    param t : Tensor<2, 3, MomentOfInertia>
    constraint t > 0
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "Tensor > 0");
    let has_eigenvalues = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind) && d.message.contains("eigenvalues"));
    assert!(
        has_eigenvalues,
        "CmpOperandKind for Tensor operand must mention 'eigenvalues'; got: {errs:?}"
    );
    let has_trace = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind) && d.message.contains("trace"));
    assert!(
        has_trace,
        "CmpOperandKind for Tensor operand must mention 'trace'; got: {errs:?}"
    );
}

/// `param v : Vector3<Force>` with `constraint v < 0` must produce
/// `DiagnosticCode::CmpOperandKind`.  Eigenvalues/trace fixit is NOT required
/// for Vector (only for Tensor/Matrix).
///
/// RED (step-3): guard not written.
/// GREEN (step-4): guard rejects Vector3 for order ops.
#[test]
fn vector3_lt_scalar_emits_cmp_operand_kind() {
    let src = r#"
structure def S {
    param v : Vector3<Force>
    constraint v < 0
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "Vector3 < 0");
}

/// `param pt : Point3<Length>` with `constraint pt == 0` must produce
/// `DiagnosticCode::CmpOperandKind`.
///
/// RED (step-3): guard not written.
/// GREEN (step-4): guard rejects Point3 for equality ops.
#[test]
fn point3_eq_scalar_emits_cmp_operand_kind() {
    let src = r#"
structure def S {
    param pt : Point3<Length>
    constraint pt == 0
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "Point3 == 0");
}

/// A List literal on the left of `==` must produce `DiagnosticCode::CmpOperandKind`.
///
/// RED (step-3): guard not written.
/// GREEN (step-4): guard rejects List kind.
#[test]
fn list_eq_scalar_emits_cmp_operand_kind() {
    // [1, 2] == 0 — left operand is List<Int>
    let src = r#"
structure def S {
    let result : Bool = [1, 2] == 0
}
"#;
    let errs = errors(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "[1,2] == 0");
}

/// ORDER op on an Enum-typed param must produce `DiagnosticCode::CmpOperandKind`.
///
/// `eval_cmp` (Lt/Le/Gt/Ge) yields `Value::Undef` for Enum operands; the
/// guard rejects this at compile time for order ops while preserving equality.
///
/// RED (step-3): guard not written.
/// GREEN (step-4): `is_orderable_scalar` rejects Enum for order ops.
#[test]
fn enum_lt_enum_emits_cmp_operand_kind() {
    let src = r#"
enum Direction { X, Y, Z }

structure def S {
    param dir : Direction = Direction.X
    constraint dir < Direction.Y
}
"#;
    let errs = errors(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "Enum < Enum (order op)");
}

/// ORDER op on a String-typed param must produce `DiagnosticCode::CmpOperandKind`.
///
/// `eval_cmp` yields `Value::Undef` for String operands; the guard rejects this
/// for order ops while preserving String equality.
///
/// RED (step-3): guard not written.
/// GREEN (step-4): `is_orderable_scalar` rejects String for order ops.
#[test]
fn string_lt_string_emits_cmp_operand_kind() {
    let src = r#"
structure def S {
    param name : String = "foo"
    constraint name < "bar"
}
"#;
    let errs = errors(src);
    assert_has_code(&errs, DiagnosticCode::CmpOperandKind, "String < String (order op)");
}

// ══════════════════════════════════════════════════════════════════════════════
// CRUX REGRESSION TESTS — scoping decision: Enum/String/Bool EQUALITY preserved
// (must stay GREEN before AND after step-4)
// ══════════════════════════════════════════════════════════════════════════════

/// `where shape == Shape.Round { ... }` — the m5 guarded-enum idiom.
///
/// EQUALITY ops accept Enum-typed operands (`is_equatable_kind`).
/// This is the key in-scope committed example that must not break.
#[test]
fn enum_equality_in_guard_compiles_clean() {
    let src = r#"
enum Shape { Round, Square, Hex }

structure def Fitting {
    let shape = Shape.Round
    param size : Length = 10mm

    where shape == Shape.Round {
        param diameter : Length = size
    }
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "enum equality in where-guard should compile cleanly",
    );
}

/// `name == "steel"` — String equality must compile without errors.
///
/// EQUALITY ops accept String-typed operands (`is_equatable_kind`).
#[test]
fn string_equality_compiles_clean() {
    let src = r#"
structure def S {
    param name : String = "steel"
    let is_steel : Bool = name == "steel"
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`name == \"steel\"` should compile cleanly",
    );
}

/// `flag == true` — Bool equality must compile without errors.
///
/// EQUALITY ops accept Bool-typed operands (`is_equatable_kind`).
#[test]
fn bool_equality_compiles_clean() {
    let src = r#"
structure def S {
    param flag : Bool
    let check : Bool = flag == true
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`flag == true` should compile cleanly",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// LIVE STDLIB PATTERN REGRESSION (task 4229, structural_physical.ri:80)
// The EXACT form my CmpOperandKind fixit recommends must compile clean.
// ══════════════════════════════════════════════════════════════════════════════

/// `eigenvalues(m)[0] > 0.0 * 1kg * 1m * 1m` — the scalar-reduction comparison
/// that is the canonical fixit my CmpOperandKind diagnostic recommends.
///
/// This is the exact pattern used in structural_physical.ri:80 (task 4229 Rigid PD
/// constraint).  A guard that rejected its own suggested fix would be
/// self-contradictory.  Must compile clean.
#[test]
fn scalar_from_matrix_reduction_gt_dimensioned_zero_compiles_clean() {
    let src = r#"
structure def S {
    param m : Matrix<3, 3, MomentOfInertia>
    let eigs = eigenvalues(m)
    constraint eigs[0] > 0.0 * 1kg * 1m * 1m
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "eigenvalues(m)[0] > dimensioned_scalar should compile cleanly (4229 regression)",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// SCALAR REGRESSION (must stay GREEN)
// ══════════════════════════════════════════════════════════════════════════════

/// `param len : Length` + `constraint len > 0` — scalar ORDER op must compile clean.
///
/// The β coerce_zero_operand rewrites `0` to `Scalar<Length>` when the sibling is
/// a dimensioned Length scalar, so `len > 0` never hits the dimension-mismatch arm.
#[test]
fn dimensioned_scalar_gt_zero_compiles_clean() {
    let src = r#"
structure def S {
    param len : Length = 10mm
    constraint len > 0
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`len > 0` should compile cleanly (β zero-coercion)",
    );
}

/// `param iterations : Int` + `constraint iterations >= 0` — Int ORDER op must compile clean.
#[test]
fn int_ge_int_compiles_clean() {
    let src = r#"
structure def S {
    param iterations : Int = 10
    constraint iterations >= 0
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`iterations >= 0` should compile cleanly (Int ORDER op)",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// GRADUALISM / ANTI-CASCADE (must stay GREEN)
// ══════════════════════════════════════════════════════════════════════════════

/// `unknown_var > 0` — the left operand fails to resolve (`Type::Error`).
///
/// The guard must stay silent: zero `CmpOperandKind` diagnostics.
/// Only the underlying unresolved-variable error should surface.
#[test]
fn error_typed_left_no_spurious_cmp_operand_kind() {
    let errs = errors("structure def S { let x : Bool = unknown_var > 0 }");
    // There must be at least one error (the unresolved-variable one).
    assert!(
        !errs.is_empty(),
        "expected at least one error for `unknown_var > 0`, got none"
    );
    // No secondary CmpOperandKind may appear.
    let spurious = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind));
    assert!(
        !spurious,
        "`unknown_var > 0` must NOT produce a spurious CmpOperandKind — \
         left is Type::Error (anti-cascade). got: {errs:?}"
    );
}

/// A TypeParam-typed operand passes through without emitting `CmpOperandKind`.
///
/// Generic function parameter `x : T` has `Type::TypeParam("T")` at compile time;
/// the gradualism early-return silences the guard.
#[test]
fn type_param_in_comparison_no_cmp_operand_kind() {
    let src = r#"
fn compare<T>(x: T) -> Bool { x > 0 }
"#;
    let errs = errors(src);
    let spurious = errs
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::CmpOperandKind));
    assert!(
        !spurious,
        "TypeParam operand in `>` must NOT produce CmpOperandKind (gradualism). got: {errs:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// DIMENSION-MISMATCH TESTS (RED until step-6)
// ══════════════════════════════════════════════════════════════════════════════

/// `param len : Length` + `param mass : Mass` + `constraint len < mass` must
/// produce `DiagnosticCode::DimensionMismatch`.
///
/// Both operands are scalar-kind and pass the kind check, but their dimensions
/// differ → reuse format_dimension_mismatch_diagnostic (PRD §11 Q1).
///
/// RED (step-5): dimension check not yet implemented.
/// GREEN (step-6): emit_comparison_operand_diagnostics adds the dimension arm.
#[test]
fn scalar_different_dimensions_comparison_emits_dimension_mismatch() {
    let src = r#"
structure def S {
    param len : Length = 1m
    param mass : Mass = 1kg
    constraint len < mass
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(
        &errs,
        DiagnosticCode::DimensionMismatch,
        "Length < Mass (different dimensions)",
    );
}

/// `1m == 1s` — literal dimension mismatch → `DiagnosticCode::DimensionMismatch`.
///
/// RED (step-5): dimension check not yet implemented.
/// GREEN (step-6).
#[test]
fn literal_dimension_mismatch_equality_emits_dimension_mismatch() {
    let errs = errors_stdlib("structure def S { let ok : Bool = 1m == 1s }");
    assert_has_code(
        &errs,
        DiagnosticCode::DimensionMismatch,
        "1m == 1s (Length == Time literal)",
    );
}

/// `mass > 5` — dimensioned Scalar vs non-zero dimensionless Int → Error.
///
/// The β zero-coercion only fires for syntactic literal `0`; `5` stays Int.
/// Mirrors the Add/Sub Scalar-vs-Int non-dimensionless arm in expr.rs.
///
/// RED (step-5): dimension check not yet implemented.
/// GREEN (step-6).
#[test]
fn dimensioned_scalar_gt_nonzero_int_emits_error() {
    let src = r#"
structure def S {
    param mass : Mass = 1kg
    constraint mass > 5
}
"#;
    let errs = errors_stdlib(src);
    // Must emit at least one Error (no specific code required beyond that).
    assert!(
        !errs.is_empty(),
        "`mass > 5` must produce an error (dimensioned scalar vs non-zero Int); got none"
    );
}

// ── Dimension-mismatch regressions (must stay GREEN) ─────────────────────────

/// `mass1 > mass2` (same Mass dimension) — must compile clean.
#[test]
fn same_dimension_comparison_compiles_clean() {
    let src = r#"
structure def S {
    param mass1 : Mass = 1kg
    param mass2 : Mass = 2kg
    constraint mass1 > mass2
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`mass1 > mass2` (same dimension) should compile cleanly",
    );
}

/// `mass > 0` — β zero-coercion gives `0` the Mass dimension → must compile clean.
#[test]
fn dimensioned_scalar_gt_zero_literal_compiles_clean() {
    let src = r#"
structure def S {
    param mass : Mass = 1kg
    constraint mass > 0
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`mass > 0` should compile cleanly (β zero-coercion)",
    );
}

/// `ratio >= 0.5` (dimensionless Scalar) — must compile clean.
#[test]
fn dimensionless_scalar_ge_real_compiles_clean() {
    let src = r#"
structure def S {
    param ratio : Real = 0.3
    constraint ratio >= 0.5
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`ratio >= 0.5` (dimensionless scalar) should compile cleanly",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// CHAINED-COMPARISON TESTS (RED until step-8)
// ══════════════════════════════════════════════════════════════════════════════

/// `0 < m` (single) — exercises the single-comparison path from step-4.
///
/// This is a regression that must stay GREEN throughout steps 5-8.
/// The comment from step-3 noted that the truly-chained form `0 < m < 5` is
/// tested separately below (step-7/step-8).
#[test]
fn single_matrix_gt_emits_cmp_operand_kind() {
    let src = r#"
structure def S {
    param m : Matrix<3, 3, Length>
    constraint 0 < m
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(
        &errs,
        DiagnosticCode::CmpOperandKind,
        "single-comparison Matrix (0 < m, single-comparison path)",
    );
}

/// `0 < m < 5` where `m : Matrix<3,3,Length>` — TRULY CHAINED comparison whose
/// middle operand is a Matrix must produce `DiagnosticCode::CmpOperandKind`.
///
/// `0 < m < 5` parses as `BinOp("<", BinOp("<", 0, m), 5)`.  The outer `<` detects
/// the inner `<` as a comparison → the chained-desugar path fires (expr.rs:1119-1168),
/// flattens to operands `[0, m, 5]` and ops `["<", "<"]`, and builds pairs WITHOUT
/// calling `emit_comparison_operand_diagnostics`.  So today no diagnostic is emitted
/// for the bad Matrix middle-operand.
///
/// RED (step-7): chained path has no guard.
/// GREEN (step-8): `emit_comparison_operand_diagnostics` called per pair inside the
/// chained fold loop.
#[test]
fn chained_matrix_middle_truly_chained_emits_cmp_operand_kind() {
    let src = r#"
structure def S {
    param m : Matrix<3, 3, Length>
    constraint 0 < m < 5
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(
        &errs,
        DiagnosticCode::CmpOperandKind,
        "truly-chained `0 < m < 5` — chained path must emit CmpOperandKind for Matrix operand",
    );
}

/// Chained comparison where the second pair has a dimension mismatch.
///
/// `lo_len < val < hi_mass`:
/// - Pair 1: `lo_len < val` → Scalar<Length> vs Scalar<Length> → OK.
/// - Pair 2: `val < hi_mass` → Scalar<Length> vs Scalar<Mass> → dimension mismatch.
///
/// Today the chained path emits no diagnostic for pair 2; after step-8 it emits
/// `DiagnosticCode::DimensionMismatch`.
///
/// RED (step-7): chained path has no guard.
/// GREEN (step-8): dimension arm fires for pair 2.
#[test]
fn chained_dimension_mismatch_emits_error() {
    let src = r#"
structure def S {
    param lo_len : Length = 1m
    param val : Length = 2m
    param hi_mass : Mass = 5kg
    constraint lo_len < val < hi_mass
}
"#;
    let errs = errors_stdlib(src);
    assert!(
        !errs.is_empty(),
        "chained `lo_len < val < hi_mass` must produce an error (pair 2 has Length vs Mass); got none"
    );
}

/// `0 < poissons_ratio < 0.5` — chained dimensionless-scalar comparison must
/// compile clean (the materials_mechanical.ri:91 idiom).
///
/// Must stay GREEN before AND after step-8.
#[test]
fn chained_dimensionless_scalar_compiles_clean() {
    let src = r#"
structure def S {
    param poissons_ratio : Real = 0.3
    constraint 0 < poissons_ratio < 0.5
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`0 < poissons_ratio < 0.5` should compile cleanly (dimensionless scalar chained)",
    );
}

/// `0kg < mass < 5kg` — chained same-dimension scalar comparison must compile clean.
///
/// Must stay GREEN before AND after step-8.
#[test]
fn chained_same_dimension_compiles_clean() {
    let src = r#"
structure def S {
    param mass : Mass = 1kg
    constraint 0kg < mass < 5kg
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`0kg < mass < 5kg` should compile cleanly (same-dimension chained)",
    );
}

// ── ScalarParam (dimension-parametric `Scalar<Q>`) regressions ─────────────────
//
// Surfaced post-rebase against current main: the field-stdlib ε work landed
// `std.fields::threshold<D, Q: Dimension>(...) { fn_field(|p| sample(f, p) > value) }`,
// which compares `Scalar<Q>` against `Scalar<Q>`. `Scalar<Q>` resolves to
// `Type::ScalarParam("Q")` (see fn_generic_signature_tests.rs), a genuine,
// well-formed scalar. The operand-kind guard originally matched only
// `Type::Scalar { .. }`, so it wrongly rejected `ScalarParam` with CmpOperandKind,
// breaking compilation of the real stdlib. These pin the fix in
// `is_orderable_scalar` / `is_equatable_kind` (accept `Type::ScalarParam(_)`).

/// `Scalar<Q> > Scalar<Q>` inside a dimension-kinded generic fn (the
/// `std.fields::threshold` shape) must NOT emit `CmpOperandKind`.
#[test]
fn scalar_param_order_comparison_in_generic_fn_is_accepted() {
    let errs = errors("fn over<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Bool { a > b }");
    let bad: Vec<_> = errs
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        bad.is_empty(),
        "order comparison `Scalar<Q> > Scalar<Q>` must NOT emit CmpOperandKind; got: {bad:?}"
    );
}

/// `Scalar<Q> == Scalar<Q>` inside a dimension-kinded generic fn must NOT emit
/// `CmpOperandKind` (equality-op variant of the order-op regression above).
#[test]
fn scalar_param_equality_comparison_in_generic_fn_is_accepted() {
    let errs = errors("fn eqp<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Bool { a == b }");
    let bad: Vec<_> = errs
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        bad.is_empty(),
        "equality comparison `Scalar<Q> == Scalar<Q>` must NOT emit CmpOperandKind; got: {bad:?}"
    );
}

/// The real stdlib (`std.fields::threshold`, which compares `Scalar<Q>`) must
/// compile with NO `CmpOperandKind` error — the live reproduction of the failure
/// caught by `stdlib_topo` / `relation_signatures` before the fix.
#[test]
fn stdlib_scalar_param_comparison_emits_no_cmp_operand_kind() {
    let errs = errors_stdlib("structure def Probe { param x : Length = 1mm }");
    let bad: Vec<_> = errs
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        bad.is_empty(),
        "compiling the stdlib (incl. std.fields::threshold over Scalar<Q>) must NOT \
         emit CmpOperandKind; got: {bad:?}"
    );
}

// ── Constant-folded-zero comparison conformance (task 4490) ────────────────────
//
// `coerce_zero_operand` recognizes operands that EVALUATE to exactly zero (via
// `const_numeric_value`), not just a syntactic `0`. The dimensionless-vs-dimensioned
// distinction is load-bearing:
//   • a folded *dimensionless* zero adopts the dimensioned sibling → compiles clean;
//   • a folded *dimensioned* zero keeps its dimension, so a genuine dimension
//     mismatch still errors (a zero value does NOT excuse a dimension mismatch);
//   • a folded *nonzero* constant is unaffected — still errors against a dimensioned
//     sibling per the ratified `mass > 5 → Error` contract.

/// POSITIVE: `mass > 1 - 1` — folded dimensionless zero (`Int`) adopts `Mass` → clean.
#[test]
fn folded_dimensionless_zero_int_compiles_clean() {
    let errs = errors_stdlib(
        r#"structure def S {
    param m : Mass = 1kg
    constraint m > 1 - 1
}"#,
    );
    assert!(
        errs.is_empty(),
        "`mass > 1 - 1` (folded dimensionless 0) must compile clean; got: {errs:?}"
    );
}

/// POSITIVE: a multi-unit constant expression reducing to a *dimensionless* zero —
/// `2m^2 * (5m - 5m) / 0.5m^3` is `m^2·m/m^3` = dimensionless, value 0 — adopts the
/// dimensioned sibling → clean.
#[test]
fn folded_dimensionless_zero_multiunit_expr_compiles_clean() {
    let errs = errors_stdlib(
        r#"structure def S {
    param a : Area = 1m^2
    constraint a > 2m^2 * (5m - 5m) / 0.5m^3
}"#,
    );
    assert!(
        errs.is_empty(),
        "`area > 2m^2*(5m-5m)/0.5m^3` (folds to dimensionless 0) must compile clean; got: {errs:?}"
    );
}

/// POSITIVE: `mass > 2kg - 2kg` — folded zero whose dimension MATCHES (kg) → clean.
#[test]
fn folded_zero_matching_dimension_compiles_clean() {
    let errs = errors_stdlib(
        r#"structure def S {
    param m : Mass = 1kg
    constraint m > 2kg - 2kg
}"#,
    );
    assert!(
        errs.is_empty(),
        "`mass > 2kg - 2kg` (folded 0, same dimension) must compile clean; got: {errs:?}"
    );
}

/// NEGATIVE: `mass > 1m - 1m` — folds to zero but is *dimensioned* (`m`); the
/// Mass-vs-Length mismatch must still error.
#[test]
fn folded_dimensioned_zero_mismatch_still_errors() {
    let errs = errors_stdlib(
        r#"structure def S {
    param m : Mass = 1kg
    constraint m > 1m - 1m
}"#,
    );
    assert!(
        errs.iter().any(|d| d.message.contains("comparison")),
        "`mass > 1m - 1m` (0m vs kg) must error on the dimension mismatch; got: {errs:?}"
    );
}

/// NEGATIVE: a multi-unit constant expression reducing to a *dimensioned* zero —
/// `2m^2 * (5m - 5m)` is `m^3` (Volume), value 0 — compared against `Area` must error.
#[test]
fn folded_dimensioned_zero_multiunit_expr_mismatch_errors() {
    let errs = errors_stdlib(
        r#"structure def S {
    param a : Area = 1m^2
    constraint a > 2m^2 * (5m - 5m)
}"#,
    );
    assert!(
        errs.iter().any(|d| d.message.contains("comparison")),
        "`area > 2m^2*(5m-5m)` (0 m^3 vs m^2) must error on the dimension mismatch; got: {errs:?}"
    );
}

/// NEGATIVE: `mass > 3 - 1` — folds to a *nonzero* constant (`Int` 2), which is NOT
/// coerced; the ratified dimensioned-scalar-vs-nonzero-`Int` contract still errors.
#[test]
fn folded_nonzero_int_vs_dimensioned_still_errors() {
    let errs = errors_stdlib(
        r#"structure def S {
    param m : Mass = 1kg
    constraint m > 3 - 1
}"#,
    );
    assert!(
        errs.iter().any(|d| d.message.contains("comparison")),
        "`mass > 3 - 1` (folds to nonzero 2) must still error; got: {errs:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// W3 — RAW FIELD / STRUCTUREREF COMPARISON GUARD (RED until step-6, task #4629)
// ══════════════════════════════════════════════════════════════════════════════
//
// 4490 deferred Field<D,C> and StructureRef comparison operands via an
// early-return at expr.rs:371-375.  W3 (step-6) removes that deferral so
// is_orderable_scalar/is_equatable_kind adjudicate — a whole field or structure
// is neither orderable nor equatable → CmpOperandKind.
//
// After W1 (max(field)→codomain scalar) and W2 (envelope_von_mises→Field type),
// the two real examples that caused the original deferral now type their
// reductions as scalars, so removing the deferral does not break them.

/// A raw `Field<Real, Real>` (from fn_field) on the left of an order comparison
/// must produce `DiagnosticCode::CmpOperandKind`.
///
/// `fn_field(|p| 2.0 * p)` creates a `Field<Real, Real>`; comparing the field
/// itself (not its `max` reduction) against a scalar is a meaningless total-order
/// comparison that the guard must reject.  Note that `max(f) < 1.0` is correct
/// (W1 reduces `max(field)` to a scalar at compile time) — this test exercises
/// the case where the field is compared *directly*.
///
/// RED (step-5 W3): the Field/StructureRef deferral at expr.rs:371-375 skips
///   adjudication and the guard is silent — no CmpOperandKind emitted.
/// GREEN (step-6 W3): deferral removed; is_orderable_scalar rejects Field →
///   CmpOperandKind is emitted.
#[test]
fn raw_field_lt_scalar_emits_cmp_operand_kind() {
    // fn_field is a core intercepting builtin; no stdlib needed.
    let src = r#"
structure def FieldCmpTest {
    let f   = fn_field(|p| 2.0 * p)
    let bad = f < 1.0
}
"#;
    let errs = errors(src);
    assert_has_code(
        &errs,
        DiagnosticCode::CmpOperandKind,
        "raw Field<Real,Real> < 1.0 must emit CmpOperandKind (W3 RED until step-6)",
    );
}

/// A `StructureRef` (e.g. the result of `stress_invariants`) on the left of an
/// order comparison must produce `DiagnosticCode::CmpOperandKind`.
///
/// `stress_invariants(tensor)` types as `StructureRef("StressInvariants")`
/// (pinned by analysis_stress_fn_compile.rs); a whole structure has no total
/// order, so `inv < 1.0` must be rejected.
///
/// RED (step-5 W3): the Field/StructureRef deferral at expr.rs:371-375 skips
///   adjudication and the guard is silent — no CmpOperandKind emitted.
/// GREEN (step-6 W3): deferral removed; is_orderable_scalar rejects StructureRef →
///   CmpOperandKind is emitted.
#[test]
fn structure_ref_lt_scalar_emits_cmp_operand_kind() {
    let src = r#"
structure def StructRefCmpTest {
    let stress = matrix([[1.0e6Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa,   0.0Pa, 0.0Pa],
                         [0.0Pa,   0.0Pa, 0.0Pa]])
    let inv = stress_invariants(stress)
    let bad = inv < 1.0
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(
        &errs,
        DiagnosticCode::CmpOperandKind,
        "StructureRef(StressInvariants) < 1.0 must emit CmpOperandKind (W3 RED until step-6)",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// W5 — B2 GRADUALISM STRICT ERRORING (RED until step-8, task #4629)
// ══════════════════════════════════════════════════════════════════════════════
//
// 4490 suppressed Real-vs-Scalar[D] dimension mismatches with a blanket
// `!ld.is_dimensionless() && !rd.is_dimensionless()` guard, deferring the
// `efficiency > 5mm` class of bugs (dimensionless ratio vs dimensioned threshold).
// The rationale was that `purpose P(subject : Structure)` member accesses like
// `subject.width` returned `Real` (dimensionless fallback), so
// `subject.width > 0mm` would have produced a spurious mismatch in a generic body.
//
// W5 (step-8 GREEN) removes the suppression and simultaneously fixes the root
// cause: the wildcard "Structure" member access is now typed as TypeParam (not
// dimensionless Real), which triggers the existing TypeParam early-return in the
// guard (lines 353-357), keeping generic bodies clean.  Concrete Real-vs-Scalar[D]
// bugs then error correctly.

/// A genuinely dimensionless ratio compared against a dimensioned threshold must
/// emit `DiagnosticCode::DimensionMismatch`.
///
/// `let efficiency : Real = 0.85` is a dimensionless value; `efficiency > 5mm`
/// compares `Real` vs `Scalar<Length>` — a dimension-kind bug the user likely did
/// not intend (they probably meant `efficiency > 0.85`, not `> 5mm`).
///
/// RED (step-7 W5): the B2 dimensionless suppression at expr.rs:421
///   (`!ld.is_dimensionless() && !rd.is_dimensionless()`) silently swallows this
///   — one operand (ld = Real) IS dimensionless → suppression fires → no error.
/// GREEN (step-8 W5): suppression removed; `ld != rd` (DIMENSIONLESS ≠ LENGTH)
///   triggers DimensionMismatch.
#[test]
fn dimensionless_ratio_gt_dimensioned_threshold_emits_dimension_mismatch() {
    let src = r#"
structure def S {
    let efficiency : Real = 0.85
    constraint efficiency > 5mm
}
"#;
    let errs = errors_stdlib(src);
    assert_has_code(
        &errs,
        DiagnosticCode::DimensionMismatch,
        "Real > Scalar<Length> must emit DimensionMismatch (W5 RED until step-8)",
    );
}

/// `subject.width > 0mm` in a purpose body with a wildcard `Structure` subject
/// must compile WITHOUT emitting any error, both before and after W5.
///
/// **Before W5 (step-7):** `subject.width` types as `Real` (dimensionless fallback)
///   and the B2 suppression (`!ld.is_dimensionless()` is false) silences the
///   mismatch — compiles clean.
/// **After W5 (step-8):** `subject.width` types as `TypeParam("StructureMember")`
///   (new wildcard typing) → the TypeParam gradualism early-return (expr.rs:353-357)
///   skips adjudication — compiles clean for a different (correct) reason.
///
/// Must stay GREEN throughout. This is the B2 canary for the generic-body case.
#[test]
fn generic_structure_subject_member_gt_dimensioned_compiles_clean() {
    let src = r#"
purpose ok_purpose(subject : Structure) {
    constraint subject.width > 0mm
}
"#;
    let module = compile_source_with_stdlib(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "`subject.width > 0mm` in generic-Structure purpose body must compile cleanly \
         (W5 regression — B2 gradualism)",
    );
}

/// `examples/kinematic/counter_mass_balance.ri` must compile with zero Error
/// diagnostics after W5 removes the B2 dimensionless suppression.
///
/// The `d < 1um` constraint in `counter_mass_balance.ri` involves:
///   `d = magnitude(element_of_com_magnitudes)` — after task 4612,
///   `magnitude(Point3<Length>)` types as `Scalar<Length>`, so `d` is a
///   dimensioned Length scalar.  `d < 1um` is therefore `Length < Length`
///   (same dimension → no mismatch regardless of the B2 suppression).
///
/// An old comment in the source (corrected by W6 step-11) claimed the operand
/// was dimensionless Real and credited the B2 suppression; this regression pin
/// confirms the comparison is correct under the restored strict erroring.
#[test]
fn counter_mass_balance_example_compiles_clean_under_strict_erroring() {
    const PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/kinematic/counter_mass_balance.ri"
    );
    let src = std::fs::read_to_string(PATH)
        .expect("failed to read examples/kinematic/counter_mass_balance.ri");
    let module = compile_source_with_stdlib(&src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "counter_mass_balance.ri (`d < 1um` is Length<Length post-4612) must compile \
         clean after W5 removes B2 dimensionless suppression",
    );
}
