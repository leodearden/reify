use crate::diagnostics::SourceSpan;

/// A compiled annotation — resolved from a parsed `@name(args...)` syntax annotation.
///
/// Annotations carry compile-time metadata through to downstream consumers
/// (LSP hover, deprecation warnings, test discovery, etc.).
#[derive(Debug, Clone)]
pub struct Annotation {
    pub name: String,
    pub args: Vec<AnnotationArg>,
    pub span: SourceSpan,
}

/// A resolved annotation argument value.
///
/// Annotation args are compile-time constants, not runtime expressions.
/// Complex expressions in annotation positions are rejected during lowering.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationArg {
    String(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    Ident(String),
}
