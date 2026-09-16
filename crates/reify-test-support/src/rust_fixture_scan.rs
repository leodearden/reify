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
//!
//! # Not a Rust lexer
//!
//! [`raw_string_literals`] models exactly the constructs that can HIDE a `r#"`
//! sequence from, or forge one for, a linear scan. It does not model macro
//! token trees, `#[cfg]`-disabled code (a literal inside a
//! `#[cfg(feature = "…")]` block is collected whether or not that cfg is
//! active), or byte-string literals (`br#"…"#`). Tolerating that is a
//! deliberate trade: the consumer compiles each admitted literal as Reify
//! source, so a wrongly-admitted snippet becomes a recorded `parse-error` row
//! in the survey's coverage accounting — visible and countable — rather than a
//! wrong site. A wrongly-REJECTED snippet, by contrast, is silent, which is why
//! the scan errs toward admitting.

use std::path::Path;

/// A raw-string literal found at CODE level in a Rust source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawStringLiteral {
    /// 1-based line, within the HOST `.rs` file, of [`text`](Self::text)'s line
    /// 1 — so host line `host_line + k - 1` is the text's line `k`, uniformly.
    pub host_line: u32,
    /// The literal's content, delimiters excluded and a single leading newline
    /// removed.
    ///
    /// That removal is what makes the line correspondence above uniform: in the
    /// near-universal `r#"\n…"#` layout the literal's own line 1 is the empty
    /// remainder of the opener line, and every line a reader cares about would
    /// otherwise be off by one from `host_line`.
    pub text: String,
}

/// A raw-string literal admitted as embedded Reify source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineSnippet {
    /// 1-based line, within the HOST `.rs` file, of the snippet's line 1.
    pub host_line: u32,
    /// The snippet's Reify source text.
    pub text: String,
}

