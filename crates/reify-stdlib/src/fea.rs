//! Multi-load-case FEA reductions: `envelope_max` / `envelope_min` over a
//! `Map<String, Field<Point3, T : Ordered>>` of per-case scalar fields.
//!
//! Compositional primitive — any per-case scalar field (von Mises,
//! displacement magnitude, etc.) flows through. The output is a fresh
//! `Field<Point3, T>` whose value at each grid point is the per-point
//! max/min across the case axis.
//!
//! # Source-kind staging
//!
//! Same Sampled-only staging as `crates/reify-expr/src/field_reductions.rs`:
//! FEA produces `FieldSourceKind::Sampled` results via
//! `engine_eval::elaborate_field`, so the eager per-grid-point reduction
//! is sufficient. Non-Sampled sources (Analytical, Composed, Imported,
//! and the derived wrappers VonMises / PrincipalStresses / MaxShear /
//! SafetyFactor / Gradient / Divergence / Curl / Laplacian) return
//! `Value::Undef` — the deferred path would require numerical reduction
//! across a Map of lambda-domains, out of scope for this task.

use reify_types::Value;

/// Evaluate a multi-load-case FEA stdlib function by name.
///
/// Returns `Some(value)` if the name is a recognised FEA function,
/// `None` otherwise (so the dispatch chain in `lib.rs` can fall through).
pub(crate) fn eval_fea(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "envelope_max" => envelope_reduce(args, false),
        "envelope_min" => envelope_reduce(args, true),
        _ => return None,
    })
}

/// Per-grid-point reduction across a `Map<String, Field<Point3, T>>` of
/// per-case Sampled fields. `find_min == false` selects the maximum;
/// `find_min == true` selects the minimum.
fn envelope_reduce(args: &[Value], _find_min: bool) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let map = match &args[0] {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };

    // Single-case sanity: return the inner Field unchanged. Avoids paying
    // the SampledField rebuild cost when only one case is provided and
    // prevents drift in the result's `name` / `oob_emitted` slot.
    if map.len() == 1 {
        let only = map.values().next().expect("len == 1");
        return match only {
            Value::Field { .. } => only.clone(),
            _ => Value::Undef,
        };
    }

    // Multi-case branch lands in step-6.
    Value::Undef
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use reify_types::{
        FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Type, Value,
    };

    // ── test helpers ────────────────────────────────────────────────────────

    /// Construct a 1-D `SampledField` from per-axis grid coords and data.
    fn make_sampled_1d(name: &str, axis: Vec<f64>, data: Vec<f64>) -> SampledField {
        let bounds_min = vec![*axis.first().expect("axis must be non-empty")];
        let bounds_max = vec![*axis.last().expect("axis must be non-empty")];
        let spacing = if axis.len() >= 2 {
            vec![axis[1] - axis[0]]
        } else {
            vec![1.0]
        };
        SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min,
            bounds_max,
            spacing,
            axis_grids: vec![axis],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Wrap a `SampledField` in a `Value::Field { source: Sampled, .. }`.
    fn wrap_sampled_field(sf: SampledField, domain: Type, codomain: Type) -> Value {
        Value::Field {
            domain_type: domain,
            codomain_type: codomain,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(sf)),
        }
    }

    /// Build a `Value::Map` from `(case_name, Value::Field)` pairs.
    fn make_envelope_map(cases: &[(&str, Value)]) -> Value {
        let mut map = BTreeMap::new();
        for (name, field) in cases {
            map.insert(Value::String((*name).to_string()), field.clone());
        }
        Value::Map(map)
    }

    // ── dispatcher-signal tests ─────────────────────────────────────────────

    #[test]
    fn eval_fea_unknown_returns_none() {
        assert!(eval_fea("foo", &[]).is_none());
    }

    #[test]
    fn eval_fea_envelope_max_returns_some() {
        assert!(eval_fea("envelope_max", &[]).is_some());
    }

    #[test]
    fn eval_fea_envelope_min_returns_some() {
        assert!(eval_fea("envelope_min", &[]).is_some());
    }

    // ── single-case passthrough ─────────────────────────────────────────────

    #[test]
    fn envelope_max_single_case_returns_field_unchanged() {
        let sf = make_sampled_1d(
            "f",
            vec![0.0, 1.0, 2.0, 3.0, 4.0],
            vec![1.0, 5.0, 3.0, 4.0, 2.0],
        );
        let field = wrap_sampled_field(sf, Type::Real, Type::Real);
        let map = make_envelope_map(&[("only", field.clone())]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        assert_eq!(result, field);
    }

    #[test]
    fn envelope_min_single_case_returns_field_unchanged() {
        let sf = make_sampled_1d(
            "f",
            vec![0.0, 1.0, 2.0, 3.0, 4.0],
            vec![1.0, 5.0, 3.0, 4.0, 2.0],
        );
        let field = wrap_sampled_field(sf, Type::Real, Type::Real);
        let map = make_envelope_map(&[("only", field.clone())]);

        let result = eval_fea("envelope_min", &[map]).unwrap();
        assert_eq!(result, field);
    }

    // ── two-case per-grid-point reductions ──────────────────────────────────

    /// Helper: extract the inner SampledField from a Sampled Value::Field.
    fn extract_sampled(v: &Value) -> &SampledField {
        match v {
            Value::Field { source, lambda, .. } if matches!(source, FieldSourceKind::Sampled) => {
                match lambda.as_ref() {
                    Value::SampledField(sf) => sf,
                    _ => panic!("expected SampledField in Sampled lambda slot"),
                }
            }
            _ => panic!("expected Sampled Value::Field, got {:?}", v),
        }
    }

    #[test]
    fn envelope_max_two_sampled_real_codomain_returns_per_grid_max() {
        let axis = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![1.0, 5.0, 3.0, 4.0, 2.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, 2.0, 4.0, 1.0, 5.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let sf = extract_sampled(&result);

        assert_eq!(sf.kind, SampledGridKind::Regular1D);
        assert_eq!(sf.axis_grids, vec![axis.clone()]);
        assert_eq!(sf.bounds_min, vec![0.0]);
        assert_eq!(sf.bounds_max, vec![4.0]);
        assert_eq!(sf.spacing, vec![1.0]);
        assert_eq!(sf.interpolation, InterpolationKind::Linear);
        assert_eq!(sf.data, vec![3.0, 5.0, 4.0, 4.0, 5.0]);

        // Outer Value::Field domain/codomain types are propagated unchanged.
        match &result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(*domain_type, Type::Real);
                assert_eq!(*codomain_type, Type::Real);
                assert!(matches!(source, FieldSourceKind::Sampled));
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }
}
