//! Tolerance-scope extraction from purpose declarations.
//!
//! Activates the dormant tolerance-scope infrastructure described in
//! `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! section, "Tolerance lives at the purpose"): walks each active purpose's
//! subject graph, extracts every `RepresentationWithin(subject, tolerance)`
//! constraint, propagates the tolerance onto every reachable entity, and
//! combines contributions across purposes via `min` (tighter satisfies
//! looser — same partial-order semantics as the cache-side
//! `ToleranceBucket`).
//!
//! # MVP scope clip
//!
//! Today this module only recognises `RepresentationWithin(<bare-param>, <length-literal>)`
//! where `<bare-param>` is the purpose's `StructureRef`-typed parameter. Member-access
//! subjects (`RepresentationWithin(subject.head, tol)`) are deferred to a follow-up —
//! the PRD's "tighter entity-level overrides" semantics is fully achievable in the MVP
//! via *multiple active purposes with overlapping subjects* (purpose A bound to
//! `bracket`, purpose B bound to `bracket.head`, with B tighter).

#[cfg(test)]
mod tests {
    use super::*;
    use reify_test_support::builders::CompiledPurposeBuilder;
    use reify_types::{
        CompiledExpr, DimensionVector, Type, Value, ValueCellId,
    };

    #[test]
    fn extract_tolerance_bindings_returns_single_binding_for_one_representation_within() {
        // Build: RepresentationWithin(ValueRef("subject", "self") : StructureRef("Bracket"),
        //                              Literal(Scalar { si_value: 1e-6, dim: LENGTH }))
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef("Bracket".to_string()),
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
        let constraint_expr = CompiledExpr::user_function_call(
            "RepresentationWithin".to_string(),
            vec![subject_arg, tol_arg],
            Type::Bool,
        );

        let purpose = CompiledPurposeBuilder::new("manufacturing")
            .param("subject", "Structure")
            .constraint("subject", 0, None, constraint_expr)
            .build();

        let bindings = extract_tolerance_bindings(&purpose, "MyDesign");

        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].subject_entity, "MyDesign");
        assert_eq!(bindings[0].si_tolerance, 1e-6);
    }
}
