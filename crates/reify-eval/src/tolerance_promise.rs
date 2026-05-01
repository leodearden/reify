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

use reify_types::{
    DeterminacyState, Diagnostic, DiagnosticCode, DimensionVector, PersistentMap, Value,
    ValueCellId,
};

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
    // four runtime gates, one per line of the function body. Contrast with
    // `tolerance_combine::extract_output_tolerance_bound`, which performs a
    // six-gate scan over a constraint vector — here we have a direct
    // composite-key map lookup, so entity match and member match collapse
    // into a single gate.
    //   Gate 1 (composite-key lookup) `values.get(&cell_id)?` discriminates
    //                                 simultaneously on entity name and on
    //                                 member name (`ValueCellId` is keyed on
    //                                 both), so `OtherInput.tolerance` (test
    //                                 case (g)) and `STEPInput.source` (test
    //                                 case (h)) are both rejected by this
    //                                 single line. A None result here short-
    //                                 circuits with `?`.
    //   Gate 2 (Value::Scalar)        the `match value` arm skips Bool / Int
    //                                 / Undef / String etc. Value variants
    //                                 stored at the canonical cell (covers
    //                                 test case (f)).
    //   Gate 3 (LENGTH dimension)     `dimension != DimensionVector::LENGTH`
    //                                 skips DIMENSIONLESS / Money / Force /
    //                                 other non-LENGTH Scalar literals
    //                                 (covers test case (e)).
    //   Gate 4 (finite & non-negative) `!si_value.is_finite() || si_value < 0.0`
    //                                 skips NaN / ±Inf (covers (a)/(b)/(c))
    //                                 and negative finite (covers (d)). NaN
    //                                 must be rejected because NaN
    //                                 comparisons always evaluate false; the
    //                                 `>= 0.0` half mirrors
    //                                 `is_promise_insufficient`'s debug-
    //                                 assert `is_finite() && >= 0.0`
    //                                 invariant.
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

/// Test whether an imported-geometry tolerance promise is insufficient to
/// satisfy a downstream demand.
///
/// Returns `true` iff `demanded < promise` (strict). The strict comparison
/// — not `<=` — is the canonical design decision: a demand exactly equal to
/// the promise IS satisfiable. The promise is an upper bound on the
/// as-imported representation error, and a demand at the same level can be
/// satisfied by the as-imported realization. Mirrors the partial-order
/// "tighter satisfies looser" rule established by `tolerance_bucket`'s
/// `cached_tol <= requested_tol` lookup — contrapositive `cached_tol >
/// requested_tol` (cached strictly looser) cannot satisfy. Same logic here:
/// `promise > demand` (i.e. `demand < promise`) is insufficient; equal is
/// not insufficient.
///
/// # Truth table
///
/// | `demanded` | `promise` | result | reason                                |
/// |------------|-----------|--------|---------------------------------------|
/// | `1µm`      | `50µm`    | `true` | demand strictly tighter — insufficient |
/// | `50µm`     | `1µm`     | `false`| demand looser — promise covers it      |
/// | `1µm`      | `1µm`     | `false`| equal — strict `<`, not `<=`           |
/// | `0.0`      | `1µm`     | `true` | zero is the tightest possible demand   |
///
/// # Panics
///
/// In debug builds: panics if either argument is NaN, ±Inf, or negative.
/// The canonical message format `"TolerancePromise: tolerance must be finite
/// and non-negative, got {tol}"` matches
/// [`crate::tolerance_combine::combine_demanded_tolerance`]'s debug-assert
/// so authoring errors surface with one voice across the four `tolerance_*`
/// modules. Upstream extractors
/// ([`extract_input_tolerance_promise`] and
/// [`crate::tolerance_combine::extract_output_tolerance_bound`]) silently
/// skip these malformed shapes so the comparator's invariant holds at every
/// call site in practice.
pub fn is_promise_insufficient(demanded: f64, promise: f64) -> bool {
    debug_assert!(
        demanded.is_finite() && demanded >= 0.0,
        "TolerancePromise: tolerance must be finite and non-negative, got {demanded}"
    );
    debug_assert!(
        promise.is_finite() && promise >= 0.0,
        "TolerancePromise: tolerance must be finite and non-negative, got {promise}"
    );
    demanded < promise
}

