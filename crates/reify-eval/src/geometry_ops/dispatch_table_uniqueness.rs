//! Runtime uniqueness backstop for the production `*_COMPILERS` dispatch tables: for
//! each table, asserts its key column has exactly `Kind::VARIANT_COUNT` distinct keys
//! (no variant registered twice while another goes unregistered). Same check
//! `geometry_modify.rs` runs for `ModifyKind::CASES` in
//! `single_geom_target_kinds_cases_table_unique_variant_set`.

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
