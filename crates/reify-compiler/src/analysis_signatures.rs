//! Compiler signatures for the FEA stress-analysis **reduction** builtins
//! (FEA-5, task 2884) — the frozen §analysis contract.
//!
//! Holds the single source of truth for the analysis-reduction builtin name
//! family ([`ANALYSIS_FN_NAMES`]), the name-only classification predicate
//! ([`is_analysis_typed_fn`]), and the shape-dependent result-type resolver
//! ([`analysis_fn_result_type`]).
//!
//! Unlike the math-linalg family, analysis reductions take a stress tensor
//! argument and return a *reduced* type: a Scalar (von_mises/max_shear), a
//! List of scalars (principal_stresses), a dimensionless Real (safety_factor),
//! or a StructureRef (stress_invariants). All five are pure eval-builtins
//! already dispatched by name in `reify_stdlib::eval_builtin` (analysis.rs).
//!
//! **Field arguments (task #6577).** The same names also accept a
//! `Field<D, Tensor<2,3,Q>>` — the shape `ElasticResult.stress` carries — and
//! there eval does NOT reduce: `crates/reify-expr/src/lib.rs` intercepts the
//! call and returns a lazily-wrapped `Value::Field`
//! (`crates/reify-expr/src/analysis.rs:132-224`). The compile-time result must
//! therefore be a `Type::Field` too, because `value_type_kind_matches`
//! (`crates/reify-eval/src/lib.rs:330`) maps `Value::Field` only onto
//! `Type::Field`. [`field_tensor_arg`] mirrors eval's shape gate and each arm
//! mirrors eval's per-name arity gate, so the compiler never claims a Field
//! result for a call eval would answer with `Value::Undef`.
//!
//! The call STAYS a `FunctionCall` (eval untouched). Only the compile-time
//! result type is fixed here, eliminating the first-arg `Tensor` drift in
//! `expr.rs`'s `NoUserFunctions` ladder. This is the established pattern for
//! pure eval-builtins: `eigenvalues`/`magnitude`/`determinant` (`math_signatures.rs`)
//! and `body_mass_props` (`is_dynamics_query` → `StructureRef("MassProperties")`).
//!
//! Wired into `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder
//! after the `is_joint_typed_fn` arm. The family is pinned disjoint from all
//! sibling families by the `units.rs` disjointness test.

use reify_core::{DimensionVector, Type};
use reify_ir::CompiledExpr;

use crate::signatures_common::scalar_or_real;

/// The complete set of FEA stress-analysis reduction builtin names recognised
/// by the compiler. Single source of truth — imported into the `units.rs` test
/// module to pin disjointness from all sibling families.
///
/// **5 names**: `von_mises`, `principal_stresses`, `max_shear`,
/// `safety_factor`, `stress_invariants`.
///
/// Case-sensitive: Reify function names are snake_case.
pub const ANALYSIS_FN_NAMES: &[&str] = &[
    "von_mises",
    "principal_stresses",
    "max_shear",
    "safety_factor",
    "stress_invariants",
];

/// Is `name` a FEA stress-analysis reduction builtin the compiler types via
/// [`analysis_fn_result_type`]? Name-only classification, mirroring
/// `is_math_typed_fn` and `is_joint_typed_fn`. Case-sensitive.
pub(crate) fn is_analysis_typed_fn(name: &str) -> bool {
    ANALYSIS_FN_NAMES.contains(&name)
}

