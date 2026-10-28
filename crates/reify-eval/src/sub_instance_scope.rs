//! Template → sub-instance rescoping for the build-phase post-processes. A
//! template's direct, non-collection `sub` declarations are each seen as the
//! instance entity `"<parent>.<sub>"` that their scoped value cells live
//! under, and a child template's own `ValueRef`s are rewritten onto it. The
//! scope is one level deep. Collection subs are excluded because they have
//! no single scoped entity.
//!
//! It also owns the compound key under which a parent's `named_steps` hold
//! each sub's (or `let`-bound instance's) realization steps: the seeders write
//! it with [`scoped_step_key`] and [`SubInstance::named_steps_view`] reads it
//! back.

use std::collections::HashMap;

use reify_compiler::TopologyTemplate;
use reify_core::ValueCellId;
use reify_ir::{CompiledExpr, KernelHandle};

/// The parent `named_steps` key for step `member` of the sub (or `let`
/// binding) `binding`. It matches the name the compiler gives
/// `GeomRef::Sub` for `self.<binding>.<member>`.
pub(crate) fn scoped_step_key(binding: &str, member: &str) -> String {
    format!("{}{member}", scoped_step_key_prefix(binding))
}

fn scoped_step_key_prefix(binding: &str) -> String {
    format!("{binding}.")
}

/// One direct, non-collection sub-component of a parent template, paired with
/// the child template it instantiates.
pub(crate) struct SubInstance<'t> {
    scoped_entity: String,
    sub_name: &'t str,
    child_template: &'t TopologyTemplate,
}

/// The direct non-collection subs of `template` whose child template is in
/// `templates`, in declaration order.
pub(crate) fn direct_sub_instances<'t>(
    template: &'t TopologyTemplate,
    templates: &'t [TopologyTemplate],
) -> impl Iterator<Item = SubInstance<'t>> {
    template
        .sub_components
        .iter()
        .filter(|sub| !sub.is_collection)
        .filter_map(move |sub| {
            let child_template = reify_compiler::find_template(templates, &sub.structure_name)?;
            Some(SubInstance {
                scoped_entity: format!("{}.{}", template.name, sub.name),
                sub_name: &sub.name,
                child_template,
            })
        })
}

