//! Integration tests that pin the cross-crate public API surface for
//! `field_import_provenance`.
//!
//! The `use` statements below are the real test: if any of the three exports
//! goes missing or its visibility is narrowed to `pub(crate)`, this file will
//! fail to compile, immediately flagging the regression. Runtime assertions
//! are kept minimal — the unit tests in `field_import_provenance.rs` cover
//! behaviour; this file covers reachability.

use reify_core::ContentHash;
use reify_eval::field_import_provenance::build_field_import_provenance;
use reify_ir::FieldImportProvenance;

#[test]
fn build_field_import_provenance_pins_cross_crate_public_api_surface() {
    // Construct across the crate boundary to confirm all three exports are
    // publicly reachable from outside reify-eval.
    let _prov: FieldImportProvenance = build_field_import_provenance(
        "/tmp/fea_results.vdb",
        "OpenVDB",
        ContentHash::of(b"vdb file bytes here"),
        Some(50e-6),
        1_700_000_000,
    );
}