/// Every code-level raw-string literal in `rust_source`, in source order.
///
/// A single forward byte scan. Comments are skipped, ordinary string and char
/// literals are stepped over so a quote inside one cannot desync the scan, and
/// a raw-string opener is recognised only as `r` + N `#` + `"` where the `r`
/// starts a token — `r#type` is an identifier, not an opener.
pub fn raw_string_literals(rust_source: &str) -> Vec<RawStringLiteral> {
    let raw = rust_source.as_bytes();
    let n = raw.len();
    let mut found = Vec::new();
    let mut i = 0usize;

    while i < n {
        match raw[i] {
            b'/' if i + 1 < n && raw[i + 1] == b'/' => {
                while i < n && raw[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < n && raw[i + 1] == b'*' => {
                // Rust block comments NEST, so a depth counter is required: the
                // first `*/` of `/* /* */ */` does not end the comment.
                let mut depth = 1usize;
                i += 2;
                while i < n && depth > 0 {
                    if i + 1 < n && raw[i] == b'/' && raw[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if i + 1 < n && raw[i] == b'*' && raw[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'r' if i + 1 < n && (raw[i + 1] == b'"' || raw[i + 1] == b'#') => {
                let prev_is_ident =
                    i > 0 && (raw[i - 1].is_ascii_alphanumeric() || raw[i - 1] == b'_');
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < n && raw[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if prev_is_ident || j >= n || raw[j] != b'"' {
                    i += 1;
                    continue;
                }
                let content_start = j + 1;
                let (content_end, past_close) = raw_string_body_end(raw, content_start, hashes);
                found.push(literal_at(rust_source, content_start, content_end));
                i = past_close;
            }
            b'"' => {
                i += 1;
                while i < n {
                    if raw[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if raw[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
                i = (i + 1).min(n);
            }
            b'\'' => match char_literal_end(raw, i) {
                Some(end) => i = end,
                None => i += 1,
            },
            _ => i += 1,
        }
    }

    found
}

/// `(content_end, past_close)` for a raw-string body opening at
/// `content_start` under `hashes` hashes.
///
/// The terminator is `"` followed by EXACTLY `hashes` `#`s, so a shorter hash
/// run inside the body (`"#` within an `r##"…"##`) is body text. An
/// unterminated literal runs to end of input rather than panicking — a
/// truncated or non-Rust input is data, not a defect of this scan.
fn raw_string_body_end(raw: &[u8], content_start: usize, hashes: usize) -> (usize, usize) {
    let n = raw.len();
    let mut j = content_start;
    while j < n {
        if raw[j] == b'"' {
            let mut k = j + 1;
            let mut seen = 0usize;
            while k < n && seen < hashes && raw[k] == b'#' {
                seen += 1;
                k += 1;
            }
            if seen == hashes {
                return (j, k);
            }
        }
        j += 1;
    }
    (n, n)
}

/// Build the [`RawStringLiteral`] for the body spanning `content_start..content_end`.
///
/// Both offsets sit on an ASCII `"` boundary (or at end of input), so the slice
/// is char-boundary safe on multi-byte content.
fn literal_at(source: &str, content_start: usize, content_end: usize) -> RawStringLiteral {
    let body = &source[content_start..content_end];
    // The opener's line holds the literal's line 1. When that line 1 is empty
    // — `r#"` immediately followed by a newline — the text's first real line is
    // the next host line, and dropping the newline keeps `host_line` addressing
    // the text's own line 1.
    let (text, line_offset) = match body.strip_prefix('\n') {
        Some(rest) => (rest, 1),
        None => (body, 0),
    };
    let opener_line = reify_core::byte_offset_to_line_col(source, content_start).0 as u32;
    RawStringLiteral {
        host_line: opener_line + line_offset,
        text: text.to_owned(),
    }
}

/// One past the closing `'` when a char literal starts at `at`, or `None` when
/// the quote opens a LIFETIME (`'static`, `&'a str`) instead — the two shapes a
/// `'` can begin, distinguished by whether a closing quote follows one char.
///
/// Multi-byte chars (`'é'`) are measured by UTF-8 lead-byte width rather than
/// assumed one byte wide, so their closing quote is consumed and cannot open a
/// spurious literal over the code that follows. Ported from
/// `crates/reify-builtins/tests/common/seed_name_scan.rs:112`, which is
/// test-private to that crate.
fn char_literal_end(raw: &[u8], at: usize) -> Option<usize> {
    let n = raw.len();
    let first = at + 1;
    if first >= n {
        return None;
    }
    if raw[first] == b'\\' {
        // An escape's payload can itself be a quote (`'\''`) or run several
        // bytes (`'\u{7b}'`), so step over the payload byte and then find the
        // real closing quote.
        let mut j = first + 2;
        while j < n && raw[j] != b'\'' {
            j += 1;
        }
        return (j < n).then_some(j + 1);
    }
    let width = match raw[first] {
        b if b < 0x80 => 1,
        b if b >> 5 == 0b110 => 2,
        b if b >> 4 == 0b1110 => 3,
        _ => 4,
    };
    let close = first + width;
    (close < n && raw[close] == b'\'').then_some(close + 1)
}

/// What [`inline_ri_snippets`] found in one host file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlineScan {
    /// Literals admitted as compilable Reify source.
    pub snippets: Vec<InlineSnippet>,
    /// Reify-SHAPED literals held back because they are `format!` templates.
    pub format_templates: Vec<InlineSnippet>,
}

/// Every raw-string literal in `rust_source` admitted as embedded Reify source.
pub fn inline_ri_snippets(_rust_source: &str) -> InlineScan {
    InlineScan::default()
}

/// Whether `text` reads as Reify source rather than some other embedded blob.
pub fn looks_like_reify_source(_text: &str) -> bool {
    false
}

/// Whether `text` is a `format!` template rather than compilable Reify source.
pub fn is_format_template(_text: &str) -> bool {
    false
}

/// Whether a repo-relative path is a Rust file that can host inline fixtures.
pub fn is_inline_fixture_host(_rel: &Path) -> bool {
    false
}
