use crate::SourceSpan;

/// A syntactic identifier paired with its source span.
///
/// Used wherever a name in the source text needs to carry precise location
/// information — e.g. trait refinement entries (`trait Derived : Base`) so
/// that diagnostics can highlight exactly the offending token rather than the
/// surrounding declaration.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpannedIdent {
    /// The identifier text.
    pub name: String,
    /// Byte-offset span of the identifier in the source file.
    pub span: SourceSpan,
}
