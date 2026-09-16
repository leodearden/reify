//! Extract embedded Reify-DSL snippets from Rust test source.
//!
//! Inline `.ri` fixtures — Reify source carried inside a Rust raw-string
//! literal rather than a tracked `.ri` file — are invisible to `git ls-files`,
//! so a corpus survey that enumerates files alone never sees them. This module
//! is the enumeration primitive that closes that gap for the struct-ctor
//! field-type conformance corpus survey (task #7543).
//!
//! Modelled on `crates/reify-builtins/tests/common/seed_name_scan.rs:137`
//! (`strip_comments_and_collect_literals`), the workspace's existing
//! comment-aware Rust mini-lexer, and deliberately its INVERSE: that one BLANKS
//! raw strings so a fixture's text cannot be mistaken for Rust code, while this
//! one COLLECTS exactly those raw strings because their text is the subject.
//! The hazards both must handle are the same — line and NESTING block comments,
//! `r#*"…"#*` with an arbitrary hash count, the raw-IDENTIFIER guard against
//! `r#type`, and char-literal-versus-lifetime disambiguation — which is why the
//! shape is shared rather than reinvented. It is test-private to
//! reify-builtins, so it is a citation and a model, not a call.

use std::path::Path;

/// A raw-string literal found at CODE level in a Rust source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStringLiteral {
    /// 1-based line, within the HOST `.rs` file, of the literal's first CONTENT
    /// line — not of its `r#"` opener.
    pub host_line: u32,
    /// The literal's content, delimiters excluded.
    pub text: String,
}

/// A raw-string literal admitted as embedded Reify source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSnippet {
    /// 1-based line, within the HOST `.rs` file, of the snippet's first line.
    pub host_line: u32,
    /// The snippet's Reify source text.
    pub text: String,
}

/// Every code-level raw-string literal in `rust_source`, in source order.
pub fn raw_string_literals(_rust_source: &str) -> Vec<RawStringLiteral> {
    Vec::new()
}

/// Every raw-string literal in `rust_source` admitted as embedded Reify source.
pub fn inline_ri_snippets(_rust_source: &str) -> Vec<InlineSnippet> {
    Vec::new()
}

/// Whether `text` reads as Reify source rather than some other embedded blob.
pub fn looks_like_reify_source(_text: &str) -> bool {
    false
}

/// Whether a repo-relative path is a Rust file that can host inline fixtures.
pub fn is_inline_fixture_host(_rel: &Path) -> bool {
    false
}
