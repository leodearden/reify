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

use std::collections::BTreeMap;
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
        "result_for" => result_for(args),
        "linear_combine" => linear_combine(args),
        _ => return None,
    })
}

/// Weighted linear superposition of per-case `ElasticResult` displacement and
/// stress fields from a `MultiCaseResult`.
///
/// # Input shape
///
/// `args == [MultiCaseResult-shaped Map, Map<Value::String, numeric>]`
///
/// - `args[0]`: A `MultiCaseResult` struct instance
///   (`Value::Map { "cases" -> Value::Map<Value::String, ElasticResult-Map> }`).
/// - `args[1]`: A non-empty `Value::Map<Value::String, numeric>` of (case name,
///   weight) pairs. Accepted weight types are `Value::Real`, `Value::Int`,
///   and `Value::Scalar` **with a dimensionless dimension** (i.e.
///   `dimension.is_dimensionless()` is true). A `Value::Scalar` with a
///   non-dimensionless dimension (e.g. `1.4 m`) is explicitly rejected to
///   `Value::Undef` — the contract is stated here in production code rather
///   than relying on test coverage alone (Task 2544 convention). Non-finite
///   values — NaN, ±Inf — also reject to `Value::Undef`.
///
/// # Output
///
/// A synthesised `ElasticResult`-shaped `Value::Map` with keys:
///   - `displacement`: combined Sampled Field (weighted sum, name="linear_combine")
///   - `stress`:       combined Sampled Field (weighted sum, name="linear_combine")
///   - `frame`:        `Value::Undef` (tet-elastic convention per solver_elastic.ri:282-289)
///   - `max_von_mises`: `Value::Real(max(|combined_stress.data|))` over finite data,
///     or `Value::Undef` when the stress buffer is empty or contains no finite values
///   - `converged`:   `Value::Bool(true)`
///   - `iterations`:  `Value::Undef` (synthesised, not solved — distinguishes from solver-converged-on-iter-0)
///
/// # Failure modes (silent-Undef per PRD task #10 deferral)
///
/// - arity != 2
/// - `args[0]` is not a valid `MultiCaseResult` (non-Map / no `cases` key /
///   `cases` not a Map)
/// - `args[1]` is not `Value::Map` or is empty
/// - any weight key is not `Value::String`
/// - any weight value is not `Value::Real`, `Value::Int`, or a dimensionless
///   `Value::Scalar` (i.e. `Value::Scalar` with non-dimensionless dimension
///   such as `1.4 m` is rejected)
/// - any weight value has a non-finite representation (NaN, ±Inf)
/// - a weight name is absent from `base_results.cases`
/// - a case value is not `Value::Map`
/// - a case Map is missing `displacement` or `stress` key
/// - a displacement or stress field is not Sampled-source
/// - a displacement or stress field has Sampled-source but non-SampledField lambda
/// - displacement or stress fields across cases fail `metadata_matches`
///   (grid, domain_type, codomain_type inequality)
fn linear_combine(args: &[Value]) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }

    // Unwrap args[0] as a MultiCaseResult (via extract_cases_map).
    let cases_map = match extract_cases_map(&args[0]) {
        Some(m) => m,
        None => return Value::Undef,
    };

    // args[1] must be a non-empty Map<String, numeric>.
    let weights_map = match &args[1] {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    if weights_map.is_empty() {
        return Value::Undef;
    }

    // Validate weights: collect (weight: f64, case_map: &BTreeMap) pairs.
    // Each entry must have a String key, a finite numeric weight, a known case
    // name, and a Map-typed case entry.
    let mut weighted_cases: Vec<(f64, &BTreeMap<Value, Value>)> =
        Vec::with_capacity(weights_map.len());
    for (name_val, weight_val) in weights_map {
        // Key must be a string.
        let case_name = match name_val {
            Value::String(s) => s,
            _ => return Value::Undef,
        };
        // Weight must be Real, Int, or a dimensionless Scalar.
        // Explicit pattern match makes the dimensionless-only contract visible
        // in production code (Task 2544 convention: contract in impl, not just tests).
        let weight = match weight_val {
            Value::Real(r) => *r,
            Value::Int(i) => *i as f64,
            Value::Scalar { si_value, dimension } if dimension.is_dimensionless() => *si_value,
            _ => return Value::Undef,
        };
        // Non-finite weights (NaN, ±Inf) would poison the accumulator — reject.
        if !weight.is_finite() {
            return Value::Undef;
        }
        // Case name must exist in base_results.cases.
        let case_val = match cases_map.get(&Value::String(case_name.clone())) {
            Some(v) => v,
            None => return Value::Undef,
        };
        // Case value must be a Map (ElasticResult struct instance).
        let case_map = match case_val {
            Value::Map(m) => m,
            _ => return Value::Undef,
        };
        weighted_cases.push((weight, case_map));
    }

    // Borrow the first weighted case's sampled fields as the reference for
    // metadata and types. Only lightweight metadata (kind, bounds, spacing,
    // axis_grids, types) is cloned for the output; the data buffer is accessed
    // via slice — no per-case Vec<f64> clone for large field buffers.
    // Safety: weighted_cases is non-empty by the is_empty() guard above.
    let ref_cm = weighted_cases[0].1;
    let ref_disp_val = match ref_cm.get(&Value::String("displacement".to_string())) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let (ref_disp_dom, ref_disp_cod, ref_disp_sf) = match as_sampled_field(ref_disp_val) {
        Some(t) => t,
        None => return Value::Undef,
    };
    let ref_stress_val = match ref_cm.get(&Value::String("stress".to_string())) {
        Some(v) => v,
        None => return Value::Undef,
    };
    let (ref_stress_dom, ref_stress_cod, ref_stress_sf) = match as_sampled_field(ref_stress_val) {
        Some(t) => t,
        None => return Value::Undef,
    };

    let n_disp = ref_disp_sf.data.len();
    let n_stress = ref_stress_sf.data.len();

    // Initialise accumulators to 0.0.
    let mut combined_disp: Vec<f64> = vec![0.0; n_disp];
    let mut combined_stress: Vec<f64> = vec![0.0; n_stress];

    // Outer loop over weighted cases; inner loop over indices.
    // Mirrors envelope_reduce's outer-cases / inner-indices nesting for
    // vectorisation and bounds-check-free iteration.
    // Borrows each case's SampledField data slice — no per-case Vec clone.
    for (i, (weight, case_map)) in weighted_cases.iter().enumerate() {
        let disp_val = match case_map.get(&Value::String("displacement".to_string())) {
            Some(v) => v,
            None => return Value::Undef,
        };
        let (dom_d, cod_d, sf_d) = match as_sampled_field(disp_val) {
            Some(t) => t,
            None => return Value::Undef,
        };
        let stress_val = match case_map.get(&Value::String("stress".to_string())) {
            Some(v) => v,
            None => return Value::Undef,
        };
        let (dom_s, cod_s, sf_s) = match as_sampled_field(stress_val) {
            Some(t) => t,
            None => return Value::Undef,
        };

        // Validate metadata against reference (skip first case — it IS the ref).
        if i > 0 {
            if !metadata_matches(ref_disp_sf, sf_d, ref_disp_dom, ref_disp_cod, dom_d, cod_d) {
                return Value::Undef;
            }
            if !metadata_matches(ref_stress_sf, sf_s, ref_stress_dom, ref_stress_cod, dom_s, cod_s) {
                return Value::Undef;
            }
        }

        for (out, &v) in combined_disp.iter_mut().zip(sf_d.data.iter()) {
            *out += weight * v;
        }
        for (out, &v) in combined_stress.iter_mut().zip(sf_s.data.iter()) {
            *out += weight * v;
        }
    }

    // Compute max_von_mises: max(|combined_stress|) over finite values.
    // Scalar interpretation (pre-task-#3117); finite-only for NaN-safety.
    // Uses reduce(f64::max) rather than fold(0.0, f64::max) so that an empty
    // or all-non-finite filter yields None → Value::Undef, distinguishing
    // "no finite data" from "genuine zero stress". Mirrors envelope_reduce's
    // NaN-sentinel discipline at fea.rs:517-524.
    let mvm: Option<f64> = combined_stress
        .iter()
        .filter(|v| v.is_finite())
        .map(|v| v.abs())
        .reduce(f64::max);
    let mvm_value = match mvm {
        Some(v) => Value::Real(v),
        None => Value::Undef,
    };

    // Build output SampledFields: clone only lightweight metadata from reference;
    // the accumulated data buffers are moved directly into the output.
    let out_disp_sf = SampledField {
        name: "linear_combine".to_string(),
        kind: ref_disp_sf.kind,
        bounds_min: ref_disp_sf.bounds_min.clone(),
        bounds_max: ref_disp_sf.bounds_max.clone(),
        spacing: ref_disp_sf.spacing.clone(),
        axis_grids: ref_disp_sf.axis_grids.clone(),
        interpolation: ref_disp_sf.interpolation,
        data: combined_disp,
        oob_emitted: AtomicBool::new(false),
    };
    let out_stress_sf = SampledField {
        name: "linear_combine".to_string(),
        kind: ref_stress_sf.kind,
        bounds_min: ref_stress_sf.bounds_min.clone(),
        bounds_max: ref_stress_sf.bounds_max.clone(),
        spacing: ref_stress_sf.spacing.clone(),
        axis_grids: ref_stress_sf.axis_grids.clone(),
        interpolation: ref_stress_sf.interpolation,
        data: combined_stress,
        oob_emitted: AtomicBool::new(false),
    };

    let out_disp_field = Value::Field {
        domain_type: ref_disp_dom.clone(),
        codomain_type: ref_disp_cod.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(out_disp_sf)),
    };
    let out_stress_field = Value::Field {
        domain_type: ref_stress_dom.clone(),
        codomain_type: ref_stress_cod.clone(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(out_stress_sf)),
    };

    // Build the synthesised ElasticResult Map.
    let mut result_map = BTreeMap::new();
    result_map.insert(Value::String("displacement".to_string()), out_disp_field);
    result_map.insert(Value::String("stress".to_string()), out_stress_field);
    result_map.insert(Value::String("frame".to_string()), Value::Undef);
    result_map.insert(Value::String("max_von_mises".to_string()), mvm_value);
    result_map.insert(Value::String("converged".to_string()), Value::Bool(true));
    // iterations = Undef: synthesised result, not solved — same rationale as
    // frame: Value::Undef above. Distinguishes from solver-converged-on-iter-0.
    // .ri audit (task 3246): fea_multi_case.ri:143 doc-comment is stale
    // (says "0"; doc alignment deferred per FILES_TO_MODIFY scope).
    // solver_elastic.ri's `iterations : Int` field belongs to the
    // solver-produced ElasticResult, not to linear_combine's synthesised
    // output — no runtime .ri code pattern-matches Int on this field.
    result_map.insert(Value::String("iterations".to_string()), Value::Undef);

    Value::Map(result_map)
}

