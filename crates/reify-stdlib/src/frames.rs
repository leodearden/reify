use reify_types::Value;

pub(crate) fn dispatch(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn frames_dispatch_transform3_identity() {
        // Sentinel: once implemented, transform3_identity() → Frame3 identity
        assert!(dispatch("transform3_identity", &[]).is_some());
    }

    #[test]
    fn frames_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
