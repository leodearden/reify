//! Kernel-less selector-leaf `Value` fixtures: a `Value::Selector` leaf over a symbolic `TestBody` geometry handle (no kernel handle), for tests that need a selector-typed value without a live kernel.

use reify_core::identity::RealizationNodeId;
use reify_core::ty::SelectorKind;
use reify_ir::Value;
use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};

/// A `Value::Selector` leaf of `kind` running `query` over a kernel-less
/// `TestBody` geometry handle — the one cross-crate home for that test-only
/// shape. Panics if `query` rejects `kind`.
#[track_caller]
pub fn leaf_selector(kind: SelectorKind, query: LeafQuery) -> Value {
    let target = GeometryHandleRef {
        realization_ref: RealizationNodeId::new("TestBody", 0),
        upstream_values_hash: [0u8; 32],
        kernel_handle: None,
    };
    match SelectorValue::leaf(kind, target, query) {
        Ok(sv) => Value::Selector(sv),
        Err(e) => panic!("leaf_selector: {kind:?} leaf rejected its query: {e:?}"),
    }
}

/// [`leaf_selector`] over `LeafQuery::Named(name)`: the shape the symbolic
/// named-leaf ctors (`face(b, "root")`, `vertex(b, "tip")`, …) mint at eval
/// time.
#[track_caller]
pub fn named_leaf_selector(kind: SelectorKind, name: &str) -> Value {
    leaf_selector(kind, LeafQuery::Named(name.into()))
}

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
        leaf_selector(
            SelectorKind::Edge,
            LeafQuery::BySurfaceKind(FaceSurfaceKind::Plane),
        );
    }
}
