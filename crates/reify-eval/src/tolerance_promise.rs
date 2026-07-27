//! Imported-geometry tolerance-promise extraction and diagnostic.
//!
//! See `docs/prds/v0_2/per-purpose-tolerance.md` ("Resolved design decisions"
//! → "Imported geometry promise") and arch §10.4 / §14.5 for the contract:
//! an `Input` occurrence template carries a `provenance : Provenance` parameter
//! whose `tolerance_guarantee : Length` field the runtime treats as both an
//! *assertion* (used for downstream budget allocation) and a *promise* (cannot
//! be verified for arbitrary STEP/STL input). When a downstream demand is
//! tighter than the import promise the runtime emits a `Severity::Warning`
//! diagnostic and proceeds with the as-imported realization — see
//! [`imported_tolerance_promise_diagnostic`].
//!
//! # Recognition shape
//!
//! Unlike output occurrences (whose tolerance is encoded as a
//! `RepresentationWithin(subject, lit)` *constraint* on the template — see
//! [`crate::tolerance_combine::extract_output_tolerance_bound`]), an Input
//! occurrence's tolerance is encoded in its `provenance : Provenance`
//! parameter's `tolerance_guarantee : Length` field: the template carries a
//! `param provenance : Provenance = Provenance(... tolerance_guarantee: X ...)`
//! declaration, and the post-`eval()` `Snapshot.values` map contains an entry
//! keyed by `ValueCellId(input_template_name, "provenance")` whose value is a
//! `Value::StructureInstance` with `fields["tolerance_guarantee"] =
//! Value::Scalar { dimension == LENGTH, si_value }`. The stdlib `STEPInput`
//! occurrence in `io.ri` is the canonical example: its default provenance
//! sets `tolerance_guarantee: 0.001mm = 1e-6 m`.

use reify_core::{Diagnostic, DiagnosticCode, DimensionVector, ValueCellId};
use reify_ir::{DeterminacyState, PersistentMap, Value};

