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

use reify_core::{Diagnostic, DiagnosticCode, Type};
use reify_ir::{FieldSourceKind, SampledField, Value};

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
        "envelope_von_mises" => envelope_von_mises(args),
        "envelope_max_principal" => envelope_max_principal(args),
        "envelope_displacement_magnitude" => envelope_displacement_magnitude(args),
        // `worst_case` real implementation lives in
        // `crates/reify-expr/src/lib.rs` (Lambda-aware, requires `EvalContext`).
        // The arm here is a permanent stub returning `Value::Undef` — fired
        // only when the lib.rs dispatch declines (e.g., wrong arg shape).
        // Preserves the "recognised name" contract for direct `eval_builtin`
        // callers; mirrors the dual-arm pattern of `von_mises`'s lib.rs
        // Field-arg arm coexisting with `eval_analysis`'s tensor-arg arm.
        "worst_case" => Value::Undef,
        // `solve_load_cases` primary path is the `@optimized("solver::multi_case")`
        // ComputeNode trampoline (`crates/reify-eval/src/compute_targets/multi_case.rs`),
        // which fires when the engine has the trampoline registered.  The
        // `eval_solve_load_cases` interceptor in `crates/reify-expr/src/lib.rs` is
        // the unregistered/pure-eval fallback (EvalContext-aware, calls
        // `invoke_solve_elastic_static` per LoadCase).  This `eval_fea` stub fires
        // only when the lib.rs dispatch declines (wrong arity), preserving the
        // "recognised name" contract for direct `eval_builtin` callers.
        "solve_load_cases" => Value::Undef,
        // `worst_buckling_case` — argmin over cases of modes[0].eigenvalue.
        // Smaller λ = closer to buckling = worst case.  No reference_load needed
        // (a common positive scalar doesn't change argmin).  Implemented directly
        // in eval_fea (no .ri decl, no lib.rs interceptor) because it reads
        // eigenvalue scalars — no Lambda or EvalContext required.
        "worst_buckling_case" => worst_buckling_case(args),
        // `envelope_critical_load` — min over cases of eigenvalue × reference_load.
        // Deviation from PRD §7: takes an explicit reference_load: Force arg
        // (mirrors task ε DD-1 — BucklingResult stores no applied load magnitude).
        "envelope_critical_load" => envelope_critical_load(args),
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
///   weight) pairs. Accepted weight types are `Value::Real` and `Value::Int`.
///   Per Invariant V (real-dimensionless unification) a dimensionless quantity
///   is always materialized as `Value::Real`, so any `Value::Scalar` reaching
///   this consumer is dimensioned (e.g. `1.4 m`) and is rejected to
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
/// Per-field accessor for a per-case ElasticResult value, handling both
/// `Value::Map` (legacy / synthetic fixture shape) and
/// `Value::StructureInstance` (the shape that `solve_load_cases` emits at
/// runtime, task 4088).
///
/// Mirrors the e2e dual-shape accessor in
/// `crates/reify-eval/tests/buckling_mode_shape.rs:77-83`.
///
/// Returns `Some(&value)` when `case_val` is a Map or StructureInstance that
/// contains `field`, `None` otherwise.  Returning `Option<&Value>` (not a
/// normalised `BTreeMap`) preserves borrows into the per-case value, so
/// callers that return `&SampledField` / `&Type` references do not need to
/// clone.
///
/// **Key layouts differ:**
/// - `Value::Map` keys by `Value::String(field)` (BTreeMap<Value,Value>)
/// - `Value::StructureInstance` keys by plain `String` (PersistentMap<String,Value>)
fn case_field<'a>(case_val: &'a Value, field: &str) -> Option<&'a Value> {
    match case_val {
        Value::Map(m) => m.get(&Value::String(field.to_string())),
        Value::StructureInstance(data) => data.fields.get(&field.to_string()),
        _ => None,
    }
}

/// True iff `case_val` is a valid per-case ElasticResult container:
/// either `Value::Map` or `Value::StructureInstance`.
///
/// NOTE: any `StructureInstance` type_name is accepted here (mirrors the
/// pre-existing any-`Map` behaviour). If a future diagnostic ever needs to
/// distinguish a foreign struct (e.g. `BucklingResult` passed by mistake),
/// gate on `data.type_name == "ElasticResult"` in this function.
fn is_case_container(case_val: &Value) -> bool {
    matches!(case_val, Value::Map(_) | Value::StructureInstance(_))
}