impl<'t> SubInstance<'t> {
    pub(crate) fn child_template(&self) -> &'t TopologyTemplate {
        self.child_template
    }

    /// The `"<parent>.<sub>"` entity this instance's cells live under.
    pub(crate) fn scoped_entity(&self) -> &str {
        &self.scoped_entity
    }

    /// The id of this instance's copy of the child cell `member`.
    pub(crate) fn scoped_id(&self, member: &str) -> ValueCellId {
        ValueCellId::new(self.scoped_entity.as_str(), member)
    }

    /// `expr` with every `ValueRef` into the child template moved onto this
    /// instance's scoped entity. Refs into any other entity are unchanged.
    pub(crate) fn rescope(&self, expr: &CompiledExpr) -> CompiledExpr {
        let child_entity = self.child_template.name.as_str();
        expr.clone().map_value_refs(&mut |id| {
            if id.entity == child_entity {
                ValueCellId::new(self.scoped_entity.as_str(), id.member)
            } else {
                id
            }
        })
    }

    /// This instance's own step-name → handle map, read back out of the
    /// parent's [`scoped_step_key`] entries for this sub.
    pub(crate) fn named_steps_view(
        &self,
        parent_named_steps: &HashMap<String, KernelHandle>,
    ) -> HashMap<String, KernelHandle> {
        let prefix = scoped_step_key_prefix(self.sub_name);
        parent_named_steps
            .iter()
            .filter_map(|(key, handle)| {
                key.strip_prefix(prefix.as_str())
                    .map(|member| (member.to_string(), *handle))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use reify_core::{Type, ValueCellId};
    use reify_ir::{
        BinOp, CompiledExpr, CompiledExprKind, GeometryHandleId, KernelHandle, KernelId, Value,
        ValueMap,
    };
    use reify_test_support::parse_and_compile_with_stdlib;

    use super::{ScopedArgs, direct_sub_instances, scoped_arg_values, scoped_step_key};

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

    fn occt_handle(id: u64) -> KernelHandle {
        KernelHandle {
            kernel: KernelId::Occt,
            id: GeometryHandleId(id),
        }
    }

    #[test]
    fn named_steps_view_strips_only_this_subs_prefix() {
        let compiled = compile_mini();
        let parent = mini_parent(&compiled);
        let instance = direct_sub_instances(parent, &compiled.templates)
            .next()
            .expect("MiniParent has one direct instance");

        let parent_steps: HashMap<String, KernelHandle> = [
            ("body", 1),
            ("c.body", 2),
            ("c.inner.body", 3),
            ("c2.body", 4),
            ("cs.body", 5),
        ]
        .into_iter()
        .map(|(name, id)| (name.to_string(), occt_handle(id)))
        .collect();

        let expected: HashMap<String, KernelHandle> = [("body", 2), ("inner.body", 3)]
            .into_iter()
            .map(|(name, id)| (name.to_string(), occt_handle(id)))
            .collect();
        assert_eq!(instance.named_steps_view(&parent_steps), expected);
    }

    #[test]
    fn named_steps_view_reads_back_what_scoped_step_key_writes() {
        let compiled = compile_mini();
        let parent = mini_parent(&compiled);
        let instance = direct_sub_instances(parent, &compiled.templates)
            .next()
            .expect("MiniParent has one direct instance");

        assert_eq!(scoped_step_key("c", "body"), "c.body");

        let parent_steps: HashMap<String, KernelHandle> = [
            (scoped_step_key("c", "body"), occt_handle(7)),
            (scoped_step_key("c2", "body"), occt_handle(8)),
        ]
        .into_iter()
        .collect();

        let expected: HashMap<String, KernelHandle> =
            [("body".to_string(), occt_handle(7))].into_iter().collect();
        assert_eq!(instance.named_steps_view(&parent_steps), expected);
    }

    const GUARDED_SOURCE: &str = r#"
structure def Leaf {
    param w : Length = 1mm
}

structure def Tree {
    param depth : Int = 0
    sub child = Tree(depth: depth - 1) where depth > 0
    sub leaf = Leaf(w: 2mm)
}
"#;

    fn tree_sub<'c>(
        compiled: &'c reify_compiler::CompiledModule,
        sub_name: &str,
    ) -> &'c reify_compiler::SubComponentDecl {
        reify_compiler::find_template(&compiled.templates, "Tree")
            .expect("Tree template must compile")
            .sub_components
            .iter()
            .find(|sub| sub.name == sub_name)
            .unwrap_or_else(|| panic!("Tree must declare sub `{sub_name}`"))
    }

    #[test]
    fn guarded_sub_with_no_committed_arg_cell_is_guard_declined() {
        let compiled = parse_and_compile_with_stdlib(GUARDED_SOURCE);
        let child = tree_sub(&compiled, "child");

        let verdict = scoped_arg_values("Tree", child, &ValueMap::new());

        assert!(
            matches!(verdict, ScopedArgs::GuardDeclined),
            "a guarded sub with none of its arg cells committed was declined by eval, got {verdict:?}"
        );
    }

    #[test]
    fn guarded_sub_with_committed_arg_cells_is_elaborated() {
        let compiled = parse_and_compile_with_stdlib(GUARDED_SOURCE);
        let child = tree_sub(&compiled, "child");
        let mut values = ValueMap::new();
        values.insert(ValueCellId::new("Tree.child", "depth"), Value::Int(-1));

        let ScopedArgs::Elaborated(args) = scoped_arg_values("Tree", child, &values) else {
            panic!("a guarded sub whose arg cells eval committed is an elaborated instance");
        };

        assert_eq!(args, vec![("depth", Some(&Value::Int(-1)))]);
    }

    #[test]
    fn unguarded_sub_is_never_guard_declined() {
        let compiled = parse_and_compile_with_stdlib(GUARDED_SOURCE);
        let leaf = tree_sub(&compiled, "leaf");

        let ScopedArgs::Elaborated(args) = scoped_arg_values("Tree", leaf, &ValueMap::new()) else {
            panic!("an unguarded sub always has an instance, so its missing cells stay visible");
        };

        assert_eq!(args, vec![("w", None)]);
    }
}
