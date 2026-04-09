use reify_types::Value;
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn numeric_dispatch_abs_int() {
        assert_eq!(dispatch("abs", &[Value::Int(-5)]), Some(Value::Int(5)));
    }

    #[test]
    fn numeric_dispatch_unknown_returns_none() {
        assert!(dispatch("unknown_fn", &[]).is_none());
    }
}
