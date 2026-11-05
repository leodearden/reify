//! Behavioural pin of `DiagnosticIdentity`: two diagnostics are the same
//! finding exactly when they agree on severity, code and message.

use std::collections::HashSet;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticIdentity, DiagnosticLabel, SourceSpan};

#[test]
fn identity_ignores_labels_and_candidates() {
    let first = Diagnostic::error("unknown type `Frobnicate`")
        .with_code(DiagnosticCode::UnresolvedType)
        .with_label(DiagnosticLabel::new(SourceSpan::new(3, 13), "here"))
        .with_candidates(["std::Frob"]);
    let second = Diagnostic::error("unknown type `Frobnicate`")
        .with_code(DiagnosticCode::UnresolvedType)
        .with_label(DiagnosticLabel::new(SourceSpan::new(40, 50), "and here"))
        .with_label(DiagnosticLabel::new(SourceSpan::new(60, 61), "again"))
        .with_candidates(["std::Frobnicator", "std::Frob"]);

    assert_eq!(
        DiagnosticIdentity::of(&first),
        DiagnosticIdentity::of(&second)
    );

    let identities: HashSet<DiagnosticIdentity<'_>> = [&first, &second]
        .into_iter()
        .map(DiagnosticIdentity::of)
        .collect();
    assert_eq!(identities.len(), 1, "Hash must agree with Eq");
}

#[test]
fn identity_distinguishes_severity() {
    let error = Diagnostic::error("shadowed name").with_code(DiagnosticCode::UnresolvedType);
    let warning = Diagnostic::warning("shadowed name").with_code(DiagnosticCode::UnresolvedType);

    assert_ne!(
        DiagnosticIdentity::of(&error),
        DiagnosticIdentity::of(&warning)
    );
}

#[test]
fn identity_distinguishes_code_including_none_vs_some() {
    let uncoded = Diagnostic::error("unknown type parameter");
    let unresolved =
        Diagnostic::error("unknown type parameter").with_code(DiagnosticCode::UnresolvedType);
    let fn_param =
        Diagnostic::error("unknown type parameter").with_code(DiagnosticCode::FnUnknownTypeParam);

    assert_ne!(
        DiagnosticIdentity::of(&uncoded),
        DiagnosticIdentity::of(&unresolved)
    );
    assert_ne!(
        DiagnosticIdentity::of(&unresolved),
        DiagnosticIdentity::of(&fn_param)
    );
}

#[test]
fn identity_distinguishes_message() {
    let first = Diagnostic::error("unknown type `A`").with_code(DiagnosticCode::UnresolvedType);
    let second = Diagnostic::error("unknown type `B`").with_code(DiagnosticCode::UnresolvedType);

    assert_ne!(
        DiagnosticIdentity::of(&first),
        DiagnosticIdentity::of(&second)
    );
}
