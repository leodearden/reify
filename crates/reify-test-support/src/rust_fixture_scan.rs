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
//! # Measured basis
//!
//! Over the tree at `cb868ea32e`, [`is_inline_fixture_host`] admits **1,870**
//! of the 1,932 tracked `.rs` — every one under `crates/`, none of which
//! carries a `target` component — leaving 62 excluded, all under the two
//! out-of-scope roots named on the predicate. Across the admitted hosts
//! [`raw_string_literals`] collects **3,734** literals, of which
//! [`looks_like_reify_source`] admits **3,306**, [`is_format_template`] holds
//! back **72**, and **356** are dropped as not Reify at all (JSON payloads,
//! Rust-source fixtures, expected-diagnostic prose — re-checked at this
//! commit, not carried over). **496** hosts carry at least one admitted
//! snippet.
//!
//! Widening the host predicate from the file-NAME scope it started with added
//! 563 hosts carrying 231 literals, of which 179 were admitted, 1 held back as
//! a template and 51 dropped. The drop share therefore moved 8.7% → 9.5%,
//! which is the proportionate movement a corpus change makes; the newly
//! admitted snippets are `#[cfg(test)]`-module fixtures that read as ordinary
//! Reify declarations. Whether any of them fail to COMPILE is not restated
//! here: the survey artifact's own inline-coverage section counts them, per
//! member, with a machine-derived reason.
//!
//! Those are MEASUREMENTS at a named commit, not invariants — recorded so a
//! future reader can tell a filter regression (the admitted share collapses)
//! from an ordinary corpus change (every figure drifts together).
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
///
/// Composes the collector with both admission predicates. A literal that is not
/// Reify at all is dropped outright — there is nothing to survey and nothing to
/// disclose — while a Reify-SHAPED `format!` template is held back in
/// [`InlineScan::format_templates`] so the caller reports it under its own
/// coverage reason instead of letting it land as a noise `parse-error` row.
pub fn inline_ri_snippets(rust_source: &str) -> InlineScan {
    let mut scan = InlineScan::default();
    for lit in raw_string_literals(rust_source) {
        if !looks_like_reify_source(&lit.text) {
            continue;
        }
        let snippet = InlineSnippet {
            host_line: lit.host_line,
            text: lit.text,
        };
        if is_format_template(&snippet.text) {
            scan.format_templates.push(snippet);
        } else {
            scan.snippets.push(snippet);
        }
    }
    scan
}

/// Reify DECLARATION openers, matched at the start of a line.
///
/// `structure ` covers both `structure def X` and the inline `structure X {`;
/// `trait `, `occurrence ` and `constraint ` likewise cover their `def` and
/// inline forms. `param` is handled separately because it needs its `:` to be
/// distinguishable from ordinary prose.
const REIFY_DECLARATION_OPENERS: &[&str] = &[
    "module ",
    "import ",
    "structure ",
    "occurrence ",
    "constraint ",
    "trait ",
    "enum ",
    "purpose ",
    "#precision",
];

/// Whether `text` reads as Reify source rather than some other embedded blob.
///
/// LINE-ANCHORED declaration grammar, never a bare substring test: a raw string
/// is admitted only when some line OPENS a Reify declaration. A substring
/// heuristic such as `contains("let ")` admits the eight Rust-source fixtures in
/// `crates/reify-builtins/tests/common/seed_name_scan.rs`, whose `let` and
/// `match` lines are Rust; compiling those would poison the census with rows
/// describing no Reify site at all.
pub fn looks_like_reify_source(text: &str) -> bool {
    text.lines().any(opens_a_reify_declaration)
}

fn opens_a_reify_declaration(line: &str) -> bool {
    let mut rest = line.trim_start();
    // `pub` / `priv` visibility and `@attr` annotations may precede the keyword
    // in either order. Each strip consumes at least two bytes, so this
    // terminates.
    while let Some(next) = ["pub ", "priv "]
        .iter()
        .find_map(|p| rest.strip_prefix(p))
        .or_else(|| strip_attribute(rest))
    {
        rest = next.trim_start();
    }
    if let Some(after) = rest.strip_prefix("param ") {
        return after.contains(':');
    }
    REIFY_DECLARATION_OPENERS
        .iter()
        .any(|opener| rest.starts_with(opener))
}

