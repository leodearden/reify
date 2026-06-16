//! Type-hygiene λ compiler-layer integration gate — §10 boundary-test table
//! (task #4495).
//!
//! # Purpose
//!
//! This file is the compiler-layer integration gate for the type-hygiene cluster
//! (PRD `docs/prds/v0_6/type-hygiene.md`).  It characterises the compile-time
//! §10 boundary behaviours delivered by α (4490), β (4485), ζ (4493), and η (4487).
//!
//! All rows here are GREEN-on-write: they assert behaviour that was shipped by
//! the dependency tasks; this gate adds no production code.
//!
//! **Row ownership:** each §10 row is tested in exactly ONE canonical file; this
//! gate owns the compile-time rows.  Runtime/eval rows (2, 4b, 6, 7b, 8, 9, 13,
//! 14) live in `crates/reify-eval/tests/type_hygiene_integration_gate.rs`; the
//! CLI row (12) lives in `crates/reify-cli/tests/cli_type_hygiene_strict.rs`.
//!
//! # §10 boundary-test table + row→owner cross-reference
//!
//! | Row | Description                                                  | Owner (compiler gate fn)                                              |
//! |-----|--------------------------------------------------------------|-----------------------------------------------------------------------|
//! | 1   | tensor>scalar → CmpOperandKind+fixit                         | `row_1_tensor_gt_scalar_emits_cmp_operand_kind_with_fixit`            |
//! | 2   | mass>0 OK / VIOLATED                                         | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 3   | bare 0 vs tensor → no zero-adoption → CmpOperandKind         | `row_3_bare_zero_vs_tensor_no_adoption_gives_kind_error`              |
//! | 4a  | `x and 5` err                                                | `row_4a_non_bool_and_operand_emits_logical_operand_not_bool`          |
//! | 4b  | Kleene runtime preserved                                     | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 5   | TypeParam gradualism no-diag                                 | `row_5_type_param_in_comparison_no_cmp_operand_kind`                  |
//! | 6   | moi(b,material.density) via let → non-Undef tensor           | `reify-eval/tests/type_hygiene_integration_gate.rs` (OCCT-gated)     |
//! | 7a  | moi(b,7850.0) → ArgTypeMismatch                              | `row_7a_bare_real_density_to_moi_gives_arg_type_mismatch`             |
//! | 7b  | moi runtime density rejection never-silent                   | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 8   | body_mass_props(b,Pressure) loud reject                      | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 9   | body_mass_props(b) no-Material warn + water UNCHANGED        | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 10  | scalar override of tensor-defaulted trait param → mismatch   | `row_10_scalar_override_of_tensor_defaulted_param_errors`             |
//! | 11  | compatible same-type override conforms                       | `row_11_tensor_typed_override_of_tensor_defaulted_param_conforms`     |
//! | 12  | `reify check --strict` exit both ways                        | `crates/reify-cli/tests/cli_type_hygiene_strict.rs`                  |
//! | 13  | distinct indeterminacy messages                              | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |
//! | 14  | RNEA identical post-κ                                        | `reify-eval/tests/type_hygiene_integration_gate.rs`                   |

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{
    assert_no_error_diagnostics, compile_source, compile_source_with_stdlib, errors_only,
};

// ── §10 row 1: tensor > scalar → CmpOperandKind + fixit ──────────────────────

/// §10 row 1: `param t : Tensor<2,3,MomentOfInertia>` + `constraint t > 0`
/// must emit ≥1 `DiagnosticCode::CmpOperandKind` Error whose message contains
/// BOTH `"eigenvalues"` AND `"trace"` (the tensor/matrix-specific fixit).
///
/// α (task #4490) `emit_comparison_operand_diagnostics` fires for Tensor operands
/// and populates the fixit with the two canonical scalar reductions.
///
/// References `comparison_operand_guard_tests.rs::tensor_gt_scalar_emits_cmp_operand_kind_with_fixit`.
/// Characterisation of landed α behavior; GREEN on write.
#[test]
fn row_1_tensor_gt_scalar_emits_cmp_operand_kind_with_fixit() {
    let src = r#"
structure def Row1 {
    param t : Tensor<2, 3, MomentOfInertia>
    constraint t > 0
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        !errors.is_empty(),
        "§10 row 1: `constraint t > 0` with Tensor<2,3,MomentOfInertia> t must emit \
         ≥1 CmpOperandKind Error (α operand-kind guard). All diagnostics: {:#?}",
        module.diagnostics
    );
    let has_eigenvalues = errors.iter().any(|d| d.message.contains("eigenvalues"));
    assert!(
        has_eigenvalues,
        "§10 row 1: CmpOperandKind message must contain 'eigenvalues' (tensor fixit); \
         errors: {errors:#?}"
    );
    let has_trace = errors.iter().any(|d| d.message.contains("trace"));
    assert!(
        has_trace,
        "§10 row 1: CmpOperandKind message must contain 'trace' (tensor fixit); \
         errors: {errors:#?}"
    );
}

