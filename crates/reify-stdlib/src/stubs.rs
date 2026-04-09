use reify_types::Value;

pub(crate) fn dispatch(name: &str, _args: &[Value]) -> Option<Value> {
    match name {
        // --- Determinacy predicates (stubs) ---
        // These predicates inspect DeterminacyState which is tracked in the Engine's
        // snapshot, not in Value itself. Like sample(), the actual behavior is
        // intercepted at the eval layer (reify-expr/reify-eval) where snapshot state
        // is available. These stubs serve as documentation and fallback.
        "determined" => Some(Value::Undef),
        "undetermined" => Some(Value::Undef),
        "constrained" => Some(Value::Undef),
        "partially_determined" => Some(Value::Undef),

        // --- Field operations (stubs) ---
        // These are handled by reify-expr's eval_expr FunctionCall interceptor
        // for actual lambda application; the stdlib entries serve as documentation
        // and fallback for direct stdlib calls.
        "sample" => Some(Value::Undef),     // Requires EvalContext for lambda application
        "gradient" => Some(Value::Undef),   // Numeric differentiation not yet implemented
        "divergence" => Some(Value::Undef), // Numeric differentiation not yet implemented
        "curl" => Some(Value::Undef),       // Numeric differentiation not yet implemented

        _ => None,
    }
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
