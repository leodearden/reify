//! Tolerance stack-up builtins: Contributor value-shape builders.
//! T1 Phase 1 — math arms in T2/T5.

use reify_ir::Value;

/// Evaluate a tolerance stack-up builtin by name.
///
/// Returns `Some(value)` if the name is a recognised stack-up function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_stackup(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
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