// ── §10 row 3: bare 0 vs tensor → no zero-adoption → CmpOperandKind ──────────

/// §10 row 3: β (task #4485) performs NO zero-adoption against tensors — when
/// the comparison operand is a tensor, `coerce_zero_operand` is NOT invoked, and
/// α's kind-guard fires `CmpOperandKind` for the tensor operand.
///
/// The test writes `constraint 0 > t` (bare zero on the LEFT, tensor on the RIGHT)
/// to explicitly probe the "zero-vs-tensor" direction; the assertion is the same
/// CmpOperandKind error (the tensor on the right triggers the guard regardless of
/// which side the zero appears on).
///
/// References `comparison_operand_guard_tests.rs::tensor_gt_scalar_emits_cmp_operand_kind_with_fixit`
/// (row 1 covers the RHS-tensor case; row 3 covers the LHS-zero case).
/// Characterisation of landed β/α interaction; GREEN on write.
#[test]
fn row_3_bare_zero_vs_tensor_no_adoption_gives_kind_error() {
    let src = r#"
structure def Row3 {
    param t : Tensor<2, 3, MomentOfInertia>
    constraint 0 > t
}
"#;
    let module = compile_source_with_stdlib(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        !errors.is_empty(),
        "§10 row 3: `constraint 0 > t` with Tensor t must emit ≥1 CmpOperandKind Error \
         (β does NOT adopt bare zero as tensor; α kind-guard fires). \
         All diagnostics: {:#?}",
        module.diagnostics
    );
}

// ── §10 row 4a: `x and 5` → LogicalOperandNotBool ────────────────────────────

/// §10 row 4a: `param x : Bool` + `constraint x and 5` must emit ≥1
/// `DiagnosticCode::LogicalOperandNotBool` Error whose message contains `"and"`.
///
/// α (task #4490) guards the And/Or operators against non-Bool operands.
/// The right operand `5` (Int) is not Bool → the guard fires.
///
/// References `and_or_operand_guard_tests.rs::and_non_bool_right_emits_logical_operand_not_bool`.
/// Characterisation of landed α behavior; GREEN on write.
#[test]
fn row_4a_non_bool_and_operand_emits_logical_operand_not_bool() {
    let src = r#"
structure def Row4a {
    param x : Bool
    constraint x and 5
}
"#;
    let module = compile_source(src);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::LogicalOperandNotBool)
        })
        .collect();
    assert!(
        !errors.is_empty(),
        "§10 row 4a: `constraint x and 5` with Bool x must emit ≥1 LogicalOperandNotBool \
         Error (α And/Or guard, non-Bool right operand). All diagnostics: {:#?}",
        module.diagnostics
    );
    let has_and_msg = errors.iter().any(|d| d.message.contains("and"));
    assert!(
        has_and_msg,
        "§10 row 4a: LogicalOperandNotBool message must contain 'and'; errors: {errors:#?}"
    );
}

// ── §10 row 5: TypeParam gradualism — no CmpOperandKind for generic fn ─────────

/// §10 row 5: a TypeParam-typed operand in a comparison must NOT emit
/// `DiagnosticCode::CmpOperandKind` (decision-6 gradualism).
///
/// Generic function parameter `x : T` has `Type::TypeParam("T")` at compile time;
/// the α guard's early-return silences CmpOperandKind to avoid false positives for
/// polymorphic functions whose type argument will be resolved at call sites.
///
/// References `comparison_operand_guard_tests.rs::type_param_in_comparison_no_cmp_operand_kind`.
/// Characterisation of landed α gradualism; GREEN on write.
#[test]
fn row_5_type_param_in_comparison_no_cmp_operand_kind() {
    // A generic function with a TypeParam operand in a comparison.
    let src = r#"
fn compare<T>(x: T) -> Bool { x > 0 }
"#;
    let module = compile_source(src);
    let spurious_kind_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::CmpOperandKind))
        .collect();
    assert!(
        spurious_kind_errors.is_empty(),
        "§10 row 5: TypeParam operand `x : T` in `x > 0` must NOT produce \
         CmpOperandKind (gradualism early-return). Got: {spurious_kind_errors:#?}"
    );
}

// ── §10 row 7a: moi(b, 7850.0) → ArgTypeMismatch ────────────────────────────

