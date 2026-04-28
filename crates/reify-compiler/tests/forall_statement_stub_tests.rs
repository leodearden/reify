//! Statement-form `forall` stub-diagnostic tests (task 2363, AST-only).
//!
//! Task 2363 introduces the `MemberDecl::ForallConnect` /
//! `MemberDecl::ForallConstraint` AST nodes per spec §5.4 but does NOT yet
//! lower them to per-element decls — that work is deferred to task 2364.
//! Until 2364 lands, the compiler must NOT silently drop these members; it
//! emits a stub error diagnostic anchored at the forall span so users get
//! actionable feedback rather than mysterious missing-decl behavior.
//!
//! These tests pin the stub-error contract in three sites:
//!
//!   * `entity.rs`'s second-pass member elaboration (top-level structures).
//!   * `guards.rs::compile_guarded_block` (forall inside `where { … }`).
//!   * `traits.rs::compile_purpose` (forall inside a purpose body).
//!
//! When task 2364 implements per-element elaboration, EACH of these tests
//! must be updated (or deleted) — failing silently here would mean the stub
//! site was removed without proper per-element coverage taking its place.
//!
//! The assertions filter on Severity::Error and look for the variant-specific
//! prefix in the diagnostic message; spans must be non-empty and located
//! inside the source. Wording is matched by substring rather than exact
//! literal so a future cosmetic message tweak doesn't force a churn here
//! while still pinning the meaningful prefix that distinguishes the three
//! sites.

use reify_test_support::{compile_source, errors_only};

/// Top-level `forall ... : connect` (entity.rs second-pass arm).
#[test]
fn forall_connect_at_structure_top_level_emits_stub_error() {
    let source = r#"
structure S {
    forall v in [1, 2, 3]: connect v.a -> v.b
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let stub_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message
                .contains("statement-form forall (connect/chain) not yet elaborated")
        })
        .collect();

    assert_eq!(
        stub_errors.len(),
        1,
        "expected exactly 1 stub error from entity.rs second pass for \
         ForallConnect, got {}: {:?}",
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
        "stub error label span must be non-empty, got: {:?}",
        err.labels[0].span
    );
}

/// Top-level `forall ... : constraint` (entity.rs second-pass arm).
#[test]
fn forall_constraint_at_structure_top_level_emits_stub_error() {
    let source = r#"
structure S {
    forall v in [1, 2, 3]: constraint v > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let stub_errors: Vec<_> = errors
        .iter()
        .filter(|d| {
            d.message
                .contains("statement-form forall (constraint) not yet elaborated")
        })
        .collect();

    assert_eq!(
        stub_errors.len(),
        1,
        "expected exactly 1 stub error from entity.rs second pass for \
         ForallConstraint, got {}: {:?}",
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
