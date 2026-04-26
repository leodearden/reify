use std::fmt;

/// A byte-offset span in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    /// Byte offset of the start of the span.
    pub start: u32,
    /// Byte offset of the end of the span (exclusive).
    pub end: u32,
}

impl SourceSpan {
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end);
        Self { start, end }
    }

    pub fn empty(offset: u32) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// The raw `usize` byte-offset value that identifies the prelude sentinel.
    ///
    /// Equals `u32::MAX as usize` — the value you get when casting
    /// `SourceSpan::prelude().start` or `SourceSpan::prelude().end` to `usize`.
    /// Both `reify_types::byte_offset_to_line_col` and
    /// `gui::engine::offset_to_line_col_fast` check for this exact value and
    /// return `(1, 1)` without further computation.
    ///
    /// Prefer this constant over a bare `u32::MAX as usize` literal so the
    /// sentinel contract is expressed in one canonical location.
    pub const PRELUDE_SENTINEL_OFFSET: usize = u32::MAX as usize;

    /// A sentinel span used for prelude-originated entries that have no
    /// meaningful byte-offset in the current compilation unit.
    ///
    /// The value `{ start: u32::MAX, end: u32::MAX }` is guaranteed to fall
    /// outside any real source (files are bounded well below 4 GiB in
    /// practice).  Use [`SourceSpan::is_prelude`] to detect this sentinel
    /// before converting offsets to line/column positions.
    ///
    /// # Renderer behaviour
    ///
    /// - Both `reify_types::byte_offset_to_line_col` and the GUI/LSP fast path
    ///   (`gui::engine::offset_to_line_col_fast`) short-circuit the prelude
    ///   sentinel ([`SourceSpan::PRELUDE_SENTINEL_OFFSET`]) to `(1, 1)` — the
    ///   same "no user-file location" fallback used by `mcp_context::get_diagnostics`
    ///   when no labels are present.  This prevents a `debug_assert` panic
    ///   (debug builds) and a silent past-last-line mis-report (release builds).
    /// - Ad-hoc offset converters that do **not** route through one of those
    ///   helpers (e.g. `reify_lsp::convert::offset_to_position`) apply
    ///   `offset.min(source.len())` clamping instead, producing an EOF-position
    ///   rather than `(1, 1)`.  Callers using such converters must guard with
    ///   [`SourceSpan::is_prelude`] before the offset conversion.
    /// - The provenance truth for prelude-originated entries is carried by the
    ///   label *message* (e.g. "defined in stdlib prelude"), not the span
    ///   coordinates.  For explicit control over presentation, check
    ///   [`SourceSpan::is_prelude`] and substitute a "no user-file location"
    ///   message rather than relying on any numeric fallback.
    pub fn prelude() -> Self {
        Self {
            start: u32::MAX,
            end: u32::MAX,
        }
    }

    /// Returns `true` if this span is the prelude sentinel produced by
    /// [`SourceSpan::prelude`].
    pub fn is_prelude(&self) -> bool {
        self.start == u32::MAX && self.end == u32::MAX
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: SourceSpan) -> SourceSpan {
        SourceSpan {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
// Explicit rename keeps wire format PascalCase even if a future variant uses a non-PascalCase identifier.
#[cfg_attr(feature = "serde", serde(rename_all = "PascalCase"))]
pub enum Severity {
    /// Informational note.
    Info,
    /// Warning — something suspicious but not an error.
    Warning,
    /// Error — prevents compilation or evaluation.
    Error,
}

impl Severity {
    /// Canonical wire/log format string for this severity.
    ///
    /// Returns `"Error"`, `"Warning"`, or `"Info"` (PascalCase).
    ///
    /// This is the **single source of truth** for how severity appears in
    /// `DiagnosticInfo.severity` (wire format) and in structured log fields.
    /// It MUST stay in lock-step with the `#[serde(rename_all = "PascalCase")]`
    /// derive on this enum — a feature-gated cross-check in the inline tests
    /// (`#[cfg(feature = "serde")]`) is pinned by a unit test.
    ///
    /// Note: `Display` intentionally keeps lowercase (`"error"`, `"warning"`,
    /// `"info"`) for CLI/human-readable output. Do not change `Display` to
    /// PascalCase — that would silently alter the MCP CLI wire format.
    #[inline]
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
            Severity::Info => "Info",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

/// Typed identifier for the *kind* of diagnostic emitted, decoupled from the
/// human-readable message text.
///
/// Test assertions and downstream tooling (e.g. the MCP wire layer) match on
/// `DiagnosticCode` rather than on substrings of `Diagnostic.message`, so
/// reword-the-message changes do not break tests or downstream consumers.
///
/// `#[non_exhaustive]` lets future variants be added without breaking external
/// match exhaustiveness. The serde derives are feature-gated to mirror the
/// `Severity` enum, and `rename_all = "PascalCase"` keeps the wire identifier
/// stable (e.g. `"TraitNotImplemented"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "PascalCase"))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// Origin: `crates/reify-compiler/src/expr.rs` (instance qualified-access).
    /// Replaces canonical message:
    /// `"sub-component '<name>' (type '<structure>') does not implement trait '<trait>'"`.
    TraitNotImplemented,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs::collect_all_requirements`.
    /// Replaces canonical message: `"unresolved trait: '<name>'"`.
    UnresolvedTrait,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs::collect_all_requirements`.
    /// Replaces canonical message:
    /// `"trait refinement chain too deep (exceeded <N> levels) at '<trait>'"`.
    TraitRefinementChainTooDeep,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Param/Let path).
    /// Replaces canonical message:
    /// `"conflicting trait requirements for '<name>': trait '…' requires …, trait '…' requires …"`.
    ConflictingTraitRequirements,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Sub path).
    /// Replaces canonical message:
    /// `"conflicting trait sub requirements for '<name>': trait '…' requires sub '…', …"`.
    ConflictingTraitSubRequirements,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Let-binding default conflict).
    /// Replaces canonical message:
    /// `"conflicting trait let bindings for '<name>': trait '…' and trait '…' provide different expressions"`.
    ConflictingTraitLetBindings,
    /// Origin: `crates/reify-compiler/src/trait_requirements.rs` (Param/Constraint default conflict).
    /// Replaces canonical message:
    /// `"conflicting trait defaults for '<name>': trait '…' has …, trait '…' has …"`.
    ConflictingTraitDefaults,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "missing required member …".
    MissingRequiredMember,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "missing required sub-component …".
    MissingRequiredSubComponent,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "type mismatch for trait member …".
    TypeMismatchForTraitMember,
    /// Origin: `crates/reify-compiler/src/conformance` (forward-completeness; not yet wired).
    /// Reserved for the conformance-checker producer that emits "type does not conform to trait …".
    TypeNotConformingToTrait,
}

