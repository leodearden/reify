//! Parser diagnostics (`E_GcodeParseError` per PRD §1).
//!
//! Every error carries the 1-indexed source line number plus a
//! dialect-specific reason. Populated by the TDD steps in
//! `docs/prds/v0_3/trajectory-input-shaping.md` §11 task μ.

/// A parse failure with source-line provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    /// 1-indexed source line number where the failure occurred. The
    /// counter advances on every physical line (blank, comment-only, and
    /// content alike) so error messages line up with editor line numbers.
    pub line: usize,
    pub kind: ParseErrorKind,
}

/// Dialect-agnostic reason classification for a parse failure.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseErrorKind {
    /// Leading token did not match a recognised G-code or M-code for this
    /// dialect (e.g. `G99`, `M999`, or a bare word like `Garbage`).
    UnknownCommand(String),
    /// A parameter `<letter><value>` failed to parse as `f64`.
    InvalidParameter { letter: char, value: String },
    /// A non-blank line had no recognisable command token (currently
    /// unused by `parse_marlin`; reserved for future stricter dialects).
    MissingCommand,
}
