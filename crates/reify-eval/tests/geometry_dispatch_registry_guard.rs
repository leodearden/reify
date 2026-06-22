//! Integration gate for the geometry-op dispatch-registry refactor (L6 of PRD
//! docs/prds/geometry-op-dispatch-registry.md, section 9).
//!
//! TESTS-ONLY: no production change.  All source files scanned are read at
//! test-execution time via `std::fs::read_to_string`; this file modifies none.
//!
//! Guards implemented:
//!
//! **(1) Cross-crate live guarantee**: every [`reify_ir::geometry::GeometryOpDiscriminants`]
//! value resolves via `descriptor_for`; table length equals `COUNT`.
//!
//! **(2) Canary retirement**: `GEOMETRY_OP_VARIANT_COUNT` const definition is
//! absent from `reify-ir`; `EXPECTED_DISPATCH_COUNT` is absent from
//! `reify-compiler`; `GEOMETRY_QUERY_VARIANT_COUNT` is present (out-of-scope
//! query canary untouched per PRD section 7).

// ── Step-5: Cross-crate live descriptor-table guarantee ──────────────────────

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

// ── Step-6: Canary-retirement + query-untouched ──────────────────────────────

/// Verify canary retirement: the `const GEOMETRY_OP_VARIANT_COUNT` definition
/// is absent from `reify-ir`; `EXPECTED_DISPATCH_COUNT` is absent from
/// `reify-compiler`; the out-of-scope `const GEOMETRY_QUERY_VARIANT_COUNT`
/// is still present in `reify-ir` (per PRD §7).
///
/// Matching the const-definition form (not the bare identifier) is
/// comment-tolerant: a surviving historical comment at reify-ir geometry.rs
/// near line 7644 names `GEOMETRY_OP_VARIANT_COUNT` without defining it.
#[test]
fn canaries_retired_and_query_canary_untouched() {
    let reify_ir_geometry = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reify-ir/src/geometry.rs"
    ))
    .expect("could not read reify-ir/src/geometry.rs");

    let reify_compiler_geometry = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../reify-compiler/src/geometry.rs"
    ))
    .expect("could not read reify-compiler/src/geometry.rs");

    // Match const-definition form, not bare name substring (comment-tolerant).
    let op_variant_count_def = "const GEOMETRY_OP_VARIANT_COUNT";
    assert!(
        !reify_ir_geometry
            .lines()
            .any(|line| line.contains(op_variant_count_def)),
        "reify-ir geometry.rs still defines `{op_variant_count_def}` — \
         the L1 canary was retired in task #4670 and must not be re-introduced"
    );

    let expected_dispatch_def = "EXPECTED_DISPATCH_COUNT";
    assert!(
        !reify_compiler_geometry
            .lines()
            .any(|line| line.contains(expected_dispatch_def)),
        "reify-compiler geometry.rs still contains `{expected_dispatch_def}` — \
         the L3 canary was retired in task #4672 and must not be re-introduced"
    );

    // GEOMETRY_QUERY_VARIANT_COUNT is out-of-scope per §7 and must be untouched.
    let query_count_def = "const GEOMETRY_QUERY_VARIANT_COUNT";
    assert!(
        reify_ir_geometry
            .lines()
            .any(|line| line.contains(query_count_def)),
        "reify-ir geometry.rs is missing `{query_count_def}` — \
         the query canary is out-of-scope and must not have been removed"
    );
}
