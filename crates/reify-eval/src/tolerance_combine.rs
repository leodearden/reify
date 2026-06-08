//! Output-occurrence × active-purpose tolerance combiner.
//!
//! See `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! → "Tolerance lives at the purpose") for the contract that drives this
//! module.
//!
//! # Recognition-shape twin
//!
//! The `extract_output_tolerance_bound` extractor and the new
//! `recognize_representation_within` asserter share recognition gates via the
//! private `match_representation_within_shape` helper — both functions call it
//! so there is a single gate implementation that cannot drift (retiring the
//! TODO that lived in the extractor's doc before task 4199 γ).
//!
//! The canonical recognition shape:
//! `UserFunctionCall("RepresentationWithin", [<ValueRef typed StructureRef>,
//! <Literal Scalar LENGTH finite>=0])`

use crate::graph::ConstraintNodeData;
use reify_core::{ConstraintNodeId, DimensionVector, Diagnostic, Type, ValueCellId};
use reify_ir::{CompiledExpr, CompiledExprKind, PersistentMap, Satisfaction, Value, ValueMap};
use reify_ir::value::GeometryHandleRef;
use std::collections::BTreeMap;

/// Combine an output occurrence's tolerance bound with the active purpose's
/// tolerance bound under partial-order "tighter satisfies looser" semantics.
///
/// The two bounds are conceptually different lookups but share the same f64
/// units (SI metres):
/// - `output_bound` — from a `RepresentationWithin(subject, tol)` constraint
///   declared on the output occurrence's template (e.g. `STEPOutput`).
/// - `purpose_bound` — from
///   [`crate::Engine::active_tolerance_for`], computed by the
///   active-purpose subject prefix-scan in `tolerance_scope`.
///
/// # Combination rule
///
/// | output_bound  | purpose_bound | result                |
/// |---------------|---------------|-----------------------|
/// | `Some(o)`     | `Some(p)`     | `Some(o.min(p))`      |
/// | `Some(t)`     | `None`        | `Some(t)`             |
/// | `None`        | `Some(t)`     | `Some(t)`             |
/// | `None`        | `None`        | `None`                |
///
/// The `min`-when-both-Some rule is the same partial-order semantics as the
/// cache-side `tolerance_bucket` (lookup uses the `<=` rule) and the
/// purpose-side `tolerance_scope::merge_with_min`: tighter satisfies looser,
/// so the smaller of two demanded tolerances wins.
pub fn combine_demanded_tolerance(
    output_bound: Option<f64>,
    purpose_bound: Option<f64>,
) -> Option<f64> {
    // Mirror `tolerance_bucket::insert/lookup` and `tolerance_budget::
    // per_stage_tolerance` posture: NaN/Inf/negative tolerances would
    // propagate silently into demand callers (NaN comparisons always evaluate
    // false, so a stale NaN min could never be displaced). Panic in debug
    // builds at the call site rather than letting the bad value contaminate
    // a downstream realization. Same panic-message format across the four
    // tolerance_* modules so authoring errors surface with one voice.
    for tol in [output_bound, purpose_bound].iter().flatten() {
        debug_assert!(
            tol.is_finite() && *tol >= 0.0,
            "ToleranceCombine: tolerance must be finite and non-negative, got {tol}"
        );
    }
    match (output_bound, purpose_bound) {
        (Some(o), Some(p)) => Some(o.min(p)),
        (Some(t), None) | (None, Some(t)) => Some(t),
        (None, None) => None,
    }
}

// ── Private shared recognition helper ────────────────────────────────────────