/// `rest` past a leading `@attr` annotation, or `None` when there is none.
fn strip_attribute(rest: &str) -> Option<&str> {
    let after_at = rest.strip_prefix('@')?;
    let end = after_at
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(after_at.len(), |(i, _)| i);
    (end > 0).then(|| &after_at[end..])
}

/// Whether `text` is a `format!` template rather than compilable Reify source.
///
/// Two tells, both taken from the live shape at
/// `crates/reify-compiler/tests/ambient_default_injection_tests.rs:136-144`:
/// doubled `{{`/`}}` braces (how a template escapes a literal brace, which
/// Reify never writes), and a bare `{ident}` substitution. An EMPTY `{}` body
/// is ordinary Reify, so a placeholder must name something.
pub fn is_format_template(text: &str) -> bool {
    text.contains("{{") || text.contains("}}") || has_named_placeholder(text)
}

fn has_named_placeholder(text: &str) -> bool {
    let bytes = text.as_bytes();
    for open in 0..bytes.len() {
        if bytes[open] != b'{' {
            continue;
        }
        let is_name_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut close = open + 1;
        while close < bytes.len() && is_name_byte(bytes[close]) {
            close += 1;
        }
        if close > open + 1 && close < bytes.len() && bytes[close] == b'}' {
            return true;
        }
    }
    false
}

/// Whether a repo-relative path is a Rust source file that can host inline
/// fixtures: every tracked `.rs` under `crates/`, build output excluded.
///
/// PATH SHAPE ONLY, and that is the design. What makes a raw string a fixture
/// is its CONTENT — [`looks_like_reify_source`]'s job — so a file hosting no
/// Reify simply contributes zero snippets, at the cost of one lexer pass. A
/// file-NAME scope was the alternative, and it measurably under-reaches: a
/// `tests`-directory-or-exactly-`tests.rs` clause admitted 1,307 of the 1,932
/// tracked `.rs`, missing 10 of the 12 `crates/*/src/**/*tests.rs` hosts and
/// every `#[cfg(test)] mod tests` inside a production `src/*.rs` — 409 of the
/// 561 tracked `crates/*/src/**/*.rs` carry one. Keeping the admission in one
/// content filter is what keeps the scope SPOT instead of a hand-maintained
/// list of file-name shapes.
///
/// Deliberately NOT [`crate::ignore_hygiene::walk_test_rs_files`], the
/// workspace's other test-file enumerator: its `has_tests_component` predicate
/// is private, so the shapes above cannot be reached by composing with it, and
/// it is a filesystem walk where the survey needs a git-index enumeration — an
/// untracked `.rs` is reproducible from no commit, and the artifact is stamped
/// against one.
///
/// Two roots are excluded by DECISION rather than oversight: `gui/src-tauri/**`
/// (the Tauri sidecar) and `tree-sitter-reify/**` (the grammar crate) are
/// separate cargo and grammar projects — 62 tracked `.rs` between them, 43 and
/// 19. The survey artifact discloses both by name in its residual-scope
/// limitation, so the boundary is readable rather than inferred from this
/// predicate's source.
pub fn is_inline_fixture_host(rel: &Path) -> bool {
    let mut names: Vec<&str> = Vec::new();
    for component in rel.components() {
        match component {
            std::path::Component::Normal(name) => match name.to_str() {
                Some(name) => names.push(name),
                None => return false,
            },
            // A root, prefix, `.` or `..` component means this is not the
            // repo-relative path the predicate is defined over.
            _ => return false,
        }
    }
    let Some((file, dirs)) = names.split_last() else {
        return false;
    };
    file.ends_with(".rs") && dirs.first() == Some(&"crates") && !dirs.contains(&"target")
}
