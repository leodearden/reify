use super::*;

/// Infer the return type of a list-helper stdlib call from the compiled
/// argument list.
///
/// Returns `Some(Type)` for the two recognised helpers when their structural
/// pattern matches; `None` otherwise.  The caller's existing `else { fallback
/// }` branch handles both unknown names AND structural-mismatch cases —
/// preserving anti-cascade identically.
///
/// Recognised helpers:
/// - `single(List<T>)` → `T`
/// - `flat_map(List<A>, (A) → List<B>)` → `List<B>`
pub(crate) fn infer_list_helper_return_type(
    _name: &str,
    _compiled_args: &[CompiledExpr],
) -> Option<Type> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a cheap synthetic CompiledExpr with the given result_type.
    fn arg(ty: Type) -> CompiledExpr {
        CompiledExpr::literal(Value::Undef, ty)
    }

    // --- single ---

    #[test]
    fn single_list_int_returns_int() {
        let args = vec![arg(Type::List(Box::new(Type::Int)))];
        assert_eq!(
            infer_list_helper_return_type("single", &args),
            Some(Type::Int)
        );
    }

    #[test]
    fn single_non_list_arg_returns_none() {
        let args = vec![arg(Type::Int)];
        assert_eq!(infer_list_helper_return_type("single", &args), None);
    }

    #[test]
    fn single_no_args_returns_none() {
        assert_eq!(infer_list_helper_return_type("single", &[]), None);
    }

    // --- flat_map ---

    #[test]
    fn flat_map_lambda_returning_list_bool_returns_list_bool() {
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::List(Box::new(Type::Bool))),
        };
        let args = vec![arg(Type::List(Box::new(Type::Int))), arg(lambda_ty)];
        assert_eq!(
            infer_list_helper_return_type("flat_map", &args),
            Some(Type::List(Box::new(Type::Bool)))
        );
    }

    #[test]
    fn flat_map_lambda_returning_non_list_returns_none() {
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Real),
        };
        let args = vec![arg(Type::List(Box::new(Type::Int))), arg(lambda_ty)];
        assert_eq!(infer_list_helper_return_type("flat_map", &args), None);
    }

    #[test]
    fn flat_map_non_function_second_arg_returns_none() {
        let args = vec![arg(Type::List(Box::new(Type::Int))), arg(Type::Int)];
        assert_eq!(infer_list_helper_return_type("flat_map", &args), None);
    }

    #[test]
    fn flat_map_wrong_arity_returns_none() {
        let args = vec![arg(Type::List(Box::new(Type::Int)))];
        assert_eq!(infer_list_helper_return_type("flat_map", &args), None);
    }

    // --- unknown name ---

    #[test]
    fn unknown_name_returns_none() {
        let args = vec![arg(Type::List(Box::new(Type::Int)))];
        assert_eq!(infer_list_helper_return_type("take", &args), None);
    }
}