/// A diagnostic message with location and optional labels.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub labels: Vec<DiagnosticLabel>,
    /// Typed kind of this diagnostic. `None` for legacy emissions that have
    /// not yet been migrated. Producers attach a code via [`Diagnostic::with_code`].
    pub code: Option<DiagnosticCode>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            labels: Vec::new(),
            code: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            labels: Vec::new(),
            code: None,
        }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Info,
            message: message.into(),
            labels: Vec::new(),
            code: None,
        }
    }

    pub fn with_label(mut self, label: DiagnosticLabel) -> Self {
        self.labels.push(label);
        self
    }

    /// Attach a typed [`DiagnosticCode`] to this diagnostic.
    ///
    /// Mirrors [`Diagnostic::with_label`]: builder-fluent, takes ownership,
    /// returns `Self`. Callers chain `.with_code(DiagnosticCode::X)` between
    /// `Diagnostic::error(...)` and `.with_label(...)`.
    pub fn with_code(mut self, code: DiagnosticCode) -> Self {
        self.code = Some(code);
        self
    }
}

/// A label pointing to a specific location in source code.
#[derive(Debug, Clone)]
pub struct DiagnosticLabel {
    pub span: SourceSpan,
    pub message: String,
}

impl DiagnosticLabel {
    pub fn new(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// A lightweight reference to a diagnostic (for constraint results etc.)
#[derive(Debug, Clone)]
pub struct DiagnosticRef {
    pub index: usize,
}

#[cfg(test)]
mod tests {
    use super::{Diagnostic, DiagnosticCode, SourceSpan};

    #[test]
    fn prelude_sentinel_is_prelude() {
        assert!(
            SourceSpan::prelude().is_prelude(),
            "SourceSpan::prelude() must satisfy is_prelude()"
        );
    }

    #[test]
    fn empty_zero_is_not_prelude() {
        assert!(
            !SourceSpan::empty(0).is_prelude(),
            "SourceSpan::empty(0) must NOT satisfy is_prelude()"
        );
    }

    #[test]
    fn regular_span_is_not_prelude() {
        assert!(
            !SourceSpan::new(0, 5).is_prelude(),
            "SourceSpan::new(0, 5) must NOT satisfy is_prelude()"
        );
    }

    #[test]
    fn prelude_distinct_from_empty_zero() {
        assert_ne!(
            SourceSpan::prelude(),
            SourceSpan::empty(0),
            "SourceSpan::prelude() must be distinct from SourceSpan::empty(0)"
        );
    }

    /// `Diagnostic::error` defaults `code` to `None` — opt-in via `with_code` only.
    #[test]
    fn diagnostic_default_code_is_none() {
        let d = Diagnostic::error("x");
        assert_eq!(d.code, None);
    }

    /// `with_code` attaches the supplied `DiagnosticCode` and is fluent (returns `Self`).
    #[test]
    fn diagnostic_with_code_attaches_code() {
        let d = Diagnostic::error("x").with_code(DiagnosticCode::TraitNotImplemented);
        assert_eq!(d.code, Some(DiagnosticCode::TraitNotImplemented));
    }

    /// `DiagnosticCode` is `Copy + Clone + PartialEq + Eq + Hash + Debug`.
    /// (Compile-tested by exercising each of those bounds in the body.)
    #[test]
    fn diagnostic_code_derives() {
        use std::collections::HashSet;
        let a = DiagnosticCode::TraitNotImplemented;
        let b: DiagnosticCode = a; // Copy
        let c = a; // Copy again — `a` still usable below
        assert_eq!(a, b); // PartialEq
        assert_eq!(a, c); // PartialEq
        let _: DiagnosticCode = Clone::clone(&a); // Clone (explicit to bypass clippy::clone_on_copy)
        let mut set: HashSet<DiagnosticCode> = HashSet::new();
        assert!(set.insert(a)); // Hash + Eq
        assert!(!set.insert(b)); // dedup on Eq
        let _ = format!("{:?}", a); // Debug
    }

    /// Under `feature = "serde"`, `DiagnosticCode` serializes to its PascalCase variant name.
    #[cfg(feature = "serde")]
    #[test]
    fn diagnostic_code_serde_pascal_case() {
        let s = serde_json::to_string(&DiagnosticCode::TraitNotImplemented).unwrap();
        assert_eq!(s, "\"TraitNotImplemented\"");
    }
}

/// A diagnostic (error/warning) projected to human-readable line/column positions.
///
/// This is a presentation type — it holds 1-based `line`/`column` positions
/// derived from `SourceSpan` byte-offsets via `byte_offset_to_line_col`.
/// It lives in reify-types (not reify-mcp) so that the engine layer can produce
/// it without importing from the MCP adapter layer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiagnosticInfo {
    pub file_path: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub message: String,
    pub code: Option<String>,
}
