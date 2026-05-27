//! Output-occurrence × active-purpose tolerance combiner.
//!
//! See `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! → "Tolerance lives at the purpose") for the contract that drives this
//! module.
//!
//! # Recognition-shape twin
//!
//! The output-bound extractor below duplicates the `RepresentationWithin`
//! shape-recognition gates from
//! [`crate::tolerance_scope::extract_tolerance_bindings`] (top-level
//! `UserFunctionCall("RepresentationWithin", [<ValueRef typed
//! StructureRef>, <Literal Scalar LENGTH finite>])`). The duplication is a
//! deliberate scope clip for task 2650 — a future shared helper would prevent
//! drift between the two recognition sites at the cost of touching
//! `tolerance_scope.rs`'s public surface (TODO).

use crate::graph::ConstraintNodeData;
use reify_core::{ConstraintNodeId, DimensionVector, Type};
use reify_ir::{CompiledExprKind, PersistentMap, Value};

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
    //   Gate 2 (UserFunctionCall +    skips non-RepresentationWithin or
    //           name + arity)         wrong-arity outer shapes
    //   Gate 3 (ValueRef +             skips non-ValueRef subjects or non-
    //           StructureRef)         StructureRef result types
    //   Gate 4a (LENGTH dimension)    skips non-LENGTH Scalar literals
    //   Gate 4b (is_finite())         skips NaN / ±Inf tolerance literals
    //   Gate 4c (>= 0.0)              skips negative finite tolerance
    //                                 literals (contract symmetry with
    //                                 `combine_demanded_tolerance`'s
    //                                 debug-assert `is_finite() && >= 0.0`)
    // Every non-match path uses `continue` — no `panic!`, `expect`, or
    // `unwrap` is reachable, so a malformed graph never crashes the engine.
    let mut tightest: Option<f64> = None;
    for (id, data) in constraints.iter() {
        // Gate 1: entity exact-match filter.
        if id.entity != output_template_name {
            continue;
        }

        // Gate 2: top-level UserFunctionCall("RepresentationWithin", [arg0, arg1]).
        let (function_name, args) = match &data.expr.kind {
            CompiledExprKind::UserFunctionCall {
                function_name,
                args,
            } => (function_name, args),
            _ => continue,
        };
        if function_name != "RepresentationWithin" {
            continue;
        }
        if args.len() != 2 {
            continue;
        }

        // Gate 3: arg0 must be a ValueRef whose result_type is StructureRef(_).
        // Note: the purpose-param-membership check from
        // `tolerance_scope::extract_tolerance_bindings` is dropped here —
        // output occurrences have a fixed `param subject : Structure` binding
        // pattern at the template level, so the StructureRef type-tag gate
        // alone is sufficient to identify the subject argument.
        let subject_arg = &args[0];
        if !matches!(subject_arg.kind, CompiledExprKind::ValueRef(_)) {
            continue;
        }
        if !matches!(subject_arg.result_type, Type::StructureRef(_)) {
            continue;
        }

        // Gate 4: arg1 must be a Literal(Value::Scalar { dimension == LENGTH, .. })
        // with finite, non-negative si_value. NaN/±Inf would propagate into
        // the bound and stick (NaN comparisons always evaluate false).
        // Negative finite values are also silently skipped here so the
        // extractor stays in lockstep with `combine_demanded_tolerance`'s
        // debug-assert `is_finite() && >= 0.0` invariant — without this gate,
        // a negative bound would survive extraction, then crash the engine in
        // debug builds and win an `o.min(p)` race in release builds.
        let tol_arg = &args[1];
        let si_value = match &tol_arg.kind {
            CompiledExprKind::Literal(Value::Scalar {
                si_value,
                dimension,
            }) if *dimension == DimensionVector::LENGTH => *si_value,
            _ => continue,
        };
        if !crate::tolerance_gate::is_valid_tolerance_si(si_value) {
            continue;
        }

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
}
