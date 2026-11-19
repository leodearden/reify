//! Kernel-less selector-leaf `Value` fixtures: a `Value::Selector` leaf over a symbolic `TestBody` geometry handle (no kernel handle), for tests that need a selector-typed value without a live kernel.

#[cfg(test)]
mod tests {
    use reify_core::ty::SelectorKind;
    use reify_ir::FaceSurfaceKind;
    use reify_ir::value::LeafQuery;

    use super::{leaf_selector, named_leaf_selector};
    use crate::value_decompose::assert_selector_leaf_targeting;

    #[test]
    fn leaf_selector_mints_a_kernel_less_leaf_of_the_given_kind_and_query() {
        for (kind, query) in [
            (SelectorKind::Face, LeafQuery::All),
            (SelectorKind::Edge, LeafQuery::Named("tip".into())),
            (SelectorKind::Vertex, LeafQuery::All),
        ] {
            let v = leaf_selector(kind, query.clone());
            assert_selector_leaf_targeting(Some(&v), "leaf_selector", kind, None, |q| {
                assert_eq!(q, &query)
            });
        }
    }

    #[test]
    fn named_leaf_selector_is_leaf_selector_over_a_named_query() {
        assert_eq!(
            named_leaf_selector(SelectorKind::Face, "x_max"),
            leaf_selector(SelectorKind::Face, LeafQuery::Named("x_max".into()))
        );
    }

    #[test]
    #[should_panic(expected = "leaf_selector: Edge leaf rejected its query")]
    fn leaf_selector_panics_naming_the_kind_when_the_query_rejects_it() {
        leaf_selector(SelectorKind::Edge, LeafQuery::BySurfaceKind(FaceSurfaceKind::Plane));
    }
}
