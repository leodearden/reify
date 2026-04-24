use super::*;

/// Emit a pass-2 "unresolved name" ICE diagnostic and return `Type::Real` as
/// the established fallback.
///
/// This centralises the three identical ICE patterns in `entity.rs` and
/// `guards.rs`. Every site resolves a declared `Param` name from the scope
/// built in pass 1; if the name is absent the pass-1 registration invariant
/// was violated, which is a compiler bug, not a user error.
///
/// The emitted diagnostic message is:
/// `"internal compiler error: unresolved {context} '{name}' in pass 2"`
/// with a label `"ICE: name should have been registered in pass 1"` at `span`.
///
/// Callers use this as:
/// ```text
/// .unwrap_or_else(|| emit_ice_unresolved("name", &param.name, param.span, diagnostics))
/// ```
pub(crate) fn emit_ice_unresolved(
    context: &str,
    name: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(
            "internal compiler error: unresolved {context} '{name}' in pass 2"
        ))
        .with_label(DiagnosticLabel::new(
            span,
            "ICE: name should have been registered in pass 1",
        )),
    );
    Type::Real
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_ice_unresolved_pushes_one_error_diagnostic() {
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved("name", "foo", SourceSpan::empty(0), &mut diags);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn emit_ice_unresolved_formats_message_with_context_and_name() {
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved("name", "foo", SourceSpan::empty(0), &mut diags);
        assert_eq!(
            diags[0].message,
            "internal compiler error: unresolved name 'foo' in pass 2"
        );

        let mut diags2: Vec<Diagnostic> = vec![];
        emit_ice_unresolved("guarded member", "bar", SourceSpan::empty(0), &mut diags2);
        assert_eq!(
            diags2[0].message,
            "internal compiler error: unresolved guarded member 'bar' in pass 2"
        );
    }

    #[test]
    fn emit_ice_unresolved_attaches_ice_label_with_span() {
        let expected_span = SourceSpan::new(10, 20);
        let mut diags: Vec<Diagnostic> = vec![];
        emit_ice_unresolved("name", "foo", expected_span, &mut diags);
        assert_eq!(diags[0].labels.len(), 1);
        assert_eq!(diags[0].labels[0].span, expected_span);
        assert_eq!(
            diags[0].labels[0].message,
            "ICE: name should have been registered in pass 1"
        );
    }

    #[test]
    fn emit_ice_unresolved_returns_type_real() {
        let mut diags: Vec<Diagnostic> = vec![];
        let ty = emit_ice_unresolved("name", "x", SourceSpan::empty(0), &mut diags);
        assert_eq!(ty, Type::Real);
    }

    #[test]
    fn emit_ice_unresolved_appends_without_clearing() {
        let mut diags: Vec<Diagnostic> = vec![];
        diags.push(Diagnostic::warning("pre-existing diagnostic"));
        emit_ice_unresolved("name", "y", SourceSpan::empty(0), &mut diags);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].message, "pre-existing diagnostic");
        assert_eq!(diags[0].severity, Severity::Warning);
    }
}
