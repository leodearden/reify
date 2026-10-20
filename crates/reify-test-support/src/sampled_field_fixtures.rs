//! Sampled-field values for exercising field reductions (`max`, `argmin`, ...)
//! without an elaborated model.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_core::Type;
use reify_ir::{FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Value};

/// A dimensionless, linearly interpolated 1-D sampled field named `name`, with one
/// `data` sample per node of the uniformly spaced, increasing `axis`.
pub fn sampled_field_1d(name: &str, axis: Vec<f64>, data: Vec<f64>) -> Value {
    assert_eq!(
        axis.len(),
        data.len(),
        "sampled_field_1d needs one sample per axis node: axis {axis:?}, data {data:?}"
    );
    assert!(
        axis.len() >= 2,
        "sampled_field_1d needs at least two axis nodes to define a spacing: axis {axis:?}"
    );
    let spacing = axis[1] - axis[0];
    assert!(
        spacing > 0.0
            && axis
                .windows(2)
                .all(|w| ((w[1] - w[0]) - spacing).abs() <= 1e-12 * spacing.abs()),
        "sampled_field_1d needs a uniformly spaced, increasing axis: axis {axis:?}"
    );

    let sf = SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular1D,
        bounds_min: vec![axis[0]],
        bounds_max: vec![axis[axis.len() - 1]],
        spacing: vec![spacing],
        axis_grids: vec![axis],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: AtomicBool::new(false),
    };
    Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(sf)),
    }
}

/// A 1-D sampled field over x ∈ {0, 1, 2, 3} with data {5, 9, 2, 7}.
///
/// Global max 9 at x = 1, global min 2 at x = 2.  Restricted to x ∈ [2, 3] the
/// max MOVES to 7 at x = 3 (the min stays 2 at x = 2).  The global maximiser is
/// deliberately outside that box, so a bounded result is distinguishable from
/// an unbounded one by value alone, and the bounded and global winners are
/// different grid nodes by construction.
pub fn extremum_probe_field() -> Value {
    sampled_field_1d("probe", vec![0.0, 1.0, 2.0, 3.0], vec![5.0, 9.0, 2.0, 7.0])
}

/// The canonical `bounding_box(solid)` shape: two 3-component `Value::Point`
/// corners (lo, 0, 0) and (hi, 0, 0).  A 1-D field consults only the x span.
pub fn x_span_bbox(lo: f64, hi: f64) -> Value {
    Value::BoundingBox {
        min: Box::new(Value::Point(vec![
            Value::Real(lo),
            Value::Real(0.0),
            Value::Real(0.0),
        ])),
        max: Box::new(Value::Point(vec![
            Value::Real(hi),
            Value::Real(0.0),
            Value::Real(0.0),
        ])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// The axis and data behind a `sampled_field_1d` value, panicking on any other shape.
    fn sampled(field: &Value) -> &SampledField {
        let Value::Field { lambda, .. } = field else {
            panic!("expected a Value::Field, got {field:?}");
        };
        let Value::SampledField(sf) = lambda.as_ref() else {
            panic!("expected a Value::SampledField lambda, got {lambda:?}");
        };
        sf
    }

    #[test]
    fn sampled_field_1d_builds_a_regular_linear_dimensionless_field_over_the_axis() {
        let field = sampled_field_1d("p", vec![0.0, 1.0, 2.0, 3.0], vec![5.0, 9.0, 2.0, 7.0]);
        let Value::Field {
            domain_type,
            codomain_type,
            source,
            ..
        } = &field
        else {
            panic!("expected a Value::Field, got {field:?}");
        };
        assert_eq!(*domain_type, Type::dimensionless_scalar());
        assert_eq!(*codomain_type, Type::dimensionless_scalar());
        assert_eq!(*source, FieldSourceKind::Sampled);

        let sf = sampled(&field);
        assert_eq!(sf.name, "p");
        assert_eq!(sf.kind, SampledGridKind::Regular1D);
        assert_eq!(sf.bounds_min, [0.0]);
        assert_eq!(sf.bounds_max, [3.0]);
        assert_eq!(sf.spacing, [1.0]);
        assert_eq!(sf.axis_grids, [vec![0.0, 1.0, 2.0, 3.0]]);
        assert_eq!(sf.interpolation, InterpolationKind::Linear);
        assert_eq!(sf.data, [5.0, 9.0, 2.0, 7.0]);
        assert!(!sf.oob_emitted.load(Ordering::Relaxed));
    }

    #[test]
    #[should_panic(expected = "uniformly spaced")]
    fn sampled_field_1d_rejects_an_irregular_axis() {
        sampled_field_1d("p", vec![0.0, 1.0, 3.0], vec![5.0, 9.0, 2.0]);
    }

    #[test]
    #[should_panic(expected = "one sample per axis node")]
    fn sampled_field_1d_rejects_mismatched_axis_and_data() {
        sampled_field_1d("p", vec![0.0, 1.0, 2.0], vec![5.0, 9.0]);
    }

    #[test]
    fn the_extremum_probe_fields_global_maximiser_lies_outside_the_x_span_2_to_3() {
        let field = extremum_probe_field();
        let sf = sampled(&field);
        let nodes: Vec<(f64, f64)> = sf.axis_grids[0]
            .iter()
            .copied()
            .zip(sf.data.iter().copied())
            .collect();
        let extremum = |nodes: &[(f64, f64)], pick_max: bool| -> (f64, f64) {
            nodes
                .iter()
                .copied()
                .reduce(|best, n| {
                    let better = if pick_max { n.1 > best.1 } else { n.1 < best.1 };
                    if better { n } else { best }
                })
                .expect("the probe field has nodes")
        };
        let in_span: Vec<(f64, f64)> = nodes
            .iter()
            .copied()
            .filter(|(x, _)| (2.0..=3.0).contains(x))
            .collect();

        assert_eq!(extremum(&nodes, true), (1.0, 9.0), "global argmax");
        assert_eq!(extremum(&nodes, false), (2.0, 2.0), "global argmin");
        assert_eq!(
            extremum(&in_span, true),
            (3.0, 7.0),
            "argmax over x in [2, 3]"
        );
        assert_eq!(
            extremum(&in_span, false),
            (2.0, 2.0),
            "argmin over x in [2, 3]"
        );
    }

    #[test]
    fn x_span_bbox_spans_lo_to_hi_on_x_and_is_degenerate_on_y_and_z() {
        assert_eq!(
            x_span_bbox(2.0, 3.0),
            Value::BoundingBox {
                min: Box::new(Value::Point(vec![
                    Value::Real(2.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ])),
                max: Box::new(Value::Point(vec![
                    Value::Real(3.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ])),
            }
        );
    }
}
