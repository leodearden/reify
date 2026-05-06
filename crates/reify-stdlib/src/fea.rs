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
//!
//! # Per-index reduction invariants (mirrored)
//!
//! The per-grid-point reduction in `envelope_reduce` mirrors the
//! NaN-skip + `total_cmp` + first-occurrence-wins discipline documented
//! on `reify-expr::field_reductions::argmax_argmin_index` (around
//! line 198): non-finite values are skipped via `is_finite()`, extrema
//! are selected via IEEE 754 `total_cmp`, and the first finite case at
//! each index wins on ties (strict `is_lt`/`is_gt`, not `is_le`/`is_ge`).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reify_types::{FieldSourceKind, SampledField, Type, Value};

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
///
/// Mirrors the NaN-skip + `total_cmp` + first-occurrence-wins discipline
/// documented on `crates/reify-expr/src/field_reductions.rs::argmax_argmin_index`
/// (around line 198): non-finite values are skipped via `is_finite()`,
/// extrema are selected via IEEE 754 `total_cmp`, and the first finite
/// case at each index wins on ties.
fn envelope_reduce(args: &[Value], find_min: bool) -> Value {
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

    // Capture the first case as the canonical reference for grid metadata
    // and per-case Sampled extraction.
    let mut iter = map.values();
    let first = iter.next();
    let (ref_domain, ref_codomain, ref_sf): (&Type, &Type, &SampledField) = match first {
        Some(Value::Field {
            source: FieldSourceKind::Sampled,
            lambda,
            domain_type,
            codomain_type,
        }) => match lambda.as_ref() {
            Value::SampledField(sf) => (domain_type, codomain_type, sf),
            // Defensive: a Sampled source must carry a SampledField in
            // its lambda slot (mirrors field_reductions.rs:96-99).
            _ => return Value::Undef,
        },
        // Empty Map or non-Sampled / non-Field first value → Undef.
        Some(_) | None => return Value::Undef,
    };

    // Collect per-case data slices, validating metadata equality with
    // the reference along the way. Mismatched grids/types → Undef.
    let mut cases_data: Vec<&[f64]> = Vec::with_capacity(map.len());
    cases_data.push(&ref_sf.data);
    for v in iter {
        let (dom, cod, sf) = match v {
            Value::Field {
                source: FieldSourceKind::Sampled,
                lambda,
                domain_type,
                codomain_type,
            } => match lambda.as_ref() {
                Value::SampledField(sf) => (domain_type, codomain_type, sf),
                _ => return Value::Undef,
            },
            _ => return Value::Undef,
        };
        if dom != ref_domain || cod != ref_codomain || !grids_equal(ref_sf, sf) {
            return Value::Undef;
        }
        cases_data.push(&sf.data);
    }

    // Per-grid-point reduction: NaN-skip + total_cmp + first-occurrence-wins.
    // If all per-case data[i] are non-finite, the all-non-finite sentinel
    // `f64::NAN` is written so downstream reductions will skip the index.
    let n = ref_sf.data.len();
    let mut out_data = Vec::with_capacity(n);
    for i in 0..n {
        let mut best: Option<f64> = None;
        for &slice in &cases_data {
            let v = slice[i];
            if !v.is_finite() {
                continue;
            }
            match best {
                None => best = Some(v),
                Some(b) => {
                    let cmp = v.total_cmp(&b);
                    let take = if find_min { cmp.is_lt() } else { cmp.is_gt() };
                    if take {
                        best = Some(v);
                    }
                }
            }
        }
        out_data.push(best.unwrap_or(f64::NAN));
    }

    let result_sf = SampledField {
        name: "envelope".to_string(),
        kind: ref_sf.kind,
        bounds_min: ref_sf.bounds_min.clone(),
        bounds_max: ref_sf.bounds_max.clone(),
        spacing: ref_sf.spacing.clone(),
        axis_grids: ref_sf.axis_grids.clone(),
        interpolation: ref_sf.interpolation,
        data: out_data,
        oob_emitted: AtomicBool::new(false),
    };

    // codomain_type and domain_type are propagated unchanged from the
    // validated reference case — pinned by
    // `envelope_max_pressure_codomain_preserves_dimension`. Future
    // refactors must NOT silently rewrap (e.g. coerce to Type::Real) or
    // a Pressure-codomain envelope would be incompatible with downstream
    // dimensional comparisons (`max(envelope) < yield_stress`).
    Value::Field {
        domain_type: ref_domain.clone(),
        codomain_type: ref_codomain.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(result_sf)),
    }
}

