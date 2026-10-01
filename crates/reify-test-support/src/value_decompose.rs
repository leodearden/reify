use reify_ir::Value;

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

#[cfg(test)]
mod tests {
    use reify_core::dimension::DimensionVector;
    use reify_core::identity::RealizationNodeId;
    use reify_core::ty::SelectorKind;
    use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorValue};
    use reify_ir::{GeometryHandleId, Value};

    use super::{assert_selector_leaf, assert_selector_leaf_targeting, decompose_point3, read_f64};

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
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, Some(GeometryHandleId(1)));
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
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, Some(GeometryHandleId(1)));
        assert_selector_leaf(Some(&cell), "lbl", SelectorKind::Face, |_| {});
    }

    #[test]
    #[should_panic(expected = "lbl must be a Leaf selector node")]
    fn assert_selector_leaf_panics_on_non_leaf_node() {
        let Value::Selector(leaf) =
            selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, Some(GeometryHandleId(1)))
        else {
            unreachable!("selector_leaf_cell always builds a Value::Selector");
        };
        let cell = Value::Selector(SelectorValue::union(vec![leaf]).expect("union construction"));
        assert_selector_leaf(Some(&cell), "lbl", SelectorKind::Edge, |_| {});
    }

    #[test]
    fn assert_selector_leaf_targeting_accepts_matching_kernel_handle() {
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, Some(GeometryHandleId(1)));
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
        let cell = selector_leaf_cell(SelectorKind::Edge, LeafQuery::All, Some(GeometryHandleId(1)));
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
        assert_selector_leaf_targeting(Some(&Value::Undef), "lbl", SelectorKind::Edge, None, |_| {});
    }
}
