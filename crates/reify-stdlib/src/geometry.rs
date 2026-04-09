use reify_types::Value;

pub(crate) fn dispatch(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn geometry_dispatch_plane_xy() {
        let result = dispatch("plane_xy", &[Value::length(1.0)]);
        assert!(result.is_some(), "plane_xy should be handled by geometry dispatch");
        assert!(
            matches!(result, Some(Value::Plane { .. })),
            "plane_xy should return a Plane value"
        );
    }

    #[test]
    fn geometry_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
