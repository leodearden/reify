pub mod annotation;
pub mod constraint;
pub mod expr;
pub mod field;
pub mod module;
pub mod pragma;
pub mod purpose;
pub mod topology;
pub mod trait_def;

pub use annotation::*;
pub use constraint::*;
pub use expr::*;
pub use field::*;
pub use module::*;
pub use pragma::*;
pub use purpose::*;
pub use topology::*;
pub use trait_def::*;

#[cfg(test)]
mod reexport_contract_tests {
    //! Guard tests: verify that core builder functions remain accessible
    //! via the `crate::builders` module path after submodule extraction.
    use crate::builders::{
        CompiledFieldBuilder, CompiledModuleBuilder, CompiledPurposeBuilder, CompiledTraitBuilder,
        TopologyTemplateBuilder, TraitDefBuilder, binop, conditional_expr, eq, equality_constraint,
        fn_call, ge, gt, lambda_expr, le, list_expr, literal, lt, map_expr, method_call_expr, ne,
        neg, not, range_constraint, sample_call, set_expr, user_fn_call, value_ref,
        value_ref_typed,
    };
    use reify_core::Type;
    use reify_ir::{BinOp, Value};

    #[test]
    fn expr_builders_accessible_via_module_path() {
        // Compilation-only: if this compiles, the re-export contract holds.
        let _ = literal(Value::Int(1));
        let _ = value_ref("E", "m");
        let _ = value_ref_typed("E", "m", Type::dimensionless_scalar());
        let a = literal(Value::Int(1));
        let b = literal(Value::Int(2));
        let _ = binop(BinOp::Add, a.clone(), b.clone());
        let _ = gt(a.clone(), b.clone());
        let _ = lt(a.clone(), b.clone());
        let _ = ge(a.clone(), b.clone());
        let _ = le(a.clone(), b.clone());
        let _ = eq(a.clone(), b.clone());
        let _ = ne(a.clone(), b.clone());
        let _ = not(literal(Value::Bool(true)));
        let _ = neg(literal(Value::Int(1)));
        let _ = list_expr(vec![literal(Value::Int(1))]);
        let _ = set_expr(vec![literal(Value::Int(1))]);
        let _ = map_expr(vec![(
            literal(Value::String("k".into())),
            literal(Value::Int(1)),
        )]);
        let _ = conditional_expr(
            literal(Value::Bool(true)),
            literal(Value::Int(1)),
            literal(Value::Int(2)),
        );
        let _ = fn_call("f", "q::f", vec![], Type::dimensionless_scalar());
        let _ = user_fn_call("f", vec![], Type::dimensionless_scalar());
        let _ = method_call_expr(literal(Value::Int(1)), "m", vec![], Type::Int);
        let _ = sample_call(
            literal(Value::Real(0.0)),
            literal(Value::Real(1.0)),
            Type::dimensionless_scalar(),
        );
        let _ = lambda_expr(vec![("x", Type::dimensionless_scalar())], literal(Value::Real(1.0)));
    }

    #[test]
    fn constraint_builders_accessible_via_module_path() {
        let exprs = range_constraint(
            "E",
            "m",
            Type::dimensionless_scalar(),
            literal(Value::Real(0.0)),
            literal(Value::Real(1.0)),
        );
        assert_eq!(exprs.len(), 2);
        assert_eq!(exprs[0].result_type, Type::Bool);
        assert_eq!(exprs[1].result_type, Type::Bool);

        let eq_exprs = equality_constraint("E", "m", Type::dimensionless_scalar(), literal(Value::Real(1.0)));
        assert_eq!(eq_exprs.len(), 1);
        assert_eq!(eq_exprs[0].result_type, Type::Bool);
    }

    #[test]
    fn all_builders_accessible_via_module_path() {
        // Compilation-only: verify all builder structs are accessible
        let _ = TopologyTemplateBuilder::new("T").build();
        let _ = TraitDefBuilder::new("T").build();
        let _ = CompiledTraitBuilder::new("T").build();
        let _ = CompiledFieldBuilder::new("f", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .build();
        let _ = CompiledPurposeBuilder::new("p").build();
        let _ = CompiledModuleBuilder::new(reify_core::ModulePath::new(vec!["t".into()])).build();
    }

    #[test]
    fn annotation_and_pragma_helpers_accessible_via_module_path() {
        use crate::builders::{
            ann_bool, ann_ident, ann_int, ann_real, ann_str, annotation, annotation_with_args,
            field_literal_expr, laplacian_call, pragma, pragma_bare, pragma_bool, pragma_ident,
            pragma_kv, pragma_number, pragma_string, pragma_with_args,
        };
        use reify_core::{DEPRECATED_ANNOTATION, TEST_ANNOTATION};

        // Annotation helpers
        let _ = ann_str("hello");
        let _ = ann_int(42);
        let _ = ann_real(3.125);
        let _ = ann_bool(true);
        let _ = ann_ident("foo");
        let ann = annotation(TEST_ANNOTATION);
        let _ = annotation_with_args(DEPRECATED_ANNOTATION, vec![ann_str("use Foo")]);

        // Pragma helpers
        let _ = pragma_ident("opt_level");
        let _ = pragma_number(2.0);
        let _ = pragma_string("hello");
        let _ = pragma_bool(true);
        let _ = pragma_kv("level", pragma_number(2.0));
        let _ = pragma_bare(pragma_ident("opt"));
        let _ = pragma("inline");
        let _ = pragma_with_args("cfg", vec![pragma_bare(pragma_ident("test"))]);

        // Annotation methods on builders (compile-time check)
        let _ = TopologyTemplateBuilder::new("T")
            .annotation(ann.clone())
            .build();
        let _ = TraitDefBuilder::new("T").annotation(ann.clone()).build();
        let _ = CompiledTraitBuilder::new("T")
            .annotation(ann.clone())
            .build();
        let _ = CompiledFieldBuilder::new("f", Type::Geometry, Type::dimensionless_scalar())
            .imported()
            .annotation(ann.clone())
            .build();
        let _ = CompiledPurposeBuilder::new("p")
            .annotation(ann.clone())
            .build();

        // Field expr helpers
        let field_expr = field_literal_expr(
            Type::Geometry,
            Type::dimensionless_scalar(),
            reify_ir::FieldSourceKind::Analytical,
            Value::Bool(false),
        );
        let _ = laplacian_call(field_expr, Type::dimensionless_scalar());
    }
}
