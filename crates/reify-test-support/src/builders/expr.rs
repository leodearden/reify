use std::sync::Arc;

use reify_core::{ContentHash, DimensionVector, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, FieldSourceKind, ResolvedFunction, TAG_CONDITIONAL, TAG_FUNCTION_CALL, TAG_USER_FUNCTION_CALL, UnOp, Value};

// --- Expression builders ---

/// Create a literal expression from a value, inferring the type.
///
/// Supports most Value variants including M5 types (Enum, List, Set, Map, Option,
/// Lambda, Field). For empty collections, element/value type defaults to `Real`;
/// `Option(None)` defaults to `Bool`. Use [`try_infer_type()`] on the value if
/// you need to detect genuinely ambiguous cases before constructing an expression.
///
/// **Panics** for Frame, Transform, Tensor, and Matrix — their types cannot be
/// inferred from the value alone. Use [`literal_frame`], [`literal_transform`],
/// or `CompiledExpr::literal(value, type)` directly.
pub fn literal(v: Value) -> CompiledExpr {
    let ty = v.infer_type();
    CompiledExpr::literal(v, ty)
}

/// Create a literal Frame expression with explicit dimensionality.
///
/// Use this instead of [`literal`] for Frame values, since Frame dimensionality
/// cannot be inferred from the value alone.
pub fn literal_frame(v: Value, dims: usize) -> CompiledExpr {
    CompiledExpr::literal(v, Type::Frame(dims))
}

/// Create a literal Transform expression with explicit dimensionality.
///
/// Use this instead of [`literal`] for Transform values, since Transform
/// dimensionality cannot be inferred from the value alone.
pub fn literal_transform(v: Value, dims: usize) -> CompiledExpr {
    CompiledExpr::literal(v, Type::Transform(dims))
}

/// Create a value reference expression.
pub fn value_ref(entity: &str, member: &str) -> CompiledExpr {
    // Default to length type; callers can use value_ref_typed for specifics
    CompiledExpr::value_ref(
        ValueCellId::new(entity, member),
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    )
}

/// Create a value reference expression with an explicit type.
pub fn value_ref_typed(entity: &str, member: &str, ty: Type) -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new(entity, member), ty)
}

/// Create a binary operation expression.
pub fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    let result_type = infer_binop_type(op, &left.result_type, &right.result_type);
    CompiledExpr::binop(op, left, right, result_type)
}

/// Create a > comparison.
pub fn gt(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Gt, left, right, Type::Bool)
}

/// Create a < comparison.
pub fn lt(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Lt, left, right, Type::Bool)
}

/// Create a >= comparison.
pub fn ge(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Ge, left, right, Type::Bool)
}

/// Create a <= comparison.
pub fn le(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Le, left, right, Type::Bool)
}

/// Create an == comparison.
pub fn eq(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool)
}

/// Create a != comparison.
pub fn ne(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Ne, left, right, Type::Bool)
}

/// Create an AND expression.
pub fn and(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::And, left, right, Type::Bool)
}

/// Create an OR expression.
pub fn or(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Or, left, right, Type::Bool)
}

/// Create a NOT expression.
pub fn not(operand: CompiledExpr) -> CompiledExpr {
    CompiledExpr::unop(UnOp::Not, operand, Type::Bool)
}

/// Create a negation expression.
pub fn neg(operand: CompiledExpr) -> CompiledExpr {
    let ty = operand.result_type.clone();
    CompiledExpr::unop(UnOp::Neg, operand, ty)
}

/// Create a list literal expression, inferring element type from the first element.
///
/// Panics if `elements` is empty — use `CompiledExpr::list_literal` directly for empty lists.
pub fn list_expr(elements: Vec<CompiledExpr>) -> CompiledExpr {
    assert!(
        !elements.is_empty(),
        "list_expr: use CompiledExpr::list_literal for empty lists"
    );
    let elem_ty = elements[0].result_type.clone();
    let result_type = Type::List(Box::new(elem_ty));
    CompiledExpr::list_literal(elements, result_type)
}

