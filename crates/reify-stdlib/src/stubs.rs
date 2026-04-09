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

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use reify_types::{FieldSourceKind, Type, Value};

    // --- Determinacy predicate stubs ---

    #[test]
    fn determined_stub_returns_undef() {
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "determined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "undetermined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "constrained stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "partially_determined stub should return Undef, got {:?}",
            result
        );
    }

    // --- Field operation stubs ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        let field = Value::Field {
            domain_type: Type::StructureRef("Point3".into()),
            codomain_type: Type::length(),
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(
            result.is_undef(),
            "gradient stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: Type::StructureRef("Point3".into()),
            codomain_type: Type::StructureRef("Vector3".into()),
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(
            result.is_undef(),
            "divergence stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: Type::StructureRef("Point3".into()),
            codomain_type: Type::StructureRef("Vector3".into()),
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(
            result.is_undef(),
            "curl stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: Type::StructureRef("Point3".into()),
            codomain_type: Type::length(),
            source: FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
    }
}
