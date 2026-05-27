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
    if args.len() != 2 {
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
    let nominal = Value::Scalar { si_value: nominal_si, dimension: DimensionVector::LENGTH };
    let tol = Value::Scalar { si_value: tol_si, dimension: DimensionVector::LENGTH };
    make_contributor_map(nominal, tol.clone(), tol, 1, "Normal")
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