/// Create a set literal expression, inferring element type from the first element.
///
/// Panics if `elements` is empty — use `CompiledExpr::set_literal` directly for empty sets.
pub fn set_expr(elements: Vec<CompiledExpr>) -> CompiledExpr {
    assert!(
        !elements.is_empty(),
        "set_expr: use CompiledExpr::set_literal for empty sets"
    );
    let elem_ty = elements[0].result_type.clone();
    let result_type = Type::Set(Box::new(elem_ty));
    CompiledExpr::set_literal(elements, result_type)
}

/// Create a map literal expression, inferring key/value types from the first entry.
///
/// Panics if `entries` is empty — use `CompiledExpr::map_literal` directly for empty maps.
pub fn map_expr(entries: Vec<(CompiledExpr, CompiledExpr)>) -> CompiledExpr {
    assert!(
        !entries.is_empty(),
        "map_expr: use CompiledExpr::map_literal for empty maps"
    );
    let key_ty = entries[0].0.result_type.clone();
    let val_ty = entries[0].1.result_type.clone();
    let result_type = Type::Map(Box::new(key_ty), Box::new(val_ty));
    CompiledExpr::map_literal(entries, result_type)
}

/// Create a conditional expression. Result type is taken from `then_branch`.
pub fn conditional_expr(
    condition: CompiledExpr,
    then_branch: CompiledExpr,
    else_branch: CompiledExpr,
) -> CompiledExpr {
    let result_type = then_branch.result_type.clone();
    let content_hash = ContentHash::of(&[TAG_CONDITIONAL])
        .combine(condition.content_hash)
        .combine(then_branch.content_hash)
        .combine(else_branch.content_hash);
    CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        },
        result_type,
        content_hash,
    }
}

/// Create a standard function call expression with a fully-qualified function name.
pub fn fn_call(
    name: &str,
    qualified_name: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    let mut content_hash =
        ContentHash::of(&[TAG_FUNCTION_CALL]).combine(ContentHash::of_str(qualified_name));
    for arg in &args {
        content_hash = content_hash.combine(arg.content_hash);
    }
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: qualified_name.to_string(),
            },
            args,
        },
        result_type,
        content_hash,
    }
}

/// Create a user-defined function call expression.
pub fn user_fn_call(
    function_name: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    let mut content_hash =
        ContentHash::of(&[TAG_USER_FUNCTION_CALL]).combine(ContentHash::of_str(function_name));
    for arg in &args {
        content_hash = content_hash.combine(arg.content_hash);
    }
    CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: function_name.to_string(),
            args,
        },
        result_type,
        content_hash,
    }
}

/// Create a method call expression.
pub fn method_call_expr(
    object: CompiledExpr,
    method: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    CompiledExpr::method_call(object, method.to_string(), args, result_type)
}

/// Create a field `sample` call: `std::field::sample(field, point) -> result_type`.
pub fn sample_call(field: CompiledExpr, point: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call(
        "sample",
        "std::field::sample",
        vec![field, point],
        result_type,
    )
}

/// Create a field `gradient` call: `std::field::gradient(field) -> result_type`.
pub fn gradient_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("gradient", "std::field::gradient", vec![field], result_type)
}

/// Create a field `divergence` call: `std::field::divergence(field) -> result_type`.
pub fn divergence_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call(
        "divergence",
        "std::field::divergence",
        vec![field],
        result_type,
    )
}

/// Create a field `curl` call: `std::field::curl(field) -> result_type`.
pub fn curl_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("curl", "std::field::curl", vec![field], result_type)
}

/// Create a `Value::Field` literal expression with explicit domain, codomain, source, and lambda.
///
/// Use this instead of [`literal`] for Field values — the type cannot be inferred from the
/// value alone; domain and codomain must be provided explicitly.
pub fn field_literal_expr(
    domain: Type,
    codomain: Type,
    source: FieldSourceKind,
    lambda: Value,
) -> CompiledExpr {
    let value = Value::Field {
        domain_type: domain.clone(),
        codomain_type: codomain.clone(),
        source,
        lambda: Arc::new(lambda),
    };
    let result_type = Type::Field {
        domain: Box::new(domain),
        codomain: Box::new(codomain),
    };
    CompiledExpr::literal(value, result_type)
}

