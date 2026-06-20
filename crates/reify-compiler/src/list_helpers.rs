use reify_core::Type;
use reify_ir::CompiledExpr;

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
/// - `generate(Int, (Int) → B)` → `List<B>`
pub(crate) fn infer_list_helper_return_type(
    name: &str,
    compiled_args: &[CompiledExpr],
) -> Option<Type> {
    match name {
        "single" => {
            // single(List<T>) -> T  (task 2698).
            //
            // Unwrap the list element type so downstream cells see T, not
            // List<T>.  Falls through to the generic first-arg fallback
            // (returns None here) when the structural pattern doesn't match
            // (e.g., poisoned type or non-list argument), preserving
            // anti-cascade.
            if let Some(arg) = compiled_args.first()
                && let Type::List(inner) = &arg.result_type
            {
                Some((**inner).clone())
            } else {
                None
            }
        }
        "flat_map" => {
            // flat_map(List<A>, (A) -> List<B>) -> List<B>  (task 2698).
            //
            // Read the lambda's return_type, populated by the Lambda
            // compilation arm at expr.rs:~1741.  The return_type must itself
            // be `List<_>` for this branch to fire — a non-list lambda body
            // (e.g. `flat_map([1, 2], |x| x)`) is a runtime type error
            // (silently propagates as Value::Undef per the task 2698
            // convention) and would yield a misleading non-list cell type if
            // we returned it here.
            //
            // Falls through to the first-arg fallback (returns None here)
            // when the structural pattern doesn't match (poisoned types,
            // wrong arity, second arg not a Function, or lambda body not a
            // List), preserving anti-cascade and ensuring the cell type stays
            // List<_>.
            if compiled_args.len() == 2
                && let Type::Function { return_type, .. } = &compiled_args[1].result_type
                && matches!(**return_type, Type::List(_))
            {
                Some((**return_type).clone())
            } else {
                None
            }
        }
        "generate" => {
            // generate(Int, (Int) -> B) -> List<B>  (task 3994 / structural-query ζ).
            //
            // Apply the lambda over indices 0..n-1 and collect the results; the
            // cell type is `List<B>` where `B` is the lambda's body (return) type,
            // read from `compiled_args[1]`'s `Type::Function` return_type (populated
            // by the Lambda compilation arm at expr.rs:~4374).
            //
            // UNLIKE `flat_map` (which requires the body to itself be a `List<_>`),
            // `generate` wraps ANY body type `B` into `List<B>` — the body is the
            // element, not a sub-list to flatten.
            //
            // Falls through to the first-arg fallback (returns None here) when the
            // structural pattern doesn't match (poisoned type, wrong arity, or
            // second arg not a Function), preserving anti-cascade and keeping the
            // cell type honest.
            if compiled_args.len() == 2
                && let Type::Function { return_type, .. } = &compiled_args[1].result_type
            {
                Some(Type::List(Box::new((**return_type).clone())))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::infer_list_helper_return_type;
    use reify_core::Type;
    use reify_ir::{CompiledExpr, Value};

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
            return_type: Box::new(Type::dimensionless_scalar()),
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

    #[test]
    fn flat_map_too_many_args_returns_none() {
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::List(Box::new(Type::Bool))),
        };
        let args = vec![
            arg(Type::List(Box::new(Type::Int))),
            arg(lambda_ty),
            arg(Type::Int),
        ];
        assert_eq!(infer_list_helper_return_type("flat_map", &args), None);
    }

    // --- generate (task 3994 / structural-query ζ) ---

    #[test]
    fn generate_int_and_lambda_returning_length_returns_list_length() {
        // generate(Int, (Int) -> Length) -> List<Length>: the element type is
        // the lambda's body type verbatim (UNLIKE flat_map, which requires the
        // body to itself be a List).
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::length()),
        };
        let args = vec![arg(Type::Int), arg(lambda_ty)];
        assert_eq!(
            infer_list_helper_return_type("generate", &args),
            Some(Type::List(Box::new(Type::length())))
        );
    }

    #[test]
    fn generate_lambda_returning_int_returns_list_int() {
        // A non-list body type is fine for generate (the cell is List<body>).
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Int),
        };
        let args = vec![arg(Type::Int), arg(lambda_ty)];
        assert_eq!(
            infer_list_helper_return_type("generate", &args),
            Some(Type::List(Box::new(Type::Int)))
        );
    }

    #[test]
    fn generate_non_function_second_arg_returns_none() {
        let args = vec![arg(Type::Int), arg(Type::Int)];
        assert_eq!(infer_list_helper_return_type("generate", &args), None);
    }

    #[test]
    fn generate_wrong_arity_returns_none() {
        let args = vec![arg(Type::Int)];
        assert_eq!(infer_list_helper_return_type("generate", &args), None);
    }

    #[test]
    fn generate_too_many_args_returns_none() {
        let lambda_ty = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::length()),
        };
        let args = vec![arg(Type::Int), arg(lambda_ty), arg(Type::Int)];
        assert_eq!(infer_list_helper_return_type("generate", &args), None);
    }

    #[test]
    fn generate_poisoned_second_arg_returns_none() {
        // A Type::Error (poison) second arg is not a Function -> None (anti-cascade).
        let args = vec![arg(Type::Int), arg(Type::Error)];
        assert_eq!(infer_list_helper_return_type("generate", &args), None);
    }

    // --- unknown name ---

    #[test]
    fn unknown_name_returns_none() {
        let args = vec![arg(Type::List(Box::new(Type::Int)))];
        assert_eq!(infer_list_helper_return_type("take", &args), None);
    }
}
