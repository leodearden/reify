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
    Some(match name {
        "effective_tolerance_zone" => effective_tolerance_zone(args),
        "iso_it_tolerance" => iso_it_tolerance(args),
        _ => return None,
    })
}

// ─── iso_it_tolerance helpers ────────────────────────────────────────────────

/// Look up the IT grade factor for grades IT5–IT18.
///
/// Returns `None` for grades outside this range (IT4, IT19+, negatives).
fn it_grade_factor(grade: i64) -> Option<f64> {
    match grade {
        5 => Some(7.0),
        6 => Some(10.0),
        7 => Some(16.0),
        8 => Some(25.0),
        9 => Some(40.0),
        10 => Some(64.0),
        11 => Some(100.0),
        12 => Some(160.0),
        13 => Some(250.0),
        14 => Some(400.0),
        15 => Some(640.0),
        16 => Some(1000.0),
        17 => Some(1600.0),
        18 => Some(2500.0),
        _ => None,
    }
}

/// Parse `iso_it_tolerance` arguments, returning `(grade, min_mm, max_mm)`.
///
/// Requires exactly 3 args: `Value::Int(grade)`, two finite LENGTH scalars.
/// Returns `None` on wrong arity or wrong types; the size/range validity gate
/// is applied separately in `iso_it_tolerance` (and reused by `diagnose`).
fn parse_iso_well_typed(args: &[Value]) -> Option<(i64, f64, f64)> {
    if args.len() != 3 {
        return None;
    }
    let grade = match &args[0] {
        Value::Int(n) => *n,
        _ => return None,
    };
    let min_si = validate_dimensioned_scalar(&args[1], DimensionVector::LENGTH)?;
    let max_si = validate_dimensioned_scalar(&args[2], DimensionVector::LENGTH)?;
    Some((grade, min_si * 1e3, max_si * 1e3))
}

/// Validate that a nominal size range is within the supported ISO 286-1 envelope.
///
/// Returns `true` iff:
/// - `min_mm > 0` and `max_mm > 0` (positive sizes)
/// - `min_mm ≤ max_mm` (non-inverted range)
/// - `max_mm ≤ 500.0` (within the standardised upper step bound, inclusive)
///
/// The 500 mm limit is the upper end of the standardised step ranges in
/// ISO 286-1; the cube-root formula is only validated against published cells
/// for sizes in the 3–500 mm range.
fn iso_size_in_envelope(min_mm: f64, max_mm: f64) -> bool {
    min_mm > 0.0 && max_mm > 0.0 && min_mm <= max_mm && max_mm <= 500.0
}

/// Compute ISO 286-1 standard tolerance (IT grade) for a given grade and size range.
///
/// Formula: D = √(min_mm · max_mm), i = 0.45·∛D + 0.001·D, tol = factor·i.
/// Returns the tolerance as a finite LENGTH scalar in SI metres, or `Value::Undef`
/// for unsupported grades, wrong arg types, or out-of-envelope sizes.
fn iso_it_tolerance(args: &[Value]) -> Value {
    let (grade, min_mm, max_mm) = match parse_iso_well_typed(args) {
        Some(t) => t,
        None => return Value::Undef,
    };
    let factor = match it_grade_factor(grade) {
        Some(f) => f,
        None => return Value::Undef,
    };
    if !iso_size_in_envelope(min_mm, max_mm) {
        return Value::Undef;
    }
    let d = (min_mm * max_mm).sqrt();
    let i = 0.45 * d.cbrt() + 0.001 * d;
    let tol_um = factor * i;
    sanitize_value(Value::Scalar { si_value: tol_um * 1e-6, dimension: DimensionVector::LENGTH })
}

// ─── private helpers ─────────────────────────────────────────────────────────

