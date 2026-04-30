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
/// Returns the tighter (smaller) of the two when both Some — only the
/// both-Some branch is wired in this scaffold; the other arms will be
/// completed in step-4.
pub fn combine_demanded_tolerance(
    output_bound: Option<f64>,
    purpose_bound: Option<f64>,
) -> Option<f64> {
    match (output_bound, purpose_bound) {
        (Some(o), Some(p)) => Some(o.min(p)),
        _ => unimplemented!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
