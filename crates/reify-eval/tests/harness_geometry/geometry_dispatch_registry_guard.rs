//! Cross-crate integration gate for the geometry-op dispatch-registry refactor
//! (L6 of PRD docs/prds/geometry-op-dispatch-registry.md, section 9).
//!
//! TESTS-ONLY: no production change.
//!
//! The single test in this file — [`descriptor_table_is_the_live_op_handled_guarantee`]
//! — proves that `reify-ir`'s descriptor-table API is **publicly accessible from a
//! consumer crate** (`reify-eval`):
//!
//! - [`reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS`] length equals
//!   `GeometryOpDiscriminants::COUNT` (both items are `pub` and agree at link time);
//! - [`reify_ir::geometry::descriptor_for`] resolves a representative discriminant to
//!   `Some` (the function and the discriminant enum are `pub` and callable downstream).
//!
//! The unique signal here is cross-crate public usability — the dimension that L1's
//! in-crate completeness test (`geometry_op_descriptors_table_is_complete` in
//! `reify-ir/src/geometry.rs`) cannot cover.  Exhaustive per-variant coverage and
//! disc-uniqueness are left to L1, which already asserts exactly one row per
//! discriminant and `len == COUNT`, together implying uniqueness.
//! Behavioral equivalence (task point 3) is delivered by `scripts/verify.sh --scope
//! all` over the existing OCCT-gated golden e2e suite plus the L4 byte-identical
//! characterization oracle.

/// Cross-crate public-usability proof: `descriptor_for`, `GEOMETRY_OP_DESCRIPTORS`,
/// and `GeometryOpDiscriminants` must be `pub` and linkable from a consumer crate.
///
/// Trimmed to the minimum that L1 (`geometry_op_descriptors_table_is_complete`) cannot
/// cover: a `len == COUNT` cross-crate read plus a single-variant spot-call.
/// Exhaustive per-variant + uniqueness checks are left to L1.
#[test]
fn descriptor_table_is_the_live_op_handled_guarantee() {
    use reify_ir::geometry::{
        descriptor_for, GeometryOpDiscriminants, GEOMETRY_OP_DESCRIPTORS,
    };
    use strum::{EnumCount, IntoEnumIterator};

    // Table length equals the discriminant count (cross-crate read of GEOMETRY_OP_DESCRIPTORS
    // and GeometryOpDiscriminants::COUNT — proves both are `pub` and agree at link time).
    let disc_count = GeometryOpDiscriminants::COUNT;
    let table_len = GEOMETRY_OP_DESCRIPTORS.len();
    assert_eq!(
        table_len,
        disc_count,
        "GEOMETRY_OP_DESCRIPTORS has {table_len} rows but GeometryOpDiscriminants::COUNT is \
         {disc_count} — add a matching row or check for a duplicate"
    );

    // Spot-call descriptor_for on the first discriminant (proves descriptor_for and
    // GeometryOpDiscriminants variants are publicly callable from downstream).
    let first = GeometryOpDiscriminants::iter()
        .next()
        .expect("GeometryOpDiscriminants has at least one variant");
    assert!(
        descriptor_for(first).is_some(),
        "descriptor_for({first:?}) returned None — cross-crate call to the live table failed"
    );
}
