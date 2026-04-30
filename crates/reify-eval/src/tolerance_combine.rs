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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::ConstraintNodeData;
    use reify_types::{
        CompiledExpr, ConstraintNodeId, ContentHash, DimensionVector, PersistentMap, Type, Value,
        ValueCellId,
    };

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
            Value::Scalar { si_value, dimension },
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