/// Extract the inner `cases` `BTreeMap` from a `MultiCaseResult` struct
/// instance (`Value::Map { "cases" -> Value::Map }`).
///
/// Returns `Some` with a reference to the inner BTreeMap when the shape
/// matches, or `None` on any shape mismatch:
///   - `arg` is not `Value::Map`
///   - outer Map has no `"cases"` key
///   - `"cases"` value is not `Value::Map`
///
/// Factored out to avoid the four-step boilerplate duplicated between
/// `case_names` and `result_for`. Every future accessor that reads `cases`
/// from a `MultiCaseResult` instance should route through this helper.
fn extract_cases_map(arg: &Value) -> Option<&BTreeMap<Value, Value>> {
    let outer = match arg {
        Value::Map(m) => m,
        _ => return None,
    };
    match outer.get(&Value::String("cases".to_string())) {
        Some(Value::Map(m)) => Some(m),
        _ => None,
    }
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
///   - `args[0]` is not `Value::Map` or has no `"cases"` key mapping to a Map
///
/// Diagnostic emission is deferred to PRD task #10 (Diagnostic mapping for
/// multi-case-specific failure modes).
fn case_names(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match extract_cases_map(&args[0]) {
        Some(cases) => Value::List(cases.keys().cloned().collect()),
        None => Value::Undef,
    }
}

/// Look up a single case by name from a `MultiCaseResult` struct instance.
///
/// # Input shape
///
/// `args == [Value::Map { "cases" -> Value::Map<Value::String, Value> }, Value::String(key)]`
///
/// The first arg is the `MultiCaseResult` struct instance (field-keyed by
/// `Value::String`). The second arg is the case name to look up.
///
/// # Output
///
/// The `Value` stored at `cases[key]` (an `ElasticResult` Map), or
/// `Value::Undef` if the key is absent from the Map.
///
/// # Failure modes
///
/// All argument-shape failures collapse to `Value::Undef` (silent-Undef
/// discipline, mirroring `envelope_reduce`):
///   - arity != 2
///   - `args[0]` is not `Value::Map`
///   - `args[1]` is not `Value::String`
///   - outer Map has no `"cases"` key
///   - `"cases"` value is not `Value::Map`
///   - key is absent from the `cases` Map (missing key → silent Undef
///     per PRD task #10 deferral; matches the `envelope_*` convention)
///
/// Diagnostic emission is deferred to PRD task #10.
fn result_for(args: &[Value]) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    let cases = match extract_cases_map(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };
    let key = match &args[1] {
        Value::String(s) => s,
        _ => return Value::Undef,
    };
    cases
        .get(&Value::String(key.clone()))
        .cloned()
        .unwrap_or(Value::Undef)
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
    // Use as_sampled_field to extract the reference case; avoids duplicating
    // the three-level match pattern shared with linear_combine.
    let (ref_domain, ref_codomain, ref_sf): (&Type, &Type, &SampledField) = match first {
        Some(v) => match as_sampled_field(v) {
            Some(t) => t,
            // Defensive: non-Sampled source or Sampled with non-SampledField lambda.
            None => return Value::Undef,
        },
        // Empty Map (already guarded above, but kept for exhaustiveness).
        None => return Value::Undef,
    };

    // Collect per-case data slices, validating metadata equality with
    // the reference along the way. Mismatched grids/types → Undef.
    let mut cases_data: Vec<&[f64]> = Vec::with_capacity(map.len());
    cases_data.push(&ref_sf.data);
    for v in iter {
        let (dom, cod, sf) = match as_sampled_field(v) {
            Some(t) => t,
            None => return Value::Undef,
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

/// Extract `(&domain_type, &codomain_type, &SampledField)` from a
/// `Value::Field` whose `source` is `Sampled` and whose `lambda` slot holds
/// a `Value::SampledField`. Returns `None` on any mismatch:
///   - value is not `Value::Field`
///   - `source` is not `FieldSourceKind::Sampled`
///   - `lambda` does not hold a `Value::SampledField`
///
/// Used by both `envelope_reduce` and `linear_combine` to avoid duplicating
/// the three-level match pattern. Returning borrows avoids cloning the
/// (potentially large) data buffer — callers only clone lightweight metadata
/// (`bounds_min/max`, `spacing`, `axis_grids`) when constructing output.
fn as_sampled_field(v: &Value) -> Option<(&Type, &Type, &SampledField)> {
    match v {
        Value::Field {
            source: FieldSourceKind::Sampled,
            lambda,
            domain_type,
            codomain_type,
        } => match lambda.as_ref() {
            Value::SampledField(sf) => Some((domain_type, codomain_type, sf)),
            _ => None,
        },
        _ => None,
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

    // ── result_for dispatcher signal ─────────────────────────────────────────

    #[test]
    fn result_for_dispatcher_returns_some() {
        // `result_for` must be a recognised FEA function name — `eval_fea`
        // must return `Some(_)`, not `None`. The actual value may be Undef
        // (wrong arity), but `None` would mean the arm is missing.
        assert!(eval_fea("result_for", &[]).is_some());
    }

    // ── result_for happy path ────────────────────────────────────────────────

    #[test]
    fn result_for_existing_key_returns_the_elastic_result_value() {
        // Fixture: MCR with one case "operating" whose ElasticResult has
        // iterations=42 as a recognisable distinguishing value.
        let er_op = make_fixture_elastic_result(42);
        let mcr = make_multi_case_result_value(&[("operating", er_op.clone())]);

        let result = eval_fea(
            "result_for",
            &[mcr, Value::String("operating".to_string())],
        )
        .unwrap();

        assert_eq!(
            result, er_op,
            "result_for should return the exact ElasticResult value for the key"
        );
    }

    #[test]
    fn result_for_missing_key_returns_undef() {
        let er_op = make_fixture_elastic_result(42);
        let mcr = make_multi_case_result_value(&[("operating", er_op)]);

        let result = eval_fea(
            "result_for",
            &[mcr, Value::String("missing".to_string())],
        )
        .unwrap();

        assert!(
            result.is_undef(),
            "result_for with a missing key should return Undef (silent-Undef per PRD task #10)"
        );
    }

    // ── result_for argument-shape negative paths ─────────────────────────────

    #[test]
    fn result_for_zero_args_returns_undef() {
        assert!(eval_fea("result_for", &[]).unwrap().is_undef());
    }

    #[test]
    fn result_for_one_arg_returns_undef() {
        // arity must be exactly 2 (mcr, key)
        let mcr = make_multi_case_result_value(&[]);
        assert!(eval_fea("result_for", &[mcr]).unwrap().is_undef());
    }

    #[test]
    fn result_for_three_args_returns_undef() {
        let mcr = make_multi_case_result_value(&[]);
        assert!(
            eval_fea(
                "result_for",
                &[mcr, Value::String("k".to_string()), Value::Int(3)]
            )
            .unwrap()
            .is_undef()
        );
    }

    #[test]
    fn result_for_non_map_first_arg_returns_undef() {
        assert!(
            eval_fea("result_for", &[Value::Int(1), Value::String("k".to_string())])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn result_for_non_string_key_returns_undef() {
        // Second arg must be Value::String — passing e.g. Value::Real rejects.
        let mcr = make_multi_case_result_value(&[]);
        assert!(
            eval_fea("result_for", &[mcr, Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn result_for_map_without_cases_field_returns_undef() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("other".to_string()), Value::Int(1));
        assert!(
            eval_fea(
                "result_for",
                &[Value::Map(m), Value::String("k".to_string())]
            )
            .unwrap()
            .is_undef()
        );
    }

    #[test]
    fn result_for_cases_field_non_map_returns_undef() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("cases".to_string()), Value::Int(99));
        assert!(
            eval_fea(
                "result_for",
                &[Value::Map(m), Value::String("k".to_string())]
            )
            .unwrap()
            .is_undef()
        );
    }

    // ── linear_combine dispatcher signal ────────────────────────────────────

    #[test]
    fn linear_combine_dispatcher_returns_some() {
        // `linear_combine` must be a recognised FEA function name — `eval_fea`
        // must return `Some(_)`, not `None`. The actual value may be Undef
        // (wrong arity), but `None` would mean the arm is missing.
        assert!(eval_fea("linear_combine", &[]).is_some());
    }

    #[test]
    fn linear_combine_zero_args_returns_undef() {
        // arity must be exactly 2 (base_results, weights).
        assert!(eval_fea("linear_combine", &[]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_three_args_returns_undef() {
        // arity must be exactly 2 — three args rejects.
        let a = Value::Int(1);
        let b = Value::Int(2);
        let c = Value::Int(3);
        assert!(eval_fea("linear_combine", &[a, b, c]).unwrap().is_undef());
    }

    // ── linear_combine base/weights argument-shape rejection ────────────────

    #[test]
    fn linear_combine_non_map_base_returns_undef() {
        // args[0] must be a Map — an Int rejects immediately.
        let weights = Value::Map(BTreeMap::new());
        assert!(
            eval_fea("linear_combine", &[Value::Int(42), weights])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_base_without_cases_field_returns_undef() {
        // A Map without a "cases" key is not a valid MultiCaseResult struct.
        let mut m = BTreeMap::new();
        m.insert(Value::String("other_field".to_string()), Value::Int(1));
        let base = Value::Map(m);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("A".to_string()), Value::Real(1.0));
        let weights = Value::Map(weights_map);
        assert!(
            eval_fea("linear_combine", &[base, weights])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_base_cases_field_non_map_returns_undef() {
        // "cases" key present but value is not a Map.
        let mut m = BTreeMap::new();
        m.insert(Value::String("cases".to_string()), Value::Int(99));
        let base = Value::Map(m);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("A".to_string()), Value::Real(1.0));
        let weights = Value::Map(weights_map);
        assert!(
            eval_fea("linear_combine", &[base, weights])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_non_map_weights_returns_undef() {
        // args[1] must be a Map — a Real rejects immediately.
        let mcr = make_multi_case_result_value(&[]);
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_empty_weights_returns_undef() {
        // weights map must be non-empty.
        let mcr = make_multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
        let empty_weights = Value::Map(BTreeMap::new());
        assert!(
            eval_fea("linear_combine", &[mcr, empty_weights])
                .unwrap()
                .is_undef()
        );
    }

    // ── linear_combine weights iteration validation ──────────────────────────

    #[test]
    fn linear_combine_non_string_weight_key_returns_undef() {
        // Weight keys must be Value::String — Int key rejects.
        let mcr = make_multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::Int(7), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(weights_map)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_non_numeric_weight_value_returns_undef() {
        // Weight values must be numeric (as_f64() returns Some) — String rejects.
        let mcr = make_multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(
            Value::String("A".to_string()),
            Value::String("oops".to_string()),
        );
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(weights_map)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_unknown_case_name_returns_undef() {
        // Weight references a case name absent from base_results.cases.
        let mcr = make_multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("missing".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(weights_map)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_case_value_not_a_map_returns_undef() {
        // base_results.cases["A"] is Value::Int(123) — not a Map.
        let mcr = make_multi_case_result_value(&[("A", Value::Int(123))]);
        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("A".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(weights_map)])
                .unwrap()
                .is_undef()
        );
    }

    // ── linear_combine test helpers ─────────────────────────────────────────

    /// Build a fixture `ElasticResult`-shaped Map with Field-typed
    /// displacement and stress fields. Used by linear_combine tests.
    ///
    /// Unlike `make_fixture_elastic_result` (which uses Value::Real(0.0)
    /// placeholders), this variant accepts actual Field-typed Values for
    /// displacement and stress — required for linear_combine which reads
    /// and arithmetic-combines those Fields.
    fn make_fixture_elastic_result_with_fields(
        displacement: Value,
        stress: Value,
    ) -> Value {
        let mut m = BTreeMap::new();
        m.insert(Value::String("displacement".to_string()), displacement);
        m.insert(Value::String("stress".to_string()), stress);
        m.insert(Value::String("max_von_mises".to_string()), Value::Real(0.0));
        m.insert(Value::String("converged".to_string()), Value::Bool(true));
        m.insert(Value::String("iterations".to_string()), Value::Int(0));
        Value::Map(m)
    }

    // ── linear_combine happy path ────────────────────────────────────────────

    #[test]
    fn linear_combine_single_case_weight_two_doubles_displacement_and_stress() {
        // Single-case MultiCaseResult where case "A" has Sampled 1-D Real-codomain
        // fields. Calling linear_combine with weight=2.0 should double all data.
        let axis = vec![0.0, 1.0, 2.0];

        let disp_sf = make_sampled_1d("disp", axis.clone(), vec![1.0, 2.0, 3.0]);
        let disp_field = wrap_sampled_field(disp_sf, Type::Real, Type::Real);

        let stress_sf = make_sampled_1d("stress", axis.clone(), vec![10.0, 20.0, 30.0]);
        let stress_field = wrap_sampled_field(stress_sf, Type::Real, Type::Real);

        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);

        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("A".to_string()), Value::Real(2.0));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(weights_map)]).unwrap();

        // Result must be a Map (struct instance), not Undef.
        assert!(
            !result.is_undef(),
            "linear_combine should return a Map, not Undef"
        );
        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        // Check displacement field: data should be [2.0, 4.0, 6.0].
        let disp = result_map
            .get(&Value::String("displacement".to_string()))
            .expect("result must have 'displacement' key");
        let disp_sf = extract_sampled(disp);
        assert_eq!(disp_sf.data, vec![2.0, 4.0, 6.0], "displacement data should be 2x input");

        // Check stress field: data should be [20.0, 40.0, 60.0].
        let stress = result_map
            .get(&Value::String("stress".to_string()))
            .expect("result must have 'stress' key");
        let stress_sf = extract_sampled(stress);
        assert_eq!(stress_sf.data, vec![20.0, 40.0, 60.0], "stress data should be 2x input");

        // frame must be Undef (tet-elastic convention).
        let frame = result_map
            .get(&Value::String("frame".to_string()))
            .expect("result must have 'frame' key");
        assert!(frame.is_undef(), "frame must be Value::Undef");

        // max_von_mises = max(|[20,40,60]|) = 60.0.
        let mvm = result_map
            .get(&Value::String("max_von_mises".to_string()))
            .expect("result must have 'max_von_mises' key");
        assert_eq!(*mvm, Value::Real(60.0), "max_von_mises should be 60.0");

        // converged = true.
        let converged = result_map
            .get(&Value::String("converged".to_string()))
            .expect("result must have 'converged' key");
        assert_eq!(*converged, Value::Bool(true));

        // iterations = Undef (synthesised, not solved — distinguishes from solver-converged-on-iter-0).
        let iterations = result_map
            .get(&Value::String("iterations".to_string()))
            .expect("result must have 'iterations' key");
        assert_eq!(*iterations, Value::Undef);
    }

    // ── linear_combine multi-case LRFD happy path ───────────────────────────

    fn approx_eq_slice(a: &[f64], b: &[f64], tol: f64) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b.iter())
                .all(|(x, y)| (x - y).abs() <= tol)
    }

    #[test]
    fn linear_combine_lrfd_d_and_l_produces_correct_weighted_sum() {
        // Two cases "D" and "L" with the LRFD combination 1.4D + 1.7L.
        // Expected combined disp: [1.4*1+1.7*10, 1.4*2+1.7*20] = [18.4, 36.8]
        // Expected combined stress: [1.4*100+1.7*1000, 1.4*200+1.7*2000] = [1840, 3680]
        let axis = vec![0.0, 1.0];

        let d_disp = wrap_sampled_field(
            make_sampled_1d("disp_d", axis.clone(), vec![1.0, 2.0]),
            Type::Real,
            Type::Real,
        );
        let d_stress = wrap_sampled_field(
            make_sampled_1d("stress_d", axis.clone(), vec![100.0, 200.0]),
            Type::Real,
            Type::Real,
        );
        let l_disp = wrap_sampled_field(
            make_sampled_1d("disp_l", axis.clone(), vec![10.0, 20.0]),
            Type::Real,
            Type::Real,
        );
        let l_stress = wrap_sampled_field(
            make_sampled_1d("stress_l", axis.clone(), vec![1000.0, 2000.0]),
            Type::Real,
            Type::Real,
        );

        let case_d = make_fixture_elastic_result_with_fields(d_disp, d_stress);
        let case_l = make_fixture_elastic_result_with_fields(l_disp, l_stress);
        let mcr = make_multi_case_result_value(&[("D", case_d), ("L", case_l)]);

        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("D".to_string()), Value::Real(1.4));
        weights_map.insert(Value::String("L".to_string()), Value::Real(1.7));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(weights_map)]).unwrap();
        assert!(!result.is_undef(), "linear_combine should return a Map");

        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        let disp = result_map
            .get(&Value::String("displacement".to_string()))
            .expect("result must have 'displacement' key");
        let disp_sf = extract_sampled(disp);
        assert!(
            approx_eq_slice(&disp_sf.data, &[18.4, 36.8], 1e-9),
            "displacement data mismatch: {:?}",
            disp_sf.data
        );

        let stress = result_map
            .get(&Value::String("stress".to_string()))
            .expect("result must have 'stress' key");
        let stress_sf = extract_sampled(stress);
        assert!(
            approx_eq_slice(&stress_sf.data, &[1840.0, 3680.0], 1e-9),
            "stress data mismatch: {:?}",
            stress_sf.data
        );

        let mvm = result_map
            .get(&Value::String("max_von_mises".to_string()))
            .expect("result must have 'max_von_mises' key");
        match mvm {
            Value::Real(v) => assert!(
                (v - 3680.0).abs() <= 1e-9,
                "max_von_mises mismatch: {}",
                v
            ),
            other => panic!("expected Value::Real for max_von_mises, got {:?}", other),
        }
    }

    #[test]
    fn linear_combine_negative_weight_produces_signed_difference() {
        // Negative weights are valid (e.g. 1.4D - 0.7L LRFD code combination).
        // Pins IEEE-754 multiply-and-add behaviour for negative weight values.
        // A = [10.0, 20.0], B = [4.0, 8.0], weights {A: 1.4, B: -0.7}
        // expected = [1.4*10 - 0.7*4, 1.4*20 - 0.7*8] = [14.0-2.8, 28.0-5.6] = [11.2, 22.4]
        let axis = vec![0.0, 1.0];
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis.clone(), vec![10.0, 20.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100.0, 200.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![4.0, 8.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![40.0, 80.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);

        let mut weights_map = BTreeMap::new();
        weights_map.insert(Value::String("A".to_string()), Value::Real(1.4));
        weights_map.insert(Value::String("B".to_string()), Value::Real(-0.7));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(weights_map)]).unwrap();
        assert!(!result.is_undef());
        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        let disp = result_map
            .get(&Value::String("displacement".to_string()))
            .expect("result must have 'displacement' key");
        let disp_sf = extract_sampled(disp);
        assert!(
            approx_eq_slice(&disp_sf.data, &[11.2, 22.4], 1e-9),
            "displacement data mismatch with negative weight: {:?}",
            disp_sf.data
        );
        // stress = [1.4*100 - 0.7*40, 1.4*200 - 0.7*80] = [140-28, 280-56] = [112, 224]
        let stress = result_map
            .get(&Value::String("stress".to_string()))
            .expect("result must have 'stress' key");
        let stress_sf = extract_sampled(stress);
        assert!(
            approx_eq_slice(&stress_sf.data, &[112.0, 224.0], 1e-9),
            "stress data mismatch with negative weight: {:?}",
            stress_sf.data
        );
        // max_von_mises = max(|[112, 224]|) = 224.0
        match result_map.get(&Value::String("max_von_mises".to_string())).unwrap() {
            Value::Real(v) => assert!((v - 224.0).abs() <= 1e-9, "max_von_mises: {}", v),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    // ── linear_combine mesh/codomain/source incompatibility rejection ────────

    #[test]
    fn linear_combine_displacement_grid_axis_lengths_mismatch_returns_undef() {
        // Case A displacement has 5 grid points, case B has 4 — mismatch → Undef.
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", vec![0.0, 1.0, 2.0, 3.0, 4.0], vec![1.0, 2.0, 3.0, 4.0, 5.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", vec![0.0, 1.0, 2.0, 3.0, 4.0], vec![10.0, 20.0, 30.0, 40.0, 50.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", vec![0.0, 1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0, 40.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_stress_grid_bounds_min_mismatch_returns_undef() {
        // Cases share displacement grids but stress grids differ in bounds_min.
        let axis_a = vec![0.0, 1.0, 2.0];
        let axis_b = vec![1.0, 2.0, 3.0]; // different bounds
        let shared = wrap_sampled_field(
            make_sampled_1d("d", axis_a.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis_a.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis_b, vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(shared.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_codomain_type_mismatch_returns_undef() {
        // Case A stress has Real codomain, case B has Pressure codomain.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_disp = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Scalar { dimension: DimensionVector::PRESSURE },
        );
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_displacement_non_sampled_source_returns_undef() {
        // Case B's displacement has FieldSourceKind::Analytical — non-Sampled → Undef.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_stress = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_displacement_sampled_with_non_sampledfield_lambda_returns_undef() {
        // Case B's displacement is Sampled-source but lambda is Value::Undef (degenerate).
        let axis = vec![0.0, 1.0, 2.0];
        let shared_stress = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef), // Sampled but non-SampledField lambda
        };
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    // ── linear_combine Pressure codomain preservation ────────────────────────

    #[test]
    fn linear_combine_pressure_codomain_stress_preserves_dimension() {
        // Both cases have Pressure-codomain stress. The combined result must:
        //   1. propagate codomain_type = Type::Scalar { PRESSURE } (not coerce to Real)
        //   2. produce correct weighted data
        //   3. compute max_von_mises over the combined Pressure-valued data
        let axis = vec![0.0, 1.0];
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };

        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis.clone(), vec![1.0, 2.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![1.0, 2.0]),
            Type::Real,
            Type::Real,
        );

        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100e6, 250e6]),
            Type::Real,
            pressure.clone(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![150e6, 200e6]),
            Type::Real,
            pressure.clone(),
        );

        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);

        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        assert!(!result.is_undef());

        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        // Stress field must preserve Pressure codomain_type.
        let stress_field = result_map
            .get(&Value::String("stress".to_string()))
            .expect("result must have 'stress' key");
        match stress_field {
            Value::Field { codomain_type, .. } => {
                assert_eq!(*codomain_type, pressure, "codomain_type must be Pressure");
            }
            other => panic!("expected Value::Field, got {:?}", other),
        }

        // Combined stress data: [100e6+150e6, 250e6+200e6] = [250e6, 450e6].
        let stress_sf = extract_sampled(stress_field);
        assert!(
            approx_eq_slice(&stress_sf.data, &[250e6, 450e6], 1.0),
            "stress data mismatch: {:?}",
            stress_sf.data
        );

        // max_von_mises = 450e6.
        match result_map.get(&Value::String("max_von_mises".to_string())).unwrap() {
            Value::Real(v) => assert!((v - 450e6).abs() <= 1.0, "max_von_mises: {}", v),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    // ── linear_combine NaN safety ────────────────────────────────────────────

    #[test]
    fn linear_combine_combined_stress_with_nan_yields_finite_max_von_mises() {
        // If one stress data element propagates to NaN (e.g. from a NaN input),
        // max_von_mises should be the max of the *finite* values, not NaN.
        // Consistent with envelope_reduce's is_finite discipline.
        let axis = vec![0.0, 1.0, 2.0];

        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        // Stress data with NaN at index 1.
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100.0, f64::NAN, 300.0]),
            Type::Real,
            Type::Real,
        );

        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        assert!(!result.is_undef());

        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        // max_von_mises must be finite — NaN at index 1 is skipped.
        // Finite values: |100.0|=100.0, |300.0|=300.0 → max = 300.0.
        match result_map.get(&Value::String("max_von_mises".to_string())).unwrap() {
            Value::Real(v) => {
                assert!(v.is_finite(), "max_von_mises must be finite, got {}", v);
                assert!((*v - 300.0).abs() <= 1e-9, "max_von_mises must be 300.0, got {}", v);
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    // ── linear_combine all-non-finite stress → Undef max_von_mises ──────────

    #[test]
    fn linear_combine_all_nonfinite_stress_yields_undef_max_von_mises() {
        // When all combined stress data is non-finite (e.g. all NaN), the
        // max_von_mises slot must be Value::Undef — not Value::Real(0.0).
        // RED: current fold(0.0, f64::max) at fea.rs:220-224 collapses the
        // empty/all-non-finite filter result to Real(0.0), which is
        // indistinguishable from genuine zero stress.
        // After step-5 lands the reduce(f64::max) returns None → Undef.
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        // All stress data is NaN — no finite values.
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![f64::NAN, f64::NAN, f64::NAN]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        // The function should not return Undef wholesale — only the
        // max_von_mises slot is Undef; the displacement field is still present.
        assert!(
            !result.is_undef(),
            "linear_combine must return a Map even when all stress is NaN"
        );
        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        // Displacement must still be present.
        assert!(
            result_map.contains_key(&Value::String("displacement".to_string())),
            "result must contain displacement even when stress is all-NaN"
        );

        // max_von_mises must be Undef — distinguishable from genuine zero stress.
        let mvm = result_map
            .get(&Value::String("max_von_mises".to_string()))
            .expect("result must have 'max_von_mises' key");
        assert!(
            mvm.is_undef(),
            "max_von_mises must be Value::Undef when all stress is non-finite, \
             got {:?} — must be distinguishable from genuine zero stress",
            mvm
        );
    }

    #[test]
    fn linear_combine_empty_stress_buffer_yields_undef_max_von_mises() {
        // When the stress SampledField has zero data points (n_stress = 0),
        // combined_stress is an empty Vec.  reduce(f64::max) on an empty
        // iterator returns None → max_von_mises must be Value::Undef.
        //
        // This pins the second None-branch of the reduce logic: the
        // all-non-finite test (above) exercises "non-empty but all-NaN";
        // this test exercises "empty buffer" directly.  A future refactor
        // reintroducing fold(0.0, f64::max) on an empty buffer would
        // collapse this to Real(0.0) and be caught immediately.
        let axis = vec![0.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis, vec![1.0]),
            Type::Real,
            Type::Real,
        );
        // Stress field with zero data points — construct directly since
        // make_sampled_1d panics on empty axis.
        let empty_stress_sf = SampledField {
            name: "s".to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![0.0],
            spacing: vec![1.0],
            axis_grids: vec![vec![]],
            interpolation: InterpolationKind::Linear,
            data: vec![],
            oob_emitted: AtomicBool::new(false),
        };
        let stress_field = wrap_sampled_field(empty_stress_sf, Type::Real, Type::Real);
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        assert!(
            !result.is_undef(),
            "linear_combine must return a Map even when stress buffer is empty"
        );
        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        // Displacement field must still be produced.
        assert!(
            result_map.contains_key(&Value::String("displacement".to_string())),
            "result must contain displacement even when stress buffer is empty"
        );

        // max_von_mises must be Undef — empty buffer is distinct from genuine
        // zero stress (which would have finite data summing to zero).
        let mvm = result_map
            .get(&Value::String("max_von_mises".to_string()))
            .expect("result must have 'max_von_mises' key");
        assert!(
            mvm.is_undef(),
            "max_von_mises must be Value::Undef when stress buffer is empty (n_stress=0), \
             got {:?} — empty buffer is distinct from genuine zero stress",
            mvm
        );
    }

    // ── linear_combine malformed ElasticResult Map rejection ─────────────────

    #[test]
    fn linear_combine_case_missing_displacement_key_returns_undef() {
        // Case A's ElasticResult Map has no "displacement" key (only stress etc.)
        let axis = vec![0.0, 1.0, 2.0];
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        // Build a partial ElasticResult missing the displacement key.
        let mut partial = BTreeMap::new();
        partial.insert(Value::String("stress".to_string()), stress_field);
        partial.insert(Value::String("max_von_mises".to_string()), Value::Real(30.0));
        partial.insert(Value::String("converged".to_string()), Value::Bool(true));
        partial.insert(Value::String("iterations".to_string()), Value::Int(0));
        let partial_case = Value::Map(partial);

        let mcr = make_multi_case_result_value(&[("A", partial_case)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_case_missing_stress_key_returns_undef() {
        // Case A's ElasticResult Map has no "stress" key (only displacement etc.)
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis, vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        // Build a partial ElasticResult missing the stress key.
        let mut partial = BTreeMap::new();
        partial.insert(Value::String("displacement".to_string()), disp_field);
        partial.insert(Value::String("max_von_mises".to_string()), Value::Real(3.0));
        partial.insert(Value::String("converged".to_string()), Value::Bool(true));
        partial.insert(Value::String("iterations".to_string()), Value::Int(0));
        let partial_case = Value::Map(partial);

        let mcr = make_multi_case_result_value(&[("A", partial_case)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    // ── linear_combine NaN/Inf weight rejection ──────────────────────────────

    #[test]
    fn linear_combine_nan_weight_returns_undef() {
        // A NaN weight would poison the accumulator and cause max_von_mises to
        // collapse to 0.0 (via the is_finite filter). Reject at the weight-
        // validation stage instead — matching the silent-Undef discipline.
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(f64::NAN));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef(),
            "NaN weight must reject to Undef"
        );
    }

    #[test]
    fn linear_combine_inf_weight_returns_undef() {
        // ±Inf weights would poison the accumulator just as NaN does.
        // This is a regression-pin test: the existing is_finite() guard at
        // fea.rs:132 already rejects ±Inf, so it passes against current code.
        // It locks the guard in place before the weight-extraction rewrite in
        // step-3 (dimensionless-only Scalar match).
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);

        // +Inf weight must reject to Undef.
        let mut wm_pos = BTreeMap::new();
        wm_pos.insert(Value::String("A".to_string()), Value::Real(f64::INFINITY));
        assert!(
            eval_fea("linear_combine", &[mcr.clone(), Value::Map(wm_pos)])
                .unwrap()
                .is_undef(),
            "+Inf weight must reject to Undef"
        );

        // -Inf weight must reject to Undef.
        let mut wm_neg = BTreeMap::new();
        wm_neg.insert(
            Value::String("A".to_string()),
            Value::Real(f64::NEG_INFINITY),
        );
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm_neg)])
                .unwrap()
                .is_undef(),
            "-Inf weight must reject to Undef"
        );
    }

    // ── linear_combine dimensionful scalar weight rejection ──────────────────

    #[test]
    fn linear_combine_dimensionful_scalar_weight_returns_undef() {
        // A Value::Scalar with a non-dimensionless dimension (e.g. 1.4 m) must
        // reject to Undef. RED: current weight_val.as_f64() at fea.rs:127-130
        // silently accepts the SI-numeric component (1.4) regardless of dimension,
        // so the call succeeds and returns a Map. After step-3 lands the explicit
        // pattern match rejects non-dimensionless Value::Scalar.
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);

        // Weight is 1.4 m — a dimensionful scalar (LENGTH dimension).
        let mut wm = BTreeMap::new();
        wm.insert(
            Value::String("A".to_string()),
            Value::Scalar {
                si_value: 1.4,
                dimension: DimensionVector::LENGTH,
            },
        );
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef(),
            "dimensionful scalar weight (1.4 m) must reject to Undef"
        );
    }

    // ── linear_combine stress-field source rejection (symmetric with disp) ───

    #[test]
    fn linear_combine_stress_non_sampled_source_returns_undef() {
        // Case B's stress has FieldSourceKind::Analytical — non-Sampled → Undef.
        // Symmetric to linear_combine_displacement_non_sampled_source_returns_undef.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_disp = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis, vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    #[test]
    fn linear_combine_stress_sampled_with_non_sampledfield_lambda_returns_undef() {
        // Case B's stress is Sampled-source but lambda is Value::Undef (degenerate).
        // Symmetric to linear_combine_displacement_sampled_with_non_sampledfield_lambda_returns_undef.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_disp = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis, vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let b_stress = Value::Field {
            domain_type: Type::Real,
            codomain_type: Type::Real,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef), // Sampled but non-SampledField lambda
        };
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }

    // ── linear_combine Int weight accepted ───────────────────────────────────

    #[test]
    fn linear_combine_int_weight_accepted() {
        // Value::Int is a valid weight (as_f64() returns Some and the result is
        // finite). Pinned because the doc lists Int as accepted and a match that
        // accidentally restricts to Real would silently break integer weights.
        let axis = vec![0.0, 1.0, 2.0];
        let disp_sf = make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]);
        let disp_field = wrap_sampled_field(disp_sf, Type::Real, Type::Real);
        let stress_sf = make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]);
        let stress_field = wrap_sampled_field(stress_sf, Type::Real, Type::Real);
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = make_multi_case_result_value(&[("A", case_a)]);

        let mut wm = BTreeMap::new();
        // Int weight 2 — must be treated identically to Real(2.0).
        wm.insert(Value::String("A".to_string()), Value::Int(2));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        assert!(!result.is_undef(), "Int weight must be accepted");

        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        let disp = result_map.get(&Value::String("displacement".to_string())).unwrap();
        let disp_sf = extract_sampled(disp);
        assert_eq!(disp_sf.data, vec![2.0, 4.0, 6.0], "Int(2) weight must double displacement");
    }

    // ── linear_combine displacement codomain mismatch ────────────────────────

    #[test]
    fn linear_combine_displacement_codomain_type_mismatch_returns_undef() {
        // Case A displacement has Real codomain, case B has Pressure codomain.
        // Symmetric to linear_combine_codomain_type_mismatch_returns_undef
        // (which only tests stress codomain mismatch).
        let axis = vec![0.0, 1.0, 2.0];
        let shared_stress = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::Real,
            Type::Real,
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Real,
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::Real,
            Type::Scalar { dimension: DimensionVector::PRESSURE }, // codomain mismatch
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = make_multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap().is_undef());
    }
}