/// Result type for a FEA stress-analysis reduction builtin, derived from the
/// compiled argument structure.
///
/// # Concrete-tensor forms (arg0 is a `Tensor`/`Matrix`)
///
/// - `von_mises` / `max_shear` → `scalar_or_real(tensor_quantity(arg0))`.
///   A Pressure tensor → `Scalar<Pressure>`; a dimensionless tensor → `Real`.
///   Mirrors `trace` / `magnitude` in `math_fn_result_type`.
/// - `principal_stresses` → `List(scalar_or_real(tensor_quantity(arg0)))`.
///   Mirrors the `eigenvalues` arm (matrix → List of eigenvalues).
/// - `safety_factor` → `Type::dimensionless_scalar()` (dimensionless yield/von_mises ratio).
/// - `stress_invariants` → `Type::StructureRef("StressInvariants")` (the
///   struct def in `std.fea`). Mirrors `is_dynamics_query` → `MassProperties`.
///
/// # Field-argument forms (arg0 is a `Field` with a 3x3 tensor codomain, task #6577)
///
/// - `von_mises` / `max_shear` at arity 1 → `Field<D, scalar_or_real(Q)>`.
///
/// The result is a **`Type::Field`, not a `Type::Scalar`**: eval does not reduce
/// a field eagerly, it wraps it LAZILY and hands back a `Value::Field`
/// (`crates/reify-expr/src/lib.rs` dispatch → `analysis::compute_von_mises` /
/// `compute_max_shear` → `wrap_tensor_field`, `crates/reify-expr/src/analysis.rs:132-157`).
/// `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`) maps a
/// `Value::Field` ONLY onto a `Type::Field`, and the mismatch is enforced in
/// production at `engine_admin.rs:124` as `EngineError::TypeKindMismatch` — so a
/// scalar compile-time type here would trade a dimension bug for a kind lie.
/// This is the same shape `fea_envelope_result_type` uses for `envelope_von_mises`
/// (`units.rs:1071-1093`). The consumer half needs no change: `max`/`min` already
/// reduce a `Type::Field` codomain via `reduce_field_codomain`
/// (`math_signatures.rs:275-287`), so `max(von_mises(stress))` is `Scalar<Pressure>`.
///
/// Only reached for names in [`ANALYSIS_FN_NAMES`] (the caller gates on
/// [`is_analysis_typed_fn`]); the `_` arm is therefore unreachable in practice
/// and returns a harmless `Type::dimensionless_scalar()`.
pub(crate) fn analysis_fn_result_type(name: &str, args: &[CompiledExpr]) -> Type {
    // Field-argument forms (task #6577). Gated on eval's exact shape+arity so the
    // compiler's claim stays narrower-or-equal to what eval can honour; every
    // fall-through lands on `Value::Undef`, which is kind-compatible with any type
    // (`value_type_kind_matches`, crates/reify-eval/src/lib.rs:313).
    if let Some((domain, dim)) = field_tensor_arg(args, 0)
        && matches!(name, "von_mises" | "max_shear")
        && args.len() == 1
    {
        return Type::Field {
            domain: Box::new(domain.clone()),
            codomain: Box::new(scalar_or_real(dim)),
        };
    }

    match name {
        // von_mises / max_shear: scalar reduction of the tensor quantity.
        // Scalar<Pressure> for a Pressure tensor; Real for dimensionless.
        // Mirrors `trace`/`magnitude` in math_fn_result_type.
        "von_mises" | "max_shear" => scalar_or_real(tensor_quantity(args, 0)),

        // principal_stresses: eigenvalues of the stress tensor.
        // Returns a List whose element type carries the tensor's quantity.
        // Mirrors the `eigenvalues` arm in math_fn_result_type.
        "principal_stresses" => Type::List(Box::new(scalar_or_real(tensor_quantity(args, 0)))),

        // safety_factor: yield / von_mises → dimensionless Real regardless of
        // input dimensions (pressure cancels).
        "safety_factor" => Type::dimensionless_scalar(),

        // stress_invariants: returns a StressInvariants StructureInstance.
        // The struct def lives in std.fea (`crates/reify-compiler/stdlib/fea.ri`).
        // Mirrors `is_dynamics_query` → `StructureRef("MassProperties")`.
        "stress_invariants" => Type::StructureRef("StressInvariants".to_string()),

        // Unreachable in practice — the caller gates on is_analysis_typed_fn.
        _ => Type::dimensionless_scalar(),
    }
}

/// The quantity dimension carried by a `Tensor` / `Matrix` arg at position
/// `i`, defaulting to `DIMENSIONLESS` when the arg is absent or not a tensor.
fn tensor_quantity(args: &[CompiledExpr], i: usize) -> DimensionVector {
    match args.get(i).map(|a| &a.result_type) {
        Some(Type::Tensor { quantity, .. }) | Some(Type::Matrix { quantity, .. }) => {
            match quantity.as_ref() {
                Type::Scalar { dimension } => *dimension,
                _ => DimensionVector::DIMENSIONLESS,
            }
        }
        _ => DimensionVector::DIMENSIONLESS,
    }
}