/// Build the `Severity::Warning` diagnostic emitted when a downstream
/// demand is strictly tighter than the imported-geometry tolerance promise
/// declared on an `Input` occurrence.
///
/// The diagnostic carries
/// [`DiagnosticCode::ImportedTolerancePromiseInsufficient`] for filter-by-code
/// downstream consumers (LSP / IDE / batch pipelines) and a canonical
/// human-readable message naming the input template, the demanded
/// tolerance, and the promised tolerance.
///
/// # Severity rationale
///
/// PRD `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design
/// decisions" → "Imported geometry promise"): "When a downstream demand
/// is tighter than the import promise, emit a diagnostic (warn, not error)
/// and proceed with the as-imported realization. Users opt into explicit
/// re-meshing/healing through a stdlib helper rather than the runtime
/// silently doing it." Warning matches the established convention for
/// similar advisory codes (`FieldOutOfBounds`, `TraitUserAsserted`,
/// `TopologyTagStale`) where downstream tooling can choose to surface as
/// harder failures via filter-by-code at the consumer side. The PRD
/// explicitly rejects silent re-meshing — the user must opt in via a
/// stdlib helper.
///
/// # Arguments
///
/// - `input_template_name` — the `Input` occurrence template name (e.g.
///   `"STEPInput"`); appears verbatim in the diagnostic message so authors
///   can locate the import site.
/// - `demanded` — the demanded tolerance in SI metres (the tighter side).
/// - `promise` — the imported-geometry tolerance promise in SI metres
///   (the looser side; `promise > demanded`).
pub fn imported_tolerance_promise_diagnostic(
    input_template_name: &str,
    demanded: f64,
    promise: f64,
) -> Diagnostic {
    let message = format!(
        "imported geometry '{input_template_name}' tolerance promise {promise}m is \
         insufficient for downstream demand {demanded}m; proceeding with \
         as-imported realization"
    );
    Diagnostic::warning(message).with_code(DiagnosticCode::ImportedTolerancePromiseInsufficient)
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

    /// Silent-skip audit: every malformed entry below must be silently
    /// rejected so the one valid entry survives. Mirrors
    /// `extract_output_tolerance_bound_skips_non_finite_non_length_and_unrelated_entity`
    /// in `tolerance_combine.rs`. Pins each gate independently so a future
    /// refactor that drops a gate fails this test on the specific case it
    /// regressed.
    #[test]
    fn extract_input_tolerance_promise_silent_skip_audit() {
        // Local helper: insert a Scalar value under a given (entity, member).
        // Free function (not a closure) so the caller can re-borrow `values`
        // immutably between successive insertions and observations.
        fn insert_scalar(
            values: &mut PersistentMap<ValueCellId, (Value, DeterminacyState)>,
            entity: &str,
            member: &str,
            si_value: f64,
            dim: DimensionVector,
        ) {
            values.insert(
                ValueCellId::new(entity, member),
                (
                    Value::Scalar {
                        si_value,
                        dimension: dim,
                    },
                    DeterminacyState::Determined,
                ),
            );
        }

        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        // (g) Entry under different entity (`OtherInput`, "tolerance") with a
        // tighter valid value — must be silently skipped by Gate 1 (cell
        // lookup keyed by entity name). Inserted FIRST so that if the
        // lookup ever broadened to scan-all, this tighter value would
        // incorrectly win and the assertion would fail.
        insert_scalar(
            &mut values,
            "OtherInput",
            "tolerance",
            1e-9,
            DimensionVector::LENGTH,
        );

        // (h) Entry under same entity but different member
        // (`STEPInput`, "source") with a String value — must be silently
        // skipped by Gate 1 (member name mismatch). Use a String value so
        // the test would also catch a future bug where the extractor
        // accidentally accepted non-Scalar values at a different member.
        values.insert(
            ValueCellId::new("STEPInput", "source"),
            (
                Value::String("file.step".to_string()),
                DeterminacyState::Determined,
            ),
        );

        // The cases (a)..(f) below each individually exercise Gate 4a /
        // 4b / 3 / 2 in sequence under the same key (`STEPInput`, "tolerance");
        // since PersistentMap.insert overrides on a duplicate key, we
        // observe one at a time by clearing the assertion after each.

        // Sub-block (a) NaN.
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            f64::NAN,
            DimensionVector::LENGTH,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(a) NaN tolerance literal must be silently skipped by is_finite()"
        );

        // Sub-block (b) +Inf.
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            f64::INFINITY,
            DimensionVector::LENGTH,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(b) +Inf tolerance literal must be silently skipped by is_finite()"
        );

        // Sub-block (c) -Inf.
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            f64::NEG_INFINITY,
            DimensionVector::LENGTH,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(c) -Inf tolerance literal must be silently skipped by is_finite()"
        );

        // Sub-block (d) negative finite.
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            -1e-3,
            DimensionVector::LENGTH,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(d) negative finite tolerance literal must be silently skipped by >= 0.0"
        );

        // Sub-block (e) wrong dimension (DIMENSIONLESS).
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            1.0,
            DimensionVector::DIMENSIONLESS,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(e) DIMENSIONLESS Scalar literal must be silently skipped by LENGTH gate"
        );

        // Sub-block (f) Bool variant directly under the canonical key.
        values.insert(
            ValueCellId::new("STEPInput", "tolerance"),
            (Value::Bool(true), DeterminacyState::Determined),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(f) non-Scalar (Bool) Value must be silently skipped by Value::Scalar gate"
        );

        // Finally, install the one valid entry — must survive every gate.
        insert_scalar(
            &mut values,
            "STEPInput",
            "tolerance",
            50e-6,
            DimensionVector::LENGTH,
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(50e-6),
            "valid (LENGTH, finite, non-negative) entry under (STEPInput, \
             \"tolerance\") must survive every gate; the unrelated (g) and \
             (h) entries (OtherInput.tolerance and STEPInput.source) must be \
             ignored"
        );

        // Cross-check: (g)'s tighter value under OtherInput must extract too
        // when queried by its own template name — proves Gate 1's entity
        // discrimination is bidirectional (not just "STEPInput-only").
        assert_eq!(
            extract_input_tolerance_promise(&values, "OtherInput"),
            Some(1e-9),
            "Gate 1's entity discrimination must be bidirectional — querying \
             by OtherInput's name must return its valid value, not be \
             accidentally tied to STEPInput's"
        );

        // Cross-check: a query for an entity that has no tolerance cell
        // must return None (covers Gate 1's None branch when no entry
        // exists at all).
        assert_eq!(
            extract_input_tolerance_promise(&values, "NonExistentInput"),
            None,
            "Gate 1 must return None when no (entity, \"tolerance\") cell exists"
        );
    }

    /// Pinned by the strict-`<` design decision (see plan.json
    /// design_decisions): "A demand exactly equal to the promise IS
    /// satisfiable: the promise is an upper bound on the as-imported
    /// representation error, and a demand at that same level can be
    /// satisfied by the as-imported realization." Mirrors the partial-order
    /// "tighter satisfies looser" rule established by `tolerance_bucket`'s
    /// `cached_tol <= requested_tol` lookup — contrapositive `cached_tol >
    /// requested_tol` (cached strictly looser) cannot satisfy. Same logic
    /// here: `promise > demand` (i.e. `demand < promise`) is insufficient;
    /// equal is not insufficient.
    #[test]
    fn is_promise_insufficient_returns_true_iff_demanded_strictly_less_than_promise() {
        // (a) demand tighter than promise — insufficient.
        assert!(
            is_promise_insufficient(1e-6, 50e-6),
            "(a) demand 1µm strictly tighter than promise 50µm — insufficient"
        );

        // (b) demand looser than promise — sufficient (promise's upper-bound
        //     guarantee covers the looser demand).
        assert!(
            !is_promise_insufficient(50e-6, 1e-6),
            "(b) demand 50µm looser than promise 1µm — sufficient"
        );

        // (c) demand equal to promise — sufficient (strict `<`, not `<=`).
        //     This is the canonical design-decision pin: equal-tolerance is
        //     NOT insufficient.
        assert!(
            !is_promise_insufficient(1e-6, 1e-6),
            "(c) demand == promise (1µm == 1µm) — strict `<` rules this \
             sufficient; flipping to `<=` would regress this assertion"
        );

        // (d) demand zero with positive promise — zero is the tightest
        //     possible demand, strictly less than any positive promise, so
        //     insufficient.
        assert!(
            is_promise_insufficient(0.0, 1e-6),
            "(d) demand 0.0 strictly less than promise 1µm — insufficient"
        );

        // Symmetric edge case: both zero — strict `<` is false, sufficient.
        assert!(
            !is_promise_insufficient(0.0, 0.0),
            "edge: demand == promise == 0.0 — strict `<` rules this sufficient"
        );
    }

    // Debug-build NaN/±Inf/negative panic tests. Mirror the
    // `combine_panics_in_debug_on_*` precedent in
    // `tolerance_combine.rs:466-505`. Canonical panic message format
    // ("tolerance must be finite and non-negative") is shared across the
    // four tolerance_* modules so authoring errors surface with one voice.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_nan_demanded() {
        is_promise_insufficient(f64::NAN, 1e-6);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_nan_promise() {
        is_promise_insufficient(1e-6, f64::NAN);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_positive_infinity_demanded() {
        is_promise_insufficient(f64::INFINITY, 1e-6);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_positive_infinity_promise() {
        is_promise_insufficient(1e-6, f64::INFINITY);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_negative_infinity_demanded() {
        is_promise_insufficient(f64::NEG_INFINITY, 1e-6);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_negative_infinity_promise() {
        is_promise_insufficient(1e-6, f64::NEG_INFINITY);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_negative_demanded() {
        is_promise_insufficient(-1.0e-3, 1e-6);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "tolerance must be finite and non-negative")]
    fn is_promise_insufficient_panics_in_debug_on_negative_promise() {
        is_promise_insufficient(1e-6, -1.0e-3);
    }

    /// Pinned by the diagnostic-emission contract: when a downstream demand
    /// is tighter than an imported-geometry tolerance promise, the runtime
    /// emits a `Severity::Warning` diagnostic carrying
    /// `DiagnosticCode::ImportedTolerancePromiseInsufficient` and a message
    /// naming the input template. PRD: "emit a diagnostic (warn, not error)
    /// and proceed with the as-imported realization."
    ///
    /// Asserts pin functional contract only — severity, code, and the
    /// template name in the message — not the surrounding English prose.
    /// Downstream consumers filter by `DiagnosticCode`, not by substrings of
    /// the rendered message, so locking specific words ("insufficient",
    /// "proceeding with", etc.) creates wording-churn maintenance without
    /// protecting any real consumer contract.
    #[test]
    fn imported_tolerance_promise_diagnostic_builds_warning_with_code_and_template_name() {
        use reify_types::{DiagnosticCode, Severity};

        let diag = imported_tolerance_promise_diagnostic("STEPInput", 1e-6, 50e-6);

        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic severity must be Warning (PRD: warn, not error — \
             runtime proceeds with as-imported realization)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::ImportedTolerancePromiseInsufficient),
            "diagnostic code must round-trip the typed variant for downstream \
             filter-by-code consumers"
        );
        assert!(
            diag.message.contains("STEPInput"),
            "message must name the input template so authors can locate the \
             import site (got: {:?})",
            diag.message
        );
    }
}