/// Strict grid-equality predicate for two `SampledField`s. Floats compared
/// via `to_bits()` to mirror `SampledField`'s own `PartialEq` semantics
/// (see `crates/reify-types/src/value.rs:149-189`). Excludes the data
/// buffer (caller compares element-wise on a per-index basis); excludes
/// `name` and `oob_emitted` (runtime-only, not semantic content).
fn grids_equal(a: &SampledField, b: &SampledField) -> bool {
    a.kind == b.kind
        && a.interpolation == b.interpolation
        && a.data.len() == b.data.len()
        && a.axis_grids.len() == b.axis_grids.len()
        && floats_bit_equal(&a.bounds_min, &b.bounds_min)
        && floats_bit_equal(&a.bounds_max, &b.bounds_max)
        && floats_bit_equal(&a.spacing, &b.spacing)
        && a.axis_grids
            .iter()
            .zip(b.axis_grids.iter())
            .all(|(x, y)| floats_bit_equal(x, y))
}

/// Bit-equal float-slice comparison. Used by `grids_equal` to mirror
/// `SampledField::PartialEq`'s `to_bits()` discipline.
fn floats_bit_equal(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.to_bits() == y.to_bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use reify_types::{
        DimensionVector, FieldSourceKind, InterpolationKind, SampledField, SampledGridKind, Type,
        Value,
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

    #[test]
    fn envelope_min_two_sampled_real_codomain_returns_per_grid_min() {
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

        let result = eval_fea("envelope_min", &[map]).unwrap();
        let sf = extract_sampled(&result);

        assert_eq!(sf.data, vec![1.0, 2.0, 3.0, 1.0, 2.0]);
        assert_eq!(sf.axis_grids, vec![axis]);
        assert_eq!(sf.kind, SampledGridKind::Regular1D);
    }

    // ── codomain dimension preservation ─────────────────────────────────────

    #[test]
    fn envelope_max_pressure_codomain_preserves_dimension() {
        let axis = vec![0.0, 1.0, 2.0];
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![100e6, 250e6, 180e6]),
            Type::Real,
            pressure.clone(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![150e6, 200e6, 220e6]),
            Type::Real,
            pressure.clone(),
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();

        // The reduction must NOT drop or rewrite codomain_type — it lives on
        // the outer Value::Field and should propagate from the reference case.
        match &result {
            Value::Field { codomain_type, .. } => assert_eq!(*codomain_type, pressure),
            other => panic!("expected Value::Field, got {:?}", other),
        }

        // And the per-index data is still correctly reduced.
        let sf = extract_sampled(&result);
        assert_eq!(sf.data, vec![150e6, 250e6, 220e6]);
    }

    // ── NaN / non-finite per-index handling ─────────────────────────────────

    #[test]
    fn envelope_max_skips_nan_per_index() {
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![1.0, f64::NAN, 3.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![f64::NAN, 5.0, 2.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let sf = extract_sampled(&result);

        // NaN-skip per index → only finite entries participate.
        assert_eq!(sf.data, vec![1.0, 5.0, 3.0]);
    }

    #[test]
    fn envelope_max_all_nan_at_index_yields_nan() {
        let axis = vec![0.0, 1.0, 2.0];
        // At index 1, case_a=NaN and case_b=Inf — both non-finite. The
        // result must materialise the all-non-finite sentinel `NaN`,
        // not the first non-finite seen and not 0.0.
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![1.0, f64::NAN, 3.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, f64::INFINITY, 2.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let sf = extract_sampled(&result);

        assert_eq!(sf.data[0], 3.0);
        // NaN != NaN under PartialEq — must use is_nan.
        assert!(sf.data[1].is_nan(), "expected NaN at index 1, got {}", sf.data[1]);
        assert_eq!(sf.data[2], 3.0);
    }
}
