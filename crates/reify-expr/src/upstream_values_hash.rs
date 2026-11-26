#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, CompiledExprKind, Value};

    fn int(n: i64) -> CompiledExpr {
        CompiledExpr::literal(Value::Int(n), Type::Int)
    }

    fn literal_value(expr: &CompiledExpr) -> Value {
        assert!(
            !expr.contains_cross_sub_geometry_ref(),
            "the fold must never evaluate an arg holding a CrossSubGeometryRef"
        );
        match &expr.kind {
            CompiledExprKind::Literal(v) => v.clone(),
            _ => Value::Undef,
        }
    }

    fn cross_sub_ref() -> CompiledExpr {
        CompiledExpr::cross_sub_geometry_ref(
            ValueCellId::new("Parent.sub", "body"),
            Type::Geometry,
        )
    }

    fn hash(args: &[(String, CompiledExpr)]) -> [u8; 32] {
        realization_upstream_values_hash(args, literal_value)
    }

    #[test]
    fn same_args_under_same_evaluator_hash_equal() {
        let args = vec![("w".to_string(), int(1)), ("h".to_string(), int(2))];
        assert_eq!(hash(&args), hash(&args.clone()));
    }

    #[test]
    fn changing_an_arg_value_or_name_changes_the_hash() {
        let base = vec![("w".to_string(), int(1)), ("h".to_string(), int(2))];
        let other_value = vec![("w".to_string(), int(1)), ("h".to_string(), int(3))];
        let other_name = vec![("w".to_string(), int(1)), ("d".to_string(), int(2))];
        assert_ne!(hash(&base), hash(&other_value));
        assert_ne!(hash(&base), hash(&other_name));
    }

    #[test]
    fn arg_holding_a_cross_sub_geometry_ref_is_skipped_unevaluated() {
        let without = vec![("w".to_string(), int(1))];
        let top_level = vec![
            ("w".to_string(), int(1)),
            ("target".to_string(), cross_sub_ref()),
        ];
        let nested = vec![
            (
                "angle".to_string(),
                CompiledExpr::binop(BinOp::Gt, cross_sub_ref(), int(4), Type::Bool),
            ),
            ("w".to_string(), int(1)),
        ];
        assert_eq!(hash(&top_level), hash(&without));
        assert_eq!(hash(&nested), hash(&without));
    }
}
