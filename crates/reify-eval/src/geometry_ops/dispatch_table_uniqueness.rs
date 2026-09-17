//! Runtime uniqueness backstop for the production `*_COMPILERS` dispatch tables.
//!
//! `geometry_ops.rs`'s `lock_dispatch_table!` macro catches a *count* mismatch
//! between a table and its `Kind::VARIANT_COUNT` at compile time, but a table with
//! exactly the right number of rows and a duplicate-with-omission edit (two rows for
//! one variant, zero for another) passes that check silently — the same gap that was
//! closed for `ModifyKind::CASES` in `geometry_modify.rs` by
//! `single_geom_target_kinds_cases_table_unique_variant_set`. This module applies the
//! same fix to the seven production dispatch tables themselves: collect each table's
//! key column into a `HashSet` and assert the set's size still equals
//! `Kind::VARIANT_COUNT`.
//!
//! A sibling file rather than an addition to `tests.rs`: the checks here are a small,
//! self-contained concern, and `tests.rs` (already 30k+ lines) has its own
//! `compile_geometry_op_registry_completeness` covering the adjacent "every variant
//! has *some* registered compiler" property — this module covers the orthogonal "no
//! variant is registered twice while another goes unregistered" property instead of
//! growing that file further.

use super::*;
use std::collections::HashSet;

/// Asserts every key in `$table` is distinct, i.e. the table has exactly one row per
/// `$kind` variant rather than merely the right row *count* (see module docs).
macro_rules! assert_dispatch_table_keys_unique {
    ($table:ident, $kind:ty) => {{
        let keys: HashSet<$kind> = $table.iter().map(|&(k, _)| k).collect();
        assert_eq!(
            keys.len(),
            <$kind>::VARIANT_COUNT,
            "{}: duplicate-with-omission row — every {} variant must appear exactly \
             once in the production dispatch table (got {} unique keys, expected {})",
            stringify!($table),
            stringify!($kind),
            keys.len(),
            <$kind>::VARIANT_COUNT,
        );
    }};
}

#[test]
fn primitive_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(PRIMITIVE_COMPILERS, reify_compiler::PrimitiveKind);
}

#[test]
fn modify_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(MODIFY_COMPILERS, reify_compiler::ModifyKind);
}

#[test]
fn transform_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(TRANSFORM_COMPILERS, reify_compiler::TransformKind);
}

#[test]
fn pattern_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(PATTERN_COMPILERS, reify_compiler::PatternKind);
}

#[test]
fn sweep_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(SWEEP_COMPILERS, reify_compiler::SweepKind);
}

#[test]
fn curve_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(CURVE_COMPILERS, reify_compiler::CurveKind);
}

#[test]
fn profile_compilers_keys_are_unique() {
    assert_dispatch_table_keys_unique!(PROFILE_COMPILERS, reify_compiler::ProfileKind);
}
