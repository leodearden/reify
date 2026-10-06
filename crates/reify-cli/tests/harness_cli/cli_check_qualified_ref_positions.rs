//! End-to-end `reify check` gate: an UNBOUND qualified reference (one whose
//! qualifier is not a declared import binding) must stay LOUD in TYPE position
//! and in `sub` structure_name position, so task #5505 (ν) cannot silently
//! soften either diagnostic. Backs the per-position loudness claim on
//! `namespaced_name_text` in `crates/reify-syntax/src/ts_parser.rs`.
//!
//! Every case asserts stderr TEXT, never exit status alone: a missing fixture
//! also exits 1.

use crate::common;

/// TYPE position: `param p : obj.width = 5mm` where `obj` is undeclared.
#[test]
fn check_unbound_qualified_ref_in_type_position_is_loud() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("qualified_ref_undeclared_type.ri"),
    );

    assert!(
        !status.success(),
        "expected non-zero exit.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("unresolved type: obj.width"),
        "expected 'unresolved type: obj.width' in stderr.\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// `sub` structure_name position: `sub s = obj.width()` where `obj` is
/// undeclared.
#[test]
fn check_unbound_qualified_ref_in_sub_position_is_loud() {
    let (status, stdout, stderr) = common::run_subcommand(
        "check",
        &common::fixture_path("qualified_ref_undeclared_sub.ri"),
    );

    assert!(
        !status.success(),
        "expected non-zero exit.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("references unknown structure \"obj.width\""),
        "expected 'references unknown structure \"obj.width\"' in stderr.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("sub-component \"s\""),
        "expected 'sub-component \"s\"' in stderr.\nstdout: {stdout}\nstderr: {stderr}"
    );
}