/// Create a field `laplacian` call: `std::field::laplacian(field) -> result_type`.
pub fn laplacian_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call(
        "laplacian",
        "std::field::laplacian",
        vec![field],
        result_type,
    )
}

/// Create a lambda expression with named parameters.
///
/// Generates param IDs with `ValueCellId::new("__lambda", name)` for each parameter.
pub fn lambda_expr(params: Vec<(&str, Type)>, body: CompiledExpr) -> CompiledExpr {
    let param_types: Vec<Type> = params.iter().map(|(_, ty)| ty.clone()).collect();
    let return_type = body.result_type.clone();
    let result_type = Type::Function {
        params: param_types,
        return_type: Box::new(return_type),
    };
    let param_ids: Vec<ValueCellId> = params
        .iter()
        .map(|(name, _)| ValueCellId::new("__lambda", *name))
        .collect();
    let compiled_params: Vec<(String, Option<Type>)> = params
        .into_iter()
        .map(|(name, ty)| (name.to_string(), Some(ty)))
        .collect();
    CompiledExpr::lambda(compiled_params, param_ids, body, vec![], result_type)
}

fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    match op {
        BinOp::Eq
        | BinOp::Ne
        | BinOp::Lt
        | BinOp::Le
        | BinOp::Gt
        | BinOp::Ge
        | BinOp::And
        | BinOp::Or => Type::Bool,
        BinOp::Add | BinOp::Sub => left.clone(), // same dimension required
        BinOp::Mul => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => Type::Scalar {
                dimension: ld.mul(rd),
            },
            (Type::Scalar { .. }, _) | (_, Type::Scalar { .. }) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Div => match (left, right) {
            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd }) => {
                let result = ld.div(rd);
                if result.is_dimensionless() {
                    Type::Real
                } else {
                    Type::Scalar { dimension: result }
                }
            }
            (Type::Scalar { .. }, _) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::CompiledExprKind;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn literal_enum_produces_enum_type() {
        let expr = literal(Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        });
        assert_eq!(expr.result_type, Type::Enum("Color".to_string()));
        assert!(matches!(
            expr.kind,
            CompiledExprKind::Literal(Value::Enum { .. })
        ));
    }

    #[test]
    fn literal_list_produces_list_type() {
        let expr = literal(Value::List(vec![Value::Int(1), Value::Int(2)]));
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Int)));
        assert!(matches!(
            expr.kind,
            CompiledExprKind::Literal(Value::List(_))
        ));
    }

    #[test]
    fn literal_set_produces_set_type() {
        let mut s = BTreeSet::new();
        s.insert(Value::Int(1));
        let expr = literal(Value::Set(s));
        assert_eq!(expr.result_type, Type::Set(Box::new(Type::Int)));
    }

    #[test]
    fn literal_map_produces_map_type() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("k".into()), Value::Int(1));
        let expr = literal(Value::Map(m));
        assert_eq!(
            expr.result_type,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn literal_option_some_produces_option_type() {
        let expr = literal(Value::Option(Some(Box::new(Value::Int(1)))));
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Int)));
    }

    #[test]
    fn literal_option_none_produces_option_bool_fallback() {
        let expr = literal(Value::Option(None));
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Bool)));
    }

    #[test]
    fn literal_empty_list_uses_real_fallback() {
        let expr = literal(Value::List(vec![]));
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Real)));
    }

    #[test]
    fn literal_empty_set_uses_real_fallback() {
        let expr = literal(Value::Set(BTreeSet::new()));
        assert_eq!(
            expr.result_type,
            Type::Set(Box::new(Type::Real)),
            "empty Set should produce Set(Real)"
        );
    }

    #[test]
    fn literal_empty_map_uses_string_real_fallback() {
        let expr = literal(Value::Map(BTreeMap::new()));
        assert_eq!(
            expr.result_type,
            Type::Map(Box::new(Type::String), Box::new(Type::Real)),
            "empty Map should produce Map(String, Real)"
        );
    }

    // --- Collection expression builder tests ---

    #[test]
    fn list_expr_produces_list_literal_with_correct_type() {
        let e1 = literal(Value::Int(1));
        let e2 = literal(Value::Int(2));
        let expr = list_expr(vec![e1, e2]);
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::ListLiteral(_)));
    }

    #[test]
    fn set_expr_produces_set_literal_with_correct_type() {
        let e1 = literal(Value::Int(1));
        let expr = set_expr(vec![e1]);
        assert_eq!(expr.result_type, Type::Set(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::SetLiteral(_)));
    }

    #[test]
    fn map_expr_produces_map_literal_with_correct_type() {
        let k = literal(Value::String("key".into()));
        let v = literal(Value::Int(99));
        let expr = map_expr(vec![(k, v)]);
        assert_eq!(
            expr.result_type,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
        assert!(matches!(expr.kind, CompiledExprKind::MapLiteral(_)));
    }

    // --- conditional_expr, fn_call, user_fn_call tests ---

    #[test]
    fn conditional_expr_uses_then_branch_type() {
        let cond = literal(Value::Bool(true));
        let then_b = literal(Value::Int(1));
        let else_b = literal(Value::Int(2));
        let expr = conditional_expr(cond, then_b, else_b);
        assert_eq!(expr.result_type, Type::Int);
        assert!(matches!(expr.kind, CompiledExprKind::Conditional { .. }));
    }

    #[test]
    fn fn_call_produces_function_call_with_resolved_function() {
        let arg = literal(Value::Real(1.0));
        let expr = fn_call("sin", "std::math::sin", vec![arg], Type::Real);
        assert_eq!(expr.result_type, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.name, "sin");
            assert_eq!(function.qualified_name, "std::math::sin");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected FunctionCall kind");
        }
    }

    #[test]
    fn user_fn_call_produces_user_function_call() {
        let arg = literal(Value::Int(1));
        let expr = user_fn_call("my_func", vec![arg], Type::Int);
        assert_eq!(expr.result_type, Type::Int);
        if let CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } = &expr.kind
        {
            assert_eq!(function_name, "my_func");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected UserFunctionCall kind");
        }
    }

    // --- method_call_expr and lambda_expr tests ---

    #[test]
    fn method_call_expr_produces_method_call_kind() {
        let obj = list_expr(vec![literal(Value::Int(1))]);
        let expr = method_call_expr(obj, "count", vec![], Type::Int);
        assert_eq!(expr.result_type, Type::Int);
        if let CompiledExprKind::MethodCall { method, args, .. } = &expr.kind {
            assert_eq!(method, "count");
            assert!(args.is_empty());
        } else {
            panic!("expected MethodCall kind");
        }
    }

    #[test]
    fn lambda_expr_produces_lambda_with_function_type() {
        let body = literal(Value::Real(1.0));
        let expr = lambda_expr(vec![("x", Type::Real)], body);
        assert_eq!(
            expr.result_type,
            Type::Function {
                params: vec![Type::Real],
                return_type: Box::new(Type::Real),
            }
        );
        if let CompiledExprKind::Lambda {
            params, param_ids, ..
        } = &expr.kind
        {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "x");
            assert_eq!(param_ids.len(), 1);
            assert_eq!(param_ids[0], ValueCellId::new("__lambda", "x"));
        } else {
            panic!("expected Lambda kind");
        }
    }

    // --- Field operation expression helpers tests ---

    #[test]
    fn sample_call_produces_function_call_with_std_field_sample() {
        let field_e = literal(Value::Real(0.0)); // dummy field expr
        let point_e = literal(Value::Real(1.0));
        let expr = sample_call(field_e, point_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::sample");
            assert_eq!(args.len(), 2);
        } else {
            panic!("expected FunctionCall kind for sample_call");
        }
        assert_eq!(expr.result_type, Type::Real);
    }

    #[test]
    fn gradient_call_produces_function_call_with_std_field_gradient() {
        let field_e = literal(Value::Real(0.0));
        let expr = gradient_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::gradient");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected FunctionCall kind for gradient_call");
        }
    }

    #[test]
    fn divergence_call_produces_function_call_with_std_field_divergence() {
        let field_e = literal(Value::Real(0.0));
        let expr = divergence_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, .. } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::divergence");
        } else {
            panic!("expected FunctionCall kind for divergence_call");
        }
    }

    #[test]
    fn curl_call_produces_function_call_with_std_field_curl() {
        let field_e = literal(Value::Real(0.0));
        let expr = curl_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, .. } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::curl");
        } else {
            panic!("expected FunctionCall kind for curl_call");
        }
    }

    // --- laplacian_call tests (step 17) ---

    #[test]
    fn laplacian_call_produces_function_call_with_std_field_laplacian() {
        let field_e = literal(Value::Real(0.0));
        let expr = laplacian_call(field_e, Type::Real);
        assert_eq!(expr.result_type, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.name, "laplacian");
            assert_eq!(function.qualified_name, "std::field::laplacian");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected FunctionCall kind for laplacian_call");
        }
    }

    // --- field_literal_expr tests (step 15) ---

    #[test]
    fn field_literal_expr_produces_literal_with_field_type() {
        use reify_ir::FieldSourceKind;
        let domain = Type::Geometry;
        let codomain = Type::Real;
        let lambda = Value::Undef;
        let expr = field_literal_expr(
            domain.clone(),
            codomain.clone(),
            FieldSourceKind::Analytical,
            lambda,
        );
        assert_eq!(
            expr.result_type,
            Type::Field {
                domain: Box::new(domain),
                codomain: Box::new(codomain),
            }
        );
        assert!(matches!(
            expr.kind,
            CompiledExprKind::Literal(Value::Field { .. })
        ));
    }

    #[test]
    fn field_literal_expr_wraps_lambda() {
        use reify_ir::FieldSourceKind;
        let domain = Type::Geometry;
        let codomain = Type::Real;
        let lambda = Value::Int(42);
        let expr = field_literal_expr(
            domain.clone(),
            codomain.clone(),
            FieldSourceKind::Sampled,
            lambda.clone(),
        );
        if let CompiledExprKind::Literal(Value::Field {
            source,
            lambda: lam,
            ..
        }) = &expr.kind
        {
            assert_eq!(*source, FieldSourceKind::Sampled);
            assert_eq!(**lam, lambda);
        } else {
            panic!("expected Literal(Value::Field)");
        }
    }

    #[test]
    fn literal_frame_helper_produces_frame3_type() {
        let frame_value = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            basis: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
        };
        let expr = literal_frame(frame_value, 3);
        assert_eq!(expr.result_type, Type::Frame(3));
        assert!(matches!(
            expr.kind,
            CompiledExprKind::Literal(Value::Frame { .. })
        ));
    }

    #[test]
    fn literal_transform_helper_produces_transform3_type() {
        let transform_value = Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
        };
        let expr = literal_transform(transform_value, 3);
        assert_eq!(expr.result_type, Type::Transform(3));
        assert!(matches!(
            expr.kind,
            CompiledExprKind::Literal(Value::Transform { .. })
        ));
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Transform")]
    fn literal_transform_value_panics() {
        let transform_value = Value::Transform {
            rotation: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            translation: Box::new(Value::Vector(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
        };
        literal(transform_value);
    }

    #[test]
    #[should_panic(expected = "infer_type() cannot infer Frame")]
    fn literal_frame_value_panics() {
        let frame_value = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            basis: Box::new(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
        };
        literal(frame_value);
    }
}
