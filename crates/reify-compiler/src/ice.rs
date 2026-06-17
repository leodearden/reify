use super::*;

/// The kind of item that could not be resolved in pass 2, for use with
/// [`emit_ice_unresolved`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnresolvedKind {
    /// A named entity (structure parameter, port member, etc.).
    Name,
    /// A guarded member binding.
    GuardedMember,
}

impl UnresolvedKind {
    /// Returns the grammatical phrase that fits
    /// `"unresolved {phrase} '{name}' in pass 2"`.
    pub(crate) fn as_phrase(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::GuardedMember => "guarded member",
        }
    }
}

/// Emit a pass-2 "unresolved name" ICE diagnostic and return `Type::dimensionless_scalar()` as
/// the established fallback.
///
/// This centralises the three structurally identical ICE patterns in `entity.rs` and
/// `guards.rs`. Every site resolves a declared `Param` name from the scope
/// built in pass 1; if the name is absent the pass-1 registration invariant
/// was violated, which is a compiler bug, not a user error.
///
/// The emitted diagnostic message is:
/// `"internal compiler error: unresolved {phrase} '{name}' in pass 2"`
/// where `{phrase}` is determined by [`UnresolvedKind::as_phrase`],
/// with a label `"ICE: name should have been registered in pass 1"` at `span`.
///
/// Callers use this as:
/// ```text
/// .unwrap_or_else(|| emit_ice_unresolved(UnresolvedKind::Name, &param.name, param.span, diagnostics))
/// ```
pub(crate) fn emit_ice_unresolved(
    kind: UnresolvedKind,
    name: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    let phrase = kind.as_phrase();
    diagnostics.push(
        Diagnostic::error(format!(
            "internal compiler error: unresolved {phrase} '{name}' in pass 2"
        ))
        .with_label(DiagnosticLabel::new(
            span,
            "ICE: name should have been registered in pass 1",
        )),
    );
    Type::dimensionless_scalar()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_ice_unresolved_pushes_one_error_diagnostic() {
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved(
            UnresolvedKind::Name,
            "foo",
            SourceSpan::empty(0),
            &mut diags,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn emit_ice_unresolved_formats_message_with_kind_and_name() {
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved(
            UnresolvedKind::Name,
            "foo",
            SourceSpan::empty(0),
            &mut diags,
        );
        let msg = &diags[0].message;
        assert!(
            msg.contains("unresolved name"),
            "expected 'unresolved name' in {msg:?}"
        );
        assert!(msg.contains("'foo'"), "expected name 'foo' in {msg:?}");

        let mut diags2: Vec<Diagnostic> = vec![];
        emit_ice_unresolved(
            UnresolvedKind::GuardedMember,
            "bar",
            SourceSpan::empty(0),
            &mut diags2,
        );
        let msg2 = &diags2[0].message;
        assert!(
            msg2.contains("unresolved guarded member"),
            "expected 'unresolved guarded member' in {msg2:?}"
        );
        assert!(msg2.contains("'bar'"), "expected name 'bar' in {msg2:?}");
    }

    #[test]
    fn emit_ice_unresolved_attaches_ice_label_with_span() {
        let expected_span = SourceSpan::new(10, 20);
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved(UnresolvedKind::Name, "foo", expected_span, &mut diags);
        assert_eq!(diags[0].labels.len(), 1);
        assert_eq!(diags[0].labels[0].span, expected_span);
        let label_msg = &diags[0].labels[0].message;
        assert!(
            label_msg.contains("ICE"),
            "expected 'ICE' in label {label_msg:?}"
        );
        assert!(
            label_msg.contains("pass 1"),
            "expected 'pass 1' in label {label_msg:?}"
        );
    }

    #[test]
    fn emit_ice_unresolved_returns_type_error() {
        let mut diags: Vec<Diagnostic> = vec![];
        let ty = emit_ice_unresolved(UnresolvedKind::Name, "x", SourceSpan::empty(0), &mut diags);
        assert_eq!(ty, Type::Error);
    }

    #[test]
    fn unresolved_kind_as_phrase_returns_expected_strings() {
        assert_eq!(UnresolvedKind::Name.as_phrase(), "name");
        assert_eq!(UnresolvedKind::GuardedMember.as_phrase(), "guarded member");
    }
}
