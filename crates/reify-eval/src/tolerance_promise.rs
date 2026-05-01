//! Imported-geometry tolerance-promise extraction and diagnostic.
//!
//! See `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! → "Imported geometry promise") and arch §10.4 / §14.5 for the contract:
//! an `Input` occurrence template carries a `tolerance` parameter that the
//! runtime treats as both an *assertion* (used for downstream budget
//! allocation) and a *promise* (cannot be verified for arbitrary STEP/STL
//! input). When a downstream demand is tighter than the import promise the
//! runtime emits a `Severity::Warning` diagnostic and proceeds with the
//! as-imported realization — see [`imported_tolerance_promise_diagnostic`].
//!
//! # Recognition shape
//!
//! Unlike output occurrences (whose tolerance is encoded as a
//! `RepresentationWithin(subject, lit)` *constraint* on the template — see
//! [`crate::tolerance_combine::extract_output_tolerance_bound`]), an Input
//! occurrence's tolerance is encoded as a *parameter*: the template carries a
//! `param tolerance : Length = X` declaration and the post-`eval()`
//! `Snapshot.values` map contains an entry keyed by
//! `ValueCellId(input_template_name, "tolerance")` whose value is
//! `Value::Scalar { dimension == LENGTH, si_value }`. Reading the value cell
//! is a direct match for the PRD's "parameter of Input" wording — no new
//! syntax is required and the user supplies the promise via a normal
//! `param tolerance : Length = 50um` declaration.

use reify_types::{DeterminacyState, DimensionVector, PersistentMap, Value, ValueCellId};

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{DeterminacyState, DimensionVector, PersistentMap, Value, ValueCellId};

    /// Pinned by the recognition-shape contract: the post-`eval()`
    /// `Snapshot.values` map carries an entry at
    /// `ValueCellId(input_template_name, "tolerance")` whose
    /// `Value::Scalar { dimension == LENGTH, si_value }` is the imported
    /// geometry's tolerance promise. The extractor returns `Some(si_value)`
    /// when the entry is present and well-formed.
    #[test]
    fn extract_input_tolerance_promise_returns_si_length_when_value_present() {
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        values.insert(
            ValueCellId::new("STEPInput", "tolerance"),
            (
                Value::Scalar {
                    si_value: 50e-6,
                    dimension: DimensionVector::LENGTH,
                },
                DeterminacyState::Determined,
            ),
        );

        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(50e-6),
            "well-formed (LENGTH, finite, non-negative) promise must be \
             extracted as Some(si_value) for the matching input_template_name"
        );
    }
}
