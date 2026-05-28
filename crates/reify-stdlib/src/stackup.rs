//! Tolerance stack-up builtins: Contributor value-shape builders.
//! T1 Phase 1 — math arms in T2/T5.

use std::collections::BTreeMap;

use reify_core::DimensionVector;
use reify_ir::Value;

use crate::helpers::validate_dimensioned_scalar;

/// Evaluate a tolerance stack-up builtin by name.
///
/// Returns `Some(value)` if the name is a recognised stack-up function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_stackup(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "contributor" => contributor(args),
        "contributor_asym" => contributor_asym(args),
        _ => return None,
    })
}

// --- private helpers ---

/// Validate that `v` is a LENGTH scalar with a finite `si_value`.
fn len_scalar(v: &Value) -> Option<f64> {
    validate_dimensioned_scalar(v, DimensionVector::LENGTH)
}

/// Parse a sign value: accepts only `Value::Int(1)` or `Value::Int(-1)`.
fn parse_sign(v: &Value) -> Option<i64> {
    match v {
        Value::Int(1) => Some(1),
        Value::Int(-1) => Some(-1),
        _ => None,
    }
}

// --- builder functions ---

fn contributor(args: &[Value]) -> Value {
    if !matches!(args.len(), 2 | 3) {
        return Value::Undef;
    }
    let nominal_si = match len_scalar(&args[0]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let tol_si = match len_scalar(&args[1]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let sign: i64 = if args.len() == 3 {
        match parse_sign(&args[2]) {
            Some(s) => s,
            None => return Value::Undef,
        }
    } else {
        1
    };
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let tol = Value::Scalar { si_value: tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, tol.clone(), tol, sign, "Normal")
}

fn contributor_asym(args: &[Value]) -> Value {
    if !matches!(args.len(), 3..=5) {
        return Value::Undef;
    }
    let nominal_si = match len_scalar(&args[0]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let plus_tol_si = match len_scalar(&args[1]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let minus_tol_si = match len_scalar(&args[2]) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let sign: i64 = if args.len() >= 4 {
        match parse_sign(&args[3]) {
            Some(s) => s,
            None => return Value::Undef,
        }
    } else {
        1
    };
    let dist_variant: &str = if args.len() == 5 {
        match parse_distribution(&args[4]) {
            Some(v) => v,
            None => return Value::Undef,
        }
    } else {
        "Normal"
    };
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let plus_tol = Value::Scalar { si_value: plus_tol_si, dimension: DimensionVector::LENGTH };
    let minus_tol = Value::Scalar { si_value: minus_tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, plus_tol, minus_tol, sign, dist_variant)
}

fn parse_distribution(v: &Value) -> Option<&str> {
    match v {
        Value::Enum { type_name, variant } if type_name == "Distribution" => {
            match variant.as_str() {
                s @ ("Normal" | "Uniform" | "Triangular") => Some(s),
                _ => None,
            }
        }
        _ => None,
    }
}

fn make_contributor_map(
    nominal: Value,
    plus_tol: Value,
    minus_tol: Value,
    sign: i64,
    dist_variant: &str,
) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("nominal".into()), nominal);
    m.insert(Value::String("plus_tol".into()), plus_tol);
    m.insert(Value::String("minus_tol".into()), minus_tol);
    m.insert(Value::String("sign".into()), Value::Int(sign));
    m.insert(
        Value::String("distribution".into()),
        Value::Enum { type_name: "Distribution".into(), variant: dist_variant.into() },
    );
    Value::Map(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    fn len(si: f64) -> Value {
        Value::Scalar { si_value: si, dimension: DimensionVector::LENGTH }
    }

    fn expect_map(v: Option<Value>) -> std::collections::BTreeMap<Value, Value> {
        match v {
            Some(Value::Map(m)) => m,
            other => panic!("expected Some(Map), got {:?}", other),
        }
    }

    #[test]
    fn unknown_function_returns_none() {
        assert!(eval_stackup("foo", &[]).is_none());
    }

    #[test]
    fn math_stub_names_return_none() {
        assert!(eval_stackup("stackup_worst_case", &[]).is_none());
        assert!(eval_stackup("stackup_rss", &[]).is_none());
        assert!(eval_stackup("monte_carlo_stackup", &[]).is_none());
    }

    #[test]
    fn eval_builtin_contributor_returns_map() {
        let m = match crate::eval_builtin("contributor", &[len(0.010), len(0.0001)]) {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
    }

    #[test]
    fn eval_builtin_unknown_stackup_name_returns_undef() {
        assert!(crate::eval_builtin("stackup_xyz_unknown", &[]).is_undef());
    }

    #[test]
    fn eval_builtin_t1_stub_math_names_return_undef() {
        assert!(crate::eval_builtin("stackup_worst_case", &[]).is_undef());
        assert!(crate::eval_builtin("stackup_rss", &[]).is_undef());
        assert!(crate::eval_builtin("monte_carlo_stackup", &[]).is_undef());
    }

    #[test]
    fn contributor_asym_validation_returns_undef() {
        let nom = len(0.010);
        let pt = len(0.0001);
        let mt = len(0.00005);

        // (a) arity: 0/1/2/6 args
        assert!(eval_stackup("contributor_asym", &[]).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", std::slice::from_ref(&nom)).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone()]).unwrap().is_undef());
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
            nom.clone(),
        ]).unwrap().is_undef());
        // (b) nominal wrong dim
        let force = Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE };
        assert!(eval_stackup("contributor_asym", &[force, pt.clone(), mt.clone()]).unwrap().is_undef());
        // (c) plus_tol is Value::Int (not Scalar)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), Value::Int(1), mt.clone()]).unwrap().is_undef());
        // (d) plus_tol has NaN si_value
        let nan = Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH };
        assert!(eval_stackup("contributor_asym", &[nom.clone(), nan, mt.clone()]).unwrap().is_undef());
        // (e) sign=Int(0)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone(), mt.clone(), Value::Int(0)]).unwrap().is_undef());
        // (f) sign=Real(1.0)
        assert!(eval_stackup("contributor_asym", &[nom.clone(), pt.clone(), mt.clone(), Value::Real(1.0)]).unwrap().is_undef());
        // (g) distribution is String (not Enum)
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1), Value::String("Normal".into()),
        ]).unwrap().is_undef());
        // (h) distribution Enum with wrong type_name
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Material".into(), variant: "Steel".into() },
        ]).unwrap().is_undef());
        // (i) distribution Enum with unrecognised variant
        assert!(eval_stackup("contributor_asym", &[
            nom.clone(), pt.clone(), mt.clone(), Value::Int(1),
            Value::Enum { type_name: "Distribution".into(), variant: "Lognormal".into() },
        ]).unwrap().is_undef());
    }

    #[test]
    fn contributor_asym_4arg_accepts_explicit_sign() {
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(-1),
        ]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(-1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    #[test]
    fn contributor_asym_5arg_accepts_distribution_uniform() {
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Uniform".into() };
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(1), dist,
        ]));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Uniform".into() }
        );
    }

    #[test]
    fn contributor_asym_5arg_accepts_distribution_triangular() {
        let dist = Value::Enum { type_name: "Distribution".into(), variant: "Triangular".into() };
        let m = expect_map(eval_stackup("contributor_asym", &[
            len(0.010), len(0.0001), len(0.00005), Value::Int(1), dist,
        ]));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Triangular".into() }
        );
    }

    #[test]
    fn contributor_asym_3arg_returns_map_with_asymmetric_tols() {
        let nominal = len(0.010);   // 10mm
        let plus_tol = len(0.0001); // 0.1mm
        let minus_tol = len(0.00005); // 0.05mm
        let m = expect_map(eval_stackup("contributor_asym", &[nominal, plus_tol, minus_tol]));

        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.00005));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    #[test]
    fn contributor_validation_returns_undef() {
        let nom = len(0.010);
        let tol = len(0.0001);

        // (a) zero args
        assert!(eval_stackup("contributor", &[]).unwrap().is_undef());
        // (b) one arg
        assert!(eval_stackup("contributor", std::slice::from_ref(&nom)).unwrap().is_undef());
        // (c) four args
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(1), tol.clone()]).unwrap().is_undef());
        // (d) nominal is Value::Real (not Scalar)
        assert!(eval_stackup("contributor", &[Value::Real(0.010), tol.clone()]).unwrap().is_undef());
        // (e) nominal is FORCE scalar (wrong dim)
        let force = Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE };
        assert!(eval_stackup("contributor", &[force, tol.clone()]).unwrap().is_undef());
        // (f) nominal has NaN si_value
        let nan = Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH };
        assert!(eval_stackup("contributor", &[nan, tol.clone()]).unwrap().is_undef());
        // (g) tol is ANGLE scalar (wrong dim)
        let angle = Value::Scalar { si_value: 0.1, dimension: DimensionVector::ANGLE };
        assert!(eval_stackup("contributor", &[nom.clone(), angle]).unwrap().is_undef());
        // (h) tol is Value::Int
        assert!(eval_stackup("contributor", &[nom.clone(), Value::Int(1)]).unwrap().is_undef());
        // (i) sign is Int(0)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(0)]).unwrap().is_undef());
        // (j) sign is Int(2)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Int(2)]).unwrap().is_undef());
        // (k) sign is Real(1.0) (not Int)
        assert!(eval_stackup("contributor", &[nom.clone(), tol.clone(), Value::Real(1.0)]).unwrap().is_undef());
    }

    #[test]
    fn contributor_3arg_accepts_explicit_sign_negative() {
        let m = expect_map(eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(-1));
    }

    #[test]
    fn contributor_3arg_accepts_explicit_sign_positive() {
        let m = expect_map(eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(1)]));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
    }

    #[test]
    fn contributor_2arg_returns_map_with_default_sign_and_distribution() {
        let nominal = len(0.010); // 10mm
        let tol = len(0.0001);    // 0.1mm
        let m = expect_map(eval_stackup("contributor", &[nominal, tol]));

        assert_eq!(m.len(), 5);
        assert_eq!(m[&Value::String("nominal".into())], len(0.010));
        assert_eq!(m[&Value::String("plus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("minus_tol".into())], len(0.0001));
        assert_eq!(m[&Value::String("sign".into())], Value::Int(1));
        assert_eq!(
            m[&Value::String("distribution".into())],
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() }
        );
    }

    // ─── shared helpers for step-1 and step-3 tests ──────────────────────────

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

    /// Golden 3-contributor chain (shared by worst_case and rss tests):
    /// c1(nominal=10mm, tol=0.1mm, +1), c2(5mm, 0.05mm, -1), c3(3mm, 0.2mm, +1).
    /// gap_nominal = 0.010 - 0.005 + 0.003 = 0.008 m.
    fn golden_chain() -> Value {
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(1)]).unwrap();
        let c2 =
            eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(-1)]).unwrap();
        let c3 = eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(1)]).unwrap();
        Value::List(vec![c1, c2, c3])
    }

    // ─── stackup_worst_case tests (step-1 RED; GREEN after step-2 impl) ──────

    #[test]
    fn worst_case_happy_path_golden_chain() {
        // GOLDEN chain hand-calc (SI meters):
        //   gap_nominal     = 0.010 - 0.005 + 0.003       = 0.008       m
        //   sum_plus        = 0.0001 + 0.00005 + 0.0002    = 0.00035     m
        //   sum_minus       = 0.0001 + 0.00005 + 0.0002    = 0.00035     m (symmetric)
        //   worst_case_max  = 0.008 + 0.00035              = 0.00835     m
        //   worst_case_min  = 0.008 - 0.00035              = 0.00765     m
        //   worst_case_band = (0.00035 + 0.00035) / 2      = 3.5e-4      m
        let m = expect_map(eval_stackup("stackup_worst_case", &[golden_chain()]));
        assert_eq!(m.len(), 4, "result map must have exactly 4 keys");
        let tol = 1e-12_f64;
        assert_rel_close(
            scalar_si(&m[&Value::String("nominal_gap".into())]),
            0.008,
            tol,
            "nominal_gap",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_max".into())]),
            0.00835,
            tol,
            "worst_case_max",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_min".into())]),
            0.00765,
            tol,
            "worst_case_min",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_band".into())]),
            3.5e-4,
            tol,
            "worst_case_band",
        );
    }

    #[test]
    fn worst_case_inv4_sign_flip_negates_nominal_gap_band_unchanged() {
        // INV-4: flip all signs → nominal_gap negates, worst_case_band unchanged.
        //   Flipped: c1(-1), c2(+1), c3(-1)
        //   gap_nominal = -0.010 + 0.005 - 0.003 = -0.008 m
        //   worst_case_band remains 3.5e-4 m (sum_plus / sum_minus unchanged)
        let c1 =
            eval_stackup("contributor", &[len(0.010), len(0.0001), Value::Int(-1)]).unwrap();
        let c2 =
            eval_stackup("contributor", &[len(0.005), len(0.00005), Value::Int(1)]).unwrap();
        let c3 =
            eval_stackup("contributor", &[len(0.003), len(0.0002), Value::Int(-1)]).unwrap();
        let flipped = Value::List(vec![c1, c2, c3]);
        let m = expect_map(eval_stackup("stackup_worst_case", &[flipped]));
        let tol = 1e-12_f64;
        assert_rel_close(
            scalar_si(&m[&Value::String("nominal_gap".into())]),
            -0.008,
            tol,
            "nominal_gap flipped",
        );
        assert_rel_close(
            scalar_si(&m[&Value::String("worst_case_band".into())]),
            3.5e-4,
            tol,
            "band unchanged after flip",
        );
    }

    #[test]
    fn worst_case_inv5_zero_tol_band_is_zero() {
        // INV-5: all tolerances = 0 → band=0, max==min==nominal_gap.
        let c1 = eval_stackup("contributor", &[len(0.010), len(0.0), Value::Int(1)]).unwrap();
        let c2 = eval_stackup("contributor", &[len(0.005), len(0.0), Value::Int(-1)]).unwrap();
        let zero_tol_chain = Value::List(vec![c1, c2]);
        let m = expect_map(eval_stackup("stackup_worst_case", &[zero_tol_chain]));
        let gap  = scalar_si(&m[&Value::String("nominal_gap".into())]);
        let band = scalar_si(&m[&Value::String("worst_case_band".into())]);
        let max  = scalar_si(&m[&Value::String("worst_case_max".into())]);
        let min  = scalar_si(&m[&Value::String("worst_case_min".into())]);
        assert_eq!(band, 0.0, "zero-tol: band must be 0.0");
        assert_eq!(max, gap,  "zero-tol: max == nominal_gap");
        assert_eq!(min, gap,  "zero-tol: min == nominal_gap");
    }

    #[test]
    fn worst_case_inv5_empty_chain_returns_undef() {
        assert!(
            eval_stackup("stackup_worst_case", &[Value::List(vec![])]).unwrap().is_undef(),
            "empty chain must return Undef"
        );
    }

    #[test]
    fn worst_case_inv6_non_length_field_returns_undef() {
        // A contributor map whose `nominal` is a FORCE scalar must yield Undef.
        use std::collections::BTreeMap;
        let mut bad_m: BTreeMap<Value, Value> = BTreeMap::new();
        bad_m.insert(
            Value::String("nominal".into()),
            Value::Scalar { si_value: 10.0, dimension: DimensionVector::FORCE },
        );
        bad_m.insert(Value::String("plus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("minus_tol".into()), len(0.0001));
        bad_m.insert(Value::String("sign".into()), Value::Int(1));
        bad_m.insert(
            Value::String("distribution".into()),
            Value::Enum { type_name: "Distribution".into(), variant: "Normal".into() },
        );
        let bad_chain = Value::List(vec![Value::Map(bad_m)]);
        assert!(
            eval_stackup("stackup_worst_case", &[bad_chain]).unwrap().is_undef(),
            "non-LENGTH nominal must return Undef"
        );
    }

    #[test]
    fn worst_case_malformed_inputs_return_undef() {
        use std::collections::BTreeMap;
        let nom = len(0.010);
        let tol = len(0.0001);

        // (a) args[0] is not a List
        assert!(
            eval_stackup("stackup_worst_case", &[nom.clone()]).unwrap().is_undef(),
            "non-List arg[0] must be Undef"
        );
        assert!(
            eval_stackup("stackup_worst_case", &[Value::Int(1)]).unwrap().is_undef(),
            "Int arg[0] must be Undef"
        );

        // (b) List element is not a Map
        let not_map = Value::List(vec![nom.clone()]);
        assert!(
            eval_stackup("stackup_worst_case", &[not_map]).unwrap().is_undef(),
            "non-Map element must be Undef"
        );

        // (c) Contributor map missing `sign` key
        let mut no_sign: BTreeMap<Value, Value> = BTreeMap::new();
        no_sign.insert(Value::String("nominal".into()), nom.clone());
        no_sign.insert(Value::String("plus_tol".into()), tol.clone());
        no_sign.insert(Value::String("minus_tol".into()), tol.clone());
        let missing_sign_chain = Value::List(vec![Value::Map(no_sign)]);
        assert!(
            eval_stackup("stackup_worst_case", &[missing_sign_chain]).unwrap().is_undef(),
            "missing sign key must be Undef"
        );

        // (d) sign = Int(0) is invalid
        let mut zero_sign: BTreeMap<Value, Value> = BTreeMap::new();
        zero_sign.insert(Value::String("nominal".into()), nom.clone());
        zero_sign.insert(Value::String("plus_tol".into()), tol.clone());
        zero_sign.insert(Value::String("minus_tol".into()), tol.clone());
        zero_sign.insert(Value::String("sign".into()), Value::Int(0));
        let zero_sign_chain = Value::List(vec![Value::Map(zero_sign)]);
        assert!(
            eval_stackup("stackup_worst_case", &[zero_sign_chain]).unwrap().is_undef(),
            "sign=0 must be Undef"
        );
    }

    #[test]
    fn worst_case_arity_returns_undef() {
        // 0 args → Undef
        assert!(
            eval_stackup("stackup_worst_case", &[]).unwrap().is_undef(),
            "0 args must be Undef"
        );
        // 2 args → Undef
        let chain = golden_chain();
        assert!(
            eval_stackup("stackup_worst_case", &[chain.clone(), chain]).unwrap().is_undef(),
            "2 args must be Undef"
        );
    }
}