/// The `(domain, element dimension)` of arg `i` when it is a `Type::Field` whose
/// codomain is a 3x3 tensor/matrix of scalars. `None` otherwise.
///
/// Deliberately mirrors eval's gate — `analysis::tensor_element_dimension`
/// (`crates/reify-expr/src/analysis.rs:25-43`), reached via `validate_tensor_field`
/// (`:60-112`) — so the compile-time type and the `Value::Field` eval produces
/// agree under `value_type_kind_matches` (`crates/reify-eval/src/lib.rs:330`).
/// The `Type::Int` quantity branch is carried over for the same reason:
/// `tensor_element_dimension` maps it to `DIMENSIONLESS`.
///
/// Distinct from [`tensor_quantity`], which handles the CONCRETE tensor/matrix
/// arg forms and is deliberately left untouched by task #6577: teaching it to
/// recurse into a Field codomain would yield a `Scalar` compile-time type for a
/// call eval answers with a `Value::Field`.
fn field_tensor_arg(args: &[CompiledExpr], i: usize) -> Option<(&Type, DimensionVector)> {
    let Some(Type::Field { domain, codomain }) = args.get(i).map(|a| &a.result_type) else {
        return None;
    };
    let dim = match codomain.as_ref() {
        Type::Matrix {
            m: 3,
            n: 3,
            quantity,
        }
        | Type::Tensor {
            rank: 2,
            n: 3,
            quantity,
        } => match quantity.as_ref() {
            Type::Scalar { dimension } => *dimension,
            Type::Int => DimensionVector::DIMENSIONLESS,
            _ => return None,
        },
        _ => return None,
    };
    Some((domain.as_ref(), dim))
}