/// Compute effective tolerance zone given tolerance value, material condition,
/// and bonus departure.
///
/// - `RFS`       → zone = tolerance_value (`bonus_departure` is truly ignored;
///   `args[2]` need not be a valid LENGTH scalar for `RFS`)
/// - `MMC`/`LMC` → zone = tolerance_value + bonus_departure.  `bonus_departure`
///   must be a finite LENGTH scalar.
/// - Any other variant / non-enum / wrong type_name → `Value::Undef`
///
/// A negative zone (e.g. a negative `tolerance_value` under RFS, or
/// `tol + departure < 0` under MMC/LMC) is physically meaningless and returns
/// `Value::Undef` regardless of the material condition.
fn effective_tolerance_zone(args: &[Value]) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    let tol = match validate_dimensioned_scalar(&args[0], DimensionVector::LENGTH) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let zone = match &args[1] {
        Value::Enum { type_name, variant } if type_name == "MaterialCondition" => {
            match variant.as_str() {
                // RFS: bonus departure is semantically irrelevant; skip validation.
                "RFS" => tol,
                // MMC / LMC: validate departure and sum.
                "MMC" | "LMC" => {
                    let departure =
                        match validate_dimensioned_scalar(&args[2], DimensionVector::LENGTH) {
                            Some(v) => v,
                            None => return Value::Undef,
                        };
                    tol + departure
                }
                _ => return Value::Undef,
            }
        }
        _ => return Value::Undef,
    };
    // Negative tolerance zone is physically meaningless under any material condition.
    if zone < 0.0 {
        return Value::Undef;
    }
    sanitize_value(Value::Scalar { si_value: zone, dimension: DimensionVector::LENGTH })
}