/// §10 row 7a: `let i = moment_of_inertia(b, 7850.0)` (bare Real density) must
/// emit ≥1 `DiagnosticCode::ArgTypeMismatch` Error naming both `"moment_of_inertia"`
/// and `"Density"` (ζ task #4493 compile-time density-spec guard).
///
/// References `builtin_arg_signature_tests.rs::moment_of_inertia_bare_real_density_gives_arg_type_mismatch`.
/// Characterisation of landed ζ behavior; GREEN on write.
#[test]
fn row_7a_bare_real_density_to_moi_gives_arg_type_mismatch() {
    let src = r#"
structure def Row7a {
    let b = box(50mm, 30mm, 10mm)
    let i = moment_of_inertia(b, 7850.0)
}
"#;
    let module = compile_source_with_stdlib(src);
    let arg_type_mismatches: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch) && d.severity == Severity::Error
        })
        .collect();
    assert!(
        !arg_type_mismatches.is_empty(),
        "§10 row 7a: `moment_of_inertia(b, 7850.0)` must emit ≥1 ArgTypeMismatch Error \
         (ζ compile-time density-spec check). All diagnostics: {:#?}",
        module.diagnostics
    );
    let d = &arg_type_mismatches[0];
    assert!(
        d.message.contains("moment_of_inertia"),
        "§10 row 7a: ArgTypeMismatch message must name 'moment_of_inertia'; got: {}",
        d.message
    );
    assert!(
        d.message.contains("Density"),
        "§10 row 7a: ArgTypeMismatch message must name 'Density'; got: {}",
        d.message
    );
}

// ── §10 row 10: scalar override of tensor-defaulted trait param → mismatch ────

/// §10 row 10: a conformer declaring `param moi : MomentOfInertia` against a
/// trait that defaults `param moi : Tensor<2,3,MomentOfInertia>` must emit
/// ≥1 Error containing `"type mismatch for trait member 'moi'"`.
///
/// η (task #4487) conformance collision rule (checker.rs phase 5) rejects the
/// type reduction (tensor → scalar) as incompatible.
///
/// References `trait_default_collision_tests.rs::scalar_override_of_tensor_defaulted_trait_param_errors`.
/// Characterisation of landed η behavior; GREEN on write.
#[test]
fn row_10_scalar_override_of_tensor_defaulted_param_errors() {
    let src = r#"
trait Row10Rigid {
    param moi : Tensor<2,3,MomentOfInertia> = matrix([
        [1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m]
    ])
}
structure def Row10ScalarOverride : Row10Rigid {
    param moi : MomentOfInertia
}
"#;
    let module = compile_source(src);
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member 'moi'")),
        "§10 row 10: scalar override of Tensor<2,3,MomentOfInertia> defaulted trait param \
         must emit ≥1 Error containing \"type mismatch for trait member 'moi'\" (η collision \
         rule). Got: {errors:#?}"
    );
}

// ── §10 row 11: compatible same-type override conforms ──────────────────────

/// §10 row 11: a conformer redeclaring `param moi : Tensor<2,3,MomentOfInertia>`
/// (the SAME type as the trait's default) must NOT emit any
/// `"type mismatch for trait member"` diagnostic.
///
/// η (task #4487) `implicitly_converts_to(Tensor<2,3,MomentOfInertia>,
/// Tensor<2,3,MomentOfInertia>)` is true → no collision reported.
///
/// References `trait_default_collision_tests.rs::tensor_typed_override_of_tensor_defaulted_param_conforms`.
/// This is the §10 row 11 "override idiom" probe — conformers may pin a measured
/// value (or re-declare the trait's declared type) without a mismatch error.
/// Characterisation of landed η behavior; GREEN on write.
#[test]
fn row_11_tensor_typed_override_of_tensor_defaulted_param_conforms() {
    let src = r#"
trait Row11Rigid {
    param moi : Tensor<2,3,MomentOfInertia> = matrix([
        [1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m],
        [0.0 * 1kg * 1m * 1m, 0.0 * 1kg * 1m * 1m, 1.0 * 1kg * 1m * 1m]
    ])
}
structure def Row11TensorOverride : Row11Rigid {
    param moi : Tensor<2,3,MomentOfInertia>
}
"#;
    let module = compile_source(src);
    assert_no_error_diagnostics(
        &module.diagnostics,
        "§10 row 11: same-type Tensor<2,3,MomentOfInertia> override must compile with \
         no Error diagnostics (η compatible override — no type mismatch for trait member)",
    );
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "§10 row 11: compatible same-type override must NOT produce \
         \"type mismatch for trait member\"; all diagnostics: {:#?}",
        module.diagnostics
    );
}
