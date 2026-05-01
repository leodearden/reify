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

/// Extract the imported-geometry tolerance promise carried by an `Input`
/// occurrence template's `param tolerance : Length = …` declaration.
///
/// Looks up the cell at `ValueCellId(input_template_name, "tolerance")` in
/// the post-`eval()` `Snapshot.values` map; returns `Some(si_value)` when the
/// entry is present and well-formed (`Value::Scalar { dimension == LENGTH,
/// si_value }` with `si_value.is_finite() && si_value >= 0.0`). Returns
/// `None` for every malformed shape — the silent-skip posture mirrors
/// [`crate::tolerance_combine::extract_output_tolerance_bound`] and
/// [`crate::tolerance_scope::extract_tolerance_bindings`].
///
/// # Recognition gates
///
/// 1. **Cell lookup:** `ValueCellId::new(input_template_name, "tolerance")`
///    must exist in `values`.
/// 2. **Outer Value shape:** the looked-up `Value` must be `Value::Scalar`.
/// 3. **Dimension:** `dimension == DimensionVector::LENGTH` (Money / Force /
///    DIMENSIONLESS scalars are silently skipped).
/// 4. **Finite & non-negative:** `si_value.is_finite() && si_value >= 0.0`.
///    NaN / ±Inf / negative finite literals are silently skipped — without
///    these gates a NaN promise would stick (NaN comparisons evaluate false)
///    and a negative promise would silently win an `o.min(p)` race in
///    release while panicking debug builds via `is_promise_insufficient`'s
///    debug_assert. Symmetric with the corresponding gates in
///    `extract_output_tolerance_bound`.
///
/// # Silent-skip posture
///
/// A malformed Input template simply contributes no promise — the diagnostic
/// doesn't fire, but the runtime doesn't crash either. This matches the
/// "activate dormant infrastructure" posture from PRD `docs/prds/v0_2/
/// per-purpose-tolerance.md` (extraction is policy-neutral).
pub fn extract_input_tolerance_promise(
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    input_template_name: &str,
) -> Option<f64> {
    // Silent-skip audit (locked by `extract_input_tolerance_promise_silent_skip_audit`):
    //   Gate 1 (cell lookup)         skips entries under different entity
    //                                or different member name (returns None
    //                                when the canonical (input, "tolerance")
    //                                cell is absent)
    //   Gate 2 (Value::Scalar)       skips Bool / Int / Undef / String etc.
    //   Gate 3 (LENGTH dimension)    skips DIMENSIONLESS / Money / Force /
    //                                other non-LENGTH Scalar literals
    //   Gate 4a (is_finite())        skips NaN / ±Inf tolerance literals
    //   Gate 4b (>= 0.0)             skips negative finite tolerance
    //                                literals (contract symmetry with
    //                                `is_promise_insufficient`'s debug-assert
    //                                `is_finite() && >= 0.0`)
    // Every non-match path returns None (or falls through to the trailing
    // None) — no `panic!`, `expect`, or `unwrap` is reachable, so a malformed
    // values map never crashes the engine.
    let cell_id = ValueCellId::new(input_template_name, "tolerance");
    let (value, _det) = values.get(&cell_id)?;
    let (si_value, dimension) = match value {
        Value::Scalar {
            si_value,
            dimension,
        } => (*si_value, *dimension),
        _ => return None,
    };
    if dimension != DimensionVector::LENGTH {
        return None;
    }
    if !si_value.is_finite() || si_value < 0.0 {
        return None;
    }
    Some(si_value)
}

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