// `scalar_or_real` is defined in `crate::signatures_common` and re-exported
// into this module via `use crate::signatures_common::scalar_or_real` above.

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    /// Independent fixture — the 5 expected names. Deliberately does NOT
    /// reference `ANALYSIS_FN_NAMES` so a drift in that slice is caught
    /// against this independent list (mirrors `joint_signatures` / `math_signatures`
    /// patterns).
    const EXPECTED_NAMES: [&str; 5] = [
        "von_mises",
        "principal_stresses",
        "max_shear",
        "safety_factor",
        "stress_invariants",
    ];

    // ── Name-family contract ─────────────────────────────────────────────────

    /// `is_analysis_typed_fn` recognises every expected analysis reduction name.
    #[test]
    fn is_analysis_typed_fn_recognises_all_expected_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_analysis_typed_fn(name),
                "is_analysis_typed_fn({name:?}) must be true (FEA stress-analysis family)"
            );
        }
    }

    /// `is_analysis_typed_fn` rejects names from sibling families, the empty
    /// name, and unknown names.
    #[test]
    fn is_analysis_typed_fn_rejects_other_family_and_unknown_names() {
        assert!(
            !is_analysis_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        assert!(
            !is_analysis_typed_fn("body_mass_props"),
            "must reject dynamics-query 'body_mass_props'"
        );
        assert!(
            !is_analysis_typed_fn("vec"),
            "must reject math-linalg 'vec'"
        );
        assert!(
            !is_analysis_typed_fn("eigenvalues"),
            "must reject math-linalg 'eigenvalues'"
        );
        assert!(
            !is_analysis_typed_fn("prismatic"),
            "must reject joint 'prismatic'"
        );
        assert!(!is_analysis_typed_fn(""), "must reject empty name");
        assert!(
            !is_analysis_typed_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so the
    /// PascalCase forms must not match.
    #[test]
    fn is_analysis_typed_fn_is_case_sensitive() {
        assert!(
            !is_analysis_typed_fn("Von_mises"),
            "PascalCase must not match"
        );
        assert!(
            !is_analysis_typed_fn("Von_Mises"),
            "PascalCase must not match"
        );
        assert!(
            !is_analysis_typed_fn("Principal_stresses"),
            "PascalCase must not match"
        );
        assert!(
            !is_analysis_typed_fn("Stress_invariants"),
            "PascalCase must not match"
        );
    }

    /// `ANALYSIS_FN_NAMES` is exactly the 5 expected names: correct count,
    /// every expected name present, and no extra entry.
    #[test]
    fn analysis_fn_names_are_exactly_the_five() {
        assert_eq!(
            ANALYSIS_FN_NAMES.len(),
            EXPECTED_NAMES.len(),
            "ANALYSIS_FN_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            ANALYSIS_FN_NAMES
        );
        for name in EXPECTED_NAMES {
            assert!(
                ANALYSIS_FN_NAMES.contains(&name),
                "ANALYSIS_FN_NAMES must contain {name:?}"
            );
        }
        for name in ANALYSIS_FN_NAMES {
            assert!(
                EXPECTED_NAMES.contains(name),
                "ANALYSIS_FN_NAMES has unexpected entry {name:?} not in the fixture"
            );
        }
    }

    // ── Result-type resolution ───────────────────────────────────────────────
    // These tests are RED until step-2 replaces the stub.

    /// Helper: a `CompiledExpr` typed as a `Tensor{rank:2, n:3, quantity:dim}`.
    fn pressure_tensor_arg() -> CompiledExpr {
        CompiledExpr::literal(
            Value::Undef,
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::PRESSURE,
                }),
            },
        )
    }

    /// Helper: a `CompiledExpr` typed as a dimensionless `Tensor{rank:2, n:3, quantity:Real}`.
    fn dimensionless_tensor_arg() -> CompiledExpr {
        CompiledExpr::literal(
            Value::Undef,
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        )
    }

    /// `von_mises(Tensor<PRESSURE>)` → `Scalar<PRESSURE>`.
    #[test]
    fn von_mises_over_pressure_tensor_is_scalar_pressure() {
        let arg = pressure_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("von_mises", &[arg]),
            Type::Scalar {
                dimension: DimensionVector::PRESSURE
            },
            "von_mises over Pressure tensor must yield Scalar<PRESSURE>"
        );
    }

    /// `von_mises(Tensor<dimensionless>)` → `Type::dimensionless_scalar()`.
    #[test]
    fn von_mises_over_dimensionless_tensor_is_real() {
        let arg = dimensionless_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("von_mises", &[arg]),
            Type::dimensionless_scalar(),
            "von_mises over a dimensionless tensor must yield Type::dimensionless_scalar() (NOT Scalar<DIMENSIONLESS>)"
        );
    }

    /// `max_shear(Tensor<PRESSURE>)` → `Scalar<PRESSURE>`.
    #[test]
    fn max_shear_over_pressure_tensor_is_scalar_pressure() {
        let arg = pressure_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("max_shear", &[arg]),
            Type::Scalar {
                dimension: DimensionVector::PRESSURE
            },
            "max_shear over Pressure tensor must yield Scalar<PRESSURE>"
        );
    }

    /// `principal_stresses(Tensor<PRESSURE>)` → `List(Scalar<PRESSURE>)`.
    #[test]
    fn principal_stresses_over_pressure_tensor_is_list_scalar_pressure() {
        let arg = pressure_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("principal_stresses", &[arg]),
            Type::List(Box::new(Type::Scalar {
                dimension: DimensionVector::PRESSURE
            })),
            "principal_stresses over Pressure tensor must yield List(Scalar<PRESSURE>)"
        );
    }

    /// `principal_stresses(Tensor<dimensionless>)` → `List(Real)`.
    #[test]
    fn principal_stresses_over_dimensionless_tensor_is_list_real() {
        let arg = dimensionless_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("principal_stresses", &[arg]),
            Type::List(Box::new(Type::dimensionless_scalar())),
            "principal_stresses over dimensionless tensor must yield List(Real)"
        );
    }

    /// `safety_factor(...)` → `Type::dimensionless_scalar()` regardless of args.
    #[test]
    fn safety_factor_is_always_real() {
        let arg = pressure_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("safety_factor", &[arg]),
            Type::dimensionless_scalar(),
            "safety_factor must always return Type::dimensionless_scalar() (dimensionless ratio)"
        );
        assert_eq!(
            analysis_fn_result_type("safety_factor", &[]),
            Type::dimensionless_scalar(),
            "safety_factor with no args must still return Type::dimensionless_scalar()"
        );
    }

    /// `stress_invariants(...)` → `Type::StructureRef("StressInvariants")`.
    #[test]
    fn stress_invariants_is_structure_ref() {
        let arg = pressure_tensor_arg();
        assert_eq!(
            analysis_fn_result_type("stress_invariants", &[arg]),
            Type::StructureRef("StressInvariants".to_string()),
            "stress_invariants must return StructureRef(\"StressInvariants\")"
        );
        assert_eq!(
            analysis_fn_result_type("stress_invariants", &[]),
            Type::StructureRef("StressInvariants".to_string()),
            "stress_invariants with no args must still return StructureRef(\"StressInvariants\")"
        );
    }
}
