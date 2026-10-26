//! Label-tagged, panicking decomposition of evaluated `Value`s for test assertions.

use reify_core::ty::SelectorKind;
use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorNode, SelectorValue};
use reify_ir::{GeometryHandleId, Value};

/// Read a numeric component (Real, Scalar, or Int) as f64 SI value.
pub fn read_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        Value::Int(i) => *i as f64,
        other => panic!("{label}: expected numeric component, got {other:?}"),
    }
}

/// Decompose a `Value::Point` of three numeric components into `[f64; 3]` (SI).
pub fn decompose_point3(v: &Value, label: &str) -> [f64; 3] {
    let comps = match v {
        Value::Point(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Value::Point len=3, got {other:?}"),
    };
    [
        read_f64(&comps[0], &format!("{label}.p[0]")),
        read_f64(&comps[1], &format!("{label}.p[1]")),
        read_f64(&comps[2], &format!("{label}.p[2]")),
    ]
}

/// Assert `cell_value` holds a `Value::Selector` whose node is a single `Leaf`
/// of the expected `kind`, then run `check_query` against the leaf's query.
///
/// Pins only the SHAPE: every numeric expectation and epsilon stays in the
/// caller's closure, so no shared tolerance can silently retune a pin.
/// `#[track_caller]` makes a failure name the calling pin, not this file.
#[track_caller]
pub fn assert_selector_leaf(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    check_query: impl FnOnce(&LeafQuery),
) {
    let node = &assert_selector_value(cell_value, label, kind).node;
    let (_, query) = leaf_node_parts(node, label);
    check_query(query);
}

/// [`assert_selector_leaf`] plus a check of the leaf target's `kernel_handle`:
/// `Some(id)` for a realized build-path leaf, `None` for a symbolic eval-path
/// mint.
#[track_caller]
pub fn assert_selector_leaf_targeting(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    expected_kernel_handle: Option<GeometryHandleId>,
    check_query: impl FnOnce(&LeafQuery),
) {
    let node = &assert_selector_value(cell_value, label, kind).node;
    let query = assert_leaf_node_targeting(node, label, expected_kernel_handle);
    check_query(query);
}

/// Node-level [`assert_selector_leaf_targeting`] for a selector that is not a
/// cell of its own, such as a composition's child: assert `node` is a single
/// `Leaf` whose target has `expected_kernel_handle`, and return its query.
#[track_caller]
pub fn assert_leaf_node_targeting<'a>(
    node: &'a SelectorNode,
    label: &str,
    expected_kernel_handle: Option<GeometryHandleId>,
) -> &'a LeafQuery {
    let (target, query) = leaf_node_parts(node, label);
    assert_eq!(
        target.kernel_handle, expected_kernel_handle,
        "{label}: leaf target kernel_handle"
    );
    query
}

/// Assert `cell_value` holds a `Value::Selector` of the expected `kind` and
/// return it, whatever its node shape (`Leaf`, `Union`, `Intersect`, ...).
#[track_caller]
pub fn assert_selector_value<'a>(
    cell_value: Option<&'a Value>,
    label: &str,
    kind: SelectorKind,
) -> &'a SelectorValue {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!("{label} must be a Value::Selector, got: {other:?}"),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    sv
}