/// Pure classifier: given the name and args of a stdlib call that returned
/// `Value::Undef`, determine whether this was a recognised tolerancing builtin
/// error and, if so, which `Diagnostic` (with `Severity::Error`) to emit.
///
/// Returns `None` for:
/// - unrecognised function names (non-tolerancing builtins, user functions, etc.)
/// - valid in-envelope calls to `iso_it_tolerance`
/// - any call to `effective_tolerance_zone`
/// - ill-typed args (wrong arity, wrong types) — parse_iso_well_typed returns None
///   and the `?` early-exits the function
///
/// Returns `Some(Diagnostic)` for out-of-envelope but well-typed calls to
/// `iso_it_tolerance` (grade outside IT5–IT18 or nominal size outside
/// `(0, 500mm]` or inverted/zero range).
///
/// Note: the diagnostic is code-less (`Diagnostic::error` sets `code: None`).
/// Adding a `DiagnosticCode` variant would expand the change to `reify-core` and
/// its exhaustive code-enumeration tests — outside α's two-file scope (see design).
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    match name {
        "iso_it_tolerance" => {
            let (grade, min_mm, max_mm) = parse_iso_well_typed(args)?;
            let in_env =
                it_grade_factor(grade).is_some() && iso_size_in_envelope(min_mm, max_mm);
            if in_env {
                None
            } else {
                Some(Diagnostic::error(
                    "E_TolerancingOutOfEnvelope: iso_it_tolerance supports \
                     IT5\u{2013}IT18 for nominal sizes \u{2264} 500 mm",
                ))
            }
        }
        _ => None,
    }
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

    // ─── step-3: RED tests for iso_it_tolerance published ISO 286-1 cells ───

    /// Extract µm from a LENGTH scalar result.
    fn um(v: &Value) -> f64 {
        scalar_si(v) * 1e6
    }

    #[test]
    fn iso_it_tolerance_grade6_18_30mm() {
        // IT6 @ Ø18–30: D=√(18·30)=23.238, ∛D=2.8537, i=1.3074, ×10=13.074 µm
        let result = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(6), len(0.018), len(0.030)],
        );
        // Must be a finite LENGTH scalar
        let result_um = um(&result);
        assert_rel_close(result_um, 13.074, 5e-3, "IT6@18-30 µm within 0.5%");
        assert_eq!(result_um.round(), 13.0, "IT6@18-30 rounds to published cell 13 µm");
    }

    #[test]
    fn iso_it_tolerance_grade7_30_50mm() {
        // IT7 @ Ø30–50: D=√(30·50)=38.730, ∛D=3.3819, i=1.5606, ×16=24.969 µm
        let result = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(7), len(0.030), len(0.050)],
        );
        let result_um = um(&result);
        assert_rel_close(result_um, 24.969, 5e-3, "IT7@30-50 µm within 0.5%");
        assert_eq!(result_um.round(), 25.0, "IT7@30-50 rounds to published cell 25 µm");
    }

    #[test]
    fn iso_it_tolerance_grade8_6_10mm() {
        // IT8 @ Ø6–10: D=√(6·10)=7.746, ∛D=1.9786, i=0.89814, ×25=22.453 µm
        let result = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(8), len(0.006), len(0.010)],
        );
        let result_um = um(&result);
        assert_rel_close(result_um, 22.453, 5e-3, "IT8@6-10 µm within 0.5%");
        assert_eq!(result_um.round(), 22.0, "IT8@6-10 rounds to published cell 22 µm");
    }

    // ─── step-5: RED tests for iso_it_tolerance envelope edges ───────────────

    // Helper: returns true if v is a finite LENGTH-dimensioned Scalar.
    fn is_finite_length_scalar(v: &Value) -> bool {
        matches!(v, Value::Scalar { si_value, dimension } if *dimension == DimensionVector::LENGTH && si_value.is_finite())
    }

    #[test]
    fn iso_it_envelope_grade4_is_undef() {
        // IT4: below IT5 — already Undef after step-4 (it_grade_factor returns None)
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(4), len(0.018), len(0.030)]
            )
            .is_undef(),
            "IT4 should return Undef (below IT5)"
        );
    }

    #[test]
    fn iso_it_envelope_grade19_is_undef() {
        // IT19: above IT18 — already Undef after step-4
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(19), len(0.018), len(0.030)]
            )
            .is_undef(),
            "IT19 should return Undef (above IT18)"
        );
    }

    #[test]
    fn iso_it_envelope_size_over_500mm_is_undef() {
        // max_mm = 600 > 500 → RED: no size gate yet in step-4
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(7), len(0.5), len(0.6)]
            )
            .is_undef(),
            "nominal size > 500mm should return Undef"
        );
    }

    #[test]
    fn iso_it_envelope_inverted_range_is_undef() {
        // min_mm (50) > max_mm (30) → RED: no inversion gate yet
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(7), len(0.050), len(0.030)]
            )
            .is_undef(),
            "inverted range min>max should return Undef"
        );
    }

    #[test]
    fn iso_it_envelope_zero_size_is_undef() {
        // min_mm = 0 → non-positive: D=0 currently gives 0 scalar, not Undef → RED
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(7), len(0.0), len(0.010)]
            )
            .is_undef(),
            "zero nominal size should return Undef"
        );
    }

    #[test]
    fn iso_it_envelope_invalid_arg_types_are_undef() {
        // grade as Value::Real → Undef
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Real(7.0), len(0.018), len(0.030)]
            )
            .is_undef(),
            "grade as Real should return Undef"
        );
        // nominal_min as Value::Int → Undef (not a LENGTH scalar)
        assert!(
            crate::eval_builtin(
                "iso_it_tolerance",
                &[Value::Int(7), Value::Int(18), len(0.030)]
            )
            .is_undef(),
            "nominal_min as Int should return Undef"
        );
    }

    #[test]
    fn iso_it_envelope_inclusive_it5_is_finite() {
        // IT5 is the lowest supported grade → must return a finite LENGTH scalar
        let r = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(5), len(0.018), len(0.030)],
        );
        assert!(is_finite_length_scalar(&r), "IT5 should return finite LENGTH scalar");
    }

    #[test]
    fn iso_it_envelope_inclusive_it18_is_finite() {
        // IT18 is the highest supported grade → must return a finite LENGTH scalar
        let r = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(18), len(0.018), len(0.030)],
        );
        assert!(is_finite_length_scalar(&r), "IT18 should return finite LENGTH scalar");
    }

    #[test]
    fn iso_it_envelope_inclusive_500mm_is_finite() {
        // max_mm = 500mm exactly → within envelope, must return a finite LENGTH scalar
        let r = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(7), len(0.4), len(0.5)],
        );
        assert!(
            is_finite_length_scalar(&r),
            "size exactly 500mm should return finite LENGTH scalar (inclusive bound)"
        );
    }

    /// Suggestion 3: document sub-3mm behavior as an explicit, tested decision.
    ///
    /// The `iso_size_in_envelope` predicate accepts any `min_mm > 0`, but the
    /// ISO 286-1 cube-root formula is only validated against published cells for
    /// the 3–500 mm step ranges.  Sizes in (0, 3) mm pass the gate and yield a
    /// computed (extrapolated) result — this is an accepted gap, not a hard error.
    #[test]
    fn iso_it_tolerance_sub_3mm_accepted_but_unvalidated() {
        // IT6 @ Ø1–3 mm: passes the envelope gate; result is extrapolated.
        let r = crate::eval_builtin(
            "iso_it_tolerance",
            &[Value::Int(6), len(0.001), len(0.003)],
        );
        assert!(
            is_finite_length_scalar(&r),
            "sub-3mm sizes (0 < max ≤ 3mm) should return a finite LENGTH scalar \
             (accepted-but-unvalidated region — formula extrapolates beyond ISO 286-1 tables)"
        );
    }

    // ─── step-7: RED tests for diagnose classifier ────────────────────────────

    #[test]
    fn diagnose_iso_it_grade4_out_of_envelope_returns_error() {
        // IT4: grade below IT5 but well-typed → Some(Error)
        use reify_core::Severity;
        let d = super::diagnose(
            "iso_it_tolerance",
            &[Value::Int(4), len(0.018), len(0.030)],
        );
        let diag = d.expect("IT4 grade out-of-envelope should return Some(Diagnostic)");
        assert_eq!(
            diag.severity,
            Severity::Error,
            "out-of-envelope diagnostic should be Error severity"
        );
    }

    #[test]
    fn diagnose_iso_it_size_over_500mm_returns_error() {
        // size > 500mm: well-typed but out of envelope → Some(Error)
        use reify_core::Severity;
        let d = super::diagnose(
            "iso_it_tolerance",
            &[Value::Int(7), len(0.5), len(0.6)],
        );
        let diag = d.expect("size > 500mm should return Some(Diagnostic)");
        assert_eq!(
            diag.severity,
            Severity::Error,
            "size-out-of-envelope diagnostic should be Error severity"
        );
    }

    #[test]
    fn diagnose_iso_it_valid_call_returns_none() {
        // Valid in-envelope call → None
        let d = super::diagnose(
            "iso_it_tolerance",
            &[Value::Int(6), len(0.018), len(0.030)],
        );
        assert!(d.is_none(), "valid in-envelope call should return None");
    }

    #[test]
    fn diagnose_efz_returns_none() {
        // effective_tolerance_zone → None (not diagnosed by this classifier)
        let d = super::diagnose(
            "effective_tolerance_zone",
            &[len(1e-4), mc("MMC"), len(2e-5)],
        );
        assert!(d.is_none(), "effective_tolerance_zone should always return None from diagnose");
    }

    #[test]
    fn diagnose_unknown_name_returns_none() {
        // Unrecognised function → None
        let d = super::diagnose("totally_unknown", &[]);
        assert!(d.is_none(), "unknown function should return None from diagnose");
    }

    // ─── amendment tests (reviewer suggestions) ───────────────────────────────

    /// Suggestion 1+2: negative zone (tol + departure < 0) should return Undef.
    #[test]
    fn efz_mmc_negative_zone_is_undef() {
        // tol=1e-4, departure=-2e-4 → zone = -1e-4 < 0 → Undef
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), mc("MMC"), len(-2e-4)]
            )
            .is_undef(),
            "negative zone (tol + departure < 0) for MMC should return Undef"
        );
    }

    /// Suggestion 1: RFS with a negative tolerance_value returns Undef — the
    /// zone < 0 guard applies uniformly to all material conditions.
    #[test]
    fn efz_rfs_negative_tol_is_undef() {
        // tol = -1e-4 < 0 → zone = -1e-4 < 0 → Undef even under RFS
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(-1e-4), mc("RFS"), len(2e-5)]
            )
            .is_undef(),
            "negative tolerance_value should return Undef under RFS"
        );
    }

    /// Suggestion 2: RFS truly ignores bonus_departure — even an invalid departure
    /// (non-LENGTH) should not cause Undef when the condition is RFS.
    #[test]
    fn efz_rfs_ignores_invalid_departure_type() {
        // RFS: departure arg is ignored; Value::Real is not a LENGTH scalar but
        // should not trigger Undef since departure is irrelevant for RFS.
        let result = crate::eval_builtin(
            "effective_tolerance_zone",
            &[len(1e-4), mc("RFS"), Value::Real(2e-5)],
        );
        let zone = scalar_si(&result);
        assert_rel_close(
            zone,
            1e-4,
            1e-9,
            "RFS zone should equal tolerance_value regardless of departure type",
        );
    }

    /// Suggestion 3: non-LENGTH bonus_departure for MMC returns Undef.
    #[test]
    fn efz_mmc_rejects_non_length_departure() {
        // MMC with Value::Real (not a LENGTH scalar) for departure → Undef.
        assert!(
            crate::eval_builtin(
                "effective_tolerance_zone",
                &[len(1e-4), mc("MMC"), Value::Real(2e-5)]
            )
            .is_undef(),
            "non-LENGTH bonus_departure for MMC should return Undef"
        );
    }

    /// Suggestion 3: diagnose returns Some(Error) for inverted range (min > max).
    #[test]
    fn diagnose_iso_it_inverted_range_returns_error() {
        use reify_core::Severity;
        // min=50mm > max=30mm → out-of-envelope (inverted), well-typed → Some(Error)
        let d = super::diagnose(
            "iso_it_tolerance",
            &[Value::Int(7), len(0.050), len(0.030)],
        );
        let diag = d.expect("inverted range should return Some(Diagnostic)");
        assert_eq!(
            diag.severity,
            Severity::Error,
            "inverted-range diagnostic should be Error severity"
        );
    }

    /// Suggestion 3: diagnose returns Some(Error) for zero nominal size.
    #[test]
    fn diagnose_iso_it_zero_size_returns_error() {
        use reify_core::Severity;
        // min=0 → non-positive → out-of-envelope, well-typed → Some(Error)
        let d = super::diagnose(
            "iso_it_tolerance",
            &[Value::Int(7), len(0.0), len(0.010)],
        );
        let diag = d.expect("zero nominal size should return Some(Diagnostic)");
        assert_eq!(
            diag.severity,
            Severity::Error,
            "zero-size diagnostic should be Error severity"
        );
    }

    /// Suggestion 3: diagnose returns None for wrong arity (ill-typed → parse returns None → ?).
    #[test]
    fn diagnose_iso_it_wrong_arity_returns_none() {
        // 2 args → parse_iso_well_typed returns None → diagnose returns None (not an error).
        let d = super::diagnose("iso_it_tolerance", &[Value::Int(7), len(0.018)]);
        assert!(d.is_none(), "wrong arity should return None (not a diagnosable error)");
    }

    // ─── η/4480 step-1: RED tests for the nominal() inert geometry marker ─────

    /// `nominal()` is a zero-arg builtin returning a deterministic, non-Undef
    /// inert marker usable as a `Geometry` default: an INVALID-handle
    /// `Value::GeometryHandle` sentinel. The marker flows nowhere — the Conforms
    /// body never reads `actual`, and the η pass keys on the explicit binding,
    /// never the default — so an inert sentinel is exactly the right shape.
    #[test]
    fn nominal_returns_inert_invalid_geometry_marker() {
        use reify_ir::GeometryHandleId;
        let result = crate::eval_builtin("nominal", &[]);
        assert!(!result.is_undef(), "nominal() should return a non-Undef marker");
        match &result {
            Value::GeometryHandle { kernel_handle, .. } => {
                assert_eq!(
                    *kernel_handle,
                    GeometryHandleId::INVALID,
                    "nominal() marker must carry the INVALID-handle sentinel"
                );
            }
            other => panic!("expected Value::GeometryHandle marker, got {:?}", other),
        }
        // Geometry-typed so `param actual : Geometry = nominal()` type-checks.
        assert_eq!(
            result.try_infer_type(),
            Some(reify_core::ty::Type::Geometry),
            "nominal() marker must be Geometry-typed"
        );
    }

    /// The marker must be deterministic across calls — no session/RNG state —
    /// so a compiled default is stable.
    #[test]
    fn nominal_is_deterministic() {
        let a = crate::eval_builtin("nominal", &[]);
        let b = crate::eval_builtin("nominal", &[]);
        assert_eq!(a, b, "nominal() must be deterministic across calls");
    }

    /// `nominal()` is strictly zero-arity: any argument yields `Undef`.
    #[test]
    fn nominal_rejects_args() {
        assert!(
            crate::eval_builtin("nominal", &[len(1e-4)]).is_undef(),
            "nominal() with 1 arg should return Undef"
        );
        assert!(
            crate::eval_builtin("nominal", &[len(1e-4), len(2e-5)]).is_undef(),
            "nominal() with 2 args should return Undef"
        );
    }
}
