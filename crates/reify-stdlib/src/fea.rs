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
        "case_names" => case_names(args),
        _ => return None,
    })
}

/// Return the keys of the `cases` Map inside a `MultiCaseResult` struct
/// instance as a `Value::List<Value::String>`, in BTreeMap lexicographic
/// (alphabetical) order.
///
/// # Input shape
///
/// `args == [Value::Map { "cases" -> Value::Map<Value::String, Value> }]`
///
/// The outer Map is the `MultiCaseResult` struct instance (field-keyed by
/// `Value::String`). The inner `"cases"` field holds the Map from case name
/// to per-case `ElasticResult`.
///
/// # Output
///
/// `Value::List(Vec<Value::String>)` of the `cases` Map's keys in BTreeMap
/// natural order (lexicographic on `Value::String`, which is deterministic
/// and content-addressing-stable per `Value::Map`'s `BTreeMap` invariant).
///
/// # Failure modes
///
/// All argument-shape failures collapse to `Value::Undef` (silent-Undef
/// discipline, mirroring `envelope_reduce`):
///   - arity != 1
///   - `args[0]` is not `Value::Map`
///   - outer Map has no `"cases"` key
///   - `"cases"` value is not `Value::Map`
///
/// Diagnostic emission is deferred to PRD task #10 (Diagnostic mapping for
/// multi-case-specific failure modes).
fn case_names(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let outer = match &args[0] {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    let cases = match outer.get(&Value::String("cases".to_string())) {
        Some(Value::Map(m)) => m,
        _ => return Value::Undef,
    };
    Value::List(cases.keys().cloned().collect())
}

/// Per-grid-point reduction across a `Map<String, Field<Point3, T>>` of
/// per-case Sampled fields. `find_min == false` selects the maximum;
/// `find_min == true` selects the minimum.
///
/// # Contract
///
/// **Input:** `args == [Value::Map(BTreeMap<Value::String, Value::Field>)]`
/// where each inner `Value::Field` is `source: Sampled` and carries a
/// `Value::SampledField` in its lambda slot. Domain `Point3`-shaped (any
/// arity 1–3 in practice — Regular1D / Regular2D / Regular3D) and any
/// `Ordered` codomain `T`.
///
/// **Output:** `Value::Field { source: Sampled, lambda: Arc(SampledField), .. }`
/// whose `data[i]` is the per-grid-point extremum across cases. Domain
/// and codomain types propagate unchanged from the validated reference.
///
/// # Semantics
///
/// 1. **Sampled-only staging.** Mirrors the staging policy of
///    `reify-expr::field_reductions` (#2913): FEA results land as
///    `FieldSourceKind::Sampled` via `engine_eval::elaborate_field`, so
///    the eager per-grid-point reduction is sufficient. Non-Sampled
///    sources (Analytical, Composed, Imported, derived wrappers) return
///    `Value::Undef` — the deferred path would require numerical
///    reduction across a Map of lambda-domains, out of scope.
///
/// 2. **Empty Map sanity.** An empty Map returns `Value::Undef` — there
///    is no reference case to validate against, and no per-case data to
///    reduce. Single-case Maps fall through to the same reduction loop
///    as N≥2 (the loop iterates once per case, so finite values flow
///    through unchanged) — this preserves Sampled-only enforcement and
///    yields a result with the canonical `name: "envelope"` regardless
///    of case count, avoiding the behaviour cliff a fast-path would
///    introduce in downstream tooling that keys off `SampledField.name`.
///
/// 3. **NaN-skip + `total_cmp` per index.** At each output index `i`:
///    non-finite values (`!is_finite()`, rejecting both NaN and ±∞) are
///    skipped; finite values fold into `best` via IEEE 754 `total_cmp`
///    with strict `is_lt`/`is_gt` so the first finite case wins on ties.
///    If no case at index `i` is finite, `data[i] = f64::NAN` so
///    downstream reductions can skip the index uniformly via the same
///    `is_finite()` discipline.
///
/// 4. **Strict grid-equality requirement.** All per-case Sampled fields
///    must share identical grid metadata (kind, axis_grids float-bits,
///    bounds_min/max float-bits, spacing float-bits, interpolation,
///    domain_type, codomain_type) — see `metadata_matches`. Mismatched
///    cases return `Value::Undef`. **User-facing implication:** if your
///    cases use different mesh sizes / element orders, the envelope
///    returns `Undef` — solve all cases on a common mesh first.
///    Cross-mesh resampling is out of scope for this primitive (PRD).
///
/// 5. **Silent-Undef diagnostics.** All failure modes (empty Map, type
///    mismatch, non-Sampled source, defective lambda slot, wrong arity,
///    non-Map argument) collapse to `Value::Undef`. Diagnostic emission
///    is deferred to PRD task #10 (Diagnostic mapping for multi-case-
///    specific failure modes); this matches the silent-Undef convention
///    shared with `analysis.rs` / `helpers::sanitize_value`.
fn envelope_reduce(args: &[Value], find_min: bool) -> Value {
    // Argument-shape validation contract (pinned by
    // `envelope_max_argument_shape_negative_paths_return_undef`):
    //   1. arity must be exactly 1 (Map<String, Field>).
    //   2. args[0] must be `Value::Map(_)`.
    //   3. each Map value must be `Value::Field { source: Sampled, .. }`
    //      with `lambda.as_ref() == Value::SampledField(_)` — non-Field
    //      values, non-Sampled sources, and lambda-slot mismatches all
    //      reject to `Value::Undef` (defensive arms in the loops below
    //      mirror `field_reductions.rs:96-99`).
    if args.len() != 1 {
        return Value::Undef;
    }
    let map = match &args[0] {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };

    // Empty Map → Undef. Diagnostic emission deferred to PRD task #10
    // (Diagnostic mapping for multi-case-specific failure modes); this
    // short-circuit matches the silent-Undef convention shared with
    // analysis.rs / sanitize_value.
    if map.is_empty() {
        return Value::Undef;
    }

    // No single-case fast path — single-case Maps fall through to the
    // multi-case reduction loop. This (a) enforces the Sampled-only
    // contract uniformly across N=1 and N>1 (a single-case Analytical /
    // Composed / Imported / derived Field rejects to Undef instead of
    // leaking through), and (b) gives the result a uniform
    // `name: "envelope"` regardless of case count — downstream tooling
    // that keys off `SampledField.name` (snapshot tests, viewer labels)
    // sees consistent output. The cost (one Vec<f64> clone) is
    // negligible compared to FEA solve cost.
    //
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
        if !metadata_matches(ref_sf, sf, ref_domain, ref_codomain, dom, cod) {
            return Value::Undef;
        }
        cases_data.push(&sf.data);
    }

    // Per-grid-point reduction: NaN-skip + total_cmp + first-occurrence-wins.
    //
    // Iteration order: outer over cases, inner over indices (zipped). This
    // (a) lets the compiler vectorise the inner loop — sequential reads
    // from one slice and one shared output vec, no per-index Option<f64>
    // setup — and (b) eliminates the per-access bounds checks: the slices
    // are equal-length (validated by `metadata_matches`), so `iter_mut()
    // .zip(slice.iter())` rides on the iterator's bounds-check-free path.
    //
    // The NaN-as-sentinel encoding: `out_data` is initialised to NaN at
    // every index. Since we only ever write finite values (after the
    // `is_finite` skip), `out.is_finite()` cleanly doubles as the
    // "we've seen a finite value at index i before" predicate. Indices
    // where no case had a finite value retain the NaN sentinel — pinned
    // by `envelope_{max,min}_all_nan_at_index_yields_nan`. The sentinel
    // lets downstream `max(envelope_max(...))` skip the index uniformly
    // via the same `is_finite()` discipline.
    //
    // Per-index protocol:
    //   1. Skip non-finite cases via `is_finite()` (rejects both NaN and
    //      ±∞, matching `sanitize_value`'s discipline).
    //   2. First finite at index `i` initialises `out` (no comparison).
    //   3. Subsequent finites compare against `out` via IEEE 754
    //      `total_cmp` with strict `is_lt`/`is_gt` so the first finite
    //      case wins on ties.
    let n = ref_sf.data.len();
    let mut out_data: Vec<f64> = vec![f64::NAN; n];
    for slice in &cases_data {
        for (out, &v) in out_data.iter_mut().zip(slice.iter()) {
            if !v.is_finite() {
                continue;
            }
            if !out.is_finite() {
                // First finite at this index — initialise without compare.
                *out = v;
            } else {
                let cmp = v.total_cmp(out);
                let take = if find_min { cmp.is_lt() } else { cmp.is_gt() };
                if take {
                    *out = v;
                }
            }
        }
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

/// Composite metadata-equality predicate used by `envelope_reduce`'s
/// per-case validation loop. Returns `false` on any of:
///   - grid mismatch (`grids_equal` — kind, axis_grids, bounds, spacing,
///     interpolation, data length)
///   - domain-type mismatch
///   - codomain-type mismatch
///
/// Captures the full reference-equality contract pinned by step-15's
/// four-mismatch tests. Mirrors the to_bits() float discipline of
/// `SampledField::PartialEq`.
fn metadata_matches(
    reference: &SampledField,
    candidate: &SampledField,
    ref_domain: &Type,
    ref_codomain: &Type,
    candidate_domain: &Type,
    candidate_codomain: &Type,
) -> bool {
    candidate_domain == ref_domain
        && candidate_codomain == ref_codomain
        && grids_equal(reference, candidate)
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
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.to_bits() == y.to_bits())
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

    /// Construct a 2-D `SampledField` from per-axis grid coords and data
    /// (axis-0 outermost, row-major).
    fn make_sampled_2d(
        name: &str,
        axis0: Vec<f64>,
        axis1: Vec<f64>,
        data: Vec<f64>,
    ) -> SampledField {
        let bounds_min = vec![
            *axis0.first().expect("axis0 must be non-empty"),
            *axis1.first().expect("axis1 must be non-empty"),
        ];
        let bounds_max = vec![
            *axis0.last().expect("axis0 must be non-empty"),
            *axis1.last().expect("axis1 must be non-empty"),
        ];
        let spacing = vec![
            if axis0.len() >= 2 {
                axis0[1] - axis0[0]
            } else {
                1.0
            },
            if axis1.len() >= 2 {
                axis1[1] - axis1[0]
            } else {
                1.0
            },
        ];
        SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular2D,
            bounds_min,
            bounds_max,
            spacing,
            axis_grids: vec![axis0, axis1],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

    /// Construct a 3-D `SampledField` from per-axis grid coords and data
    /// (axis-0 outermost, row-major: `data[i0*s1*s2 + i1*s2 + i2]`).
    fn make_sampled_3d(
        name: &str,
        axis0: Vec<f64>,
        axis1: Vec<f64>,
        axis2: Vec<f64>,
        data: Vec<f64>,
    ) -> SampledField {
        let bounds_min = vec![
            *axis0.first().expect("axis0 must be non-empty"),
            *axis1.first().expect("axis1 must be non-empty"),
            *axis2.first().expect("axis2 must be non-empty"),
        ];
        let bounds_max = vec![
            *axis0.last().expect("axis0 must be non-empty"),
            *axis1.last().expect("axis1 must be non-empty"),
            *axis2.last().expect("axis2 must be non-empty"),
        ];
        let spacing = vec![
            if axis0.len() >= 2 {
                axis0[1] - axis0[0]
            } else {
                1.0
            },
            if axis1.len() >= 2 {
                axis1[1] - axis1[0]
            } else {
                1.0
            },
            if axis2.len() >= 2 {
                axis2[1] - axis2[0]
            } else {
                1.0
            },
        ];
        SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular3D,
            bounds_min,
            bounds_max,
            spacing,
            axis_grids: vec![axis0, axis1, axis2],
            interpolation: InterpolationKind::Linear,
            data,
            oob_emitted: AtomicBool::new(false),
        }
    }

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

    // ── single-case behaviour ───────────────────────────────────────────────

    #[test]
    fn envelope_max_single_case_yields_envelope_named_copy() {
        // Single-case Map flows through the same reduction loop as N>=2.
        // Finite values pass through unchanged; the result gets the
        // canonical "envelope" name regardless of case count (no
        // behaviour cliff for downstream tooling that keys off
        // SampledField.name).
        let axis = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let data = vec![1.0, 5.0, 3.0, 4.0, 2.0];
        let sf = make_sampled_1d("f", axis.clone(), data.clone());
        let field = wrap_sampled_field(sf, Type::Real, Type::Real);
        let map = make_envelope_map(&[("only", field)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let result_sf = extract_sampled(&result);

        assert_eq!(result_sf.name, "envelope");
        assert_eq!(result_sf.data, data);
        assert_eq!(result_sf.axis_grids, vec![axis]);
        assert_eq!(result_sf.kind, SampledGridKind::Regular1D);
    }

    #[test]
    fn envelope_min_single_case_yields_envelope_named_copy() {
        let axis = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let data = vec![1.0, 5.0, 3.0, 4.0, 2.0];
        let sf = make_sampled_1d("f", axis.clone(), data.clone());
        let field = wrap_sampled_field(sf, Type::Real, Type::Real);
        let map = make_envelope_map(&[("only", field)]);

        let result = eval_fea("envelope_min", &[map]).unwrap();
        let result_sf = extract_sampled(&result);

        assert_eq!(result_sf.name, "envelope");
        assert_eq!(result_sf.data, data);
    }

    #[test]
    fn envelope_max_single_case_analytical_source_returns_undef() {
        // Sampled-only contract applies uniformly across N=1 and N>1.
        // A single-case Analytical (or any non-Sampled-source) Field
        // must reject to Undef rather than leaking through unchanged.
        let analytical = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let map = make_envelope_map(&[("only", analytical)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    // ── two-case per-grid-point reductions ──────────────────────────────────

    /// Helper: extract the inner SampledField from a Sampled Value::Field.
    fn extract_sampled(v: &Value) -> &SampledField {
        match v {
            Value::Field {
                source: FieldSourceKind::Sampled,
                lambda,
                ..
            } => match lambda.as_ref() {
                Value::SampledField(sf) => sf,
                _ => panic!("expected SampledField in Sampled lambda slot"),
            },
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
    fn envelope_min_skips_nan_per_index() {
        // Mirrors envelope_max_skips_nan_per_index for the find_min path.
        // Same NaN-skip semantics must apply — this pins that find_min
        // doesn't accidentally treat NaN as a participating value (e.g.,
        // by selecting it via partial_cmp behaviour).
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

        let result = eval_fea("envelope_min", &[map]).unwrap();
        let sf = extract_sampled(&result);

        // Per-index min over finite values only: index 0 = min(1.0) = 1.0;
        // index 1 = min(5.0) = 5.0; index 2 = min(3.0, 2.0) = 2.0.
        assert_eq!(sf.data, vec![1.0, 5.0, 2.0]);
    }

    #[test]
    fn envelope_min_all_nan_at_index_yields_nan() {
        // Mirrors envelope_max_all_nan_at_index_yields_nan for find_min.
        // At index 1, case_a=NaN and case_b=+Inf — both non-finite. The
        // result must materialise the all-non-finite NaN sentinel
        // regardless of which extremum (max/min) is requested.
        let axis = vec![0.0, 1.0, 2.0];
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

        let result = eval_fea("envelope_min", &[map]).unwrap();
        let sf = extract_sampled(&result);

        assert_eq!(sf.data[0], 1.0);
        assert!(
            sf.data[1].is_nan(),
            "expected NaN at index 1, got {}",
            sf.data[1]
        );
        assert_eq!(sf.data[2], 2.0);
    }

    #[test]
    fn envelope_max_signed_zero_pins_total_cmp_adoption() {
        // Pins three observable properties of envelope_reduce (find_max path):
        //   1. `total_cmp` adoption: under `partial_cmp`, ±0.0 compare as
        //      Equal, so a `partial_cmp + is_gt` regression always picks
        //      case_a and a `partial_cmp + is_ge` regression always picks
        //      case_b — both yield mixed-sign outputs that fail this test.
        //   2. First-finite-init (weakly): a regression that *replaces* the
        //      accumulator with each successive case AND skips the comparison
        //      leg would bleed case_b's signs through at indices where it
        //      differs — but a "wrong-seed-then-still-compare" regression
        //      passes, because the comparison leg resolves the correct sign
        //      regardless. Robust first-occurrence pinning requires
        //      case-identity output; see the TODO below.
        //   3. Comparison direction (`v.total_cmp(out)` for max): a swapped
        //      direction would apply find_min semantics and yield -0.0 everywhere.
        //
        // What this does NOT pin:
        //   - Strict (`is_gt`) vs non-strict (`is_ge`) tie-break — under
        //     `total_cmp`, +0.0 and -0.0 are distinct (total_cmp returns Less /
        //     Greater), so there is no actual tie here.
        //   - Strong first-occurrence-wins coverage: with these fixtures the
        //     comparison leg alone resolves the correct sign, so most wrong-seed
        //     regressions still pass. Both the seed-direction invariant and the
        //     strict-tie-break invariant are observable only via a future
        //     case-identity-returning reduction (envelope_argmax).
        // TODO(envelope_argmax): add tests that assert *which case* the extremum
        //   came from (not just its value) to pin first-finite-init and strict
        //   tie-break robustly. This is deferred to the envelope_argmax task.
        let axis = vec![0.0, 1.0, 2.0];
        // case_a[i] and case_b[i] have opposite signs.
        // Under total_cmp:  +0.0 > -0.0, so envelope_max must pick +0.0 at every index.
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![0.0, -0.0, 0.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![-0.0, 0.0, -0.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let sf = extract_sampled(&result);

        let pos_zero_bits = 0.0_f64.to_bits();
        for (i, &v) in sf.data.iter().enumerate() {
            assert_eq!(
                v.to_bits(),
                pos_zero_bits,
                "index {i}: expected +0.0 (total_cmp max), got bit pattern {:064b}",
                v.to_bits()
            );
        }
    }

    #[test]
    fn envelope_min_signed_zero_pins_total_cmp_adoption() {
        // Mirrors envelope_max_signed_zero_pins_total_cmp_adoption for the
        // find_min path.  Under total_cmp:  -0.0 < +0.0, so envelope_min
        // must pick -0.0 at every index.
        //
        // Pins: total_cmp adoption and comparison direction for the min path.
        // First-finite-init is only weakly covered (see the max variant above
        // for the detailed reasoning and the shared TODO(envelope_argmax)).
        // Does NOT pin strict vs non-strict tie-break (same reasoning).
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![0.0, -0.0, 0.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![-0.0, 0.0, -0.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_min", &[map]).unwrap();
        let sf = extract_sampled(&result);

        let neg_zero_bits = (-0.0_f64).to_bits();
        for (i, &v) in sf.data.iter().enumerate() {
            assert_eq!(
                v.to_bits(),
                neg_zero_bits,
                "index {i}: expected -0.0 (total_cmp min), got bit pattern {:064b}",
                v.to_bits()
            );
        }
    }

    // ── empty-Map edge ──────────────────────────────────────────────────────

    #[test]
    fn envelope_max_empty_map_returns_undef() {
        let map = Value::Map(BTreeMap::new());
        let result = eval_fea("envelope_max", &[map]).unwrap();
        assert!(result.is_undef());
    }

    #[test]
    fn envelope_min_empty_map_returns_undef() {
        let map = Value::Map(BTreeMap::new());
        let result = eval_fea("envelope_min", &[map]).unwrap();
        assert!(result.is_undef());
    }

    // ── grid / type mismatch rejection ──────────────────────────────────────

    #[test]
    fn envelope_max_grid_axis_lengths_mismatch_returns_undef() {
        let case_a = wrap_sampled_field(
            make_sampled_1d(
                "a",
                vec![0.0, 1.0, 2.0, 3.0, 4.0],
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
            ),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_grid_bounds_min_mismatch_returns_undef() {
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", vec![1.0, 2.0, 3.0, 4.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_grid_kind_mismatch_returns_undef() {
        // case_a is Regular1D (4 grid points), case_b is Regular2D (2x2=4 grid
        // points). Same data length so any data-length-only check would miss
        // this; the grid-kind / axis-count check rejects.
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_2d(
                "b",
                vec![0.0, 1.0],
                vec![0.0, 1.0],
                vec![1.0, 2.0, 3.0, 4.0],
            ),
            Type::Real,
            Type::Real,
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_codomain_type_mismatch_returns_undef() {
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    // ── argument-shape negative paths ───────────────────────────────────────
    //
    // Each branch of the argument-shape contract gets its own focused test
    // so a regression points directly at the failing rejection path rather
    // than reporting a generic bundled-test failure.
    //
    // Helpers below are parametric on `name` ("envelope_max" / "envelope_min")
    // so each #[test] shell is a one-liner and a regression bisects directly
    // to the failing branch.

    fn assert_zero_args_returns_undef(name: &str) {
        assert!(eval_fea(name, &[]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_zero_args_returns_undef() {
        // arity must be exactly 1 (Map<String, Field>).
        assert_zero_args_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_zero_args_returns_undef() {
        assert_zero_args_returns_undef("envelope_min");
    }

    fn assert_two_args_returns_undef(name: &str) {
        let map = make_envelope_map(&[]);
        let extra = Value::Real(1.0);
        assert!(eval_fea(name, &[map, extra]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_two_args_returns_undef() {
        assert_two_args_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_two_args_returns_undef() {
        assert_two_args_returns_undef("envelope_min");
    }

    fn assert_non_map_arg_returns_undef(name: &str) {
        assert!(eval_fea(name, &[Value::Real(1.0)]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_non_map_arg_returns_undef() {
        assert_non_map_arg_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_non_map_arg_returns_undef() {
        assert_non_map_arg_returns_undef("envelope_min");
    }

    fn assert_map_with_non_field_value_returns_undef(name: &str) {
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let mut bad_map = BTreeMap::new();
        bad_map.insert(Value::String("a".to_string()), case_a);
        bad_map.insert(Value::String("b".to_string()), Value::Real(7.0));
        assert!(eval_fea(name, &[Value::Map(bad_map)]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_map_with_non_field_value_returns_undef() {
        assert_map_with_non_field_value_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_map_with_non_field_value_returns_undef() {
        assert_map_with_non_field_value_returns_undef("envelope_min");
    }

    fn assert_analytical_source_returns_undef(name: &str) {
        // Field with FieldSourceKind::Analytical (source != Sampled) → Undef.
        // The source check rejects before any lambda extraction, so we
        // don't need a real lambda body.
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let analytical = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let map = make_envelope_map(&[("a", case_a), ("b", analytical)]);
        assert!(eval_fea(name, &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_analytical_source_returns_undef() {
        assert_analytical_source_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_analytical_source_returns_undef() {
        assert_analytical_source_returns_undef("envelope_min");
    }

    fn assert_sampled_with_non_sampledfield_lambda_returns_undef(name: &str) {
        // Defensive check — a Sampled-source Field whose lambda is NOT a
        // SampledField rejects to Undef. Mirrors the defensive arms in
        // field_reductions.rs:96-99 ("a Sampled source must carry a
        // SampledField in its lambda slot").
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let degenerate_sampled = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef),
        };
        let map = make_envelope_map(&[("a", case_a), ("b", degenerate_sampled)]);
        assert!(eval_fea(name, &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_sampled_source_with_non_sampledfield_lambda_returns_undef() {
        assert_sampled_with_non_sampledfield_lambda_returns_undef("envelope_max");
    }

    #[test]
    fn envelope_min_sampled_source_with_non_sampledfield_lambda_returns_undef() {
        assert_sampled_with_non_sampledfield_lambda_returns_undef("envelope_min");
    }

    // ── FEA-realistic 3-D Point3 / Pressure shape ──────────────────────────

    #[test]
    fn envelope_max_3d_point3_domain_returns_per_grid_max() {
        // 2×2×2 = 8-point Regular3D grid. Chosen so that:
        //   case_a beats case_b at indices 0, 2, 5 (per-grid maxima from a)
        //   case_b beats case_a at indices 1, 3, 4, 6, 7 (per-grid maxima from b)
        let axis = vec![0.0, 1.0];
        let domain = Type::Point {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        };
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };

        let case_a = wrap_sampled_field(
            make_sampled_3d(
                "a",
                axis.clone(),
                axis.clone(),
                axis.clone(),
                vec![10.0, 2.0, 20.0, 4.0, 3.0, 30.0, 6.0, 7.0],
            ),
            domain.clone(),
            pressure.clone(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_3d(
                "b",
                axis.clone(),
                axis.clone(),
                axis.clone(),
                vec![5.0, 8.0, 15.0, 12.0, 9.0, 25.0, 14.0, 18.0],
            ),
            domain.clone(),
            pressure.clone(),
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);

        let result = eval_fea("envelope_max", &[map]).unwrap();
        let sf = extract_sampled(&result);

        // Grid kind, axis_grids, bounds, spacing all preserved from refs.
        assert_eq!(sf.kind, SampledGridKind::Regular3D);
        assert_eq!(
            sf.axis_grids,
            vec![axis.clone(), axis.clone(), axis.clone()]
        );
        assert_eq!(sf.bounds_min, vec![0.0, 0.0, 0.0]);
        assert_eq!(sf.bounds_max, vec![1.0, 1.0, 1.0]);
        assert_eq!(sf.spacing, vec![1.0, 1.0, 1.0]);

        // Per-index max across cases.
        assert_eq!(sf.data, vec![10.0, 8.0, 20.0, 12.0, 9.0, 30.0, 14.0, 18.0]);

        // Outer Value::Field domain/codomain types are propagated unchanged.
        match &result {
            Value::Field {
                domain_type,
                codomain_type,
                source,
                ..
            } => {
                assert_eq!(*domain_type, domain);
                assert_eq!(*codomain_type, pressure);
                assert!(matches!(source, FieldSourceKind::Sampled));
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }
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
        assert!(
            sf.data[1].is_nan(),
            "expected NaN at index 1, got {}",
            sf.data[1]
        );
        assert_eq!(sf.data[2], 3.0);
    }

    // ── case_names helpers and tests ────────────────────────────────────────

    /// Build a `MultiCaseResult`-shaped `Value::Map` for unit tests.
    ///
    /// Runtime struct instances are `Value::Map<Value::String, Value>` keyed
    /// by field names (no `Value::Structure` variant exists). A
    /// `MultiCaseResult` has one field `cases` whose value is a
    /// `Value::Map<Value::String, Value>` of per-case entries. This helper
    /// constructs the outer struct-instance Map from a `(case_name, Value)`
    /// slice, letting callers pass arbitrary Values as case entries (fixture
    /// ElasticResult Maps, Value::Int sentinels, etc.).
    fn make_multi_case_result_value(cases: &[(&str, Value)]) -> Value {
        let mut inner = BTreeMap::new();
        for (name, val) in cases {
            inner.insert(Value::String((*name).to_string()), val.clone());
        }
        let mut outer = BTreeMap::new();
        outer.insert(
            Value::String("cases".to_string()),
            Value::Map(inner),
        );
        Value::Map(outer)
    }

    /// Build a minimal fixture `ElasticResult`-shaped Map for case values.
    ///
    /// Field values are placeholders (Int(0) / Bool / Real) sufficient to
    /// distinguish cases by value in assertions; the exact field semantics
    /// don't matter for `case_names` / `result_for` tests.
    fn make_fixture_elastic_result(iterations: i64) -> Value {
        let mut m = BTreeMap::new();
        m.insert(Value::String("displacement".to_string()), Value::Real(0.0));
        m.insert(Value::String("stress".to_string()), Value::Real(0.0));
        m.insert(Value::String("max_von_mises".to_string()), Value::Real(0.0));
        m.insert(Value::String("converged".to_string()), Value::Bool(true));
        m.insert(Value::String("iterations".to_string()), Value::Int(iterations));
        Value::Map(m)
    }

    // ── case_names dispatcher signal ────────────────────────────────────────

    #[test]
    fn case_names_dispatcher_returns_some() {
        // `case_names` must be a recognised FEA function name — `eval_fea`
        // must return `Some(_)`, not `None`. The actual value may be Undef
        // (wrong arity), but `None` would mean the arm is missing.
        assert!(eval_fea("case_names", &[]).is_some());
    }

    // ── case_names happy path ────────────────────────────────────────────────

    #[test]
    fn case_names_returns_sorted_keys_as_list_of_strings() {
        // Three cases with names that sort lexicographically:
        //   "operating" < "overload" < "transport"
        // BTreeMap natural order is lexicographic on Value::String, so the
        // returned list must be in this order regardless of insertion order.
        let er_op = make_fixture_elastic_result(10);
        let er_ov = make_fixture_elastic_result(20);
        let er_tr = make_fixture_elastic_result(30);
        let mcr = make_multi_case_result_value(&[
            ("transport", er_tr),
            ("operating", er_op),
            ("overload", er_ov),
        ]);

        let result = eval_fea("case_names", &[mcr]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::String("operating".to_string()),
                Value::String("overload".to_string()),
                Value::String("transport".to_string()),
            ]),
            "case_names should return keys in BTreeMap lexicographic order"
        );
    }

    // ── case_names argument-shape negative paths ─────────────────────────────

    #[test]
    fn case_names_zero_args_returns_undef() {
        assert!(eval_fea("case_names", &[]).unwrap().is_undef());
    }

    #[test]
    fn case_names_two_args_returns_undef() {
        let mcr = make_multi_case_result_value(&[]);
        assert!(
            eval_fea("case_names", &[mcr, Value::String("extra".to_string())])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn case_names_non_map_arg_returns_undef() {
        assert!(eval_fea("case_names", &[Value::Int(42)]).unwrap().is_undef());
    }

    #[test]
    fn case_names_map_without_cases_field_returns_undef() {
        // A Map without a "cases" key is not a valid MultiCaseResult struct.
        let mut m = BTreeMap::new();
        m.insert(Value::String("other_field".to_string()), Value::Int(1));
        assert!(
            eval_fea("case_names", &[Value::Map(m)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn case_names_cases_field_non_map_returns_undef() {
        // A Map with "cases" key but non-Map value is malformed.
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("cases".to_string()),
            Value::Int(99), // not a Map
        );
        assert!(
            eval_fea("case_names", &[Value::Map(m)])
                .unwrap()
                .is_undef()
        );
    }
}