/// # Failure modes (silent-Undef per PRD task #10 deferral)
///
/// - arity != 2
/// - `args[0]` is not a valid `MultiCaseResult` (non-Map / no `cases` key /
///   `cases` not a Map)
/// - `args[1]` is not `Value::Map` or is empty
/// - any weight key is not `Value::String`
/// - any weight value is not `Value::Real` or `Value::Int` (per Invariant V a
///   dimensionless quantity arrives as `Value::Real`; any `Value::Scalar` is
///   dimensioned — such as `1.4 m` — and is rejected)
/// - any weight value has a non-finite representation (NaN, ±Inf)
/// - a weight name is absent from `base_results.cases`
/// - a case value is not a `Value::Map` or `Value::StructureInstance`
/// - a case Map/StructureInstance is missing `displacement` or `stress` key
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

    // Validate weights: collect (weight: f64, case_val: &Value) pairs.
    // Each entry must have a String key, a finite numeric weight, a known case
    // name, and a Map-or-StructureInstance case entry.
    let mut weighted_cases: Vec<(f64, &Value)> = Vec::with_capacity(weights_map.len());
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
            // Per Invariant V a dimensionless quantity arrives as Value::Real;
            // any Value::Scalar reaching here is dimensioned and is rejected.
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
        // Case value must be a Map or StructureInstance (ElasticResult).
        // Accepts both: Value::Map (synthetic / legacy shape) and
        // Value::StructureInstance (solve_load_cases real-solver shape).
        if !is_case_container(case_val) {
            return Value::Undef;
        }
        weighted_cases.push((weight, case_val));
    }

    // Borrow the first weighted case's sampled fields as the reference for
    // metadata and types. Only lightweight metadata (kind, bounds, spacing,
    // axis_grids, types) is cloned for the output; the data buffer is accessed
    // via slice — no per-case Vec<f64> clone for large field buffers.
    // Safety: weighted_cases is non-empty by the is_empty() guard above.
    let ref_case = weighted_cases[0].1;
    let ref_disp_val = match case_field(ref_case, "displacement") {
        Some(v) => v,
        None => return Value::Undef,
    };
    let (ref_disp_dom, ref_disp_cod, ref_disp_sf) = match as_sampled_field(ref_disp_val) {
        Some(t) => t,
        None => return Value::Undef,
    };
    let ref_stress_val = match case_field(ref_case, "stress") {
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
    for (i, (weight, case_val)) in weighted_cases.iter().enumerate() {
        let disp_val = match case_field(case_val, "displacement") {
            Some(v) => v,
            None => return Value::Undef,
        };
        let (dom_d, cod_d, sf_d) = match as_sampled_field(disp_val) {
            Some(t) => t,
            None => return Value::Undef,
        };
        let stress_val = match case_field(case_val, "stress") {
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
            if !metadata_matches(
                ref_stress_sf,
                sf_s,
                ref_stress_dom,
                ref_stress_cod,
                dom_s,
                cod_s,
            ) {
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

/// Resolve a per-case ElasticResult's displacement and stress sampled-field
/// triples `((domain, codomain, field), (domain, codomain, field))`, or `None`
/// if the case value is not a `Value::Map` or `Value::StructureInstance`, or
/// either field is missing / non-Sampled.
/// Used by `diagnose`'s incompatible-mesh check to mirror `linear_combine`'s
/// own `as_sampled_field` resolution of the per-case `displacement`/`stress`
/// fields, so the diagnosed cause matches the real `Undef` cause and the
/// structural fields fed to `metadata_matches` are identical.
#[allow(clippy::type_complexity)]
fn resolve_case_fields(
    case_val: &Value,
) -> Option<((&Type, &Type, &SampledField), (&Type, &Type, &SampledField))> {
    let disp = as_sampled_field(case_field(case_val, "displacement")?)?;
    let stress = as_sampled_field(case_field(case_val, "stress")?)?;
    Some((disp, stress))
}

/// Post-hoc classifier for `linear_combine` failures, mirroring
/// `stackup::diagnose`. Called at the `eval_builtin` `Undef` site (which has no
/// `EvalContext`/diagnostics sink of its own) to re-derive the specific
/// multi-load-case error behind an `Undef` result and attach a stable
/// `DiagnosticCode`.
///
/// Returns `Some(Diagnostic)` only for the task-#10 modes; every other `Undef`
/// cause (and every other builtin name) returns `None`, so the result
/// propagates silently — same discipline as `stackup::diagnose`'s
/// 'Not diagnosed' set.
///
/// Check ordering faithfully mirrors `linear_combine`'s own per-entry `Undef`
/// order: arity → cases_map → weights-map → empty-weights → then, per weight
/// entry in BTreeMap key order, non-String key / non-finite weight / unknown
/// case / non-container case value → finally the incompatible-mesh comparison,
/// resolved lazily per case. The FIRST entry to fail ANY check determines the
/// verdict, exactly as `linear_combine` bails at its first failing entry, so a
/// malformed earlier entry is never masked by — nor misattributed to — a later
/// one (e.g. a non-container case value at an earlier entry yields `None`, not
/// a spurious unknown-case diagnostic for a later entry).
///
/// **Diagnosed** (`linear_combine`):
/// - `MultiLoadEmptyWeights` — weights map is empty
/// - `MultiLoadUnknownCaseInWeights` — a weight references a case name absent
///   from the `MultiCaseResult` (first such entry, in BTreeMap key order)
/// - `MultiLoadIncompatibleMeshes` — two weighted cases have
///   structurally-mismatched displacement/stress fields (the proxy for
///   differing `mesh_size`/`element_order`); the first weighted case is the
///   reference and the first case that fails `metadata_matches` is reported
///
/// **Not diagnosed** (Undef propagates silently, no code):
/// - arity error (`args.len() != 2`)
/// - `args[0]` not a `MultiCaseResult`, or `args[1]` not a `Value::Map`
/// - non-String weight key / non-numeric or non-finite weight value
/// - case value not a Map or StructureInstance / missing `displacement`|`stress` / non-Sampled field
pub fn diagnose(name: &str, args: &[Value]) -> Option<Diagnostic> {
    // Only `linear_combine` produces these multi-load-case diagnostics.
    if name != "linear_combine" {
        return None;
    }
    if args.len() != 2 {
        return None; // arity error handled elsewhere
    }
    // args[0] must be a MultiCaseResult; args[1] must be a Map. Either shape
    // mismatch is left to silent-Undef (no task-specified message).
    let cases_map = extract_cases_map(&args[0])?;
    let weights_map = match &args[1] {
        Value::Map(m) => m,
        _ => return None,
    };
    if weights_map.is_empty() {
        return Some(
            Diagnostic::error(
                "linear_combine: weights map is empty. Specify at least one weighted base case.",
            )
            .with_code(DiagnosticCode::MultiLoadEmptyWeights),
        );
    }

    // Phase 1 — single pass over weights in BTreeMap key order, mirroring
    // linear_combine's first validation loop (fea.rs:140-173). The FIRST entry
    // to fail ANY of these checks is exactly where linear_combine bails, so the
    // diagnosed (or undiagnosed) cause matches the real Undef cause:
    //   - non-String key                     → undiagnosed (None)
    //   - non-numeric/non-finite wt          → undiagnosed (None)
    //   - unknown case name                  → MultiLoadUnknownCaseInWeights
    //   - case value not Map/StructureInstance → undiagnosed (None)
    // The is_case_container check lives in THIS pass — not a later loop — so a
    // non-container case value at an earlier entry is never misattributed to a
    // later unknown-case entry. Collect (name, case_val) pairs for phase 2,
    // retaining the case NAMES linear_combine discards.
    let mut cases: Vec<(&str, &Value)> = Vec::with_capacity(weights_map.len());
    for (name_val, weight_val) in weights_map {
        let case_name = match name_val {
            Value::String(s) => s,
            _ => return None, // non-String key: linear_combine Undefs, undiagnosed
        };
        // Weight must be Real/Int/dimensionless-Scalar AND finite (mirrors the
        // weight parse + is_finite guard in linear_combine).
        let weight_ok = match weight_val {
            Value::Real(r) => r.is_finite(),
            Value::Int(_) => true,
            // Per Invariant V a dimensionless quantity arrives as Value::Real;
            // any Value::Scalar reaching here is dimensioned and is rejected.
            _ => false,
        };
        if !weight_ok {
            return None; // non-numeric / non-finite weight: undiagnosed
        }
        let case_val = match cases_map.get(&Value::String(case_name.clone())) {
            Some(v) => v,
            None => {
                let mut available: Vec<&str> = cases_map
                    .keys()
                    .filter_map(|k| match k {
                        Value::String(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                available.sort_unstable();
                let list = available.join(", ");
                return Some(
                    Diagnostic::error(format!(
                        "linear_combine: weights map references unknown case '{case_name}'. Available cases: [{list}]. Did you misspell the case name?"
                    ))
                    .with_code(DiagnosticCode::MultiLoadUnknownCaseInWeights),
                );
            }
        };
        // Case value must be a Map or StructureInstance (ElasticResult). linear_combine
        // bails here at the first offending entry, BEFORE examining later entries,
        // so return None now rather than deferring this check to phase 2 — that
        // deferral is what would otherwise misattribute the failure to a later
        // unknown-case entry.
        if !is_case_container(case_val) {
            return None; // non-Map/non-SI case value: linear_combine Undefs, undiagnosed
        }
        cases.push((case_name.as_str(), case_val));
    }

    // Phase 2 — incompatible-mesh check, mirroring linear_combine's reference
    // resolution (fea.rs:180-196) + per-case loop (fea.rs:209-250). Resolve the
    // first weighted case as the reference, then walk the rest IN ORDER,
    // resolving each case's displacement+stress and immediately comparing its
    // metadata against the reference — bailing at the first case that fails,
    // exactly as linear_combine does. Resolving lazily per case (rather than all
    // up front) ensures a malformed LATER case can't mask an incompatible-mesh
    // failure at an EARLIER case. On the first failure report the reference
    // (name1) and mismatching (name2) names. A case whose displacement|stress
    // field is missing/non-Sampled stays undiagnosed (None) — linear_combine
    // Undefs it with no task-specified message.
    if let Some((&(ref_name, ref_val), rest)) = cases.split_first() {
        let ((ref_dom_d, ref_cod_d, ref_sf_d), (ref_dom_s, ref_cod_s, ref_sf_s)) =
            resolve_case_fields(ref_val)?;
        for &(name, case_val) in rest {
            let ((dom_d, cod_d, sf_d), (dom_s, cod_s, sf_s)) = resolve_case_fields(case_val)?;
            let disp_ok = metadata_matches(ref_sf_d, sf_d, ref_dom_d, ref_cod_d, dom_d, cod_d);
            let stress_ok = metadata_matches(ref_sf_s, sf_s, ref_dom_s, ref_cod_s, dom_s, cod_s);
            if !disp_ok || !stress_ok {
                return Some(
                    Diagnostic::error(format!(
                        "linear_combine: cases '{ref_name}' and '{name}' use incompatible meshes (different mesh_size or element_order in their ElasticOptions). Superposition requires matching mesh / element-order layouts. Re-solve with consistent options or compute envelopes instead."
                    ))
                    .with_code(DiagnosticCode::MultiLoadIncompatibleMeshes),
                );
            }
        }
    }
    None
}

/// Compute von Mises equivalent stress per grid point for a 3×3 row-major
/// stress window. Thin wrapper around
/// `crate::analysis::compute_von_mises_3x3` — kept as a local symbol so the
/// call sites in `TensorProjection::apply` stay self-documenting (they
/// project a stress window per grid point on the hot path) without
/// duplicating the closed-form formula.
///
/// `compute_von_mises_3x3` is the single source of truth for the von Mises
/// closed-form formula; routing through it (rather than the eager
/// `eval_builtin("von_mises", ...)` dispatch path that wraps `Value::Tensor`)
/// avoids the per-grid-point Value wrap/unwrap cost on the SampledField.data
/// hot path.
///
/// Input layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
///               d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
///               d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
fn apply_von_mises_to_3x3_window(d: &[f64]) -> f64 {
    crate::analysis::compute_von_mises_3x3(d)
}

/// Per-grid envelope of von Mises stress across cases.
///
/// # Input shape
///
/// `args == [MultiCaseResult-shaped Map]` — `Value::Map { "cases" ->
/// Value::Map<Value::String, ElasticResult-Map> }`. Each per-case
/// ElasticResult Map must have a `stress` field bound to a Sampled
/// `Value::Field` whose codomain is a 3×3 tensor (`Type::Matrix { m: 3, n: 3, .. }`
/// or `Type::Tensor { rank: 2, n: 3, .. }`). The codomain quantity (e.g.
/// `Pressure`) is propagated unchanged to the result's scalar codomain.
///
/// # Output
///
/// `Value::Field { source: Sampled, codomain_type: <quantity>, .. }` whose
/// `data[i]` equals `max over cases of vm(stress[case].data[i*9..i*9+9])`.
/// Domain and grid metadata propagate unchanged from the per-case Sampled
/// fields (which `envelope_reduce` validates for equality).
///
/// # Failure modes (silent-Undef per PRD task #10 deferral)
///
/// - arity != 1
/// - `args[0]` is not a valid `MultiCaseResult` shape
/// - empty cases Map (delegated to `envelope_reduce`'s empty guard)
/// - any case ElasticResult is not `Value::Map`
/// - any case ElasticResult is missing the `stress` key
/// - any case stress field is not Sampled (Analytical / Composed / derived)
/// - any case stress field's codomain is not 3×3 tensor-shaped
/// - any case stress field's `data.len()` != grid_count * 9 (stride violation)
/// - per-case grid mismatch (delegated to `envelope_reduce`'s
///   `metadata_matches` enforcement)
fn envelope_von_mises(args: &[Value]) -> Value {
    envelope_tensor_projection(args, "stress", TensorProjection::VonMises)
}

/// Per-grid envelope of the largest principal stress across cases.
///
/// Same input/output shape as `envelope_von_mises`, but the per-grid scalar
/// projection is the largest eigenvalue of the 3×3 symmetric stress tensor
/// (computed via the closed-form `analysis::compute_eigenvalues_3x3`, which
/// returns eigenvalues sorted ascending — `eigs[2]` is the maximum).
///
/// Same failure modes as `envelope_von_mises`: silent-Undef on any shape
/// mismatch, missing field, wrong codomain, stride violation, or per-case
/// grid mismatch (delegated to `envelope_reduce`'s `metadata_matches`).
fn envelope_max_principal(args: &[Value]) -> Value {
    envelope_tensor_projection(args, "stress", TensorProjection::MaxPrincipal)
}

/// Per-grid envelope of the Euclidean magnitude of the displacement vector
/// across cases.
///
/// # Input shape
///
/// `args == [MultiCaseResult-shaped Map]` — `Value::Map { "cases" ->
/// Value::Map<Value::String, ElasticResult-Map> }`. Each per-case
/// ElasticResult Map must have a `displacement` field bound to a Sampled
/// `Value::Field` whose codomain is a 3-vector (`Type::Vector { n: 3, .. }`
/// or `Type::Tensor { rank: 1, n: 3, .. }`). The codomain quantity (e.g.
/// `Length`) is propagated unchanged to the result's scalar codomain —
/// magnitude does not introduce or strip dimensions.
///
/// # Output
///
/// `Value::Field { source: Sampled, codomain_type: <quantity>, .. }` whose
/// `data[i]` equals `max over cases of |displacement[case].data[i*3..i*3+3]|`,
/// where `|·|` is the Euclidean norm `sqrt(x² + y² + z²)`. Domain and grid
/// metadata propagate unchanged from the per-case Sampled fields (which
/// `envelope_reduce` validates for equality).
///
/// # Failure modes (silent-Undef per PRD task #10 deferral)
///
/// - arity != 1
/// - `args[0]` is not a valid `MultiCaseResult` shape
/// - empty cases Map (delegated to `envelope_reduce`'s empty guard)
/// - any case ElasticResult is not `Value::Map`
/// - any case ElasticResult is missing the `displacement` key
/// - any case displacement field is not Sampled (Analytical / Composed / derived)
/// - any case displacement field's codomain is not 3-vector-shaped
/// - any case displacement field's `data.len()` != grid_count * 3 (stride violation)
/// - per-case grid mismatch (delegated to `envelope_reduce`'s
///   `metadata_matches` enforcement)
fn envelope_displacement_magnitude(args: &[Value]) -> Value {
    envelope_tensor_projection(args, "displacement", TensorProjection::Magnitude)
}

/// Codomain shape for per-case Field validation in `envelope_tensor_projection`.
#[derive(Clone, Copy)]
enum TensorShape {
    /// 3×3 row-major tensor: `Type::Matrix { m: 3, n: 3, .. }` or
    /// `Type::Tensor { rank: 2, n: 3, .. }`. Stride 9 on the data buffer.
    Matrix3x3,
    /// 3-component vector: `Type::Vector { n: 3, .. }` or
    /// `Type::Tensor { rank: 1, n: 3, .. }`. Stride 3 on the data buffer.
    /// Used by `envelope_displacement_magnitude`.
    Vector3,
}

impl TensorShape {
    fn stride(self) -> usize {
        match self {
            TensorShape::Matrix3x3 => 9,
            TensorShape::Vector3 => 3,
        }
    }

    /// Returns `Some(quantity)` when `codomain` matches this tensor shape
    /// (extracts the scalar quantity for the result codomain), `None`
    /// otherwise. The result scalar codomain inherits the quantity (e.g.
    /// `Pressure` from a `Matrix<3,3,Pressure>` becomes the result's
    /// `Type::Scalar { dimension: PRESSURE }`; `Length` from a
    /// `Vector<3,Length>` becomes the result's
    /// `Type::Scalar { dimension: LENGTH }`).
    fn extract_quantity(self, codomain: &Type) -> Option<Type> {
        match (self, codomain) {
            (
                TensorShape::Matrix3x3,
                Type::Matrix {
                    m: 3,
                    n: 3,
                    quantity,
                },
            ) => Some((**quantity).clone()),
            (
                TensorShape::Matrix3x3,
                Type::Tensor {
                    rank: 2,
                    n: 3,
                    quantity,
                },
            ) => Some((**quantity).clone()),
            (TensorShape::Vector3, Type::Vector { n: 3, quantity }) => Some((**quantity).clone()),
            (
                TensorShape::Vector3,
                Type::Tensor {
                    rank: 1,
                    n: 3,
                    quantity,
                },
            ) => Some((**quantity).clone()),
            _ => None,
        }
    }
}

/// Per-grid scalar projection applied per-case before envelope_reduce.
#[derive(Clone, Copy)]
enum TensorProjection {
    /// von Mises equivalent stress on a 3×3 row-major window.
    VonMises,
    /// Largest principal stress (`eigs[2]` of the 3×3 symmetric tensor,
    /// where eigenvalues are sorted ascending). Routes through
    /// `crate::analysis::compute_eigenvalues_3x3` (`pub(crate)`-promoted
    /// for this cross-module reuse).
    MaxPrincipal,
    /// Euclidean magnitude of a 3-vector window: `sqrt(x² + y² + z²)`.
    /// Used by `envelope_displacement_magnitude`. The output preserves
    /// the input quantity unchanged (Length → Length); magnitude does
    /// not introduce or strip dimensions.
    Magnitude,
}

impl TensorProjection {
    fn shape(self) -> TensorShape {
        match self {
            TensorProjection::VonMises | TensorProjection::MaxPrincipal => TensorShape::Matrix3x3,
            TensorProjection::Magnitude => TensorShape::Vector3,
        }
    }

    /// Apply the per-window scalar projection. Window must be at least
    /// `self.shape().stride()` floats long; only the first stride elements
    /// are read.
    fn apply(self, window: &[f64]) -> f64 {
        match self {
            TensorProjection::VonMises => apply_von_mises_to_3x3_window(window),
            TensorProjection::MaxPrincipal => {
                match crate::analysis::compute_eigenvalues_3x3(window) {
                    // eigs sorted ascending; eigs[2] is the largest.
                    Some(eigs) => eigs[2],
                    None => f64::NAN,
                }
            }
            TensorProjection::Magnitude => {
                debug_assert!(
                    window.len() >= 3,
                    "TensorProjection::Magnitude requires window of at least 3 elements (got {})",
                    window.len()
                );
                // Stride-3 Vector3 magnitude. Closed-form sqrt of squared
                // components — independent of the analysis module since
                // there's no shared scalar formula to factor through.
                (window[0] * window[0] + window[1] * window[1] + window[2] * window[2]).sqrt()
            }
        }
    }
}

/// Shared body for `envelope_von_mises` / `envelope_max_principal` /
/// `envelope_displacement_magnitude`: validate the MultiCaseResult shape,
/// extract the per-case Sampled tensor/vector field, apply a per-grid-point
/// scalar projection, then dispatch to `envelope_reduce` for the across-case
/// max reduction.
///
/// `field_name` is the per-case ElasticResult key to read (`"stress"` or
/// `"displacement"`). `projection` controls the codomain shape (matrix vs
/// vector), the data stride, and the per-window scalar projection function.
///
/// # Two-pass layout
///
/// Per-case scalar projection is materialised into a fresh `Vec<f64>` of
/// length `grid_count` and wrapped in a per-case Sampled `Value::Field`
/// before `envelope_reduce` runs the across-case max. This is two passes
/// over the data and N intermediate `Vec<f64>` allocations for an N-case
/// fixture — chosen deliberately over a streaming reduction so:
/// - `envelope_reduce`'s grid-equality check (`metadata_matches`) runs on
///   already-projected scalar fields with stride-1 codomain, matching the
///   pre-existing reduction's invariants without bespoke argument shapes;
/// - the per-grid `total_cmp` + first-finite-init + NaN-skip discipline
///   lives in exactly one place (`envelope_reduce`) rather than being
///   reimplemented per projection;
/// - failure modes (grid mismatch, missing axis, etc.) report through the
///   shared `envelope_reduce` path with consistent silent-Undef semantics.
///
/// If/when FEA grids reach sizes where the intermediate allocations matter
/// (\~1e6 grid points · \~10 cases · \~1 float = \~80 MB), folding the
/// projection into `envelope_reduce`'s per-index loop via a closure becomes
/// the right trade-off; until then, the two-pass shape keeps the reduction
/// invariants centralised.
fn envelope_tensor_projection(
    args: &[Value],
    field_name: &str,
    projection: TensorProjection,
) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let cases_map = match extract_cases_map(&args[0]) {
        Some(m) => m,
        None => return Value::Undef,
    };
    if cases_map.is_empty() {
        return Value::Undef;
    }

    let shape = projection.shape();
    let stride = shape.stride();

    // Build per-case Map<String, Value::Field> of projected scalar fields.
    let mut projected_map: BTreeMap<Value, Value> = BTreeMap::new();
    for (case_name, case_val) in cases_map {
        // Per-case Sampled-field extraction: validates ElasticResult Map
        // shape, field-name lookup, Sampled-source contract, SampledField-
        // lambda invariant, and the stride contract (data.len() == grid_count
        // * expected_stride). Codomain shape is intentionally NOT checked
        // here — the projection-specific extract_quantity check (`Matrix3x3`
        // vs `Vector3`) lives below so this helper stays projection-agnostic
        // and can serve future per-case-Sampled-field accessors.
        let (dom, cod, sf) = match extract_per_case_sampled_field(case_val, field_name, stride) {
            Some(t) => t,
            None => return Value::Undef,
        };
        // Codomain must match the projection's expected tensor shape.
        let scalar_quantity = match shape.extract_quantity(cod) {
            Some(q) => q,
            None => return Value::Undef,
        };
        let grid_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();

        // Apply the per-grid-point projection to produce a scalar buffer.
        let mut scalar_data: Vec<f64> = Vec::with_capacity(grid_count);
        for i in 0..grid_count {
            let window = &sf.data[i * stride..i * stride + stride];
            scalar_data.push(projection.apply(window));
        }

        // Construct a per-case projected SampledField with stride-1 scalar
        // codomain. Grid metadata propagates unchanged from the input — only
        // the data buffer length changes (grid_count * stride → grid_count).
        //
        // `oob_emitted: AtomicBool::new(false)` is a fresh diagnostic flag,
        // not inherited from the source `sf`: the projected field is an
        // internal-only intermediary handed straight to `envelope_reduce`
        // and never directly OOB-queried by user code, so there is no
        // user-visible duplicate-warning surface to suppress. Keeping the
        // flag fresh preserves the simple "one diagnostic per
        // distinctly-instantiated SampledField" mental model.
        let projected_sf = SampledField {
            name: sf.name.clone(),
            kind: sf.kind,
            bounds_min: sf.bounds_min.clone(),
            bounds_max: sf.bounds_max.clone(),
            spacing: sf.spacing.clone(),
            axis_grids: sf.axis_grids.clone(),
            interpolation: sf.interpolation,
            data: scalar_data,
            oob_emitted: AtomicBool::new(false),
        };
        let projected_field = Value::Field {
            domain_type: dom.clone(),
            codomain_type: scalar_quantity,
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::SampledField(projected_sf)),
        };
        projected_map.insert(case_name.clone(), projected_field);
    }

    // Dispatch to envelope_reduce for the per-grid-point max across cases.
    // envelope_reduce enforces grid-equality (`metadata_matches`) and the
    // NaN-skip + total_cmp + first-occurrence-wins reduction discipline.
    envelope_reduce(&[Value::Map(projected_map)], false)
}

/// Extract a per-case Sampled `Field` from an `ElasticResult` instance,
/// validating the full silent-Undef contract for the three envelope helpers
/// (`envelope_von_mises`, `envelope_max_principal`,
/// `envelope_displacement_magnitude`).
///
/// Returns `Some((domain, codomain, sf))` when ALL of the following hold:
///   - `elastic_result` is `Value::Map` or `Value::StructureInstance`
///     (the ElasticResult struct shape — both the synthetic/fixture Map shape
///     and the real solve_load_cases StructureInstance shape are accepted)
///   - the container has the `field_name` key
///   - the value at that key is `Value::Field { source: Sampled, .. }`
///   - the Sampled lambda slot carries `Value::SampledField`
///   - `sf.data.len() == axis_grids product * expected_stride`
///     (the stride contract: stride 9 for 3×3 tensor codomain, stride 3
///     for 3-vector codomain — see `TensorShape::stride()`)
///
/// Returns `None` on any failure. Codomain shape (e.g. `Matrix3x3` vs
/// `Vector3`) is intentionally NOT checked here so the helper is reusable
/// across projections; the projection-specific `TensorShape::extract_quantity`
/// check happens at the call site.
///
/// The return tuple ordering `(dom, cod, sf)` matches `as_sampled_field`'s
/// `(&Type, &Type, &SampledField)` ordering, so callers that touch both
/// helpers can use the same destructuring shape and avoid foot-gun
/// reorderings.
fn extract_per_case_sampled_field<'a>(
    elastic_result: &'a Value,
    field_name: &str,
    expected_stride: usize,
) -> Option<(&'a Type, &'a Type, &'a SampledField)> {
    let field_val = case_field(elastic_result, field_name)?;
    let (dom, cod, sf) = as_sampled_field(field_val)?;
    let grid_count: usize = sf.axis_grids.iter().map(|g| g.len()).product();
    if sf.data.len() != grid_count * expected_stride {
        return None;
    }
    Some((dom, cod, sf))
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
    // Validation order mirrors `case_names`: extract cases-map (args[0]) first,
    // then validate the scalar `key` (args[1]). Functionally equivalent under
    // silent-Undef discipline (any bad-args combination → Value::Undef regardless
    // of order); the shared shape keeps the two accessors visually parallel.
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

// ── Buckling multi-case helpers ───────────────────────────────────────────────
//
// `worst_buckling_case` and `envelope_critical_load` operate on
// `MultiCaseBucklingResult` values (Value::Map{"cases"->Map<String,BucklingResult>}).
// They mirror the pattern of `case_names`/`result_for` for `MultiCaseResult`:
// - both are name-dispatched in `eval_fea` (no .ri decl, no lib.rs interceptor)
// - both use `extract_cases_map` to crack the outer Map
// - both follow silent-Undef discipline (all shape failures → Value::Undef)
//
// PRD reference: docs/prds/v0_5/buckling-eigensolver.md §7 + §13 task η.

/// Extract `modes[0].eigenvalue` from a `BucklingResult` StructureInstance.
///
/// Returns `Some(f64)` when the value is present and finite, `None` on any
/// shape failure:
///   - `case_val` is not a `Value::StructureInstance`
///   - `fields["modes"]` is absent or not a `Value::List`
///   - modes list is empty
///   - `modes[0]` is not a `Value::StructureInstance`
///   - `modes[0].fields["eigenvalue"]` is absent or not `Value::Real`
///
/// Called by both `worst_buckling_case` and `envelope_critical_load` so the
/// eigenvalue-extraction logic lives in exactly one place.
fn extract_first_mode_eigenvalue(case_val: &Value) -> Option<f64> {
    let data = match case_val {
        Value::StructureInstance(d) => d,
        _ => return None,
    };
    let modes = match data.fields.get(&"modes".to_string()) {
        Some(Value::List(v)) => v,
        _ => return None,
    };
    let first_mode = modes.first()?;
    let mode_data = match first_mode {
        Value::StructureInstance(d) => d,
        _ => return None,
    };
    match mode_data.fields.get(&"eigenvalue".to_string()) {
        Some(Value::Real(v)) => Some(*v),
        _ => None,
    }
}

/// Return the name of the `BucklingResult` case with the smallest first-mode
/// eigenvalue (`modes[0].eigenvalue`) from a `MultiCaseBucklingResult`.
///
/// Smaller λ = smaller load multiplier = closer to buckling = worst case.
/// A common positive reference load does not change the argmin, so no
/// reference_load argument is needed (unlike `envelope_critical_load`).
///
/// # Input shape
///
/// `args == [Value::Map { "cases" -> Value::Map<Value::String, BucklingResult> }]`
///
/// # Output
///
/// `Value::String(name)` of the min-λ case, or `Value::Undef` on any
/// shape failure or when no case yields a finite eigenvalue.
///
/// Tie-break: BTreeMap lexicographic iteration + strict `<` first-occurrence-wins
/// (same discipline as `case_names` / `result_for` / `envelope_reduce`).
///
/// # Failure modes (silent-Undef discipline)
///
///   - arity != 1
///   - `args[0]` is not `Value::Map` or has no `"cases"` key
///   - no case yields a finite `modes[0].eigenvalue`
fn worst_buckling_case(args: &[Value]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let cases = match extract_cases_map(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };

    let mut best_name: Option<&str> = None;
    let mut best_lambda = f64::INFINITY;

    for (key, case_val) in cases {
        let name = match key {
            Value::String(s) => s.as_str(),
            _ => continue, // non-String key — skip silently
        };
        let lambda = match extract_first_mode_eigenvalue(case_val) {
            Some(v) if v.is_finite() => v,
            _ => continue, // shape failure or non-finite eigenvalue — skip
        };
        // Strict `<`: first finite case that achieves the minimum wins.
        // BTreeMap iterates in lexicographic key order, giving deterministic
        // lex-first tie-break for free (mirrors case_names / worst_case).
        if lambda.total_cmp(&best_lambda).is_lt() {
            best_lambda = lambda;
            best_name = Some(name);
        }
    }

    best_name
        .map(|s| Value::String(s.to_string()))
        .unwrap_or(Value::Undef)
}

/// Return the minimum critical load across all cases in a
/// `MultiCaseBucklingResult`.
///
/// Computes `min(modes[0].eigenvalue) × reference_load` and returns it as a
/// `Value::Scalar` with the same dimension as `reference_load`.
///
/// # Input shape
///
/// `args == [mcbr: Value::Map { "cases" -> ... },
///           reference_load: Value::Scalar { si_value, dimension }]`
///
/// # Design deviation from PRD §7
///
/// PRD §7 declared `envelope_critical_load(mcbr) -> Force` (single arg).
/// This deviates by adding an explicit `reference_load: Force` parameter,
/// mirroring task ε's DD-1 for `critical_load(result, reference_load)`.
/// BucklingResult is frozen to 4 fields and stores no applied-load magnitude;
/// the kernel returns only a dimensionless multiplier λ = P_cr / F_applied,
/// so the reference load must be supplied explicitly to recover a Force result.
/// The "match per-case singletons" observable requires the same reference_load
/// used by per-case `critical_load(result_for(mcbr, name), ref)` calls.
///
/// # Failure modes (silent-Undef discipline)
///
///   - arity != 2
///   - `args[0]` is not `Value::Map` or has no `"cases"` key
///   - `args[1]` is not `Value::Scalar`
///   - no case yields a finite `modes[0].eigenvalue`
fn envelope_critical_load(args: &[Value]) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    let (ref_si, ref_dim) = match &args[1] {
        Value::Scalar {
            si_value,
            dimension,
        } => (*si_value, *dimension),
        _ => return Value::Undef,
    };
    let cases = match extract_cases_map(&args[0]) {
        Some(c) => c,
        None => return Value::Undef,
    };

    let mut min_lambda: Option<f64> = None;

    for (_, case_val) in cases {
        let lambda = match extract_first_mode_eigenvalue(case_val) {
            Some(v) if v.is_finite() => v,
            _ => continue,
        };
        min_lambda = Some(match min_lambda {
            None => lambda,
            Some(prev) => {
                if lambda.total_cmp(&prev).is_lt() {
                    lambda
                } else {
                    prev
                }
            }
        });
    }

    match min_lambda {
        Some(lambda) => Value::Scalar {
            si_value: lambda * ref_si,
            dimension: ref_dim,
        },
        None => Value::Undef,
    }
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
    // refactors must NOT silently rewrap (e.g. coerce to Type::dimensionless_scalar()) or
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

    use reify_test_support::multi_case_result_value;
    use reify_core::{DimensionVector, Type};
    use reify_ir::{
        FieldSourceKind, InterpolationKind, PersistentMap, SampledField, SampledGridKind,
        StructureInstanceData, StructureTypeId, Value,
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

    /// Construct a 1-D `SampledField` carrying a 3×3 tensor codomain.
    /// `tensors` is a list of per-grid-point row-major 9-float windows
    /// (one window per axis grid point; `tensors.len() == axis.len()`).
    /// The resulting `SampledField.data` has length `axis.len() * 9` —
    /// the stride-9 layout established locally for envelope_von_mises /
    /// envelope_max_principal in step-4 / step-6.
    fn make_sampled_tensor_3x3_1d(
        name: &str,
        axis: Vec<f64>,
        tensors: Vec<[f64; 9]>,
    ) -> SampledField {
        assert_eq!(
            tensors.len(),
            axis.len(),
            "tensor count must match axis grid point count"
        );
        let mut data: Vec<f64> = Vec::with_capacity(axis.len() * 9);
        for t in &tensors {
            data.extend_from_slice(t);
        }
        // Reuse make_sampled_1d's grid-metadata derivation so the bounds /
        // spacing handling stays single-sourced. The only difference is the
        // data length — make_sampled_1d does not enforce data.len() ==
        // axis.len(), so it accepts our stride-9 buffer unchanged.
        make_sampled_1d(name, axis, data)
    }

    /// Construct a 1-D `SampledField` carrying a Vector3 codomain.
    /// `vectors` is a list of per-grid-point [x, y, z] triples (one per
    /// axis grid point). The resulting `SampledField.data` has length
    /// `axis.len() * 3` — the stride-3 layout established locally for
    /// envelope_displacement_magnitude in step-8.
    fn make_sampled_vector3_1d(name: &str, axis: Vec<f64>, vectors: Vec<[f64; 3]>) -> SampledField {
        assert_eq!(
            vectors.len(),
            axis.len(),
            "vector count must match axis grid point count"
        );
        let mut data: Vec<f64> = Vec::with_capacity(axis.len() * 3);
        for v in &vectors {
            data.extend_from_slice(v);
        }
        make_sampled_1d(name, axis, data)
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

    #[test]
    fn eval_fea_envelope_von_mises_returns_some() {
        // Recognised name — `eval_fea` must return `Some(_)`. The actual
        // value is `Value::Undef` on empty args (arity validation rejects),
        // but the dispatch slot is reserved so the dispatch chain in
        // `lib.rs` does not fall through to the unknown-builtin path.
        assert!(eval_fea("envelope_von_mises", &[]).is_some());
    }

    #[test]
    fn eval_fea_envelope_max_principal_returns_some() {
        assert!(eval_fea("envelope_max_principal", &[]).is_some());
    }

    #[test]
    fn eval_fea_envelope_displacement_magnitude_returns_some() {
        assert!(eval_fea("envelope_displacement_magnitude", &[]).is_some());
    }

    #[test]
    fn eval_fea_worst_case_returns_some() {
        // `worst_case` is dispatched in two locations: the real (Lambda-aware)
        // implementation lives in `crates/reify-expr/src/lib.rs` (because
        // invoking a `Value::Lambda` requires `EvalContext`, which `eval_fea`
        // cannot supply), but the name is also reserved here as a stub
        // returning `Value::Undef`. The stub preserves the "recognised name"
        // contract for callers that route through `eval_builtin` directly,
        // matching the dual-arm pattern of `von_mises`'s lib.rs Field-arg
        // arm coexisting with `eval_analysis`'s tensor-arg arm.
        assert!(eval_fea("worst_case", &[]).is_some());
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
        let field = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
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
        let field = wrap_sampled_field(sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
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
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, 2.0, 4.0, 1.0, 5.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
                assert_eq!(*domain_type, Type::dimensionless_scalar());
                assert_eq!(*codomain_type, Type::dimensionless_scalar());
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, 2.0, 4.0, 1.0, 5.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            pressure.clone(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![150e6, 200e6, 220e6]),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![f64::NAN, 5.0, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![f64::NAN, 5.0, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, f64::INFINITY, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
        // TODO(envelope_argmax): add tests that assert *which case* the extremum // ptodo:allow test coverage note, no live task
        //   came from (not just its value) to pin first-finite-init and strict
        //   tie-break robustly. This is deferred to the envelope_argmax task.
        let axis = vec![0.0, 1.0, 2.0];
        // case_a[i] and case_b[i] have opposite signs.
        // Under total_cmp:  +0.0 > -0.0, so envelope_max must pick +0.0 at every index.
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![0.0, -0.0, 0.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![-0.0, 0.0, -0.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
        // for the detailed reasoning and the shared TODO(envelope_argmax)). // ptodo:allow test coverage note, no live task
        // Does NOT pin strict vs non-strict tie-break (same reasoning).
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![0.0, -0.0, 0.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![-0.0, 0.0, -0.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_grid_bounds_min_mismatch_returns_undef() {
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", vec![1.0, 2.0, 3.0, 4.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_2d(
                "b",
                vec![0.0, 1.0],
                vec![0.0, 1.0],
                vec![1.0, 2.0, 3.0, 4.0],
            ),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let map = make_envelope_map(&[("a", case_a), ("b", case_b)]);
        assert!(eval_fea("envelope_max", &[map]).unwrap().is_undef());
    }

    #[test]
    fn envelope_max_codomain_type_mismatch_returns_undef() {
        let axis = vec![0.0, 1.0, 2.0];
        let case_a = wrap_sampled_field(
            make_sampled_1d("a", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let analytical = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let degenerate_sampled = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_b = wrap_sampled_field(
            make_sampled_1d("b", axis.clone(), vec![3.0, f64::INFINITY, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
        m.insert(
            Value::String("iterations".to_string()),
            Value::Int(iterations),
        );
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
        let mcr = multi_case_result_value(&[
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
        let mcr = multi_case_result_value(&[]);
        assert!(
            eval_fea("case_names", &[mcr, Value::String("extra".to_string())])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn case_names_non_map_arg_returns_undef() {
        assert!(
            eval_fea("case_names", &[Value::Int(42)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn case_names_map_without_cases_field_returns_undef() {
        // A Map without a "cases" key is not a valid MultiCaseResult struct.
        let mut m = BTreeMap::new();
        m.insert(Value::String("other_field".to_string()), Value::Int(1));
        assert!(eval_fea("case_names", &[Value::Map(m)]).unwrap().is_undef());
    }

    #[test]
    fn case_names_cases_field_non_map_returns_undef() {
        // A Map with "cases" key but non-Map value is malformed.
        let mut m = BTreeMap::new();
        m.insert(
            Value::String("cases".to_string()),
            Value::Int(99), // not a Map
        );
        assert!(eval_fea("case_names", &[Value::Map(m)]).unwrap().is_undef());
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
        let mcr = multi_case_result_value(&[("operating", er_op.clone())]);

        let result =
            eval_fea("result_for", &[mcr, Value::String("operating".to_string())]).unwrap();

        assert_eq!(
            result, er_op,
            "result_for should return the exact ElasticResult value for the key"
        );
    }

    #[test]
    fn result_for_missing_key_returns_undef() {
        let er_op = make_fixture_elastic_result(42);
        let mcr = multi_case_result_value(&[("operating", er_op)]);

        let result = eval_fea("result_for", &[mcr, Value::String("missing".to_string())]).unwrap();

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
        let mcr = multi_case_result_value(&[]);
        assert!(eval_fea("result_for", &[mcr]).unwrap().is_undef());
    }

    #[test]
    fn result_for_three_args_returns_undef() {
        let mcr = multi_case_result_value(&[]);
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
            eval_fea(
                "result_for",
                &[Value::Int(1), Value::String("k".to_string())]
            )
            .unwrap()
            .is_undef()
        );
    }

    #[test]
    fn result_for_non_string_key_returns_undef() {
        // Second arg must be Value::String — passing e.g. Value::Real rejects.
        let mcr = multi_case_result_value(&[]);
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
        let mcr = multi_case_result_value(&[]);
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Real(1.0)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_empty_weights_returns_undef() {
        // weights map must be non-empty.
        let mcr = multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
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
        let mcr = multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
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
        let mcr = multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
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
        let mcr = multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
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
        let mcr = multi_case_result_value(&[("A", Value::Int(123))]);
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
    fn make_fixture_elastic_result_with_fields(displacement: Value, stress: Value) -> Value {
        let mut m = BTreeMap::new();
        m.insert(Value::String("displacement".to_string()), displacement);
        m.insert(Value::String("stress".to_string()), stress);
        m.insert(Value::String("max_von_mises".to_string()), Value::Real(0.0));
        m.insert(Value::String("converged".to_string()), Value::Bool(true));
        m.insert(Value::String("iterations".to_string()), Value::Int(0));
        Value::Map(m)
    }

    /// Build a fixture `ElasticResult`-shaped `Value::StructureInstance` with
    /// Field-typed displacement and stress fields. Parallel to
    /// `make_fixture_elastic_result_with_fields` but emits
    /// `Value::StructureInstance` — matching the shape that `solve_load_cases`
    /// (task 4088, multi_case.rs:214-249) emits for each per-case result at
    /// runtime.
    fn make_elastic_result_si_with_fields(displacement: Value, stress: Value) -> Value {
        let fields: PersistentMap<String, Value> = [
            ("displacement".to_string(), displacement),
            ("stress".to_string(), stress),
            ("max_von_mises".to_string(), Value::Real(0.0)),
            ("converged".to_string(), Value::Bool(true)),
            ("iterations".to_string(), Value::Int(0)),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "ElasticResult".to_string(),
            version: 1,
            fields,
        }))
    }

    // ── linear_combine happy path ────────────────────────────────────────────

    #[test]
    fn linear_combine_single_case_weight_two_doubles_displacement_and_stress() {
        // Single-case MultiCaseResult where case "A" has Sampled 1-D Real-codomain
        // fields. Calling linear_combine with weight=2.0 should double all data.
        let axis = vec![0.0, 1.0, 2.0];

        let disp_sf = make_sampled_1d("disp", axis.clone(), vec![1.0, 2.0, 3.0]);
        let disp_field = wrap_sampled_field(disp_sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

        let stress_sf = make_sampled_1d("stress", axis.clone(), vec![10.0, 20.0, 30.0]);
        let stress_field = wrap_sampled_field(stress_sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());

        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);

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
        assert_eq!(
            disp_sf.data,
            vec![2.0, 4.0, 6.0],
            "displacement data should be 2x input"
        );

        // Check stress field: data should be [20.0, 40.0, 60.0].
        let stress = result_map
            .get(&Value::String("stress".to_string()))
            .expect("result must have 'stress' key");
        let stress_sf = extract_sampled(stress);
        assert_eq!(
            stress_sf.data,
            vec![20.0, 40.0, 60.0],
            "stress data should be 2x input"
        );

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
        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() <= tol)
    }

    /// Shared body for the Map-shape and SI-shape LRFD weighted-sum tests.
    /// `make_er(displacement, stress)` builds one per-case ElasticResult;
    /// calling it twice with different builders verifies both shapes produce
    /// identical numerical output.
    fn run_linear_combine_lrfd_body(make_er: impl Fn(Value, Value) -> Value) {
        // Two cases "D" and "L" with the LRFD combination 1.4D + 1.7L.
        // Expected combined disp: [1.4*1+1.7*10, 1.4*2+1.7*20] = [18.4, 36.8]
        // Expected combined stress: [1.4*100+1.7*1000, 1.4*200+1.7*2000] = [1840, 3680]
        let axis = vec![0.0, 1.0];

        let d_disp = wrap_sampled_field(
            make_sampled_1d("disp_d", axis.clone(), vec![1.0, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let d_stress = wrap_sampled_field(
            make_sampled_1d("stress_d", axis.clone(), vec![100.0, 200.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let l_disp = wrap_sampled_field(
            make_sampled_1d("disp_l", axis.clone(), vec![10.0, 20.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let l_stress = wrap_sampled_field(
            make_sampled_1d("stress_l", axis.clone(), vec![1000.0, 2000.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );

        let case_d = make_er(d_disp, d_stress);
        let case_l = make_er(l_disp, l_stress);
        let mcr = multi_case_result_value(&[("D", case_d), ("L", case_l)]);

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
            Value::Real(v) => assert!((v - 3680.0).abs() <= 1e-9, "max_von_mises mismatch: {}", v),
            other => panic!("expected Value::Real for max_von_mises, got {:?}", other),
        }
    }

    #[test]
    fn linear_combine_lrfd_d_and_l_produces_correct_weighted_sum() {
        run_linear_combine_lrfd_body(make_fixture_elastic_result_with_fields);
    }

    // SI per-case ElasticResults (solve_load_cases real-solver shape) must
    // produce identical output to Map per-cases for the same LRFD fixture.
    #[test]
    fn linear_combine_structure_instance_per_case_superposes_not_undef() {
        run_linear_combine_lrfd_body(make_elastic_result_si_with_fields);
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100.0, 200.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![4.0, 8.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![40.0, 80.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);

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
        match result_map
            .get(&Value::String("max_von_mises".to_string()))
            .unwrap()
        {
            Value::Real(v) => assert!((v - 224.0).abs() <= 1e-9, "max_von_mises: {}", v),
            other => panic!("expected Real, got {:?}", other),
        }
    }

    // ── linear_combine mesh/codomain/source incompatibility rejection ────────

    #[test]
    fn linear_combine_displacement_grid_axis_lengths_mismatch_returns_undef() {
        // Case A displacement has 5 grid points, case B has 4 — mismatch → Undef.
        let a_disp = wrap_sampled_field(
            make_sampled_1d(
                "da",
                vec![0.0, 1.0, 2.0, 3.0, 4.0],
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
            ),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d(
                "sa",
                vec![0.0, 1.0, 2.0, 3.0, 4.0],
                vec![10.0, 20.0, 30.0, 40.0, 50.0],
            ),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", vec![0.0, 1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0, 40.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_stress_grid_bounds_min_mismatch_returns_undef() {
        // Cases share displacement grids but stress grids differ in bounds_min.
        let axis_a = vec![0.0, 1.0, 2.0];
        let axis_b = vec![1.0, 2.0, 3.0]; // different bounds
        let shared = wrap_sampled_field(
            make_sampled_1d("d", axis_a.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis_a.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis_b, vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(shared.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_codomain_type_mismatch_returns_undef() {
        // Case A stress has Real codomain, case B has Pressure codomain.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_disp = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        );
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_displacement_non_sampled_source_returns_undef() {
        // Case B's displacement has FieldSourceKind::Analytical — non-Sampled → Undef.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_stress = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis, vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_displacement_sampled_with_non_sampledfield_lambda_returns_undef() {
        // Case B's displacement is Sampled-source but lambda is Value::Undef (degenerate).
        let axis = vec![0.0, 1.0, 2.0];
        let shared_stress = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis, vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef), // Sampled but non-SampledField lambda
        };
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![1.0, 2.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );

        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100e6, 250e6]),
            Type::dimensionless_scalar(),
            pressure.clone(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_1d("sb", axis.clone(), vec![150e6, 200e6]),
            Type::dimensionless_scalar(),
            pressure.clone(),
        );

        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let case_b = make_fixture_elastic_result_with_fields(b_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);

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
        match result_map
            .get(&Value::String("max_von_mises".to_string()))
            .unwrap()
        {
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        // Stress data with NaN at index 1.
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis.clone(), vec![100.0, f64::NAN, 300.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );

        let case_a = make_fixture_elastic_result_with_fields(a_disp, a_stress);
        let mcr = multi_case_result_value(&[("A", case_a)]);
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
        match result_map
            .get(&Value::String("max_von_mises".to_string()))
            .unwrap()
        {
            Value::Real(v) => {
                assert!(v.is_finite(), "max_von_mises must be finite, got {}", v);
                assert!(
                    (*v - 300.0).abs() <= 1e-9,
                    "max_von_mises must be 300.0, got {}",
                    v
                );
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        // All stress data is NaN — no finite values.
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![f64::NAN, f64::NAN, f64::NAN]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
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
        let stress_field = wrap_sampled_field(empty_stress_sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        // Build a partial ElasticResult missing the displacement key.
        let mut partial = BTreeMap::new();
        partial.insert(Value::String("stress".to_string()), stress_field);
        partial.insert(
            Value::String("max_von_mises".to_string()),
            Value::Real(30.0),
        );
        partial.insert(Value::String("converged".to_string()), Value::Bool(true));
        partial.insert(Value::String("iterations".to_string()), Value::Int(0));
        let partial_case = Value::Map(partial);

        let mcr = multi_case_result_value(&[("A", partial_case)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_case_missing_stress_key_returns_undef() {
        // Case A's ElasticResult Map has no "stress" key (only displacement etc.)
        let axis = vec![0.0, 1.0, 2.0];
        let disp_field = wrap_sampled_field(
            make_sampled_1d("d", axis, vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        // Build a partial ElasticResult missing the stress key.
        let mut partial = BTreeMap::new();
        partial.insert(Value::String("displacement".to_string()), disp_field);
        partial.insert(Value::String("max_von_mises".to_string()), Value::Real(3.0));
        partial.insert(Value::String("converged".to_string()), Value::Bool(true));
        partial.insert(Value::String("iterations".to_string()), Value::Int(0));
        let partial_case = Value::Map(partial);

        let mcr = multi_case_result_value(&[("A", partial_case)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);

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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let stress_field = wrap_sampled_field(
            make_sampled_1d("s", axis, vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);

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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis, vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn linear_combine_stress_sampled_with_non_sampledfield_lambda_returns_undef() {
        // Case B's stress is Sampled-source but lambda is Value::Undef (degenerate).
        // Symmetric to linear_combine_displacement_sampled_with_non_sampledfield_lambda_returns_undef.
        let axis = vec![0.0, 1.0, 2.0];
        let shared_disp = wrap_sampled_field(
            make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_stress = wrap_sampled_field(
            make_sampled_1d("sa", axis, vec![10.0, 20.0, 30.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_stress = Value::Field {
            domain_type: Type::dimensionless_scalar(),
            codomain_type: Type::dimensionless_scalar(),
            source: FieldSourceKind::Sampled,
            lambda: Arc::new(Value::Undef), // Sampled but non-SampledField lambda
        };
        let case_a = make_fixture_elastic_result_with_fields(shared_disp.clone(), a_stress);
        let case_b = make_fixture_elastic_result_with_fields(shared_disp, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    // ── linear_combine Int weight accepted ───────────────────────────────────

    #[test]
    fn linear_combine_int_weight_accepted() {
        // Value::Int is a valid weight (as_f64() returns Some and the result is
        // finite). Pinned because the doc lists Int as accepted and a match that
        // accidentally restricts to Real would silently break integer weights.
        let axis = vec![0.0, 1.0, 2.0];
        let disp_sf = make_sampled_1d("d", axis.clone(), vec![1.0, 2.0, 3.0]);
        let disp_field = wrap_sampled_field(disp_sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let stress_sf = make_sampled_1d("s", axis.clone(), vec![10.0, 20.0, 30.0]);
        let stress_field = wrap_sampled_field(stress_sf, Type::dimensionless_scalar(), Type::dimensionless_scalar());
        let case_a = make_fixture_elastic_result_with_fields(disp_field, stress_field);
        let mcr = multi_case_result_value(&[("A", case_a)]);

        let mut wm = BTreeMap::new();
        // Int weight 2 — must be treated identically to Real(2.0).
        wm.insert(Value::String("A".to_string()), Value::Int(2));

        let result = eval_fea("linear_combine", &[mcr, Value::Map(wm)]).unwrap();
        assert!(!result.is_undef(), "Int weight must be accepted");

        let result_map = match &result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        let disp = result_map
            .get(&Value::String("displacement".to_string()))
            .unwrap();
        let disp_sf = extract_sampled(disp);
        assert_eq!(
            disp_sf.data,
            vec![2.0, 4.0, 6.0],
            "Int(2) weight must double displacement"
        );
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
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let a_disp = wrap_sampled_field(
            make_sampled_1d("da", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let b_disp = wrap_sampled_field(
            make_sampled_1d("db", axis.clone(), vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            }, // codomain mismatch
        );
        let case_a = make_fixture_elastic_result_with_fields(a_disp, shared_stress.clone());
        let case_b = make_fixture_elastic_result_with_fields(b_disp, shared_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);
        let mut wm = BTreeMap::new();
        wm.insert(Value::String("A".to_string()), Value::Real(1.0));
        wm.insert(Value::String("B".to_string()), Value::Real(1.0));
        assert!(
            eval_fea("linear_combine", &[mcr, Value::Map(wm)])
                .unwrap()
                .is_undef()
        );
    }

    // ── envelope_von_mises round-trip ───────────────────────────────────────

    /// Hand-rolled 3×3 tensor row-major windows used by the round-trip tests.
    /// Tensor data layout: d[0]=σ_xx, d[1]=σ_xy, d[2]=σ_xz,
    ///                     d[3]=σ_yx, d[4]=σ_yy, d[5]=σ_yz,
    ///                     d[6]=σ_zx, d[7]=σ_zy, d[8]=σ_zz
    /// (matches the layout `analysis.rs::von_mises` expects.)
    /// Shared body for the Map-shape and SI-shape von_Mises envelope round-trip
    /// tests. `make_er(displacement, stress)` builds one per-case ElasticResult.
    ///
    /// 1-D 3-grid-point fixture: hand-crafted tensors with known closed-form vm
    /// (computed off-line — pins literals instead of re-implementing the formula):
    ///   Case A: P0 uniaxial 100→vm=100, P1 pure-shear 50→vm=50√3, P2 hydrostatic→vm=0
    ///   Case B: P0 hydrostatic→vm=0, P1 uniaxial 200→vm=200, P2 pure-shear 100→vm=100√3
    ///   Envelope: data=[100, 200, 100·√3]
    fn run_envelope_von_mises_two_case_round_trip(make_er: impl Fn(Value, Value) -> Value) {
        let axis = vec![0.0, 1.0, 2.0];
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let tensor_codomain = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure.clone()),
        };
        let domain = Type::dimensionless_scalar();

        let a_tensors: Vec<[f64; 9]> = vec![
            [100.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], // P0 uniaxial σ_xx=100
            [0.0, 50.0, 0.0, 50.0, 0.0, 0.0, 0.0, 0.0, 0.0], // P1 pure shear σ_xy=50
            [200.0, 0.0, 0.0, 0.0, 200.0, 0.0, 0.0, 0.0, 200.0], // P2 hydrostatic 200
        ];
        let b_tensors: Vec<[f64; 9]> = vec![
            [50.0, 0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0, 50.0], // P0 hydrostatic 50
            [200.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],  // P1 uniaxial σ_xx=200
            [0.0, 100.0, 0.0, 100.0, 0.0, 0.0, 0.0, 0.0, 0.0], // P2 pure shear σ_xy=100
        ];

        let a_stress = wrap_sampled_field(
            make_sampled_tensor_3x3_1d("a_stress", axis.clone(), a_tensors.clone()),
            domain.clone(),
            tensor_codomain.clone(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_tensor_3x3_1d("b_stress", axis.clone(), b_tensors.clone()),
            domain.clone(),
            tensor_codomain.clone(),
        );

        // envelope_von_mises only inspects `stress`; displacement is a placeholder.
        let disp_placeholder = wrap_sampled_field(
            make_sampled_1d("disp", axis.clone(), vec![0.0, 0.0, 0.0]),
            domain.clone(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_er(disp_placeholder.clone(), a_stress);
        let case_b = make_er(disp_placeholder, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);

        let result = eval_fea("envelope_von_mises", &[mcr]).unwrap();
        let result_sf = extract_sampled(&result);

        // Hand-computed expected per-grid envelope (see doc comment above).
        // `3.0_f64.sqrt()` is the closed-form √3.
        let expected: [f64; 3] = [
            100.0,                  // max(vm_A=100, vm_B=0)
            200.0,                  // max(vm_A=50·√3, vm_B=200)
            100.0 * 3.0_f64.sqrt(), // max(vm_A=0, vm_B=100·√3)
        ];

        assert_eq!(
            result_sf.data.len(),
            axis.len(),
            "result must have one scalar per grid point"
        );
        for (i, (got, want)) in result_sf.data.iter().zip(expected.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "grid point {i}: got {got}, want {want} (envelope of per-case vm)"
            );
        }

        // Output codomain must be Pressure (same dimension as input tensor quantity).
        match &result {
            Value::Field { codomain_type, .. } => assert_eq!(*codomain_type, pressure),
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    #[test]
    fn envelope_von_mises_two_case_round_trip_returns_per_grid_max_of_per_case_von_mises() {
        run_envelope_von_mises_two_case_round_trip(make_fixture_elastic_result_with_fields);
    }

    // ── envelope_max_principal round-trip ───────────────────────────────────

    /// Closed-form largest principal stress for a 3×3 symmetric stress
    /// window — duplicates the analysis.rs `compute_eigenvalues_3x3`
    /// computation independently so the round-trip test is independent
    /// of the implementation under test.
    ///
    /// For diagonal tensors (off-diagonal = 0) eigenvalues are exactly the
    /// diagonal entries — chosen on purpose to keep the expectation
    /// closed-form simple. For block-diagonal 2×2 tensors we use the
    /// quadratic-formula expression.
    fn max_principal_diagonal(d: &[f64; 9]) -> f64 {
        // Used by the test fixture below where every tensor is diagonal:
        // eigenvalues = (d[0], d[4], d[8]); max = max of those three.
        // (The general formula is not duplicated here — the test fixture
        // sticks to diagonal tensors so the expected output stays trivial.)
        let mut diag = [d[0], d[4], d[8]];
        diag.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        diag[2]
    }

    /// Shared body for the Map-shape and SI-shape max_principal envelope round-trip
    /// tests. `make_er(displacement, stress)` builds one per-case ElasticResult.
    ///
    /// Diagonal stress tensors — eigenvalues = diagonal entries exactly, giving
    /// closed-form max-principal = max(σ_xx, σ_yy, σ_zz):
    ///   Case A: P0→100, P1→80, P2→10  |  Case B: P0→200, P1→50, P2→150
    ///   Envelope: data=[200, 80, 150]
    fn run_envelope_max_principal_two_case_round_trip(make_er: impl Fn(Value, Value) -> Value) {
        let axis = vec![0.0, 1.0, 2.0];
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let tensor_codomain = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure.clone()),
        };
        let domain = Type::dimensionless_scalar();

        let a_tensors: Vec<[f64; 9]> = vec![
            [100.0, 0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0, 20.0],
            [-30.0, 0.0, 0.0, 0.0, 80.0, 0.0, 0.0, 0.0, 60.0],
            [10.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 0.0, 10.0],
        ];
        let b_tensors: Vec<[f64; 9]> = vec![
            [40.0, 0.0, 0.0, 0.0, 200.0, 0.0, 0.0, 0.0, 60.0],
            [50.0, 0.0, 0.0, 0.0, 50.0, 0.0, 0.0, 0.0, 50.0],
            [0.0, 0.0, 0.0, 0.0, -10.0, 0.0, 0.0, 0.0, 150.0],
        ];

        let a_stress = wrap_sampled_field(
            make_sampled_tensor_3x3_1d("a_stress", axis.clone(), a_tensors.clone()),
            domain.clone(),
            tensor_codomain.clone(),
        );
        let b_stress = wrap_sampled_field(
            make_sampled_tensor_3x3_1d("b_stress", axis.clone(), b_tensors.clone()),
            domain.clone(),
            tensor_codomain.clone(),
        );

        // envelope_max_principal only inspects `stress`; displacement is a placeholder.
        let disp_placeholder = wrap_sampled_field(
            make_sampled_1d("disp", axis.clone(), vec![0.0, 0.0, 0.0]),
            domain.clone(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_er(disp_placeholder.clone(), a_stress);
        let case_b = make_er(disp_placeholder, b_stress);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);

        let result = eval_fea("envelope_max_principal", &[mcr]).unwrap();
        let result_sf = extract_sampled(&result);

        // Independently compute expected per-grid envelope via max_principal_diagonal.
        let expected: Vec<f64> = (0..axis.len())
            .map(|i| {
                max_principal_diagonal(&a_tensors[i]).max(max_principal_diagonal(&b_tensors[i]))
            })
            .collect();

        assert_eq!(
            result_sf.data.len(),
            axis.len(),
            "result must have one scalar per grid point"
        );
        for (i, (got, want)) in result_sf.data.iter().zip(expected.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "grid point {i}: got {got}, want {want} (envelope of per-case max_principal)"
            );
        }

        // Output codomain must be Pressure (preserved from input tensor's quantity).
        match &result {
            Value::Field { codomain_type, .. } => assert_eq!(*codomain_type, pressure),
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    #[test]
    fn envelope_max_principal_two_case_round_trip_returns_per_grid_max_of_per_case_max_eigenvalue()
    {
        run_envelope_max_principal_two_case_round_trip(make_fixture_elastic_result_with_fields);
    }

    // ── envelope_displacement_magnitude round-trip ──────────────────────────

    /// Closed-form Euclidean magnitude of a 3-vector window.
    /// Independently duplicates the per-grid-point projection used by
    /// `envelope_displacement_magnitude` so the round-trip test does not
    /// rely on the implementation under test.
    fn vector3_magnitude(v: &[f64; 3]) -> f64 {
        (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
    }

    /// Shared body for the Map-shape and SI-shape displacement-magnitude envelope
    /// round-trip tests. `make_er(displacement, stress)` builds one per-case
    /// ElasticResult.
    ///
    /// Hand-crafted vectors with known closed-form magnitudes:
    ///   Case A: P0=[3,4,0]→5, P1=[0,0,0]→0, P2=[1,1,1]→√3
    ///   Case B: P0=[0,0,0]→0, P1=[6,8,0]→10, P2=[2,0,0]→2
    ///   Envelope: data=[5, 10, 2]
    fn run_envelope_displacement_magnitude_two_case_round_trip(
        make_er: impl Fn(Value, Value) -> Value,
    ) {
        let axis = vec![0.0, 1.0, 2.0];
        let length = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        let vector_codomain = Type::Vector {
            n: 3,
            quantity: Box::new(length.clone()),
        };
        let domain = Type::dimensionless_scalar();

        let a_vectors: Vec<[f64; 3]> = vec![[3.0, 4.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let b_vectors: Vec<[f64; 3]> = vec![[0.0, 0.0, 0.0], [6.0, 8.0, 0.0], [2.0, 0.0, 0.0]];

        let a_disp = wrap_sampled_field(
            make_sampled_vector3_1d("a_disp", axis.clone(), a_vectors.clone()),
            domain.clone(),
            vector_codomain.clone(),
        );
        let b_disp = wrap_sampled_field(
            make_sampled_vector3_1d("b_disp", axis.clone(), b_vectors.clone()),
            domain.clone(),
            vector_codomain.clone(),
        );

        // envelope_displacement_magnitude only inspects `displacement`; stress is a placeholder.
        let stress_placeholder = wrap_sampled_field(
            make_sampled_1d("stress", axis.clone(), vec![0.0, 0.0, 0.0]),
            domain.clone(),
            Type::dimensionless_scalar(),
        );
        let case_a = make_er(a_disp, stress_placeholder.clone());
        let case_b = make_er(b_disp, stress_placeholder);
        let mcr = multi_case_result_value(&[("A", case_a), ("B", case_b)]);

        let result = eval_fea("envelope_displacement_magnitude", &[mcr]).unwrap();
        let result_sf = extract_sampled(&result);

        // Independently compute expected per-grid envelope via vector3_magnitude.
        let expected: Vec<f64> = (0..axis.len())
            .map(|i| vector3_magnitude(&a_vectors[i]).max(vector3_magnitude(&b_vectors[i])))
            .collect();

        assert_eq!(
            result_sf.data.len(),
            axis.len(),
            "result must have one scalar per grid point"
        );
        for (i, (got, want)) in result_sf.data.iter().zip(expected.iter()).enumerate() {
            assert!(
                (got - want).abs() < 1e-6,
                "grid point {i}: got {got}, want {want} (envelope of per-case magnitude)"
            );
        }

        // Output codomain must be Length (preserved from input vector's quantity).
        match &result {
            Value::Field { codomain_type, .. } => assert_eq!(*codomain_type, length),
            other => panic!("expected Value::Field, got {:?}", other),
        }
    }

    #[test]
    fn envelope_displacement_magnitude_two_case_round_trip_returns_per_grid_max_of_per_case_norm() {
        run_envelope_displacement_magnitude_two_case_round_trip(
            make_fixture_elastic_result_with_fields,
        );
    }

    // ── step-3: envelope_* accept per-case StructureInstance ─────────────────
    //
    // SI per-case ElasticResults (solve_load_cases shape) must produce the same
    // per-grid-maximum output as Map per-cases for the same fixture.
    // Each test delegates to the shared round-trip helper defined above.

    #[test]
    fn envelope_von_mises_structure_instance_per_case_returns_per_grid_max_not_undef() {
        run_envelope_von_mises_two_case_round_trip(make_elastic_result_si_with_fields);
    }

    #[test]
    fn envelope_max_principal_structure_instance_per_case_not_undef() {
        run_envelope_max_principal_two_case_round_trip(make_elastic_result_si_with_fields);
    }

    #[test]
    fn envelope_displacement_magnitude_structure_instance_per_case_not_undef() {
        run_envelope_displacement_magnitude_two_case_round_trip(
            make_elastic_result_si_with_fields,
        );
    }

    // ── envelope_{von_mises,max_principal,displacement_magnitude} negatives ─
    //
    // Argument-shape negatives for the three Tensor/Vector → scalar envelope
    // helpers. Each branch of the silent-Undef contract gets its own focused
    // test so a regression bisects directly to the failing rejection path
    // rather than reporting a generic bundled-test failure.
    //
    // Helpers below are parametric on:
    //   - `name`: the eval_fea dispatch name ("envelope_von_mises", etc.)
    //   - `field_name`: which per-case ElasticResult key to populate
    //     ("stress" for von_mises / max_principal, "displacement" for magnitude)
    //   - the expected codomain shape (3×3 matrix vs 3-vector) and stride (9 vs 3)
    //
    // Mirrors the existing `assert_zero_args_returns_undef` family at lines
    // 1683-1810 that parameterises over envelope_max / envelope_min.

    /// Build a 3x3 stress tensor field with the right codomain and a
    /// stride-9 buffer matching the given grid count. Used as the "valid
    /// other case" alongside an intentionally-bad case to exercise the
    /// per-case validation paths.
    fn make_valid_stress_field_3x3(grid: &[f64]) -> Value {
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let tensor_codomain = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure),
        };
        // Diagonal identity tensors at every grid point — eigenvalues = (1,1,1),
        // vm = 0; arbitrary content since this case is the "valid other".
        let tensors: Vec<[f64; 9]> = grid
            .iter()
            .map(|_| [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
            .collect();
        wrap_sampled_field(
            make_sampled_tensor_3x3_1d("valid_stress", grid.to_vec(), tensors),
            Type::dimensionless_scalar(),
            tensor_codomain,
        )
    }

    /// Build a 3-vector displacement field with the right codomain and
    /// a stride-3 buffer matching the given grid count.
    fn make_valid_displacement_field_v3(grid: &[f64]) -> Value {
        let length = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        let vector_codomain = Type::Vector {
            n: 3,
            quantity: Box::new(length),
        };
        let vectors: Vec<[f64; 3]> = grid.iter().map(|_| [1.0, 0.0, 0.0]).collect();
        wrap_sampled_field(
            make_sampled_vector3_1d("valid_disp", grid.to_vec(), vectors),
            Type::dimensionless_scalar(),
            vector_codomain,
        )
    }

    /// Build a per-case ElasticResult Map populating only the requested
    /// field; siblings get Undef placeholders. Used by negative tests
    /// where we want to control exactly which field the validator sees.
    fn make_elastic_result_with_only_field(field_name: &str, field_val: Value) -> Value {
        let mut m = BTreeMap::new();
        m.insert(Value::String(field_name.to_string()), field_val);
        Value::Map(m)
    }

    fn assert_envelope_helper_zero_args_returns_undef(name: &str) {
        // arity must be exactly 1 (MultiCaseResult-shaped Map).
        assert!(eval_fea(name, &[]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_zero_args_returns_undef() {
        assert_envelope_helper_zero_args_returns_undef("envelope_von_mises");
    }

    #[test]
    fn envelope_max_principal_zero_args_returns_undef() {
        assert_envelope_helper_zero_args_returns_undef("envelope_max_principal");
    }

    #[test]
    fn envelope_displacement_magnitude_zero_args_returns_undef() {
        assert_envelope_helper_zero_args_returns_undef("envelope_displacement_magnitude");
    }

    fn assert_envelope_helper_two_args_returns_undef(name: &str) {
        // arity must be exactly 1; an extra positional argument rejects.
        let mcr = multi_case_result_value(&[]);
        let extra = Value::Real(1.0);
        assert!(eval_fea(name, &[mcr, extra]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_two_args_returns_undef() {
        assert_envelope_helper_two_args_returns_undef("envelope_von_mises");
    }

    #[test]
    fn envelope_max_principal_two_args_returns_undef() {
        assert_envelope_helper_two_args_returns_undef("envelope_max_principal");
    }

    #[test]
    fn envelope_displacement_magnitude_two_args_returns_undef() {
        assert_envelope_helper_two_args_returns_undef("envelope_displacement_magnitude");
    }

    fn assert_envelope_helper_non_map_arg_returns_undef(name: &str) {
        // First argument must be a Value::Map (the MultiCaseResult struct).
        assert!(eval_fea(name, &[Value::Real(1.0)]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_non_map_arg_returns_undef() {
        assert_envelope_helper_non_map_arg_returns_undef("envelope_von_mises");
    }

    #[test]
    fn envelope_max_principal_non_map_arg_returns_undef() {
        assert_envelope_helper_non_map_arg_returns_undef("envelope_max_principal");
    }

    #[test]
    fn envelope_displacement_magnitude_non_map_arg_returns_undef() {
        assert_envelope_helper_non_map_arg_returns_undef("envelope_displacement_magnitude");
    }

    fn assert_envelope_helper_map_without_cases_field_returns_undef(name: &str) {
        // Outer Map must have a "cases" key (extract_cases_map enforces).
        let mut bad_outer = BTreeMap::new();
        bad_outer.insert(
            Value::String("not_cases".to_string()),
            Value::Map(BTreeMap::new()),
        );
        assert!(eval_fea(name, &[Value::Map(bad_outer)]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_map_without_cases_field_returns_undef() {
        assert_envelope_helper_map_without_cases_field_returns_undef("envelope_von_mises");
    }

    #[test]
    fn envelope_max_principal_map_without_cases_field_returns_undef() {
        assert_envelope_helper_map_without_cases_field_returns_undef("envelope_max_principal");
    }

    #[test]
    fn envelope_displacement_magnitude_map_without_cases_field_returns_undef() {
        assert_envelope_helper_map_without_cases_field_returns_undef(
            "envelope_displacement_magnitude",
        );
    }

    fn assert_envelope_helper_cases_field_non_map_returns_undef(name: &str) {
        // "cases" key value must be a Value::Map (extract_cases_map enforces).
        let mut bad_outer = BTreeMap::new();
        bad_outer.insert(Value::String("cases".to_string()), Value::Real(7.0));
        assert!(eval_fea(name, &[Value::Map(bad_outer)]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_cases_field_non_map_returns_undef() {
        assert_envelope_helper_cases_field_non_map_returns_undef("envelope_von_mises");
    }

    #[test]
    fn envelope_max_principal_cases_field_non_map_returns_undef() {
        assert_envelope_helper_cases_field_non_map_returns_undef("envelope_max_principal");
    }

    #[test]
    fn envelope_displacement_magnitude_cases_field_non_map_returns_undef() {
        assert_envelope_helper_cases_field_non_map_returns_undef("envelope_displacement_magnitude");
    }

    /// Per-case ElasticResult is missing the field that the helper reads.
    /// `present_field` is the unrelated field that exists on the case (so
    /// the case is a non-empty Map but lacks `expected_field_name`).
    fn assert_envelope_helper_per_case_missing_required_field_returns_undef(
        name: &str,
        expected_field_name: &str,
        present_field_name: &str,
        present_field_value: Value,
    ) {
        let bad_case = make_elastic_result_with_only_field(present_field_name, present_field_value);
        let mcr = multi_case_result_value(&[("A", bad_case)]);
        assert!(
            eval_fea(name, &[mcr]).unwrap().is_undef(),
            "{name} should return undef when per-case is missing required field \
             '{expected_field_name}' (case only carried '{present_field_name}')",
        );
    }

    #[test]
    fn envelope_von_mises_per_case_missing_stress_field_returns_undef() {
        let grid = vec![0.0, 1.0, 2.0];
        // ElasticResult with displacement only, no "stress" key.
        let disp_only = make_valid_displacement_field_v3(&grid);
        assert_envelope_helper_per_case_missing_required_field_returns_undef(
            "envelope_von_mises",
            "stress",
            "displacement",
            disp_only,
        );
    }

    #[test]
    fn envelope_max_principal_per_case_missing_stress_field_returns_undef() {
        let grid = vec![0.0, 1.0, 2.0];
        let disp_only = make_valid_displacement_field_v3(&grid);
        assert_envelope_helper_per_case_missing_required_field_returns_undef(
            "envelope_max_principal",
            "stress",
            "displacement",
            disp_only,
        );
    }

    #[test]
    fn envelope_displacement_magnitude_per_case_missing_displacement_field_returns_undef() {
        let grid = vec![0.0, 1.0, 2.0];
        // ElasticResult with stress only, no "displacement" key.
        let stress_only = make_valid_stress_field_3x3(&grid);
        assert_envelope_helper_per_case_missing_required_field_returns_undef(
            "envelope_displacement_magnitude",
            "displacement",
            "stress",
            stress_only,
        );
    }

    /// Per-case field has the right key but the wrong codomain shape — e.g.
    /// scalar Real codomain instead of Matrix<3,3,Pressure> for von_mises.
    fn assert_envelope_helper_per_case_field_wrong_codomain_returns_undef(
        name: &str,
        field_name: &str,
    ) {
        let grid = vec![0.0, 1.0, 2.0];
        // Build a Sampled Real-codomain field — wrong shape for any of the
        // three helpers (which need Matrix3x3 or Vector3 codomain).
        let bad_field = wrap_sampled_field(
            make_sampled_1d("bad", grid, vec![1.0, 2.0, 3.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let bad_case = make_elastic_result_with_only_field(field_name, bad_field);
        let mcr = multi_case_result_value(&[("A", bad_case)]);
        assert!(eval_fea(name, &[mcr]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_per_case_field_wrong_codomain_returns_undef() {
        assert_envelope_helper_per_case_field_wrong_codomain_returns_undef(
            "envelope_von_mises",
            "stress",
        );
    }

    #[test]
    fn envelope_max_principal_per_case_field_wrong_codomain_returns_undef() {
        assert_envelope_helper_per_case_field_wrong_codomain_returns_undef(
            "envelope_max_principal",
            "stress",
        );
    }

    #[test]
    fn envelope_displacement_magnitude_per_case_field_wrong_codomain_returns_undef() {
        assert_envelope_helper_per_case_field_wrong_codomain_returns_undef(
            "envelope_displacement_magnitude",
            "displacement",
        );
    }

    /// Per-case field has the right codomain but the data buffer length
    /// violates the expected stride (data.len() != grid_count * stride).
    fn assert_envelope_helper_per_case_field_wrong_stride_returns_undef(
        name: &str,
        field_name: &str,
        expected_codomain: Type,
        bad_data_len: usize,
    ) {
        let grid = vec![0.0, 1.0, 2.0];
        // Construct a SampledField with the right codomain Type but a data
        // buffer whose length is off by one — should reject at the stride
        // check `data.len() != grid_count * stride`.
        let bad_data: Vec<f64> = (0..bad_data_len).map(|i| i as f64).collect();
        let bad_sf = make_sampled_1d("bad_stride", grid, bad_data);
        let bad_field = wrap_sampled_field(bad_sf, Type::dimensionless_scalar(), expected_codomain);
        let bad_case = make_elastic_result_with_only_field(field_name, bad_field);
        let mcr = multi_case_result_value(&[("A", bad_case)]);
        assert!(eval_fea(name, &[mcr]).unwrap().is_undef());
    }

    #[test]
    fn envelope_von_mises_per_case_field_wrong_stride_returns_undef() {
        // Matrix<3,3,Pressure> expects stride 9; supply a 10-float buffer
        // (not a multiple of 9 for grid_count=3).
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let codomain = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure),
        };
        assert_envelope_helper_per_case_field_wrong_stride_returns_undef(
            "envelope_von_mises",
            "stress",
            codomain,
            10, // grid_count=3, stride=9 → expected len=27; 10 violates
        );
    }

    #[test]
    fn envelope_max_principal_per_case_field_wrong_stride_returns_undef() {
        let pressure = Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        };
        let codomain = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(pressure),
        };
        assert_envelope_helper_per_case_field_wrong_stride_returns_undef(
            "envelope_max_principal",
            "stress",
            codomain,
            10,
        );
    }

    #[test]
    fn envelope_displacement_magnitude_per_case_field_wrong_stride_returns_undef() {
        // Vector<3,Length> expects stride 3; supply a 4-float buffer
        // (not a multiple of 3 for grid_count=3 — expected len=9; 4 violates).
        let length = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        let codomain = Type::Vector {
            n: 3,
            quantity: Box::new(length),
        };
        assert_envelope_helper_per_case_field_wrong_stride_returns_undef(
            "envelope_displacement_magnitude",
            "displacement",
            codomain,
            4,
        );
    }

    // ── TensorProjection::Magnitude precondition ─────────────────────────────

    /// In dev (debug_assertions on), calling Magnitude.apply on a window
    /// shorter than the stride-3 contract must panic with diagnostic
    /// context — mirrors the >= 9 asserts on compute_von_mises_3x3 /
    /// compute_eigenvalues_3x3 elsewhere in the file.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "at least 3 elements")]
    fn tensor_projection_magnitude_window_too_short_panics_in_dev() {
        let _ = TensorProjection::Magnitude.apply(&[1.0, 2.0]); // length 2 < 3
    }

    // ── task 3029: fea::diagnose unit tests (multi-load-case FEA #10) ─────────

    // step-5 (RED): empty weights map → MultiLoadEmptyWeights diagnostic.
    // Mirrors the trigger in linear_combine_empty_weights_returns_undef. RED
    // until fea::diagnose is introduced in step-6.
    #[test]
    fn diagnose_linear_combine_empty_weights() {
        let mcr = multi_case_result_value(&[("A", make_fixture_elastic_result(0))]);
        let empty_weights = Value::Map(BTreeMap::new());
        let diag = diagnose("linear_combine", &[mcr, empty_weights])
            .expect("empty weights must produce a diagnostic");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::MultiLoadEmptyWeights)
        );
        assert_eq!(
            diag.message,
            "linear_combine: weights map is empty. Specify at least one weighted base case."
        );
    }

    // step-7 (RED): weights map references an unknown (misspelled) case →
    // MultiLoadUnknownCaseInWeights, with the available case names sorted and
    // comma-space joined. RED until the unknown-case arm lands in step-8.
    #[test]
    fn diagnose_linear_combine_unknown_case() {
        let mcr = multi_case_result_value(&[
            ("operating", make_fixture_elastic_result(0)),
            ("overload", make_fixture_elastic_result(0)),
        ]);
        let mut weights = BTreeMap::new();
        weights.insert(Value::String("operatng".to_string()), Value::Real(1.0));
        let diag = diagnose("linear_combine", &[mcr, Value::Map(weights)])
            .expect("unknown case must produce a diagnostic");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::MultiLoadUnknownCaseInWeights)
        );
        assert_eq!(
            diag.message,
            "linear_combine: weights map references unknown case 'operatng'. Available cases: [operating, overload]. Did you misspell the case name?"
        );
    }

    // step-9 (RED): two cases whose displacement fields use incompatible meshes
    // (different grid lengths → metadata_matches false) → MultiLoadIncompatibleMeshes.
    // The first weighted case (BTreeMap key order → 'operating') is the reference
    // (<name1>); the first case whose field metadata mismatches it ('overload')
    // is named in <name2>. A grid-length mismatch is the structural proxy for
    // differing mesh_size / element_order in ElasticOptions. RED until the
    // incompatible-mesh arm lands in step-10.
    /// Shared body for the Map-shape and SI-shape incompatible-mesh diagnose tests.
    /// `make_er(displacement, stress)` builds one per-case ElasticResult.
    ///
    /// 'operating' has 5 grid points; 'overload' has 4 — grid-length mismatch is
    /// the proxy for differing mesh_size / element_order in ElasticOptions.
    fn run_diagnose_linear_combine_incompatible_meshes_body(
        make_er: impl Fn(Value, Value) -> Value,
    ) {
        let op_disp = wrap_sampled_field(
            make_sampled_1d(
                "od",
                vec![0.0, 1.0, 2.0, 3.0, 4.0],
                vec![1.0, 2.0, 3.0, 4.0, 5.0],
            ),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let op_stress = wrap_sampled_field(
            make_sampled_1d(
                "os",
                vec![0.0, 1.0, 2.0, 3.0, 4.0],
                vec![10.0, 20.0, 30.0, 40.0, 50.0],
            ),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let ov_disp = wrap_sampled_field(
            make_sampled_1d("vd", vec![0.0, 1.0, 2.0, 3.0], vec![1.0, 2.0, 3.0, 4.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let ov_stress = wrap_sampled_field(
            make_sampled_1d("vs", vec![0.0, 1.0, 2.0, 3.0], vec![10.0, 20.0, 30.0, 40.0]),
            Type::dimensionless_scalar(),
            Type::dimensionless_scalar(),
        );
        let operating = make_er(op_disp, op_stress);
        let overload = make_er(ov_disp, ov_stress);
        let mcr = multi_case_result_value(&[("operating", operating), ("overload", overload)]);
        let mut weights = BTreeMap::new();
        weights.insert(Value::String("operating".to_string()), Value::Real(1.0));
        weights.insert(Value::String("overload".to_string()), Value::Real(1.0));
        let diag = diagnose("linear_combine", &[mcr, Value::Map(weights)])
            .expect("incompatible meshes must produce a diagnostic");
        assert_eq!(
            diag.code,
            Some(reify_core::DiagnosticCode::MultiLoadIncompatibleMeshes)
        );
        assert_eq!(
            diag.message,
            "linear_combine: cases 'operating' and 'overload' use incompatible meshes \
             (different mesh_size or element_order in their ElasticOptions). \
             Superposition requires matching mesh / element-order layouts. \
             Re-solve with consistent options or compute envelopes instead."
        );
    }

    #[test]
    fn diagnose_linear_combine_incompatible_meshes() {
        run_diagnose_linear_combine_incompatible_meshes_body(
            make_fixture_elastic_result_with_fields,
        );
    }

    // amend (task 3029): regression guard for the reviewer-flagged ordering
    // divergence. A weight entry naming an EXISTING case whose value is a
    // non-Map (e.g. a bare Int, not an ElasticResult struct) is where
    // linear_combine bails — undiagnosed — at fea.rs:168-171, BEFORE it
    // examines any later weight entry. diagnose must mirror that per-entry bail:
    // visiting the non-Map case 'aaa' first (BTreeMap key order: 'aaa' < 'zzz')
    // must yield None, NOT a spurious MultiLoadUnknownCaseInWeights for the
    // later unknown case 'zzz'. (Before the phase-1 case-value-Map check was
    // hoisted ahead of the later entries, diagnose mis-emitted unknown-case for
    // 'zzz' even though linear_combine's real Undef cause was the non-Map 'aaa'.)
    #[test]
    fn diagnose_linear_combine_non_map_case_before_unknown_stays_undiagnosed() {
        let mcr = multi_case_result_value(&[("aaa", Value::Int(5))]);
        let mut weights = BTreeMap::new();
        weights.insert(Value::String("aaa".to_string()), Value::Real(1.0));
        weights.insert(Value::String("zzz".to_string()), Value::Real(1.0));
        assert!(
            diagnose("linear_combine", &[mcr, Value::Map(weights)]).is_none(),
            "a non-Map case value at an earlier weight entry must stay undiagnosed \
             (mirroring linear_combine's per-entry bail), not be misattributed to \
             the later unknown-case entry"
        );
    }

    // SI per-case ElasticResults (solve_load_cases shape) must trigger the same
    // MultiLoadIncompatibleMeshes diagnostic as Map per-cases for identical mesh
    // mismatch — verifying the diagnose-mirrors-linear_combine invariant holds
    // for real-solver inputs.
    #[test]
    fn diagnose_linear_combine_incompatible_meshes_over_structure_instance_cases() {
        run_diagnose_linear_combine_incompatible_meshes_body(make_elastic_result_si_with_fields);
    }

    // ── step-3 (task θ): Kernel↔Sampled-encoding boundary contract ─────────
    // PRD §5 first bullet: div/gradient/curl Sampled fields round-trip through
    // extract_per_case_sampled_field with data.len()==grid_count*stride for
    // stride ∈ {1, 9, 3} AND codomain_type arity == stride.  Complements
    // β's reify-eval raw-data assertion (e2e_cantilever_gradient_curl_field_contract_and_identities)
    // giving the contract genuine two-way coverage (overlay G5).

    /// In-test arity helper: number of f64 scalars per grid node encoded in
    /// a Sampled field with this codomain type.
    ///   - Scalar (dimensioned/dimensionless) → 1
    ///   - Vector { n } → n
    ///   - Tensor { rank, n } → n^rank (rows-major square tensor)
    ///   - Matrix { m, n } → m * n
    ///   - any other type → 1 (unused in practice; prevents test panic)
    fn sampled_field_arity(codomain: &Type) -> usize {
        match codomain {
            Type::Scalar { .. } => 1,
            Type::Vector { n, .. } => *n,
            Type::Tensor { rank, n, .. } => n.pow(*rank as u32),
            Type::Matrix { m, n, .. } => m * n,
            _ => 1,
        }
    }

    /// Kernel↔Sampled-encoding boundary contract (PRD §5, task θ step-3).
    ///
    /// Builds a synthetic ElasticResult-shaped Value::Map on a 5-node Regular1D
    /// grid with three Sampled channels matching solver_elastic.ri's declared
    /// codomains:
    ///   - divergence  (stride 1, codomain = Real/dimensionless scalar)
    ///   - gradient    (stride 9, codomain = Tensor<2,3,Real>)
    ///   - curl        (stride 3, codomain = Vector3<Real>)
    ///
    /// Assertions:
    ///   (1) extract_per_case_sampled_field returns Some for each correct stride,
    ///       with data.len() == grid_count * stride.
    ///   (2) The returned codomain_type arity == stride for each channel.
    ///   (3) NEGATIVE: stride-mismatch and missing-field queries return None.
    #[test]
    fn kernel_sampled_encoding_boundary_contract_div_grad_curl() {
        // 5-node Regular1D grid (matches δ's proven make_1d_scalar(5,1.0,..) fixture).
        let axis: Vec<f64> = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let n = axis.len(); // 5

        // ── divergence channel: stride 1, codomain = Real (dimensionless scalar) ──
        let div_sf = make_sampled_1d(
            "divergence",
            axis.clone(),
            vec![0.1, 0.2, 0.3, 0.4, 0.5],
        );
        let div_field = wrap_sampled_field(
            div_sf,
            Type::point3(Type::length()),
            Type::dimensionless_scalar(),
        );

        // ── gradient channel: stride 9, codomain = Tensor<2,3,Real> ──────────────
        let grad_tensors: Vec<[f64; 9]> = (0..n)
            .map(|i| {
                let v = i as f64 * 0.1;
                [v, v, v, v, v, v, v, v, v]
            })
            .collect();
        let grad_sf = make_sampled_tensor_3x3_1d("gradient", axis.clone(), grad_tensors);
        let grad_field = wrap_sampled_field(
            grad_sf,
            Type::point3(Type::length()),
            Type::tensor(2, 3, Type::dimensionless_scalar()),
        );

        // ── curl channel: stride 3, codomain = Vector3<Real> ─────────────────────
        let curl_vecs: Vec<[f64; 3]> = (0..n)
            .map(|i| {
                let v = i as f64 * 0.01;
                [v, v, v]
            })
            .collect();
        let curl_sf = make_sampled_vector3_1d("curl", axis.clone(), curl_vecs);
        let curl_field = wrap_sampled_field(
            curl_sf,
            Type::point3(Type::length()),
            Type::vec3(Type::dimensionless_scalar()),
        );

        // Build a synthetic ElasticResult-shaped Value::Map (field-keyed by String).
        let result = make_envelope_map(&[
            ("divergence", div_field),
            ("gradient", grad_field),
            ("curl", curl_field),
        ]);

        // ── (1) stride-match: each extract_per_case_sampled_field returns Some ────
        let (_, div_cod_rt, div_sf_rt) =
            extract_per_case_sampled_field(&result, "divergence", 1)
                .expect("divergence stride-1 round-trip must succeed");
        let (_, grad_cod_rt, grad_sf_rt) =
            extract_per_case_sampled_field(&result, "gradient", 9)
                .expect("gradient stride-9 round-trip must succeed");
        let (_, curl_cod_rt, curl_sf_rt) =
            extract_per_case_sampled_field(&result, "curl", 3)
                .expect("curl stride-3 round-trip must succeed");

        // Verify data.len() == grid_count * stride for each channel.
        let grid_count: usize = div_sf_rt.axis_grids.iter().map(|g| g.len()).product();
        assert_eq!(
            div_sf_rt.data.len(),
            grid_count,
            "divergence data.len() must equal grid_count*1"
        );
        assert_eq!(
            grad_sf_rt.data.len(),
            grid_count * 9,
            "gradient data.len() must equal grid_count*9"
        );
        assert_eq!(
            curl_sf_rt.data.len(),
            grid_count * 3,
            "curl data.len() must equal grid_count*3"
        );

        // ── (2) codomain_type arity == stride ─────────────────────────────────────
        assert_eq!(
            sampled_field_arity(div_cod_rt),
            1,
            "divergence codomain arity must be 1 (stride 1)"
        );
        assert_eq!(
            sampled_field_arity(grad_cod_rt),
            9,
            "gradient codomain arity must be 9 (stride 9)"
        );
        assert_eq!(
            sampled_field_arity(curl_cod_rt),
            3,
            "curl codomain arity must be 3 (stride 3)"
        );

        // ── (3) negative: stride mismatch and missing field return None ───────────
        assert!(
            extract_per_case_sampled_field(&result, "divergence", 3).is_none(),
            "stride mismatch (divergence queried with stride 3) must return None"
        );
        assert!(
            extract_per_case_sampled_field(&result, "nonexistent_field", 1).is_none(),
            "missing field name must return None"
        );
    }

    // ── worst_buckling_case unit tests ──────────────────────────────────────
    //
    // RED until step-4 adds the "worst_buckling_case" arm to eval_fea:
    //   eval_fea("worst_buckling_case", ...) returns None (unrecognised name).
    //
    // GREEN after step-4:
    //   eval_fea returns Some(Value::Undef) for bad args, Some(Value::String)
    //   for the case with the smallest modes[0].eigenvalue.

    /// Build a minimal `BucklingResult` StructureInstance with one Mode at
    /// the given eigenvalue.  Mirrors the trampoline's output shape.
    fn make_buckling_result(eigenvalue: f64) -> Value {
        let mode_fields: PersistentMap<String, Value> = [
            ("eigenvalue".to_string(), Value::Real(eigenvalue)),
            ("mode_shape".to_string(), Value::Undef),
        ]
        .into_iter()
        .collect();
        let mode = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Mode".to_string(),
            version: 1,
            fields: mode_fields,
        }));
        let result_fields: PersistentMap<String, Value> = [
            ("modes".to_string(), Value::List(vec![mode])),
            ("converged".to_string(), Value::Bool(true)),
            ("iterations".to_string(), Value::Int(0)),
            ("pre_stress".to_string(), Value::Undef),
        ]
        .into_iter()
        .collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "BucklingResult".to_string(),
            version: 1,
            fields: result_fields,
        }))
    }

    /// Build a `MultiCaseBucklingResult`-shaped `Value::Map` from
    /// `(name, BucklingResult)` pairs.
    fn make_mcbr(cases: &[(&str, Value)]) -> Value {
        let inner: BTreeMap<Value, Value> = cases
            .iter()
            .map(|(n, v)| (Value::String((*n).to_string()), v.clone()))
            .collect();
        let mut outer = BTreeMap::new();
        outer.insert(Value::String("cases".to_string()), Value::Map(inner));
        Value::Map(outer)
    }

    // ── dispatcher-signal tests (worst_buckling_case) ───────────────────────

    #[test]
    fn eval_fea_worst_buckling_case_returns_some() {
        // "worst_buckling_case" must be a recognised name in eval_fea.
        // RED: before step-4, returns None → assertion fails.
        assert!(eval_fea("worst_buckling_case", &[]).is_some());
    }

    // ── correctness tests ────────────────────────────────────────────────────

    #[test]
    fn worst_buckling_case_returns_min_eigenvalue_case() {
        // "low" has eigenvalue 2.0, "high" has eigenvalue 4.0.
        // worst_buckling_case must return "low" (smallest λ = closest to buckling).
        let mcbr = make_mcbr(&[
            ("low", make_buckling_result(2.0)),
            ("high", make_buckling_result(4.0)),
        ]);
        assert_eq!(
            eval_fea("worst_buckling_case", &[mcbr]).unwrap(),
            Value::String("low".to_string())
        );
    }

    #[test]
    fn worst_buckling_case_lex_first_min_tie_break() {
        // Two cases with identical eigenvalue — lexicographically-first name wins.
        // BTreeMap iterates in "aaa" < "bbb" order, so "aaa" is first-occurrence.
        let mcbr = make_mcbr(&[
            ("aaa", make_buckling_result(3.0)),
            ("bbb", make_buckling_result(3.0)),
        ]);
        assert_eq!(
            eval_fea("worst_buckling_case", &[mcbr]).unwrap(),
            Value::String("aaa".to_string())
        );
    }

    // ── silent-Undef negative paths ─────────────────────────────────────────

    #[test]
    fn worst_buckling_case_zero_args_returns_undef() {
        assert!(eval_fea("worst_buckling_case", &[]).unwrap().is_undef());
    }

    #[test]
    fn worst_buckling_case_two_args_returns_undef() {
        let mcbr = make_mcbr(&[("a", make_buckling_result(2.0))]);
        assert!(
            eval_fea("worst_buckling_case", &[mcbr, Value::Undef])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn worst_buckling_case_non_map_arg_returns_undef() {
        assert!(eval_fea("worst_buckling_case", &[Value::Undef])
            .unwrap()
            .is_undef());
    }

    #[test]
    fn worst_buckling_case_missing_cases_key_returns_undef() {
        let map = Value::Map(BTreeMap::new());
        assert!(eval_fea("worst_buckling_case", &[map]).unwrap().is_undef());
    }

    #[test]
    fn worst_buckling_case_empty_modes_list_returns_undef() {
        // BucklingResult with empty modes → no eigenvalue → silent Undef.
        let result_fields: PersistentMap<String, Value> = [
            ("modes".to_string(), Value::List(vec![])),
            ("converged".to_string(), Value::Bool(true)),
        ]
        .into_iter()
        .collect();
        let case_val = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "BucklingResult".to_string(),
            version: 1,
            fields: result_fields,
        }));
        let mcbr = make_mcbr(&[("only", case_val)]);
        assert!(eval_fea("worst_buckling_case", &[mcbr]).unwrap().is_undef());
    }

    #[test]
    fn worst_buckling_case_non_real_eigenvalue_returns_undef() {
        // Mode with eigenvalue: Undef (non-Real) → skip → no finite eigenvalue → Undef.
        let mode_fields: PersistentMap<String, Value> = [
            ("eigenvalue".to_string(), Value::Undef),
        ]
        .into_iter()
        .collect();
        let mode = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Mode".to_string(),
            version: 1,
            fields: mode_fields,
        }));
        let result_fields: PersistentMap<String, Value> = [
            ("modes".to_string(), Value::List(vec![mode])),
        ]
        .into_iter()
        .collect();
        let case_val = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "BucklingResult".to_string(),
            version: 1,
            fields: result_fields,
        }));
        let mcbr = make_mcbr(&[("only", case_val)]);
        assert!(eval_fea("worst_buckling_case", &[mcbr]).unwrap().is_undef());
    }

    // ── envelope_critical_load unit tests ───────────────────────────────────
    //
    // Symmetrical with the worst_buckling_case tests above.
    // Uses the same make_buckling_result / make_mcbr helpers.

    // ── dispatcher-signal test (envelope_critical_load) ─────────────────────

    #[test]
    fn eval_fea_envelope_critical_load_returns_some() {
        // "envelope_critical_load" must be a recognised name in eval_fea.
        assert!(eval_fea("envelope_critical_load", &[]).is_some());
    }

    // ── correctness tests ────────────────────────────────────────────────────

    #[test]
    fn envelope_critical_load_returns_min_eigenvalue_times_reference() {
        // "low" has λ=2.0, "high" has λ=4.0, reference=1000 N.
        // envelope = min(2.0, 4.0) × 1000 = 2000 N.
        let mcbr = make_mcbr(&[
            ("low", make_buckling_result(2.0)),
            ("high", make_buckling_result(4.0)),
        ]);
        let ref_load = Value::Scalar {
            si_value: 1000.0,
            dimension: DimensionVector::FORCE,
        };
        let result = eval_fea("envelope_critical_load", &[mcbr, ref_load])
            .unwrap();
        match result {
            Value::Scalar { si_value, dimension } => {
                assert_eq!(dimension, DimensionVector::FORCE);
                assert!(
                    (si_value - 2000.0).abs() < 1e-10,
                    "expected 2000.0 N, got {si_value}"
                );
            }
            other => panic!("expected Value::Scalar{{Force}}, got: {other:?}"),
        }
    }

    #[test]
    fn envelope_critical_load_dimension_propagates_from_reference() {
        // The returned Scalar must carry the same dimension as reference_load.
        let mcbr = make_mcbr(&[("a", make_buckling_result(5.0))]);
        let ref_load = Value::Scalar {
            si_value: 500.0,
            dimension: DimensionVector::FORCE,
        };
        let result = eval_fea("envelope_critical_load", &[mcbr, ref_load])
            .unwrap();
        match result {
            Value::Scalar { dimension, .. } => {
                assert_eq!(dimension, DimensionVector::FORCE);
            }
            other => panic!("expected Value::Scalar, got: {other:?}"),
        }
    }

    // ── silent-Undef negative paths ─────────────────────────────────────────

    #[test]
    fn envelope_critical_load_zero_args_returns_undef() {
        assert!(eval_fea("envelope_critical_load", &[]).unwrap().is_undef());
    }

    #[test]
    fn envelope_critical_load_one_arg_returns_undef() {
        let mcbr = make_mcbr(&[("a", make_buckling_result(3.0))]);
        assert!(eval_fea("envelope_critical_load", &[mcbr])
            .unwrap()
            .is_undef());
    }

    #[test]
    fn envelope_critical_load_three_args_returns_undef() {
        let mcbr = make_mcbr(&[("a", make_buckling_result(3.0))]);
        let ref_load = Value::Scalar {
            si_value: 1000.0,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            eval_fea("envelope_critical_load", &[mcbr, ref_load, Value::Undef])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn envelope_critical_load_non_map_first_arg_returns_undef() {
        let ref_load = Value::Scalar {
            si_value: 1000.0,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            eval_fea("envelope_critical_load", &[Value::Undef, ref_load])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn envelope_critical_load_non_scalar_ref_returns_undef() {
        let mcbr = make_mcbr(&[("a", make_buckling_result(3.0))]);
        assert!(
            eval_fea("envelope_critical_load", &[mcbr, Value::Undef])
                .unwrap()
                .is_undef()
        );
    }

    #[test]
    fn envelope_critical_load_no_finite_eigenvalue_returns_undef() {
        // All cases have shape-failure eigenvalues → no finite min → Undef.
        let mode_fields: PersistentMap<String, Value> = [
            ("eigenvalue".to_string(), Value::Undef),
        ]
        .into_iter()
        .collect();
        let mode = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "Mode".to_string(),
            version: 1,
            fields: mode_fields,
        }));
        let result_fields: PersistentMap<String, Value> = [
            ("modes".to_string(), Value::List(vec![mode])),
        ]
        .into_iter()
        .collect();
        let case_val = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(u32::MAX),
            type_name: "BucklingResult".to_string(),
            version: 1,
            fields: result_fields,
        }));
        let mcbr = make_mcbr(&[("only", case_val)]);
        let ref_load = Value::Scalar {
            si_value: 1000.0,
            dimension: DimensionVector::FORCE,
        };
        assert!(
            eval_fea("envelope_critical_load", &[mcbr, ref_load])
                .unwrap()
                .is_undef()
        );
    }
}
