//! Builder for [`FieldImportProvenance`] — the provenance record written when
//! an imported field is ingested via an `Input` occurrence.
//!
//! See `docs/prds/v0_2/imported-field-source.md` ("Resolved design decisions"
//! → "Provenance via Input occurrence (§14.5)") and arch §14.5 for the
//! contract. This module is the `reify-eval`-side peer to
//! [`crate::tolerance_promise`], following the same focused-module pattern.

use reify_core::ContentHash;
use reify_ir::FieldImportProvenance;

/// Build a [`FieldImportProvenance`] record for a field import event.
///
/// This is the production call site for arch §14.5 / PRD
/// `docs/prds/v0_2/imported-field-source.md` ("Resolved design decisions"
/// → "Provenance via Input occurrence"). Task 5 of the decomposition plan
/// will call this builder from `elaborate_field`'s `CompiledFieldSource::Imported`
/// arm once the end-to-end wiring lands.
///
/// # Parameters
///
/// * `path` — source file path (absolute or relative).
/// * `format` — format name, e.g. `"OpenVDB"`, `"STEP"`.
/// * `content_hash` — pre-computed [`ContentHash`] (XXH3-128) of the source
///   file bytes at ingestion time. The caller is responsible for hashing —
///   this separates the potentially large I/O pass (reading/hashing gigabytes)
///   from this pure record-assembly function. Use
///   [`ContentHash::of`] for small files; for large files, stream the bytes
///   through an incremental hasher and pass the result here.
/// * `declared_tolerance_si` — tolerance declared on the `Input` occurrence's
///   `param tolerance : Length = …`, in SI metres. Malformed values (NaN,
///   ±Inf, negative finite) are silently collapsed to `None` by the Gate 4
///   filter — see [`crate::tolerance_promise::extract_input_tolerance_promise`]
///   for the canonical reference. `Some(0.0)` is preserved (lower-boundary
///   acceptance, consistent with `extract_input_tolerance_promise_accepts_zero_promise`).
///
///   > **Warning — silent drop:** if you compute a tolerance programmatically
///   > and pass `Some(value)` where `value` is NaN, ±Inf, or negative, the
///   > resulting `FieldImportProvenance.declared_tolerance_si` will be `None`
///   > with no error or warning. This is intentional (cross-extractor symmetry
///   > with `extract_input_tolerance_promise`) but means the caller has no
///   > runtime signal that the value was rejected. Validate tolerance values at
///   > the source (parse / user-input boundary) rather than relying on this
///   > function to surface programming errors.
/// * `ingestion_timestamp_secs` — Unix epoch seconds at which ingestion
///   occurred; caller-supplied so this function stays a pure function with no
///   internal `SystemTime::now()` call.
///
/// # Determinism
///
/// The function is a pure function: identical inputs always produce identical
/// `FieldImportProvenance` outputs. The caller controls both the
/// pre-computed hash and the timestamp; the Gate 4 filter is a simple
/// arithmetic predicate with no hidden state.
///
/// # Cross-extractor symmetry
///
/// The Gate 4 filter (`is_finite() && >= 0.0`) applied to
/// `declared_tolerance_si` mirrors the same gate in
/// [`crate::tolerance_promise::extract_input_tolerance_promise`] (lines
/// 163–168) and in
/// [`crate::tolerance_combine::extract_output_tolerance_bound`]. This keeps
/// the entire tolerance-promise vocabulary consistent: no malformed promise
/// can reach `FieldImportProvenance.declared_tolerance_si` and then propagate
/// into `is_promise_insufficient`'s debug_assert invariants.
pub fn build_field_import_provenance(
    path: &str,
    format: &str,
    content_hash: ContentHash,
    declared_tolerance_si: Option<f64>,
    ingestion_timestamp_secs: u64,
) -> FieldImportProvenance {
    FieldImportProvenance {
        path: path.to_string(),
        format: format.to_string(),
        content_hash,
        ingestion_timestamp_secs,
        // Gate 4 filter: mirrors `extract_input_tolerance_promise`'s Gate 4
        // (`tolerance_promise.rs:163-168`) for cross-extractor symmetry. A
        // malformed `Some(NaN)` / `Some(±Inf)` / `Some(-1.0)` cannot reach
        // `FieldImportProvenance.declared_tolerance_si` and propagate into
        // `is_promise_insufficient`'s debug_assert invariants.
        // `Some(0.0)` is preserved (lower-boundary acceptance — matches
        // `extract_input_tolerance_promise_accepts_zero_promise`).
        // NOTE: malformed values are dropped silently — see the "Warning"
        // in the function doc comment above.
        declared_tolerance_si: declared_tolerance_si
            .filter(|v| crate::tolerance_gate::is_valid_tolerance_si(*v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::ContentHash;

    #[test]
    fn build_field_import_provenance_filters_malformed_declared_tolerance_to_none() {
        let any_hash = ContentHash::of(b"");

        // Gate 4 rejects NaN (a)
        let r = build_field_import_provenance("p", "f", any_hash, Some(f64::NAN), 0);
        assert_eq!(r.declared_tolerance_si, None, "NaN should be filtered");

        // Gate 4 rejects +Inf (b)
        let r = build_field_import_provenance("p", "f", any_hash, Some(f64::INFINITY), 0);
        assert_eq!(r.declared_tolerance_si, None, "+Inf should be filtered");

        // Gate 4 rejects -Inf (c)
        let r = build_field_import_provenance("p", "f", any_hash, Some(f64::NEG_INFINITY), 0);
        assert_eq!(r.declared_tolerance_si, None, "-Inf should be filtered");

        // Gate 4 rejects negative finite (d)
        let r = build_field_import_provenance("p", "f", any_hash, Some(-1.0e-3), 0);
        assert_eq!(r.declared_tolerance_si, None, "negative should be filtered");

        // Lower-boundary acceptance: zero is accepted (e), mirrors
        // extract_input_tolerance_promise_accepts_zero_promise
        let r = build_field_import_provenance("p", "f", any_hash, Some(0.0), 0);
        assert_eq!(r.declared_tolerance_si, Some(0.0), "zero should be kept");

        // Typical valid case (f)
        let r = build_field_import_provenance("p", "f", any_hash, Some(50e-6), 0);
        assert_eq!(
            r.declared_tolerance_si,
            Some(50e-6),
            "valid positive should be kept"
        );
    }

    #[test]
    fn build_field_import_provenance_passes_through_typed_inputs_and_content_hash() {
        let hash = ContentHash::of(&[0xCAu8, 0xFE, 0xBA, 0xBE]);
        let result = build_field_import_provenance(
            "fea_results.vdb",
            "OpenVDB",
            hash,
            Some(50e-6),
            1_700_000_000,
        );

        assert_eq!(result.path, "fea_results.vdb");
        assert_eq!(result.format, "OpenVDB");
        assert_eq!(result.ingestion_timestamp_secs, 1_700_000_000);
        assert_eq!(result.declared_tolerance_si, Some(50e-6));
        assert_eq!(result.content_hash, hash);
    }

    #[test]
    fn build_field_import_provenance_is_deterministic_for_identical_inputs() {
        let hash = ContentHash::of(b"identical bytes");
        let a = build_field_import_provenance(
            "fea_results.vdb",
            "OpenVDB",
            hash,
            Some(50e-6),
            1_700_000_000,
        );
        let b = build_field_import_provenance(
            "fea_results.vdb",
            "OpenVDB",
            hash,
            Some(50e-6),
            1_700_000_000,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn build_field_import_provenance_distinguishes_distinct_content_hashes() {
        let a = build_field_import_provenance("p", "f", ContentHash::of(&[0x00, 0x01]), None, 0);
        let b = build_field_import_provenance("p", "f", ContentHash::of(&[0x00, 0x02]), None, 0);
        assert_ne!(a.content_hash, b.content_hash);
    }

    #[test]
    fn build_field_import_provenance_accepts_zero_content_hash() {
        // ContentHash::of(&[]) is well-formed; the builder must not panic
        // and must pass the hash through unchanged.
        let empty_hash = ContentHash::of(&[]);
        let result = build_field_import_provenance("p", "f", empty_hash, None, 0);
        assert_eq!(result.content_hash, empty_hash);
    }
}
