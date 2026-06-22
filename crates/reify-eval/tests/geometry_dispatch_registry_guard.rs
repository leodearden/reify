//! Cross-crate integration gate for the geometry-op dispatch-registry refactor
//! (L6 of PRD docs/prds/geometry-op-dispatch-registry.md, section 9).
//!
//! TESTS-ONLY: no production change.
//!
//! The single test in this file — [`descriptor_table_is_the_live_op_handled_guarantee`]
//! — executes the live `reify-ir` descriptor table from the CONSUMER crate
//! (`reify-eval`):
//!
//! - every [`reify_ir::geometry::GeometryOpDiscriminants`] variant resolves via
//!   [`reify_ir::geometry::descriptor_for`] to `Some`;
//! - [`reify_ir::geometry::GEOMETRY_OP_DESCRIPTORS`] length equals
//!   `GeometryOpDiscriminants::COUNT`;
//! - all `disc` fields in the table are unique.
//!
//! This is the cross-crate "every op is handled" guarantee: it proves the table
//! is public, complete, and usable downstream — the dimension that L1's in-crate
//! completeness test (`geometry_op_descriptors_table_is_complete` in `reify-ir`)
//! cannot cover.  Behavioral equivalence (task point 3) is delivered by
//! `scripts/verify.sh --scope all` over the existing OCCT-gated golden e2e suite
//! plus the L4 byte-identical characterization oracle.

/// Every `GeometryOpDiscriminants` variant must resolve via `descriptor_for`
/// to `Some`; table length equals `COUNT`; disc fields are unique.
///
/// Executes the live every-op-handled guarantee from the CONSUMER crate,
/// complementing (not duplicating) L1's in-crate completeness test which
/// cannot prove cross-crate public usability.
#[test]
fn descriptor_table_is_the_live_op_handled_guarantee() {
    use reify_ir::geometry::{
        descriptor_for, GeometryOpDiscriminants, GEOMETRY_OP_DESCRIPTORS,
    };
    use strum::{EnumCount, IntoEnumIterator};

    // Every discriminant resolves to Some.
    let mut missing = Vec::new();
    for disc in GeometryOpDiscriminants::iter() {
        if descriptor_for(disc).is_none() {
            missing.push(format!("{:?}", disc));
        }
    }
    assert!(
        missing.is_empty(),
        "descriptor_for returned None for {} discriminant(s): {:?}\n\
         — add a matching row to GEOMETRY_OP_DESCRIPTORS",
        missing.len(),
        missing
    );

    // Table length equals the discriminant count.
    let disc_count = GeometryOpDiscriminants::COUNT;
    let table_len = GEOMETRY_OP_DESCRIPTORS.len();
    assert_eq!(
        table_len,
        disc_count,
        "GEOMETRY_OP_DESCRIPTORS has {table_len} rows but GeometryOpDiscriminants::COUNT is {disc_count}"
    );

    // Disc fields are unique (no duplicate descriptor rows).
    let mut seen = std::collections::HashSet::new();
    for d in GEOMETRY_OP_DESCRIPTORS {
        assert!(
            seen.insert(d.disc),
            "duplicate descriptor row for {:?} in GEOMETRY_OP_DESCRIPTORS",
            d.disc
        );
    }
}
