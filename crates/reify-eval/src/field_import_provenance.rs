//! Builder for [`FieldImportProvenance`] — the provenance record written when
//! an imported field is ingested via an `Input` occurrence.
//!
//! See `docs/prds/v0_2/imported-field-source.md` ("Resolved design decisions"
//! → "Provenance via Input occurrence (§14.5)") and arch §14.5 for the
//! contract. This module is the `reify-eval`-side peer to
//! [`crate::tolerance_promise`], following the same focused-module pattern.

use reify_types::{ContentHash, FieldImportProvenance};

#[cfg(test)]
mod tests {
    use super::*;

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
