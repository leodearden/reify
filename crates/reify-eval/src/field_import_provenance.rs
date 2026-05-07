//! Builder for [`FieldImportProvenance`] — the provenance record written when
//! an imported field is ingested via an `Input` occurrence.
//!
//! See `docs/prds/v0_2/imported-field-source.md` ("Resolved design decisions"
//! → "Provenance via Input occurrence (§14.5)") and arch §14.5 for the
//! contract. This module is the `reify-eval`-side peer to
//! [`crate::tolerance_promise`], following the same focused-module pattern.

use reify_types::{ContentHash, FieldImportProvenance};

/// Build a [`FieldImportProvenance`] record for a field import event.
///
/// # Parameters
///
/// * `path` — source file path (absolute or relative).
/// * `format` — format name, e.g. `"OpenVDB"`.
/// * `file_bytes` — raw bytes of the source file at ingestion time; hashed
///   deterministically via [`ContentHash::of`] (XXH3-128).
/// * `declared_tolerance_si` — tolerance declared on the `Input` occurrence's
///   `param tolerance : Length = …`, in SI metres. Values that are NaN, ±Inf,
///   or negative are mapped to `None` (Gate 4 filter — mirrors
///   [`crate::tolerance_promise::extract_input_tolerance_promise`] Gate 4 for
///   cross-extractor symmetry; downstream `is_promise_insufficient`
///   debug_assert invariants rely on this contract).
/// * `ingestion_timestamp_secs` — Unix epoch seconds at which ingestion
///   occurred; caller-supplied so this function stays a pure function with no
///   internal `SystemTime::now()` call.
///
/// # Determinism
///
/// The function is deterministic: identical inputs always produce identical
/// outputs. `ContentHash::of` is backed by XXH3-128 (see `reify-types`
/// `hash::deterministic` test). The caller controls the timestamp, so there is
/// no hidden non-determinism. Empty `file_bytes` are accepted and produce a
/// well-formed `ContentHash`.
pub fn build_field_import_provenance(
    path: &str,
    format: &str,
    file_bytes: &[u8],
    declared_tolerance_si: Option<f64>,
    ingestion_timestamp_secs: u64,
) -> FieldImportProvenance {
    FieldImportProvenance {
        path: path.to_string(),
        format: format.to_string(),
        content_hash: ContentHash::of(file_bytes),
        ingestion_timestamp_secs,
        // Gate 4 filter: mirrors `extract_input_tolerance_promise`'s Gate 4
        // (`tolerance_promise.rs:163-168`) for cross-extractor symmetry. A
        // malformed `Some(NaN)` / `Some(±Inf)` / `Some(-1.0)` cannot reach
        // `FieldImportProvenance.declared_tolerance_si` and propagate into
        // `is_promise_insufficient`'s debug_assert invariants.
        // `Some(0.0)` is preserved (lower-boundary acceptance — matches
        // `extract_input_tolerance_promise_accepts_zero_promise`).
        declared_tolerance_si: declared_tolerance_si.filter(|v| v.is_finite() && *v >= 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_field_import_provenance_filters_malformed_declared_tolerance_to_none() {
        // Gate 4 rejects NaN (a)
        let r = build_field_import_provenance("p", "f", b"", Some(f64::NAN), 0);
        assert_eq!(r.declared_tolerance_si, None, "NaN should be filtered");

        // Gate 4 rejects +Inf (b)
        let r = build_field_import_provenance("p", "f", b"", Some(f64::INFINITY), 0);
        assert_eq!(r.declared_tolerance_si, None, "+Inf should be filtered");

        // Gate 4 rejects -Inf (c)
        let r = build_field_import_provenance("p", "f", b"", Some(f64::NEG_INFINITY), 0);
        assert_eq!(r.declared_tolerance_si, None, "-Inf should be filtered");

        // Gate 4 rejects negative finite (d)
        let r = build_field_import_provenance("p", "f", b"", Some(-1.0e-3), 0);
        assert_eq!(r.declared_tolerance_si, None, "negative should be filtered");

        // Lower-boundary acceptance: zero is accepted (e), mirrors
        // extract_input_tolerance_promise_accepts_zero_promise
        let r = build_field_import_provenance("p", "f", b"", Some(0.0), 0);
        assert_eq!(r.declared_tolerance_si, Some(0.0), "zero should be kept");

        // Typical valid case (f)
        let r = build_field_import_provenance("p", "f", b"", Some(50e-6), 0);
        assert_eq!(r.declared_tolerance_si, Some(50e-6), "valid positive should be kept");
    }

    #[test]
    fn build_field_import_provenance_passes_through_typed_inputs_and_hashes_bytes() {
        let result = build_field_import_provenance(
            "fea_results.vdb",
            "OpenVDB",
            &[0xCAu8, 0xFE, 0xBA, 0xBE],
            Some(50e-6),
            1_700_000_000,
        );

        assert_eq!(result.path, "fea_results.vdb");
        assert_eq!(result.format, "OpenVDB");
        assert_eq!(result.ingestion_timestamp_secs, 1_700_000_000);
        assert_eq!(result.declared_tolerance_si, Some(50e-6));
        assert_eq!(
            result.content_hash,
            ContentHash::of(&[0xCAu8, 0xFE, 0xBA, 0xBE])
        );
    }
}