/// Match the canonical `RepresentationWithin` shape in a single
/// [`CompiledExpr`] and return the three parsed fields, or `None` on any
/// gate failure.
///
/// ## Gates (mirroring `extract_output_tolerance_bound`'s inner gates)
///
/// * **Gate 2** — top-level `UserFunctionCall("RepresentationWithin", [arg0, arg1])`.
/// * **Gate 3** — `arg0` is a `ValueRef(vcid)` whose `result_type` is
///   `StructureRef(name)`.
/// * **Gate 4a** — `arg1` is a `Literal(Scalar { dimension == LENGTH, .. })`.
/// * **Gate 4b/c** — `si_value` passes `is_valid_tolerance_si` (finite + ≥ 0.0).
///
/// Returns `(subject_vcid, struct_ref_name, bound_si)` on success.
fn match_representation_within_shape(
    expr: &CompiledExpr,
) -> Option<(ValueCellId, String, f64)> {
    // Gate 2: top-level UserFunctionCall("RepresentationWithin", [arg0, arg1]).
    let (function_name, args) = match &expr.kind {
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => (function_name, args),
        _ => return None,
    };
    if function_name != "RepresentationWithin" {
        return None;
    }
    if args.len() != 2 {
        return None;
    }

    // Gate 3: arg0 must be a ValueRef whose result_type is StructureRef(name).
    let subject_arg = &args[0];
    let vcid = match &subject_arg.kind {
        CompiledExprKind::ValueRef(id) => id.clone(),
        _ => return None,
    };
    let struct_name = match &subject_arg.result_type {
        Type::StructureRef(name) => name.clone(),
        _ => return None,
    };

    // Gate 4: arg1 must be a Literal(Scalar { dimension == LENGTH, si_value })
    // with finite, non-negative si_value (4b/c via is_valid_tolerance_si).
    let tol_arg = &args[1];
    let si_value = match &tol_arg.kind {
        CompiledExprKind::Literal(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == DimensionVector::LENGTH => *si_value,
        _ => return None,
    };
    if !crate::tolerance_gate::is_valid_tolerance_si(si_value) {
        return None;
    }

    Some((vcid, struct_name, si_value))
}

// ── Assertion recognizer ──────────────────────────────────────────────────────

/// Recognise a single `RepresentationWithin` constraint expression and return
/// its three parsed components, or `None` if the expression does not match the
/// canonical shape.
///
/// Delegates directly to [`match_representation_within_shape`] — see that
/// function for the gate definitions. This function is the public asserter
/// entry point; [`extract_output_tolerance_bound`] is the budget-extractor
/// entry point. Both share the same gate implementation so they cannot drift.
///
/// ## Return value
///
/// `Some((subject_vcid, struct_ref_name, bound_si_metres))` on a match;
/// `None` on any gate failure (silent-skip posture).
pub fn recognize_representation_within(
    expr: &CompiledExpr,
) -> Option<(ValueCellId, String, f64)> {
    match_representation_within_shape(expr)
}

// ── Pure assertion eval helper ────────────────────────────────────────────────

/// Planar quantization floor for the C4 zero-bound comparator (SI metres).
///
/// β B1 validates that a planar box's achieved deviation is ≤ 1e-5 m at unit
/// scale (~1e-6 m f32 quantization — NOT exactly 0.0). Flooring a zero
/// ("exact") bound at this ceiling makes a planar subject Satisfied while a
/// coarse curved subject (β B2: ≫ 1e-5) is Violated, WITHOUT loosening any
/// non-zero (C3) bound.
pub const PLANAR_FLOOR: f64 = 1e-5;

/// Evaluate a single `RepresentationWithin` assertion expression against the
/// current value map and the post-tessellation achieved-repr-tol map.
///
/// # Contract
///
/// * Returns `None` when `expr` is not a `RepresentationWithin` shape (so
///   callers can pass arbitrary constraint expressions through safely).
/// * Returns `Some((Indeterminate, None))` when the subject cannot be resolved
///   to a key in `achieved_repr_tol` — encodes C1 (realization not run ⇒
///   never a false `Violated`).
/// * Returns `Some((Satisfied, None))` when `achieved ≤ eff_bound`.
/// * Returns `Some((Violated, Some(diag)))` when `achieved > eff_bound`;
///   the diagnostic message follows PRD §8.3 ("sampled facet deviation"
///   semantics — the metric is a sampled lower bound, so `Violated` means the
///   **measured** deviation exceeded the bound, not that a tighter bound is
///   provably unachievable).
///
/// # Subject → key resolution (hybrid)
///
/// 1. **Value-based:** look up `vcid` in `values`. If the result is a
///    `GeometryHandle`, its `realization_ref.to_string()` is the key. If it is
///    a `StructureInstance`, scan its `fields` for any `GeometryHandle` field.
/// 2. **Type-name fallback:** scan `achieved_repr_tol` keys for the prefix
///    `"{struct_name}#realization["`. If multiple keys match, take the one with
///    the **maximum** achieved value (conservative — avoids a false `Satisfied`
///    when a module has multiple realizations with varying quality). This path
///    is hydration-independent and works for the post-tessellate `check()` call
///    whose fresh `eval()` may not re-hydrate the subject's `GeometryHandle`.
///
/// # Zero-bound comparator (C4)
///
/// `eff = if bound <= 0.0 { PLANAR_FLOOR } else { bound }`. See [`PLANAR_FLOOR`].
pub fn eval_representation_within(
    id: &ConstraintNodeId,
    expr: &CompiledExpr,
    values: &ValueMap,
    achieved_repr_tol: &BTreeMap<String, f64>,
) -> Option<(Satisfaction, Option<Diagnostic>)> {
    // Step 1: recognise the shape. None → caller should treat as non-assertion.
    let (vcid, struct_name, bound) = match_representation_within_shape(expr)?;

    // Step 2: resolve the subject to an achieved_repr_tol key.
    let key = resolve_repr_tol_key(&vcid, &struct_name, values, achieved_repr_tol);

    // Step 3: look up achieved value (absent key ⇒ Indeterminate, C1).
    let achieved_opt = key.and_then(|k| achieved_repr_tol.get(&k).copied());

    // Step 4: three-valued comparison with C4 zero-bound floor.
    match achieved_opt {
        None => Some((Satisfaction::Indeterminate, None)),
        Some(achieved) => {
            let eff = if bound <= 0.0 { PLANAR_FLOOR } else { bound };
            if achieved <= eff {
                Some((Satisfaction::Satisfied, None))
            } else {
                // PRD §8.3: "sampled facet deviation exceeds bound" — the
                // metric is a sampled lower bound on the achievable deviation,
                // so this diagnostic means the measured deviation exceeded the
                // bound, not that a tighter mesh cannot satisfy it.
                let diag = Diagnostic::error(format!(
                    "RepresentationWithin: sampled facet deviation {achieved:.3e} m \
                     exceeds bound {bound:.3e} m for {}",
                    id.entity
                ));
                Some((Satisfaction::Violated, Some(diag)))
            }
        }
    }
}

/// Resolve the `RepresentationWithin` subject `vcid` to an
/// `achieved_repr_tol` map key (a `"{entity}#realization[{idx}]"` string).
///
/// Returns `None` when neither value-based resolution nor the type-name scan
/// finds a key — the caller interprets this as Indeterminate (C1).
fn resolve_repr_tol_key(
    vcid: &ValueCellId,
    struct_name: &str,
    values: &ValueMap,
    achieved_repr_tol: &BTreeMap<String, f64>,
) -> Option<String> {
    // — Value-based resolution (hydration-dependent path) ——————————————————
    if let Some(v) = values.get(vcid) {
        // Direct GeometryHandle.
        if let Some(ghr) = GeometryHandleRef::from_geometry_handle(v) {
            return Some(ghr.realization_ref.to_string());
        }
        // StructureInstance: scan fields for any GeometryHandle field.
        if let Value::StructureInstance(data) = v {
            for field_val in data.fields.values() {
                if let Some(ghr) = GeometryHandleRef::from_geometry_handle(field_val) {
                    return Some(ghr.realization_ref.to_string());
                }
            }
        }
    }

    // — Type-name scan fallback (hydration-independent) ————————————————————
    // Scan achieved_repr_tol keys for the prefix "{struct_name}#realization[".
    // If multiple keys match, take the one with the MAXIMUM achieved value
    // (conservative — guards against a false Satisfied when a module has
    // multiple realizations of the same type with varying quality).
    let prefix = format!("{}#realization[", struct_name);
    let mut best_key: Option<String> = None;
    let mut best_val: f64 = f64::NEG_INFINITY;
    for (k, &v) in achieved_repr_tol.iter() {
        if k.starts_with(&prefix) && v > best_val {
            best_val = v;
            best_key = Some(k.clone());
        }
    }
    best_key
}

/// Extract the tightest `RepresentationWithin` tolerance bound declared on
/// the named output occurrence's template, by scanning the runtime
/// constraint graph.
///
/// Output occurrences (e.g. `STEPOutput`, see arch §14.5) carry a body
/// constraint shaped like `RepresentationWithin(subject, 1um)`. When such an
/// occurrence is sub-instantiated, the constraint stays under its
/// *template-name* entity scope in the runtime graph (subs duplicate value
/// cells under scoped entity-refs but do NOT scope-duplicate constraints —
/// see `crate::graph::EvaluationGraph::from_templates`). So the extractor
/// scans `constraints` for entries with `id.entity == output_template_name`
/// regardless of how many times the occurrence was instantiated.
///
/// # Recognition gates
///
/// Mirrors [`crate::tolerance_scope::extract_tolerance_bindings`] (with the
/// purpose-param-membership check dropped — output occurrences have a fixed
/// `param subject : Structure` binding pattern at the template level rather
/// than per-purpose param identity):
///
/// 1. **Entity filter:** `id.entity == output_template_name`. Exact match —
///    no dot-boundary trickery (that semantic belongs to the descendants
///    prefix-scan, not template-name lookup).
/// 2. **Outer shape:** top-level `UserFunctionCall("RepresentationWithin",
///    [arg0, arg1])`.
/// 3. **Subject (arg0):** `ValueRef` whose `result_type` is `StructureRef(_)`.
/// 4. **Tolerance literal (arg1):** `Literal(Value::Scalar { dimension ==
///    LENGTH, si_value })` where `si_value.is_finite() && si_value >= 0.0`.
///    Negative finite literals are silently skipped to keep this extractor
///    in lockstep with [`combine_demanded_tolerance`]'s debug-assert
///    `is_finite() && >= 0.0` invariant — without the gate, a negative bound
///    would survive extraction, then panic the engine in debug builds and
///    silently win an `o.min(p)` race in release builds.
///
/// # Min-fold across multiple matches
///
/// A template author may declare multiple `RepresentationWithin` constraints
/// (e.g. inner vs outer subjects, or a guarded-group split). The extractor
/// folds via `min` under partial-order "tighter satisfies looser" semantics,
/// matching [`crate::tolerance_scope::merge_with_min`] and the cache-side
/// `tolerance_bucket` `<=` rule.
///
/// # Silent-skip posture
///
/// Non-matching shapes / non-finite literals / non-LENGTH dimensions /
/// unrelated entities are silently skipped (no panic). This matches the
/// "activate dormant infrastructure" posture from task 2647 — extraction is
/// policy-neutral; downstream consumers can layer diagnostics if a missing
/// or malformed bound is suspicious.
///
/// Returns `None` when no matching constraint exists.
///
/// # TODO
///
/// The recognition gates are duplicated from
/// [`crate::tolerance_scope::extract_tolerance_bindings`]. A shared helper
/// would prevent drift between the two extractors but requires touching the
/// `tolerance_scope` public surface — deferred to keep this task scoped to
/// the 2650 contract.
pub fn extract_output_tolerance_bound(
    constraints: &PersistentMap<ConstraintNodeId, ConstraintNodeData>,
    output_template_name: &str,
) -> Option<f64> {
    // Silent-skip audit (locked by `extract_output_tolerance_bound_skips_
    // non_finite_non_length_and_unrelated_entity`):
    //   Gate 1 (entity filter)        skips unrelated-entity entries
    //   Gates 2-4 (shape match)       delegated to
    //                                 `match_representation_within_shape` —
    //                                 see that function for the per-gate
    //                                 breakdown (UFC name+arity, ValueRef
    //                                 :StructureRef, Literal Scalar LENGTH
    //                                 finite≥0). Every non-match returns None
    //                                 → `continue` here.
    // No `panic!`, `expect`, or `unwrap` is reachable — malformed graphs
    // are silently skipped.
    let mut tightest: Option<f64> = None;
    for (id, data) in constraints.iter() {
        // Gate 1: entity exact-match filter.
        if id.entity != output_template_name {
            continue;
        }

        // Gates 2-4: shared recognition shape (UFC + ValueRef:StructureRef +
        // Literal Scalar LENGTH finite≥0). Only the bound (si_value) is needed
        // here — subject vcid and StructureRef name are discarded (C2: public
        // signature and behavior are byte-identical to the pre-factoring impl).
        let Some((_vcid, _struct_name, si_value)) =
            match_representation_within_shape(&data.expr)
        else {
            continue;
        };

        // Min-fold under partial-order "tighter satisfies looser" semantics.
        tightest = Some(match tightest {
            Some(cur) => cur.min(si_value),
            None => si_value,
        });
    }
    tightest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ConstraintNodeData;
    use reify_core::{ConstraintNodeId, ContentHash, DimensionVector, Type, ValueCellId};
    use reify_ir::{CompiledExpr, PersistentMap, Value};

    /// Build a `(ConstraintNodeId, ConstraintNodeData)` pair carrying the
    /// canonical `RepresentationWithin(<ValueRef typed StructureRef>,
    /// <Literal Scalar(LENGTH)>)` shape that `extract_output_tolerance_bound`
    /// is expected to recognise. Mirrors the
    /// `tolerance_scope::tests::representation_within_constraint` fixture
    /// but produces a graph-side `ConstraintNodeData` instead of a
    /// `CompiledPurpose` constraint.
    fn representation_within_constraint_node(
        entity: &str,
        index: u32,
        si_value: f64,
        dimension: DimensionVector,
    ) -> (ConstraintNodeId, ConstraintNodeData) {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Structure".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value,
                dimension,
            },
            Type::Scalar { dimension },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        let id = ConstraintNodeId::new(entity, index);
        let data = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr,
            content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
            optimized_target: None,
        };
        (id, data)
    }

    /// Build a `(ConstraintNodeId, ConstraintNodeData)` pair whose outer
    /// `CompiledExpr` is the caller-supplied `expr` instead of the canonical
    /// `RepresentationWithin(...)` shape. Used by the silent-skip audit
    /// fixtures that exercise outer-shape mismatches (non-`UserFunctionCall`
    /// outer kind, wrong arity, wrong arg-type) — the matcher must `continue`
    /// past these without observing them.
    fn constraint_node_with(
        entity: &str,
        index: u32,
        expr: CompiledExpr,
    ) -> (ConstraintNodeId, ConstraintNodeData) {
        let id = ConstraintNodeId::new(entity, index);
        let data = ConstraintNodeData {
            id: id.clone(),
            label: None,
            expr,
            content_hash: ContentHash::of_str(&format!("{}#constraint[{}]", entity, index)),
            optimized_target: None,
        };
        (id, data)
    }

    #[test]
    fn extract_output_tolerance_bound_skips_non_finite_non_length_and_unrelated_entity() {
        let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
            PersistentMap::default();

        // (a) NaN tolerance literal under STEPOutput — must be silently
        // skipped (no panic). NaN comparisons always evaluate false, so a
        // stale NaN min could never be displaced.
        let (id_a, data_a) = representation_within_constraint_node(
            "STEPOutput",
            0,
            f64::NAN,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_a, data_a);

        // (b) INFINITY tolerance literal under STEPOutput — silently skipped.
        let (id_b, data_b) = representation_within_constraint_node(
            "STEPOutput",
            1,
            f64::INFINITY,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_b, data_b);

        // (c) NEG_INFINITY tolerance literal under STEPOutput — silently
        // skipped.
        let (id_c, data_c) = representation_within_constraint_node(
            "STEPOutput",
            2,
            f64::NEG_INFINITY,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_c, data_c);

        // (d) DIMENSIONLESS Scalar literal under STEPOutput — silently
        // skipped by the LENGTH match-guard.
        let (id_d, data_d) = representation_within_constraint_node(
            "STEPOutput",
            3,
            1.0,
            DimensionVector::DIMENSIONLESS,
        );
        constraints.insert(id_d, data_d);

        // (e) Valid finite LENGTH RepresentationWithin under "OtherTemplate"
        // — silently skipped by the entity exact-match filter (even though
        // its tolerance is tighter than (f) below).
        let (id_e, data_e) = representation_within_constraint_node(
            "OtherTemplate",
            0,
            1e-6,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_e, data_e);

        // (g) Negative finite LENGTH literal under STEPOutput — silently
        // skipped by Gate 4c (>= 0.0). Without this gate, a negative bound
        // would survive extraction, then panic the combiner's debug-assert
        // in debug builds and win an `o.min(p)` race in release builds.
        let (id_g, data_g) =
            representation_within_constraint_node("STEPOutput", 5, -1e-3, DimensionVector::LENGTH);
        constraints.insert(id_g, data_g);

        // (h) Non-`UserFunctionCall` outer kind under STEPOutput — silently
        // skipped by Gate 2 (the `match &data.expr.kind { … _ => continue }`
        // arm). Pins that any non-UFC top-level (e.g. a top-level `Literal`)
        // never reaches the inner shape check.
        let (id_h, data_h) = constraint_node_with(
            "STEPOutput",
            6,
            CompiledExpr::literal(Value::Bool(true), Type::Bool),
        );
        constraints.insert(id_h, data_h);

        // (i) `UserFunctionCall("RepresentationWithin", [single_arg])` (arity
        // 1) under STEPOutput — silently skipped by Gate 2's arity check.
        // Pins that the arity-mismatch branch is reachable independently of
        // the outer-shape branch.
        let (id_i, data_i) = constraint_node_with(
            "STEPOutput",
            7,
            CompiledExpr::user_function_call(
                "RepresentationWithin".to_string(),
                vec![CompiledExpr::value_ref(
                    ValueCellId::new("subject", "self"),
                    Type::StructureRef("Structure".to_string()),
                )],
                Type::Bool,
            ),
        );
        constraints.insert(id_i, data_i);

        // (j) `RepresentationWithin(<ValueRef typed Real>, <length-literal>)`
        // under STEPOutput — silently skipped by Gate 3 (arg0 result_type
        // must be `StructureRef(_)`). Pins the type-tag gate independently
        // of the ValueRef-kind gate.
        let (id_j, data_j) = constraint_node_with(
            "STEPOutput",
            8,
            CompiledExpr::user_function_call(
                "RepresentationWithin".to_string(),
                vec![
                    CompiledExpr::value_ref(ValueCellId::new("subject", "self"), Type::Real),
                    CompiledExpr::literal(
                        Value::Scalar {
                            si_value: 0.5e-6,
                            dimension: DimensionVector::LENGTH,
                        },
                        Type::Scalar {
                            dimension: DimensionVector::LENGTH,
                        },
                    ),
                ],
                Type::Bool,
            ),
        );
        constraints.insert(id_j, data_j);

        // (f) The one valid finite LENGTH RepresentationWithin under
        // STEPOutput — survives all gates.
        let (id_f, data_f) =
            representation_within_constraint_node("STEPOutput", 4, 5e-6, DimensionVector::LENGTH);
        constraints.insert(id_f, data_f);

        assert_eq!(
            extract_output_tolerance_bound(&constraints, "STEPOutput"),
            Some(5e-6),
            "only the valid finite non-negative LENGTH constraint under \
             STEPOutput must survive — all other entries (NaN, ±Inf, \
             negative-finite, dimensionless, unrelated entity, non-UFC outer, \
             wrong-arity, non-StructureRef arg0) must be silently skipped"
        );
    }

    #[test]
    fn extract_output_tolerance_bound_returns_min_under_matching_entity() {
        let mut constraints: PersistentMap<ConstraintNodeId, ConstraintNodeData> =
            PersistentMap::default();

        // Two matching RepresentationWithin entries under "STEPOutput" — must
        // be folded via min so the tighter (1e-6) wins.
        let (id_a, data_a) =
            representation_within_constraint_node("STEPOutput", 0, 50e-6, DimensionVector::LENGTH);
        constraints.insert(id_a, data_a);
        let (id_b, data_b) =
            representation_within_constraint_node("STEPOutput", 1, 1e-6, DimensionVector::LENGTH);
        constraints.insert(id_b, data_b);

        // An unrelated constraint under a different entity — must be skipped
        // by the entity exact-match filter.
        let (id_c, data_c) = representation_within_constraint_node(
            "OtherEntity",
            0,
            0.5e-6,
            DimensionVector::LENGTH,
        );
        constraints.insert(id_c, data_c);

        assert_eq!(
            extract_output_tolerance_bound(&constraints, "STEPOutput"),
            Some(1e-6),
            "min-fold across the two matching entries must yield the tighter (1e-6); \
             OtherEntity's tighter 0.5e-6 must be filtered out by entity match"
        );
    }

    #[test]
    fn combine_returns_min_when_both_some() {
        assert_eq!(
            combine_demanded_tolerance(Some(50e-6), Some(1e-6)),
            Some(1e-6),
            "tighter purpose-bound (1e-6) wins over looser output-bound (50e-6)"
        );
        assert_eq!(
            combine_demanded_tolerance(Some(1e-6), Some(50e-6)),
            Some(1e-6),
            "tighter output-bound (1e-6) wins over looser purpose-bound (50e-6) — symmetric"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_nan_output_bound() {
        combine_demanded_tolerance(Some(f64::NAN), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_nan_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(f64::NAN));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_infinite_output_bound() {
        combine_demanded_tolerance(Some(f64::INFINITY), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_infinite_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(f64::INFINITY));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_negative_output_bound() {
        combine_demanded_tolerance(Some(-1.0e-3), Some(1e-6));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn combine_panics_in_debug_on_negative_purpose_bound() {
        combine_demanded_tolerance(Some(1e-6), Some(-1.0e-3));
    }

    #[test]
    fn combine_passes_through_lone_some_or_returns_none_when_both_none() {
        assert_eq!(
            combine_demanded_tolerance(Some(1e-6), None),
            Some(1e-6),
            "lone output-bound passes through when purpose-bound is None"
        );
        assert_eq!(
            combine_demanded_tolerance(None, Some(1e-6)),
            Some(1e-6),
            "lone purpose-bound passes through when output-bound is None"
        );
        assert_eq!(
            combine_demanded_tolerance(None, None),
            None,
            "both None must return None — no demand contributor exists"
        );
    }

    // ── step-1 (task 4199 γ): recognize_representation_within unit tests ───────
    //
    // These tests are RED until step-2 implements `recognize_representation_within`.

    /// Build the canonical `RepresentationWithin(<ValueRef typed
    /// StructureRef("Curved")>, <Scalar{1e-6, LENGTH}>)` expression used
    /// across multiple step-1 test cases.
    fn canonical_repr_within_expr() -> CompiledExpr {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        )
    }

    /// Canonical RepresentationWithin expression → Some((subject_vcid, "Curved", 1e-6)).
    ///
    /// This is the recognition-shape positive case: a well-formed
    /// `RepresentationWithin(ValueRef(subject.self):StructureRef("Curved"),
    /// Scalar{1e-6, LENGTH})` must yield the tuple
    /// `(ValueCellId("subject","self"), "Curved", 1e-6)`.
    #[test]
    fn recognize_repr_within_returns_some_for_canonical_shape() {
        let expr = canonical_repr_within_expr();
        let result = recognize_representation_within(&expr);
        assert!(
            result.is_some(),
            "canonical RepresentationWithin must be recognized as Some"
        );
        let (vcid, struct_name, bound) = result.unwrap();
        assert_eq!(
            vcid,
            ValueCellId::new("subject", "self"),
            "subject ValueCellId must match the ValueRef in arg0"
        );
        assert_eq!(
            struct_name, "Curved",
            "StructureRef inner name must be extracted from arg0.result_type"
        );
        assert!(
            (bound - 1e-6).abs() < 1e-15,
            "bound must match the Scalar si_value in arg1; got {bound}"
        );
    }

    /// Non-RepresentationWithin function name → None (silent-skip gate 2 name check).
    #[test]
    fn recognize_repr_within_returns_none_for_wrong_function_name() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationBetween".to_string(), // wrong name
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "wrong function name must return None (gate 2 name check)"
        );
    }

    /// Arity ≠ 2 (single arg) → None (silent-skip gate 2 arity check).
    #[test]
    fn recognize_repr_within_returns_none_for_wrong_arity() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg], // arity 1 — wrong
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "arity ≠ 2 must return None (gate 2 arity check)"
        );
    }

    /// Arg0 is a Literal (not a ValueRef) → None (gate 3 ValueRef check).
    #[test]
    fn recognize_repr_within_returns_none_for_non_value_ref_arg0() {
        // Use a Bool literal as arg0 — not a ValueRef.
        let literal_arg = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![literal_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-ValueRef arg0 must return None (gate 3)"
        );
    }

    /// Arg0 is a ValueRef but with non-StructureRef result_type (Real) → None (gate 3).
    #[test]
    fn recognize_repr_within_returns_none_for_non_structure_ref_result_type() {
        // ValueRef with result_type = Real, not StructureRef.
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::Real, // wrong result_type
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-StructureRef result_type on arg0 must return None (gate 3)"
        );
    }

    /// Arg1 has a DIMENSIONLESS dimension (not LENGTH) → None (gate 4a).
    #[test]
    fn recognize_repr_within_returns_none_for_non_length_dimension() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 1e-6,
                dimension: DimensionVector::DIMENSIONLESS, // wrong dimension
            },
            Type::Scalar {
                dimension: DimensionVector::DIMENSIONLESS,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-LENGTH dimension in arg1 must return None (gate 4a)"
        );
    }

    /// Arg1 is a NaN tolerance literal → None (gate 4b).
    #[test]
    fn recognize_repr_within_returns_none_for_nan_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: f64::NAN,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "NaN tolerance must return None (gate 4b)"
        );
    }

    /// Arg1 is +Infinity tolerance literal → None (gate 4b).
    #[test]
    fn recognize_repr_within_returns_none_for_infinite_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: f64::INFINITY,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "+Infinity tolerance must return None (gate 4b)"
        );
    }

    /// Arg1 is a negative (−1 mm) finite LENGTH tolerance literal → None (gate 4c).
    #[test]
    fn recognize_repr_within_returns_none_for_negative_tolerance() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: -1e-3, // negative finite
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "negative finite tolerance must return None (gate 4c)"
        );
    }

    /// Non-UserFunctionCall outer kind (a Bool literal) → None.
    ///
    /// Pins that the matcher short-circuits on the outer kind, never reaching
    /// the inner gates, so callers can pass arbitrary constraint expressions
    /// through `recognize_representation_within` safely.
    #[test]
    fn recognize_repr_within_returns_none_for_non_ufc_outer_kind() {
        let expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        assert_eq!(
            recognize_representation_within(&expr),
            None,
            "non-UserFunctionCall outer kind must return None"
        );
    }

    /// Zero bound (0.0) with LENGTH dimension → Some — zero is a valid non-negative
    /// finite bound (the C4 PLANAR_FLOOR comparator handles it, not the gate).
    ///
    /// Mirrors the `>= 0.0` (not `> 0.0`) posture of `is_valid_tolerance_si`.
    #[test]
    fn recognize_repr_within_returns_some_for_zero_bound() {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Curved".to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        );
        let expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );
        let result = recognize_representation_within(&expr);
        assert!(
            result.is_some(),
            "zero bound (0.0 m) is a valid non-negative finite LENGTH bound and \
             must be recognized — the PLANAR_FLOOR comparator handles zero bounds, \
             not the gate"
        );
        let (_, _, bound) = result.unwrap();
        assert_eq!(bound, 0.0, "zero bound must be preserved exactly");
    }
}
