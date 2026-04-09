use reify_types::Value;

pub(crate) fn dispatch(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn stubs_dispatch_determined() {
        let result = dispatch("determined", &[]);
        assert_eq!(result, Some(Value::Undef), "determined should return Some(Undef)");
    }

    #[test]
    fn stubs_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
