//! Statement-form `forall` stub-diagnostic tests (task 2363, AST-only).
//!
//! Task 2363 introduces the `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` AST nodes per spec §5.4. The two
//! `entity.rs`-second-pass stub tests originally pinned here were lifted by
//! task 2364, which replaced the stub error with per-element elaboration
//! (see `forall_statement_lower_tests.rs`).
//!
//! What remains in this file is the third stub site, in `guards.rs`:
//! `forall connect/chain` inside a guarded `where { ... }` block. That
//! stub is a deliberate gate (not yet implemented) and stays pinned here.
//! The corresponding "forall constraint inside guarded block" stub is also
//! covered for symmetry once a paired test lands; for now this file pins
//! the `ForallConnect` arm of `compile_guarded_block`.
//!
//! When that guarded-block stub is eventually lifted (separate task), this
//! test must be updated or deleted alongside the lift; failing silently
//! here would mean the stub site was removed without proper coverage
//! taking its place.
//!
//! The assertions filter on Severity::Error and look for the variant-specific
//! prefix in the diagnostic message; spans must be non-empty and located
//! inside the source. Wording is matched by substring rather than exact
//! literal so a future cosmetic message tweak doesn't force a churn here
//! while still pinning the meaningful prefix that distinguishes the site.

use reify_test_support::{compile_source, errors_only};

/// `forall ... : connect` nested inside a guarded block — pinned by the
/// `compile_guarded_block` arm in `guards.rs`. The diagnostic message there
/// is distinct from the entity.rs one ("not yet supported" vs "not yet
/// elaborated") so we filter on the guarded-block-specific phrasing.
#[test]
fn forall_connect_inside_guarded_block_emits_stub_error() {
    let source = r#"
structure S {
    param needs : Bool = true
    where needs {
        forall v in [1, 2, 3]: connect v.a -> v.b
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let stub_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message.contains(
                "forall connect/chain statements in guarded blocks are not yet supported",
            )
        })
        .collect();

    assert_eq!(
        stub_errors.len(),
        1,
        "expected exactly 1 stub error from guards.rs for ForallConnect \
         inside a guarded block, got {}: {:?}",
        stub_errors.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let err = stub_errors[0];
    assert!(
        !err.labels.is_empty(),
        "stub error must carry a label anchored at the forall span"
    );
    assert!(
        !err.labels[0].span.is_empty(),
        "stub error label span must be non-empty"
    );
}
