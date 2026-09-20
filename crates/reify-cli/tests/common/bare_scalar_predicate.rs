//! Bare-`Scalar` detection predicate — SINGLE SOURCE OF TRUTH.
//!
//! Pulled in by `#[path]` — source inclusion, never a Cargo dependency edge
//! (why: `crates/reify-spec-conformance/src/lib.rs`, Obligation 1) — from
//! `../harness_cli/corpus_no_bare_scalar.rs`, which owns this predicate and
//! carries its unit tests, and from
//! `crates/reify-spec-conformance/tests/fixture_tree.rs`, whose include is a
//! RELATIVE PATH ACROSS A CRATE BOUNDARY.
//!
//! That out-of-crate includer is why this file sits in `tests/common/` — the
//! retained sibling the harness-layout contract
//! (`tests/infra/test_harness_kloc_cap.sh`) deliberately never moves — rather
//! than under `tests/harness_cli/`, which that same contract moves files into
//! and splits back out of (`harness_cli_surface` is one such split). A move
//! there would break the other crate's include with a bare "couldn't read".
//! It is deliberately NOT declared from `common/mod.rs`: both consumers
//! `#[path]`-include it directly, and a `pub mod` there would compile it into
//! every harness that uses `common`.
//!
//! Walked by the guard's own `crates/**/*.rs` sweep and deliberately NOT on
//! its self-exclusion list: it carries no bare annotation on a non-comment
//! line. Write violating examples in `predicate_tests` (whose file IS
//! self-excluded), never here.
//!
//! Signal: `: *Scalar([^<a-zA-Z]|$)` (annotation) or `-> Scalar([^<a-zA-Z]|$)`
//! (codomain), with pure-comment lines and `::Scalar` excluded. Rationale for
//! every carve-out lives on the three items below.

/// Strip a trailing `//` line comment from `line`, returning the portion before
/// the first comment marker.  `://` (URL schemes that may appear in string
/// literals) are preserved — only `//` not immediately preceded by `:` is
/// treated as a comment start.
///
/// Limitation: `/* … */` block comments are **not** stripped.  No such block
/// comment with a bare-`Scalar` mention exists in the scanned corpus today.
fn strip_trailing_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Preserve `://` (URL scheme) inside string literals.
            if i == 0 || bytes[i - 1] != b':' {
                return &line[..i];
            }
        }
        i += 1;
    }
    line
}

/// Returns `true` when the `Scalar` at byte offset `abs` in `line` is the Rust
/// pretty-`Debug` (`{:#?}`) rendering of the `reify_ir::Value::Scalar` ENUM
/// VARIANT as a struct field — i.e. the whole line is `<indent><ident>: Scalar {`.
///
/// `#[derive(Debug)]` prints a struct-like variant UNQUALIFIED, eliding the
/// `Value::` prefix that [`line_has_bare_scalar`]'s `::Scalar` rule keys on, so
/// a `{:#?}` golden of a LENGTH-dimensioned IR field (task 5743's R7 raw-`Value`
/// chokepoint) reads as `width: Scalar {`. Those are Rust type names in a
/// snapshot string, never DSL type annotations, so they are excluded for the
/// same reason `Value::Scalar` already is.
///
/// Matched WHOLE-LINE and narrowly so DSL forms never match; `predicate_tests`
/// pins both directions, including `structure def X : Scalar {` — the one DSL
/// shape that is also `: <Type> {`, and which still matches.
fn is_rust_debug_scalar_field(line: &str, abs: usize) -> bool {
    // Tail must be exactly ` {` (trailing whitespace tolerated).
    if line[abs + 6..].trim_end() != " {" {
        return false;
    }
    // Head must be `<whitespace><ident>: `.
    let Some(head) = line[..abs].strip_suffix(": ") else {
        return false;
    };
    let ident = head.trim_start_matches([' ', '\t']);
    !ident.is_empty()
        && ident.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Returns `true` when `line` contains a bare `: Scalar` type annotation or a
/// bare `-> Scalar` return codomain that must be migrated.
///
/// Matches:
///   * `: *Scalar([^<a-zA-Z]|$)` where the introducing `:` is **not**
///     preceded by another `:` (i.e., `::Scalar` Rust enum paths excluded).
///   * `-> Scalar([^<a-zA-Z]|$)` — bare return codomain.
///
/// Pure comment lines (trimmed starts with `//`) are always skipped, and any
/// trailing line comment is stripped before scanning — only real source /
/// inline-DSL string content is examined.
pub fn line_has_bare_scalar(line: &str) -> bool {
    // Skip pure comment lines — doc-prose mentioning `-> Scalar` or `: Scalar`
    // in comments must not be treated as migration violations.
    if line.trim_start().starts_with("//") {
        return false;
    }

    // Strip any trailing line comment before scanning.  This prevents a
    // migrated line like `-> Length { } // was -> Scalar` from being falsely
    // flagged due to the `-> Scalar` mention in the comment.
    let line = strip_trailing_line_comment(line);

    let mut search_start = 0;
    while let Some(rel) = line[search_start..].find("Scalar") {
        let abs = search_start + rel;

        // 1. Check character immediately after "Scalar" — must not be `<` or ASCII letter.
        let after_ok = match line[abs + 6..].chars().next() {
            None => true, // end of string / line
            Some(c) => c != '<' && !c.is_ascii_alphabetic(),
        };

        // 1b. A Rust `{:#?}` Debug struct-field opener (`width: Scalar {`) is
        //     the unqualified rendering of the `Value::Scalar` enum variant —
        //     excluded for the same reason `::Scalar` is (see the helper).
        if after_ok && !is_rust_debug_scalar_field(line, abs) {
            // 2. Scan backwards from `abs`, skipping spaces, to find the
            //    preceding non-space character.  It must be:
            //    (a) a single `:` NOT preceded by another `:` → bare annotation, OR
            //    (b) `->` → bare return codomain.
            let before = &line[..abs];
            let before_trimmed = before.trim_end_matches(' ');
            // (a) annotation: ends_with(':') but NOT ends_with("::") → bare colon annotation
            if before_trimmed.ends_with(':') && !before_trimmed.ends_with("::") {
                return true;
            }
            // (b) codomain: ends_with("->") → bare return type
            if before_trimmed.ends_with("->") {
                return true;
            }
        }

        search_start = abs + 6;
    }
    false
}
