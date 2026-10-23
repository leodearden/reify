#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use reify_core::{Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, CompiledExprKind};
    use reify_test_support::parse_and_compile_with_stdlib;

    use super::direct_sub_instances;

    const MINI_SOURCE: &str = r#"
structure def Inner {
    let body = box(1mm, 1mm, 1mm)
}

structure def MiniChild {
    sub inner = Inner()
    let body = box(10mm, 20mm, 30mm)
    let v = volume(body)
}

structure def MiniParent {
    sub c = MiniChild()
    sub cs : List<MiniChild>
    constraint cs.count == 2
}
"#;

    fn compile_mini() -> reify_compiler::CompiledModule {
        parse_and_compile_with_stdlib(MINI_SOURCE)
    }

    fn mini_parent(compiled: &reify_compiler::CompiledModule) -> &reify_compiler::TopologyTemplate {
        reify_compiler::find_template(&compiled.templates, "MiniParent")
            .expect("MiniParent template must compile")
    }

    fn value_ref_ids(expr: &CompiledExpr) -> BTreeSet<(String, String)> {
        let mut ids = BTreeSet::new();
        expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(id) = &node.kind {
                ids.insert((id.entity.to_string(), id.member.to_string()));
            }
        });
        ids
    }

    #[test]
    fn direct_sub_instances_yields_only_non_collection_subs() {
        let compiled = compile_mini();
        let parent = mini_parent(&compiled);

        let instances: Vec<_> = direct_sub_instances(parent, &compiled.templates).collect();

        assert_eq!(
            instances.len(),
            1,
            "only the non-collection sub `c` is an instance"
        );
        let instance = &instances[0];
        assert_eq!(instance.child_template().name, "MiniChild");
        assert_eq!(
            instance.scoped_id("v"),
            ValueCellId::new("MiniParent.c", "v")
        );
    }

    #[test]
    fn direct_sub_instances_skips_a_sub_whose_template_is_not_in_scope() {
        let compiled = compile_mini();
        let parent = mini_parent(&compiled);
        let only_parent = vec![parent.clone()];

        assert_eq!(direct_sub_instances(parent, &only_parent).count(), 0);
    }

    #[test]
    fn rescope_maps_only_the_child_templates_own_refs() {
        let compiled = compile_mini();
        let parent = mini_parent(&compiled);
        let instance = direct_sub_instances(parent, &compiled.templates)
            .next()
            .expect("MiniParent has one direct instance");

        let expr = CompiledExpr::binop(
            BinOp::Add,
            CompiledExpr::value_ref(ValueCellId::new("MiniChild", "body"), Type::Geometry),
            CompiledExpr::value_ref(ValueCellId::new("Other", "x"), Type::Geometry),
            Type::Geometry,
        );

        let rescoped = instance.rescope(&expr);

        let expected: BTreeSet<(String, String)> = [
            ("MiniParent.c".to_string(), "body".to_string()),
            ("Other".to_string(), "x".to_string()),
        ]
        .into_iter()
        .collect();
        assert_eq!(value_ref_ids(&rescoped), expected);
    }
}