#[track_caller]
fn leaf_node_parts<'a>(
    node: &'a SelectorNode,
    label: &str,
) -> (&'a GeometryHandleRef, &'a LeafQuery) {
    match node {
        SelectorNode::Leaf { target, query } => (target, query),
        other => panic!("{label} must be a Leaf selector node, got: {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use reify_core::dimension::DimensionVector;
    use reify_core::identity::RealizationNodeId;
    use reify_core::ty::SelectorKind;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};
    use reify_ir::{GeometryHandleId, Value};

    use super::{
        assert_leaf_node_targeting, assert_selector_leaf, assert_selector_leaf_targeting,
        assert_selector_value, decompose_point3, read_f64, read_real_or_scalar,
    };

    // ── read_f64 tests ────────────────────────────────────────────────────────

    #[test]
    fn read_f64_real_returns_inner_f64() {
        assert_eq!(read_f64(&Value::Real(2.5), "x"), 2.5);
    }

    #[test]
    fn read_f64_scalar_returns_si_value() {
        let v = Value::Scalar {
            si_value: 1.5,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(read_f64(&v, "y"), 1.5);
    }

    #[test]
    fn read_f64_int_returns_cast_f64() {
        assert_eq!(read_f64(&Value::Int(7), "z"), 7.0);
    }

    #[test]
    #[should_panic(expected = "lbl: expected numeric component")]
    fn read_f64_panics_on_non_numeric_with_label() {
        read_f64(&Value::Bool(true), "lbl");
    }

    // ── read_real_or_scalar tests ─────────────────────────────────────────────

    #[test]
    fn read_real_or_scalar_real_returns_inner_f64() {
        assert_eq!(read_real_or_scalar(&Value::Real(2.5), "x"), 2.5);
    }

    #[test]
    fn read_real_or_scalar_scalar_of_any_dimension_returns_si_value() {
        let v = Value::Scalar {
            si_value: 1.5,
            dimension: DimensionVector::FORCE,
        };
        assert_eq!(read_real_or_scalar(&v, "y"), 1.5);
    }

    #[test]
    #[should_panic(expected = "lbl: expected a Real or Scalar component")]
    fn read_real_or_scalar_rejects_int_with_label() {
        read_real_or_scalar(&Value::Int(7), "lbl");
    }

    #[test]
    #[should_panic(expected = "lbl: expected a Real or Scalar component")]
    fn read_real_or_scalar_panics_on_non_numeric_with_label() {
        read_real_or_scalar(&Value::Bool(true), "lbl");
    }

    // ── decompose_point3 tests ────────────────────────────────────────────────

    #[test]
    fn decompose_point3_three_reals_returns_array() {
        let v = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert_eq!(decompose_point3(&v, "pt"), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn decompose_point3_three_length_scalars_returns_si_array() {
        let v = Value::Point(vec![
            Value::Scalar {
                si_value: 0.001,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.002,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_eq!(decompose_point3(&v, "pt"), [0.001, 0.002, 0.003]);
    }

    #[test]
    #[should_panic(expected = "label: expected Value::Point len=3")]
    fn decompose_point3_panics_on_non_point() {
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        decompose_point3(&v, "label");
    }

    #[test]
    #[should_panic(expected = "label: expected Value::Point len=3")]
    fn decompose_point3_panics_on_wrong_length() {
        let v = Value::Point(vec![Value::Real(0.0), Value::Real(0.0)]);
        decompose_point3(&v, "label");
    }

    #[test]
    #[should_panic(expected = "pt.p[0]: expected numeric component")]
    fn decompose_point3_panics_on_non_numeric_component() {
        let v = Value::Point(vec![Value::Bool(true), Value::Real(0.0), Value::Real(0.0)]);
        decompose_point3(&v, "pt");
    }

    // ── assert_selector_leaf tests ────────────────────────────────────────────

    fn selector_leaf_cell(
        kind: SelectorKind,
        query: LeafQuery,
        kernel_handle: Option<GeometryHandleId>,
    ) -> Value {
        let target = GeometryHandleRef {
            realization_ref: RealizationNodeId::new("body", 0),
            upstream_values_hash: [0u8; 32],
            kernel_handle,
        };
        Value::Selector(SelectorValue::leaf(kind, target, query).expect("leaf construction"))
    }

    #[test]
    fn assert_selector_leaf_runs_check_query_on_matching_leaf() {
        let cell = selector_leaf_cell(
            SelectorKind::Edge,
            LeafQuery::All,
            Some(GeometryHandleId(1)),
        );
        let mut seen = None;
        assert_selector_leaf(Some(&cell), "lbl", SelectorKind::Edge, |q| {
            seen = Some(q.clone());
        });
        assert_eq!(seen, Some(LeafQuery::All));
    }

    #[test]
    #[should_panic(expected = "lbl must be a Value::Selector")]
    fn assert_selector_leaf_panics_on_absent_cell() {
        assert_selector_leaf(None, "lbl", SelectorKind::Edge, |_| {});
    }

    #[test]
    #[should_panic(expected = "lbl must be a Value::Selector")]
    fn assert_selector_leaf_panics_on_non_selector_value() {
        assert_selector_leaf(Some(&Value::Undef), "lbl", SelectorKind::Edge, |_| {});
    }

    #[test]
    #[should_panic(expected = "lbl: selector kind")]
    fn assert_selector_leaf_panics_on_kind_mismatch() {
        let cell = selector_leaf_cell(
            SelectorKind::Edge,
            LeafQuery::All,
            Some(GeometryHandleId(1)),
        );
        assert_selector_leaf(Some(&cell), "lbl", SelectorKind::Face, |_| {});
    }

    #[test]
    #[should_panic(expected = "lbl must be a Leaf selector node")]
    fn assert_selector_leaf_panics_on_non_leaf_node() {
        let Value::Selector(leaf) = selector_leaf_cell(
            SelectorKind::Edge,
            LeafQuery::All,
            Some(GeometryHandleId(1)),
        ) else {
            unreachable!("selector_leaf_cell always builds a Value::Selector");
        };
        let cell = Value::Selector(SelectorValue::union(vec![leaf]).expect("union construction"));
        assert_selector_leaf(Some(&cell), "lbl", SelectorKind::Edge, |_| {});
    }

    #[test]
    fn assert_selector_leaf_targeting_accepts_matching_kernel_handle() {
        let cell = selector_leaf_cell(
            SelectorKind::Edge,
            LeafQuery::All,
            Some(GeometryHandleId(1)),
        );
        let mut seen = None;
        assert_selector_leaf_targeting(
            Some(&cell),
            "lbl",
            SelectorKind::Edge,
            Some(GeometryHandleId(1)),
            |q| seen = Some(q.clone()),
        );
        assert_eq!(seen, Some(LeafQuery::All));
    }

    #[test]
    fn assert_selector_leaf_targeting_accepts_symbolic_target() {
        let cell = selector_leaf_cell(SelectorKind::Face, LeafQuery::All, None);
        let mut seen = None;
        assert_selector_leaf_targeting(Some(&cell), "lbl", SelectorKind::Face, None, |q| {
            seen = Some(q.clone())
        });
        assert_eq!(seen, Some(LeafQuery::All));
    }

    #[test]
    #[should_panic(expected = "lbl: leaf target kernel_handle")]
    fn assert_selector_leaf_targeting_panics_on_wrong_kernel_handle() {
        let cell = selector_leaf_cell(
            SelectorKind::Edge,
            LeafQuery::All,
            Some(GeometryHandleId(1)),
        );
        assert_selector_leaf_targeting(
            Some(&cell),
            "lbl",
            SelectorKind::Edge,
            Some(GeometryHandleId(2)),
            |_| {},
        );
    }

    #[test]
    #[should_panic(expected = "lbl: leaf target kernel_handle")]
    fn assert_selector_leaf_targeting_panics_on_symbolic_vs_realized() {
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, None);
        assert_selector_leaf_targeting(
            Some(&cell),
            "lbl",
            SelectorKind::Edge,
            Some(GeometryHandleId(1)),
            |_| {},
        );
    }

    #[test]
    #[should_panic(expected = "lbl must be a Value::Selector")]
    fn assert_selector_leaf_targeting_still_checks_shape() {
        assert_selector_leaf_targeting(
            Some(&Value::Undef),
            "lbl",
            SelectorKind::Edge,
            None,
            |_| {},
        );
    }

    // ── assert_selector_value tests ───────────────────────────────────────────

    #[test]
    fn assert_selector_value_returns_selector_of_any_node_shape() {
        let Value::Selector(leaf) = selector_leaf_cell(SelectorKind::Face, LeafQuery::All, None)
        else {
            unreachable!("selector_leaf_cell always builds a Value::Selector");
        };
        let union = SelectorValue::union(vec![leaf]).expect("union construction");
        let cell = Value::Selector(union.clone());
        assert_eq!(
            assert_selector_value(Some(&cell), "lbl", SelectorKind::Face),
            &union
        );
    }

    #[test]
    #[should_panic(expected = "lbl must be a Value::Selector")]
    fn assert_selector_value_panics_on_absent_cell() {
        assert_selector_value(None, "lbl", SelectorKind::Face);
    }

    #[test]
    #[should_panic(expected = "lbl: selector kind")]
    fn assert_selector_value_panics_on_kind_mismatch() {
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, None);
        assert_selector_value(Some(&cell), "lbl", SelectorKind::Face);
    }

    // ── assert_leaf_node_targeting tests ──────────────────────────────────────

    fn selector_leaf(kind: SelectorKind, kernel_handle: Option<GeometryHandleId>) -> SelectorValue {
        let Value::Selector(leaf) = selector_leaf_cell(kind, LeafQuery::All, kernel_handle) else {
            unreachable!("selector_leaf_cell always builds a Value::Selector");
        };
        leaf
    }

    #[test]
    fn assert_leaf_node_targeting_returns_query_of_matching_leaf() {
        let leaf = selector_leaf(SelectorKind::Edge, None);
        assert_eq!(
            assert_leaf_node_targeting(&leaf.node, "lbl", None),
            &LeafQuery::All
        );
    }

    #[test]
    #[should_panic(expected = "lbl must be a Leaf selector node")]
    fn assert_leaf_node_targeting_panics_on_non_leaf_node() {
        let union = SelectorValue::union(vec![selector_leaf(SelectorKind::Edge, None)])
            .expect("union construction");
        assert_leaf_node_targeting(&union.node, "lbl", None);
    }

    #[test]
    #[should_panic(expected = "lbl: leaf target kernel_handle")]
    fn assert_leaf_node_targeting_panics_on_realized_vs_symbolic() {
        let leaf = selector_leaf(SelectorKind::Edge, Some(GeometryHandleId(1)));
        assert_leaf_node_targeting(&leaf.node, "lbl", None);
    }
}
