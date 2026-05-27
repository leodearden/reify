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

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, ValueCellId};
use reify_ir::{DeterminacyState, PersistentMap, Value};

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
///
/// # Zero-promise interpretation
///
/// `si_value == 0.0` is **accepted** — it is the lower boundary of Gate 4's
/// `is_finite() && >= 0.0` check, not a rejected value. This mirrors the
/// precedent set by [`crate::tolerance_scope::extract_tolerance_bindings`]'s
/// `accepts_zero_tolerance_literal` test (which explicitly pins `0.0` as a
/// valid tolerance literal meaning "exact representation") and
/// [`crate::tolerance_combine::extract_output_tolerance_bound`]'s identical
/// `is_finite() && >= 0.0` gate. All three extractors were built in lockstep
/// and must remain symmetric.
///
/// **Semantic reading:** a zero promise claims "imported geometry has zero
/// deviation from the ideal" — a coherent, if extremely strong, assertion.
///
/// **Footgun:** combined with [`is_promise_insufficient`]'s strict-`<` rule,
/// a zero promise vacuously satisfies *every* non-negative demand (since
/// `demanded < 0.0` is false for all `demanded >= 0.0`). Consequently, a
/// `param tolerance : Length = 0m` placeholder default silently disables the
/// [`DiagnosticCode::ImportedTolerancePromiseInsufficient`] warning: no demand
/// will ever be flagged as tighter than a promise of `0.0`.
///
/// **Correct opt-out:** authors who do *not* want to make a tolerance promise
/// should **omit the `tolerance` parameter entirely** (so the cell-lookup at
/// Gate 1 finds no entry and returns `None` — the same path as a missing
/// tolerance binding) rather than writing `param tolerance : Length = 0m` as
/// a placeholder default.
///
/// **Resolution (task 2833):** option-(b continuation) selected. The gate stays
/// at `is_finite() && >= 0.0` to preserve cross-extractor symmetry with
/// `tolerance_scope::extract_tolerance_bindings` and
/// `tolerance_combine::extract_output_tolerance_bound`. The footgun is surfaced
/// at runtime via the new [`DiagnosticCode::InputTolerancePromiseIsZero`] lint
/// emitted by [`crate::engine_tolerance::Engine::check_imported_tolerance_promise`]
/// when `promise == 0.0 && demanded > 0.0` — see
/// [`input_tolerance_promise_is_zero_diagnostic`] for the builder and
/// `tests/tolerance_import_promise.rs::engine_check_imported_tolerance_promise_emits_zero_promise_lint_when_promise_zero_and_demand_positive`
/// for the integration pin. The two characterization tests added by task 2793
/// (`extract_input_tolerance_promise_accepts_zero_promise` and
/// `is_promise_insufficient_returns_false_when_promise_is_zero_for_any_non_negative_demand`)
/// lock the lower-level extractor + comparator behavior unchanged — they remain
/// green under this resolution because the gate and the strict-`<` rule are both
/// preserved.
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
    //                                 invariant. Signed-zero note: `-0.0` is
    //                                 NOT rejected by this gate (since
    //                                 `-0.0 < 0.0` is false in IEEE-754) and
    //                                 passes through as an accepted value.
    //                                 Downstream, `-0.0 == 0.0` (IEEE-754),
    //                                 so `Engine::check_imported_tolerance_
    //                                 promise`'s `promise == 0.0` guard fires
    //                                 identically for `+0.0` and `-0.0` —
    //                                 behavior is well-defined and benign. See
    //                                 the signed-zero note in the dispatch
    //                                 comment in `engine_tolerance.rs`.
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
    if !crate::tolerance_gate::is_valid_tolerance_si(si_value) {
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
/// | `demanded` | `promise` | result  | reason                                              |
/// |------------|-----------|---------|-----------------------------------------------------|
/// | `1µm`      | `50µm`    | `true`  | demand strictly tighter — insufficient               |
/// | `50µm`     | `1µm`     | `false` | demand looser — promise covers it                    |
/// | `1µm`      | `1µm`     | `false` | equal — strict `<`, not `<=`                         |
/// | `0.0`      | `1µm`     | `true`  | zero is the tightest possible demand                 |
/// | `1µm`      | `0.0`     | `false` | promise=0 vacuously satisfies every non-neg demand   |
///
/// # Zero-promise edge case
///
/// When `promise == 0.0`, the strict-`<` rule evaluates `demanded < 0.0`,
/// which is false for every `demanded >= 0.0`. A zero promise is therefore the
/// **loosest satisfiable claim** under this comparator — it vacuously satisfies
/// every non-negative demand.
///
/// See [`extract_input_tolerance_promise`]'s `# Zero-promise interpretation`
/// subsection for the placeholder-default footgun this enables (`param
/// tolerance : Length = 0m` silently disables the
/// [`DiagnosticCode::ImportedTolerancePromiseInsufficient`] warning) and the
/// recommended opt-out (omit the `tolerance` parameter entirely rather than
/// defaulting it to `0m`).
///
/// Task 2833 surfaces this footgun at the engine query layer via
/// [`DiagnosticCode::InputTolerancePromiseIsZero`] (emitted when
/// `promise == 0.0 && demanded > 0.0`), so the comparator's vacuous-satisfaction
/// behavior is preserved as a primitive while the engine catches the
/// placeholder-default footgun at the user-facing query.
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

/// Build the `Severity::Warning` diagnostic emitted when the imported-geometry
/// tolerance promise on an `Input` occurrence template is exactly `0.0` AND
/// the downstream demand is strictly positive (`demanded > 0.0`).
///
/// This surfaces the placeholder-default footgun where `param tolerance : Length = 0m`
/// silently disables the [`DiagnosticCode::ImportedTolerancePromiseInsufficient`]
/// warning: when `promise == 0.0`, the strict-`<` rule in
/// [`is_promise_insufficient`] evaluates `demanded < 0.0`, which is false for
/// every `demanded >= 0.0`, so the insufficient branch never fires.
///
/// # Resolution (task 2833)
///
/// Option-(b continuation) was selected: the extractor gate stays at
/// `is_finite() && >= 0.0` to preserve cross-extractor symmetry, and the
/// footgun is surfaced at runtime via this diagnostic emitted at the engine
/// query layer. See [`DiagnosticCode::InputTolerancePromiseIsZero`] for the
/// full design rationale.
///
/// # Canonical message form
///
/// `"imported geometry '<input_template>' carries a zero tolerance promise \
/// (`tolerance = 0m`) but downstream demand is <demanded_str>; …"`.
///
/// The recommended opt-out is to **omit the `tolerance` parameter entirely**
/// rather than writing `param tolerance : Length = 0m` as a placeholder default.
///
/// # Arguments
///
/// - `input_template_name` — the `Input` occurrence template name (e.g.
///   `"STEPInput"`); appears verbatim in the message so authors can locate the
///   import site.
/// - `demanded` — the demanded tolerance in SI metres; rendered with µm/mm/m
///   prefixes by magnitude via `tolerance_format::format_tolerance`.
pub fn input_tolerance_promise_is_zero_diagnostic(
    input_template_name: &str,
    demanded: f64,
) -> Diagnostic {
    let demanded_str = crate::tolerance_format::format_tolerance(demanded);
    let message = format!(
        "imported geometry '{input_template_name}' carries a zero tolerance promise \
         (`tolerance = 0m`) but downstream demand is {demanded_str}; the zero promise \
         vacuously satisfies any non-negative demand, suppressing the \
         ImportedTolerancePromiseInsufficient warning. Omit the `tolerance` parameter \
         to opt out of making a promise."
    );
    Diagnostic::warning(message).with_code(DiagnosticCode::InputTolerancePromiseIsZero)
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
/// - `demanded` — the demanded tolerance in SI metres (the tighter side);
///   rendered with µm / mm / m prefixes by magnitude.
/// - `promise` — the imported-geometry tolerance promise in SI metres
///   (the looser side; `promise > demanded`); rendered with µm / mm / m
///   prefixes by magnitude.
pub fn imported_tolerance_promise_diagnostic(
    input_template_name: &str,
    demanded: f64,
    promise: f64,
) -> Diagnostic {
    let promise_str = crate::tolerance_format::format_tolerance(promise);
    let demanded_str = crate::tolerance_format::format_tolerance(demanded);
    let message = format!(
        "imported geometry '{input_template_name}' tolerance promise {promise_str} is \
         insufficient for downstream demand {demanded_str}; proceeding with \
         as-imported realization"
    );
    Diagnostic::warning(message).with_code(DiagnosticCode::ImportedTolerancePromiseInsufficient)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::{DimensionVector, ValueCellId};
    use reify_ir::{DeterminacyState, PersistentMap, Value};

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

    /// Characterization test — PASSES against current code; purpose is to LOCK
    /// the lower-boundary-acceptance contract so a future option-(a) refactor
    /// (tightening the gate from `>= 0.0` to `> 0.0`) requires a deliberate
    /// test edit and conscious review rather than slipping in silently.
    ///
    /// **Resolution-locked (task 2833):** task 2833 resolved as option-(b continuation)
    /// — the gate stays at `is_finite() && >= 0.0`. This test remains green under
    /// the resolution because the extractor gate is unchanged. The placeholder-default
    /// footgun (`param tolerance : Length = 0m` suppressing the
    /// `ImportedTolerancePromiseInsufficient` warning) is now surfaced at the engine
    /// query layer via [`DiagnosticCode::InputTolerancePromiseIsZero`] rather than
    /// by tightening this gate.
    ///
    /// `si_value == 0.0` is accepted by Gate 4's `is_finite() && si_value >= 0.0`
    /// check — zero is the lower boundary of that gate, not a rejected value.
    /// This mirrors the precedent set by
    /// `crate::tolerance_scope::tests::extract_tolerance_bindings_accepts_zero_tolerance_literal`,
    /// which explicitly pins `0.0` as a valid tolerance literal ("exact
    /// representation") under the identical gate in the sibling extractor.
    /// `crate::tolerance_combine::extract_output_tolerance_bound` applies the
    /// same `is_finite() && >= 0.0` gate for the same reason. All three
    /// extractors were built in lockstep and must remain symmetric.
    ///
    /// See the `# Zero-promise interpretation` subsection in
    /// [`extract_input_tolerance_promise`]'s docstring for the semantic reading
    /// (a zero promise claims "imported geometry has zero deviation from the
    /// ideal") and for the placeholder-default footgun this enables when
    /// authors write `param tolerance : Length = 0m`.
    #[test]
    fn extract_input_tolerance_promise_accepts_zero_promise() {
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        values.insert(
            ValueCellId::new("STEPInput", "tolerance"),
            (
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                DeterminacyState::Determined,
            ),
        );

        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(0.0),
            "si_value == 0.0 is the lower boundary of Gate 4's `is_finite() && >= 0.0` \
             check and must be returned as Some(0.0), not silently skipped"
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

    /// Characterization test — PASSES against current code; purpose is to LOCK
    /// the asymmetric vacuous-satisfaction case against silent option-(a)
    /// refactors. This pins option-(b) of task 2793: `promise == 0.0` is kept
    /// as a valid promise (not rejected by the extractor gate), and the
    /// strict-`<` rule in [`is_promise_insufficient`] therefore classifies every
    /// non-negative demand as sufficient when the promise is zero.
    ///
    /// **Resolution-locked (task 2833):** task 2833 resolved as option-(b continuation).
    /// This test characterizes the LOWER-LEVEL comparator primitive and remains
    /// green because `is_promise_insufficient`'s strict-`<` rule is unchanged.
    /// The placeholder-default footgun (`param tolerance : Length = 0m` vacuously
    /// satisfying any non-negative demand, thereby suppressing the
    /// `ImportedTolerancePromiseInsufficient` warning) is now surfaced at the engine
    /// query layer by [`DiagnosticCode::InputTolerancePromiseIsZero`] (emitted
    /// when `promise == 0.0 && demanded > 0.0`). This test locks the baseline
    /// that the engine's new lint operates on top of — a future option-(a)
    /// refactor that tightens the extractor gate to `> 0.0` would require
    /// changing `extract_input_tolerance_promise_accepts_zero_promise` first,
    /// making the semantic change explicit.
    ///
    /// **Coverage gap filled:** the existing
    /// `is_promise_insufficient_returns_true_iff_demanded_strictly_less_than_promise`
    /// test pins the `(0.0, 0.0) -> false` symmetric edge but does NOT cover
    /// the asymmetric `(positive_d, 0.0) -> false` vacuous-satisfaction case
    /// that is the core footgun documented by this task. The symmetric `(0.0,
    /// 0.0)` edge is intentionally left to that test rather than duplicated
    /// here — only the asymmetric `positive_d` cases are new coverage.
    ///
    /// **Design rationale:** see the `# Zero-promise interpretation` subsection
    /// in [`extract_input_tolerance_promise`]'s docstring. When `promise == 0.0`
    /// the strict-`<` rule evaluates `demanded < 0.0`, which is false for every
    /// `demanded >= 0.0`. A zero promise is therefore the loosest satisfiable
    /// claim under this comparator — it vacuously satisfies every non-negative
    /// demand — which is why `param tolerance : Length = 0m` silently disables
    /// the `ImportedTolerancePromiseInsufficient` warning.
    #[test]
    fn is_promise_insufficient_returns_false_when_promise_is_zero_for_any_non_negative_demand() {
        // (a) demand == 1e-12 (sub-femtometre): positive demand, vacuously
        //     satisfied because 1e-12 < 0.0 is false.
        assert!(
            !is_promise_insufficient(1e-12, 0.0),
            "(a) demand 1e-12 (sub-femtometre) vs promise 0.0 — vacuously satisfied"
        );

        // (b) demand == 1e-6 (1µm): typical CAD tolerance, vacuously satisfied.
        assert!(
            !is_promise_insufficient(1e-6, 0.0),
            "(b) demand 1µm vs promise 0.0 — vacuously satisfied"
        );

        // (c) demand == 1.0 (1 metre): coarse demand, vacuously satisfied.
        assert!(
            !is_promise_insufficient(1.0, 0.0),
            "(c) demand 1.0m vs promise 0.0 — vacuously satisfied"
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

    /// Verifies that `input_tolerance_promise_is_zero_diagnostic` builds a
    /// `Severity::Warning` carrying `DiagnosticCode::InputTolerancePromiseIsZero`,
    /// names the input template in the message, and renders `demanded` in
    /// human-readable unit-prefixed form (via `tolerance_format::format_tolerance`).
    ///
    /// Combines the shape-check (severity + code + template name) and the
    /// unit-rendering regression guard into a single test since the diagnostic
    /// has only one side (no `promise` argument to render separately).
    ///
    /// The regression guard `!diag.message.contains("0.000001m")` locks out raw
    /// SI-metre float interpolation — same convention as
    /// `imported_tolerance_promise_diagnostic_renders_human_readable_units`
    /// (task 2790).
    #[test]
    fn input_tolerance_promise_is_zero_diagnostic_builds_warning_with_code_template_name_and_human_readable_demanded()
     {
        use reify_core::{DiagnosticCode, Severity};

        let diag = input_tolerance_promise_is_zero_diagnostic("STEPInput", 1e-6);

        assert_eq!(
            diag.severity,
            Severity::Warning,
            "diagnostic severity must be Warning (PRD: warn, not error — \
             runtime proceeds with as-imported realization)"
        );
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::InputTolerancePromiseIsZero),
            "diagnostic code must be InputTolerancePromiseIsZero (not \
             ImportedTolerancePromiseInsufficient) — proves the new variant fires"
        );
        assert!(
            diag.message.contains("STEPInput"),
            "message must name the input template so authors can locate the \
             import site (got: {:?})",
            diag.message
        );
        assert!(
            diag.message.contains("1µm"),
            "demanded 1e-6 must render as '1µm' via format_tolerance \
             (got: {:?})",
            diag.message
        );
        assert!(
            !diag.message.contains("0.000001m"),
            "regression guard: raw SI-metre form '0.000001m' must not appear \
             in the message (got: {:?})",
            diag.message
        );
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
        use reify_core::{DiagnosticCode, Severity};

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

    /// Verifies that `imported_tolerance_promise_diagnostic` renders tolerance
    /// values in human-readable unit-prefixed form (`µm`/`mm`/`m`) rather than
    /// raw SI-metre floats (`0.00005m`, `0.000001m`).
    ///
    /// Task 2790: decision — use `tolerance_format::format_tolerance` so all
    /// four `tolerance_*` diagnostic messages share the same µm/mm/m magnitude
    /// bands. The regression guard (`!diag.message.contains("0.00005m")`)
    /// locks out the old raw-f64 interpolation form.
    #[test]
    fn imported_tolerance_promise_diagnostic_renders_human_readable_units() {
        // promise = 50e-6 m → "50µm"; demanded = 1e-6 m → "1µm"
        let diag = imported_tolerance_promise_diagnostic("STEPInput", 1e-6, 50e-6);

        assert!(
            diag.message.contains("50µm"),
            "promise side must render as '50µm' (got: {:?})",
            diag.message
        );
        assert!(
            diag.message.contains("1µm"),
            "demanded side must render as '1µm' (got: {:?})",
            diag.message
        );
        assert!(
            !diag.message.contains("0.00005m"),
            "regression guard: raw SI-metre form '0.00005m' must not appear \
             in the message (got: {:?})",
            diag.message
        );
    }
}