/// Extract the imported-geometry tolerance promise from an `Input` occurrence
/// template's `param provenance : Provenance` declaration.
///
/// Looks up the cell at `ValueCellId(input_template_name, "provenance")` in
/// the post-`eval()` `Snapshot.values` map, expects a `Value::StructureInstance`
/// whose `fields["tolerance_guarantee"]` is `Value::Scalar { dimension == LENGTH,
/// si_value }` with `si_value` finite and non-negative. Returns `Some(si_value)`
/// when all gates pass, `None` for every malformed shape — the silent-skip posture
/// mirrors [`crate::tolerance_combine::extract_output_tolerance_bound`] and
/// [`crate::tolerance_scope::extract_tolerance_bindings`].
///
/// # Recognition gates
///
/// 1. **Cell lookup:** `ValueCellId::new(input_template_name, "provenance")`
///    must exist in `values`. Entity name and member name are both keyed, so
///    `OtherInput.provenance` (different entity) and `STEPInput.source`
///    (different member) are both rejected here.
/// 2. **Outer Value shape:** the looked-up `Value` must be
///    `Value::StructureInstance`. Bool / Scalar / String etc. are silently
///    skipped. **Intentionally type-name-agnostic (duck-typed on field
///    shape):** the extractor does NOT check `data.type_name == "Provenance"`.
///    A non-`Provenance` struct stored at the `provenance` member that carries
///    a `tolerance_guarantee` LENGTH Scalar field is silently accepted — this
///    mirrors the sibling `extract_output_export_spec` extractor in
///    `tolerance_combine.rs` which also duck-types on field presence.
///    If type identity becomes load-bearing in a future revision, add a Gate 2b
///    `type_name == "Provenance"` check at that point.
/// 3. **Field existence:** `data.fields.get("tolerance_guarantee")` must
///    return `Some(_)`.
/// 4. **Field Value shape:** the `tolerance_guarantee` field must be
///    `Value::Scalar`. Non-Scalar variants (String, Bool, …) are silently
///    skipped.
/// 5. **Dimension:** `dimension == DimensionVector::LENGTH`. DIMENSIONLESS /
///    Force / other non-LENGTH scalars are silently skipped.
/// 6. **Finite & non-negative:** `is_valid_tolerance_si(si_value)` —
///    `si_value.is_finite() && si_value >= 0.0`. NaN / ±Inf / negative finite
///    values are silently skipped. Mirrors the identical gate in
///    `extract_output_tolerance_bound` for cross-extractor symmetry.
///
/// # Silent-skip posture
///
/// A malformed Input template simply contributes no promise — the diagnostic
/// doesn't fire, but the runtime doesn't crash either.
///
/// # Zero-promise interpretation
///
/// `si_value == 0.0` is **accepted** — it is the lower boundary of Gate 6's
/// `is_finite() && >= 0.0` check. A zero promise vacuously satisfies every
/// non-negative demand under [`is_promise_insufficient`]'s strict-`<` rule.
/// The placeholder-default footgun (`provenance.tolerance_guarantee = 0m`
/// silently disabling the [`DiagnosticCode::ImportedTolerancePromiseInsufficient`]
/// warning) is surfaced at the engine query layer via
/// [`DiagnosticCode::InputTolerancePromiseIsZero`].
///
/// **Correct opt-out:** omit the `provenance` parameter (or leave
/// `tolerance_guarantee` absent from the struct literal) so Gate 1 or Gate 3
/// returns `None`.
pub fn extract_input_tolerance_promise(
    values: &PersistentMap<ValueCellId, (Value, DeterminacyState)>,
    input_template_name: &str,
) -> Option<f64> {
    let provenance_id = ValueCellId::new(input_template_name, "provenance");
    let (value, _det) = values.get(&provenance_id)?; // Gate 1
    let Value::StructureInstance(data) = value else { return None }; // Gate 2
    let tol_value = data.fields.get("tolerance_guarantee")?; // Gate 3
    let (si_value, dimension) = match tol_value { // Gate 4
        Value::Scalar { si_value, dimension } => (*si_value, *dimension),
        _ => return None,
    };
    if dimension != DimensionVector::LENGTH { return None; } // Gate 5
    if !crate::tolerance_gate::is_valid_tolerance_si(si_value) { return None; } // Gate 6
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
/// subsection for the placeholder-default footgun this enables
/// (`provenance.tolerance_guarantee = 0m` silently disables the
/// [`DiagnosticCode::ImportedTolerancePromiseInsufficient`] warning) and the
/// recommended opt-out (omit `tolerance_guarantee` from the struct literal, or
/// omit the `provenance` parameter entirely, so Gate 1 or Gate 3 returns `None`).
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
/// This surfaces the placeholder-default footgun where
/// `provenance.tolerance_guarantee: 0m` silently disables the
/// [`DiagnosticCode::ImportedTolerancePromiseInsufficient`] warning: when
/// `promise == 0.0`, the strict-`<` rule in [`is_promise_insufficient`]
/// evaluates `demanded < 0.0`, which is false for every `demanded >= 0.0`,
/// so the insufficient branch never fires.
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
/// (`provenance.tolerance_guarantee: 0m`) but downstream demand is \
/// <demanded_str>; …"`.
///
/// The recommended opt-out is to **omit the `tolerance_guarantee` field**
/// from the provenance struct literal (so Gate 3 returns `None`), or to omit
/// the `provenance` parameter entirely (so Gate 1 returns `None`).
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
         (`provenance.tolerance_guarantee: 0m`) but downstream demand is {demanded_str}; \
         the zero promise vacuously satisfies any non-negative demand, suppressing the \
         ImportedTolerancePromiseInsufficient warning. Omit the \
         `tolerance_guarantee` field from the provenance struct (or the `provenance` \
         parameter entirely) to opt out of making a promise."
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

    /// Build a `Value::StructureInstance` for the Provenance type with the
    /// given arbitrary `tol_value` as the `tolerance_guarantee` field. This is
    /// the authoritative struct-shape literal in this test module — all other
    /// Provenance-shape builders delegate here so the struct definition lives in
    /// exactly one place; kept local here because unit tests in the library
    /// module cannot import from dev-deps.
    ///
    /// **SYNC NOTE:** This helper is the local copy of the Provenance struct
    /// shape. It is structurally mirrored by
    /// `reify_test_support::tolerance_fixtures::make_provenance_value`. If the
    /// `Provenance` shape changes (e.g. `StructureInstanceData` gains a new
    /// field, or `tolerance_guarantee` is renamed), update **both** this helper
    /// and `make_provenance_value` in
    /// `crates/reify-test-support/src/tolerance_fixtures.rs`.
    fn provenance_with_tol_field(tol_value: Value) -> Value {
        let mut fields: PersistentMap<String, Value> = PersistentMap::default();
        fields.insert("tolerance_guarantee".to_string(), tol_value);
        Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
            type_id: reify_ir::StructureTypeId(0),
            type_name: "Provenance".to_string(),
            version: 0,
            fields,
        }))
    }

    /// Build a `Value::StructureInstance` for the Provenance type with the
    /// given `tolerance_guarantee` SI value. Delegates to
    /// `provenance_with_tol_field` so the struct-shape literal appears exactly
    /// once in this file.
    fn provenance_instance(tolerance_guarantee_si: f64) -> Value {
        provenance_with_tol_field(Value::Scalar {
            si_value: tolerance_guarantee_si,
            dimension: DimensionVector::LENGTH,
        })
    }

    /// Pinned by the recognition-shape contract: the post-`eval()`
    /// `Snapshot.values` map carries an entry at
    /// `ValueCellId(input_template_name, "provenance")` whose
    /// `Value::StructureInstance` has a `fields["tolerance_guarantee"]` of
    /// `Value::Scalar { dimension == LENGTH, si_value }`. The extractor returns
    /// `Some(si_value)` when the entry is present and well-formed.
    #[test]
    fn extract_input_tolerance_promise_returns_si_length_when_value_present() {
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (provenance_instance(50e-6), DeterminacyState::Determined),
        );

        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(50e-6),
            "well-formed provenance.tolerance_guarantee (LENGTH, finite, non-negative) \
             must be extracted as Some(si_value) for the matching input_template_name"
        );
    }

    /// Silent-skip audit: every malformed entry must be silently rejected so
    /// the one valid entry survives. Tests each gate independently.
    #[test]
    fn extract_input_tolerance_promise_silent_skip_audit() {
        fn provenance_no_tol() -> Value {
            Value::StructureInstance(Box::new(reify_ir::StructureInstanceData {
                type_id: reify_ir::StructureTypeId(0),
                type_name: "Provenance".to_string(),
                version: 0,
                fields: PersistentMap::default(),
            }))
        }

        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();

        // (g) OtherInput.provenance with a tighter valid tolerance_guarantee — must
        //     be silently skipped by Gate 1 (entity name mismatch). Inserted FIRST
        //     so a scan-all regression would incorrectly win on this value.
        values.insert(
            ValueCellId::new("OtherInput", "provenance"),
            (
                provenance_instance(1e-9),
                DeterminacyState::Determined,
            ),
        );

        // (h) STEPInput.source — member mismatch, silently skipped by Gate 1.
        values.insert(
            ValueCellId::new("STEPInput", "source"),
            (
                Value::String("file.step".to_string()),
                DeterminacyState::Determined,
            ),
        );

        // (a) No provenance cell under STEPInput — Gate 1 returns None.
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(a) no provenance cell must be silently skipped by Gate 1"
        );

        // (b) provenance cell is Bool (not StructureInstance) — Gate 2 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (Value::Bool(true), DeterminacyState::Determined),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(b) non-StructureInstance provenance value (Bool) must be skipped by Gate 2"
        );

        // (c) StructureInstance with no tolerance_guarantee field — Gate 3 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (provenance_no_tol(), DeterminacyState::Determined),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(c) StructureInstance without tolerance_guarantee field must be skipped by Gate 3"
        );

        // (d) tolerance_guarantee is String (non-Scalar) — Gate 4 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::String("bad".to_string())),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(d) non-Scalar tolerance_guarantee (String) must be skipped by Gate 4"
        );

        // (e) tolerance_guarantee is Scalar DIMENSIONLESS — Gate 5 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::DIMENSIONLESS,
                }),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(e) DIMENSIONLESS tolerance_guarantee must be skipped by Gate 5 (LENGTH check)"
        );

        // (f-i) tolerance_guarantee NaN — Gate 6 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                }),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(f-i) NaN tolerance_guarantee must be skipped by Gate 6 (is_valid_tolerance_si)"
        );

        // (f-ii) +Inf — Gate 6 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::Scalar {
                    si_value: f64::INFINITY,
                    dimension: DimensionVector::LENGTH,
                }),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(f-ii) +Inf tolerance_guarantee must be skipped by Gate 6"
        );

        // (f-iii) -Inf — Gate 6 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::Scalar {
                    si_value: f64::NEG_INFINITY,
                    dimension: DimensionVector::LENGTH,
                }),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(f-iii) -Inf tolerance_guarantee must be skipped by Gate 6"
        );

        // (f-iv) negative finite — Gate 6 rejects.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (
                provenance_with_tol_field(Value::Scalar {
                    si_value: -1e-3,
                    dimension: DimensionVector::LENGTH,
                }),
                DeterminacyState::Determined,
            ),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            None,
            "(f-iv) negative tolerance_guarantee must be skipped by Gate 6"
        );

        // Valid entry — must survive all gates.
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (provenance_instance(50e-6), DeterminacyState::Determined),
        );
        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(50e-6),
            "valid provenance.tolerance_guarantee=50µm must survive all gates; \
             unrelated (g)/(h) entries must be ignored"
        );

        // Cross-check: OtherInput's provenance extracts by its own name.
        assert_eq!(
            extract_input_tolerance_promise(&values, "OtherInput"),
            Some(1e-9),
            "Gate 1 entity discrimination is bidirectional — OtherInput query \
             must return its own tolerance_guarantee, not STEPInput's"
        );

        // Cross-check: non-existent entity returns None.
        assert_eq!(
            extract_input_tolerance_promise(&values, "NonExistentInput"),
            None,
            "Gate 1 must return None when no provenance cell exists for the entity"
        );
    }

    /// Characterization test — locks the lower-boundary acceptance: a
    /// `provenance.tolerance_guarantee = 0.0` is accepted (not rejected).
    ///
    /// `si_value == 0.0` is the lower boundary of Gate 6's
    /// `is_valid_tolerance_si` check (`is_finite() && >= 0.0`), not a rejected
    /// value. Mirrors the precedent in `tolerance_scope` and
    /// `tolerance_combine`. All three extractors remain symmetric.
    #[test]
    fn extract_input_tolerance_promise_accepts_zero_promise() {
        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (provenance_instance(0.0), DeterminacyState::Determined),
        );

        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(0.0),
            "provenance.tolerance_guarantee == 0.0 is the lower boundary of Gate 6's \
             is_valid_tolerance_si check and must be returned as Some(0.0)"
        );
    }

    /// Characterization test for the intentional duck-typing posture (Gate 2):
    /// the extractor does NOT require `data.type_name == "Provenance"`. A
    /// `Value::StructureInstance` with a non-Provenance `type_name` that
    /// nonetheless carries a valid `tolerance_guarantee` LENGTH Scalar field
    /// must pass all gates and return `Some(si_value)`.
    ///
    /// This pins the deliberate design choice documented in the Gate 2
    /// docstring: the extractor is field-shape-agnostic on the outer struct
    /// type, mirroring the sibling `extract_output_export_spec` extractor
    /// (tolerance_combine.rs:535-575) which also duck-types on field presence.
    /// If a `type_name == "Provenance"` gate is added in the future, this test
    /// must be updated alongside it to make the semantic change explicit.
    #[test]
    fn extract_input_tolerance_promise_accepts_non_provenance_struct_with_matching_fields() {
        let mut fields: PersistentMap<String, Value> = PersistentMap::default();
        fields.insert(
            "tolerance_guarantee".to_string(),
            Value::Scalar {
                si_value: 25e-6,
                dimension: DimensionVector::LENGTH,
            },
        );
        // Use a clearly non-Provenance type_name to exercise Gate 2's
        // duck-typing posture.
        let non_provenance_instance = Value::StructureInstance(Box::new(
            reify_ir::StructureInstanceData {
                type_id: reify_ir::StructureTypeId(42),
                type_name: "NotProvenance".to_string(),
                version: 0,
                fields,
            },
        ));

        let mut values: PersistentMap<ValueCellId, (Value, DeterminacyState)> =
            PersistentMap::default();
        values.insert(
            ValueCellId::new("STEPInput", "provenance"),
            (non_provenance_instance, DeterminacyState::Determined),
        );

        assert_eq!(
            extract_input_tolerance_promise(&values, "STEPInput"),
            Some(25e-6),
            "Gate 2 is intentionally type-name-agnostic: a StructureInstance \
             with type_name='NotProvenance' but a valid tolerance_guarantee \
             LENGTH Scalar field must be accepted and return Some(25e-6)"
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
