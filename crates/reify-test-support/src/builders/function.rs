//! Builders for `CompiledFunction`.
//!
//! Variants (`is_pub`, `optimized_target`, ...) are spelled with struct-update
//! syntax: `CompiledFunction { is_pub: true, ..compiled_function(..) }`.

use reify_core::{ContentHash, Type};
use reify_ir::{CompiledExpr, CompiledFnBody, CompiledFunction};

/// Create a `CompiledFunction` whose params carry no defaults and whose return
/// type is the body's result type.
pub fn compiled_function(
    name: &str,
    params: &[(&str, Type)],
    let_bindings: Vec<(String, CompiledExpr)>,
    result_expr: CompiledExpr,
) -> CompiledFunction {
    let params: Vec<(String, Type)> = params
        .iter()
        .map(|(p, ty)| ((*p).to_string(), ty.clone()))
        .collect();
    CompiledFunction {
        name: name.to_string(),
        doc: None,
        is_pub: false,
        param_defaults: CompiledFunction::no_defaults_for(&params),
        params,
        return_type: result_expr.result_type.clone(),
        body: CompiledFnBody {
            let_bindings,
            result_expr,
        },
        content_hash: ContentHash::of(name.as_bytes()),
        annotations: vec![],
        optimized_target: None,
        type_params: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::{literal, value_ref};
    use reify_ir::Value;

    fn real(v: f64) -> CompiledExpr {
        literal(Value::Real(v))
    }

    #[test]
    fn compiled_function_declares_params_in_order_with_no_defaults_and_returns_the_body_type() {
        let body = real(1.0);
        let f = compiled_function(
            "hyp",
            &[("a", Type::dimensionless_scalar()), ("b", Type::length())],
            vec![],
            body.clone(),
        );

        assert_eq!(f.name, "hyp");
        assert_eq!(
            f.params,
            vec![
                ("a".to_string(), Type::dimensionless_scalar()),
                ("b".to_string(), Type::length()),
            ]
        );
        assert_eq!(f.param_defaults.len(), 2);
        assert!(f.param_defaults.iter().all(Option::is_none));
        assert_eq!(f.return_type, body.result_type);
        assert!(f.body.let_bindings.is_empty());
        assert_eq!(f.body.result_expr.content_hash, body.content_hash);
        assert_eq!(f.body.result_expr.result_type, body.result_type);
        assert!(!f.is_pub);
        assert!(f.optimized_target.is_none());
        assert!(f.annotations.is_empty());
        assert!(f.type_params.is_empty());
        assert!(f.doc.is_none());
    }

    #[test]
    fn compiled_function_keeps_let_bindings_in_declaration_order() {
        let e1 = real(2.0);
        let e2 = value_ref("f", "s");
        let f = compiled_function(
            "f",
            &[],
            vec![("s".to_string(), e1.clone()), ("t".to_string(), e2.clone())],
            real(0.0),
        );

        let names: Vec<&str> = f
            .body
            .let_bindings
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(names, ["s", "t"]);
        assert_eq!(f.body.let_bindings[0].1.content_hash, e1.content_hash);
        assert_eq!(f.body.let_bindings[1].1.content_hash, e2.content_hash);
    }

    #[test]
    fn compiled_functions_with_different_names_get_different_content_hashes() {
        let hash_of = |name: &str| -> ContentHash {
            compiled_function(
                name,
                &[("a", Type::dimensionless_scalar())],
                vec![],
                real(1.0),
            )
            .content_hash
        };

        assert_ne!(hash_of("f"), hash_of("g"));
        assert_eq!(hash_of("f"), hash_of("f"));
    }
}
