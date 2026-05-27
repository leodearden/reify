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
        _ => return None,
    })
}

fn contributor(args: &[Value]) -> Value {
    if !matches!(args.len(), 2 | 3) {
        return Value::Undef;
    }
    let nominal_si = match validate_dimensioned_scalar(&args[0], DimensionVector::LENGTH) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let tol_si = match validate_dimensioned_scalar(&args[1], DimensionVector::LENGTH) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let sign: i64 = if args.len() == 3 {
        match &args[2] {
            Value::Int(1) => 1,
            Value::Int(-1) => -1,
            _ => return Value::Undef,
        }
    } else {
        1
    };
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let tol = Value::Scalar { si_value: tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, tol.clone(), tol, sign, "Normal")
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
        assert!(eval_stackup("contributor", &[nom.clone()]).unwrap().is_undef());
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
}
