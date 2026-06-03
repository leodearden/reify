//! ISO 286-1 tolerancing builtins: `iso_it_tolerance`, `effective_tolerance_zone`.
//!
//! Task α — the producer. Implements two builtins plus a diagnose classifier;
//! no `.ri` / reify-core / reify-expr changes (those are siblings β/ε or out of
//! α's two-file scope).

use reify_core::{Diagnostic, DimensionVector};
use reify_ir::Value;

use crate::helpers::{sanitize_value, validate_dimensioned_scalar};

/// Evaluate an ISO tolerancing builtin by name.
///
/// Returns `Some(value)` if the name is a recognised tolerancing function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_tolerancing(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
}

/// Pure classifier: given the name and args of a stdlib call that returned
/// `Value::Undef`, determine whether this was a recognised tolerancing builtin
/// error and, if so, which `Diagnostic` (with `Severity::Error`) to emit.
///
/// Returns `None` for:
/// - unrecognised function names (non-tolerancing builtins, user functions, etc.)
/// - valid in-envelope calls to `iso_it_tolerance`
/// - any call to `effective_tolerance_zone`
///
/// Returns `Some(Diagnostic)` for out-of-envelope but well-typed calls to
/// `iso_it_tolerance` (grade outside IT5–IT18 or nominal size outside
/// `(0, 500mm]` or inverted/zero range).
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    let _ = (name, args);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    // ─── shared test helpers ──────────────────────────────────────────────────

    /// Build a finite LENGTH scalar (SI metres).
    fn len(si: f64) -> Value {
        Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH }
    }

    /// Extract the SI value from a LENGTH scalar; panic otherwise (test-only).
    fn scalar_si(v: &Value) -> f64 {
        match v {
            Value::Scalar { si_value, dimension } if *dimension == DimensionVector::LENGTH => {
                *si_value
            }
            other => panic!("expected LENGTH scalar, got {:?}", other),
        }
    }

    /// Assert `actual` is within `rel_tol` (relative) of `expected`.
    fn assert_rel_close(actual: f64, expected: f64, rel_tol: f64, label: &str) {
        let eps = rel_tol * expected.abs().max(1e-30_f64);
        assert!(
            (actual - expected).abs() <= eps,
            "{}: actual={:.6e} expected={:.6e} diff={:.3e} eps={:.3e}",
            label,
            actual,
            expected,
            (actual - expected).abs(),
            eps
        );
    }

    /// Build a MaterialCondition enum value.
    fn mc(variant: &str) -> Value {
        Value::Enum { type_name: "MaterialCondition".into(), variant: variant.into() }
    }

    // ─── step-1: RED tests for effective_tolerance_zone ──────────────────────

    #[test]
    fn efz_rfs_ignores_departure() {
        // RFS: zone = tolerance_value; departure ignored.
        let result = crate::eval_builtin(
            "effective_tolerance_zone",
            &[len(1e-4), mc("RFS"), len(2e-5)],
        );
        let zone = scalar_si(&result);
        assert_rel_close(zone, 1e-4, 1e-9, "RFS zone should equal tolerance_value");
    }

    #[test]
    fn efz_mmc_adds_departure() {
        // MMC: zone = tolerance_value + bonus_departure.
        let result = crate::eval_builtin(
            "effective_tolerance_zone",
            &[len(1e-4), mc("MMC"), len(2e-5)],
        );
        let zone = scalar_si(&result);
        assert_rel_close(zone, 1.2e-4, 1e-9, "MMC zone should be tol + departure");
    }

    #[test]
    fn efz_lmc_adds_departure() {
        // LMC: zone = tolerance_value + bonus_departure.
        let result = crate::eval_builtin(
            "effective_tolerance_zone",
            &[len(1e-4), mc("LMC"), len(2e-5)],
        );
        let zone = scalar_si(&result);
        assert_rel_close(zone, 1.2e-4, 1e-9, "LMC zone should be tol + departure");
    }

    #[test]
    fn efz_rejects_wrong_arity() {
        // 2 args → Undef.
        assert!(
            crate::eval_builtin("effective_tolerance_zone", &[len(1e-4), mc("RFS")]).is_undef(),
            "2 args should return Undef"
        );
        // 4 args → Undef.
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), mc("RFS"), len(2e-5), len(0.0)]
            )
            .is_undef(),
            "4 args should return Undef"
        );
    }

    #[test]
    fn efz_rejects_wrong_enum_type_name() {
        // Enum with wrong type_name → Undef.
        let wrong_type = Value::Enum {
            type_name: "Distribution".into(),
            variant: "RFS".into(),
        };
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), wrong_type, len(2e-5)]
            )
            .is_undef(),
            "wrong type_name enum should return Undef"
        );
    }

    #[test]
    fn efz_rejects_unknown_variant() {
        // Unknown MaterialCondition variant → Undef.
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), mc("Bogus"), len(2e-5)]
            )
            .is_undef(),
            "unknown variant should return Undef"
        );
    }

    #[test]
    fn efz_rejects_non_enum_material_condition() {
        // Non-enum material_condition → Undef.
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), Value::Int(0), len(2e-5)]
            )
            .is_undef(),
            "non-enum material_condition should return Undef"
        );
    }

    #[test]
    fn efz_rejects_non_length_tolerance_value() {
        // Non-LENGTH tolerance_value → Undef.
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[Value::Real(1e-4), mc("RFS"), len(2e-5)]
            )
            .is_undef(),
            "non-LENGTH tolerance_value should return Undef"
        );
    }
}
