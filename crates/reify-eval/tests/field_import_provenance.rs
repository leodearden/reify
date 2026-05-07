//! Integration tests that pin the cross-crate public API surface for
//! `field_import_provenance`.
//!
//! These tests import across crate boundaries to verify that:
//! - `reify_eval::field_import_provenance::build_field_import_provenance` is
//!   publicly reachable.
//! - `reify_types::FieldImportProvenance` is publicly reachable and its fields
//!   are directly accessible (`pub`).
//! - `reify_types::ContentHash` is publicly reachable.
//!
//! If any of the three exports goes missing or its visibility is narrowed to
//! `pub(crate)`, this file will fail to compile, immediately flagging the
//! regression.

use reify_eval::field_import_provenance::build_field_import_provenance;
use reify_types::{ContentHash, FieldImportProvenance};

#[test]
fn build_field_import_provenance_pins_cross_crate_public_api_surface() {
    let prov: FieldImportProvenance = build_field_import_provenance(
        "/tmp/fea_results.vdb",
        "OpenVDB",
        b"vdb file bytes here",
        Some(50e-6),
        1_700_000_000,
    );

    // All five fields must be publicly readable from outside reify-eval.
    assert_eq!(prov.path, "/tmp/fea_results.vdb");
    assert_eq!(prov.format, "OpenVDB");
    assert_eq!(prov.ingestion_timestamp_secs, 1_700_000_000);
    assert_eq!(prov.declared_tolerance_si, Some(50e-6));
    assert_eq!(prov.content_hash, ContentHash::of(b"vdb file bytes here"));

    // Round-trip through Clone + PartialEq.
    let prov2 = prov.clone();
    assert_eq!(prov, prov2);
}
