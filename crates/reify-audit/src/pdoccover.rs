//! PDOCCOVER — bidirectional registry↔chunk name-drift detector.
//!
//! ## Purpose
//!
//! The MCP language-reference chunks (`crates/reify-mcp/src/tools/chunks/*.md`)
//! are the surface a design-authoring agent reads before writing `.ri` source.
//! They drift from the compiler in BOTH directions, and both directions cost
//! design-author time:
//!
//! - **Omission** — a builtin the compiler recognises is documented nowhere,
//!   so the agent never learns it exists.
//! - **Fabrication** — a chunk documents a call that the compiler does *not*
//!   recognise, so the agent writes source that cannot compile.
//!
//! PDOCCOVER detects both from ONE pass. The two directions share the chunk
//! read: a single read of the chunk corpus feeds both the documented-name
//! index (omission) and the call-shaped-mention list (fabrication).
//!
//! Reference: `docs/prds/v0_6/doc-chunk-truth-enforcement.md` §(b) / leaf γ.
//!
//! ## Direction 1 — omission
//!
//! Census: the `*_NAMES` string-slice registries declared in
//! `crates/reify-compiler/src/units.rs` (the PRD-pinned corpus). Each name is
//! compliant iff exactly one of:
//!
//! 1. **documented** — word-boundary match in ≥1 `chunks/*.md`;
//! 2. **allowed** — its registry entry line carries
//!    `// pdoccover:allow — <reason>` (reason mandatory);
//! 3. **baselined** — listed in `crates/reify-audit/pdoccover-baseline.txt`.
//!
//! Anything else is an offender.
//!
//! ## Direction 2 — fabrication
//!
//! Reverse census: every **call-shaped** name mentioned in a chunk (an
//! identifier immediately followed by `(`) must exist somewhere in the
//! compiler/stdlib sources. Names that do not are fabrications.
//!
//! ## Finding categories
//!
//! All five ride at [`Severity::High`] under the single [`Pattern::PDocCover`]
//! variant, carried as a stable summary prefix (PTODO's `kind`-as-prefix
//! convention, `lib.rs` §PTodo):
//!
//! | Prefix | Meaning |
//! |---|---|
//! | `undocumented-name:` | registry name with no chunk mention, no allow, no baseline |
//! | `fabricated-name:` | chunk documents a call-shaped name that exists nowhere in source |
//! | `stale-baseline-entry:` | baselined name that IS documented — ratchet honesty |
//! | `stale-allow-entry:` | allow-marked name that IS documented — ratchet honesty |
//! | `allow-missing-reason:` | `pdoccover:allow` with no reason body — confers NO exemption |
//!
//! ## Escape hatch
//!
//! `// pdoccover:allow — <reason>` on the registry entry line (units.rs side)
//! or on the mentioning line (chunk side). The **prefixed** token is the only
//! form consumed; the bare `doccover:allow` of earlier PRD drafts was renamed
//! 2026-07-25 for uniformity with `ptodo:allow` / `ds-sentinel:allow` and is
//! deliberately NOT honoured. The reason body is mandatory: a reasonless
//! marker emits `allow-missing-reason:` and confers no exemption, so the
//! escape hatch can never become un-reviewable (PRD design decision 7).
//!
//! ## Scope
//!
//! Working-tree reads only — `ls_files()` + `std::fs`, never jcodemunch, and
//! deliberately no `reify-compiler` dependency (the audit crate stays a pure
//! text scanner, PRD §(b)). No regex crate: the audit crate has none and must
//! not gain one.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};
use std::collections::BTreeSet;

// -----------------------------------------------------------------------
// Paths
// -----------------------------------------------------------------------

/// The omission-census registry source (PRD-pinned).
pub const UNITS_PATH: &str = "crates/reify-compiler/src/units.rs";

/// The documentation chunk corpus directory prefix.
pub const CHUNKS_PREFIX: &str = "crates/reify-mcp/src/tools/chunks/";

/// Ratchet baseline. **Seeded by #5480, not by this task** — absent or empty
/// is a supported state and yields an empty allow-set with no error.
pub const BASELINE_PATH: &str = "crates/reify-audit/pdoccover-baseline.txt";

// -----------------------------------------------------------------------
// Registry model
// -----------------------------------------------------------------------

/// One name inside a `*_NAMES` registry, with its 1-based source line and the
/// reason body of a `// pdoccover:allow — <reason>` marker on that line
/// (`None` when the line carries no marker *or* a reasonless one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub name: String,
    pub line: usize,
    pub allow: Option<String>,
    /// `true` when the line carries a `pdoccover:allow` token whose reason
    /// body is blank/absent — the `allow-missing-reason` trigger.
    pub allow_missing_reason: bool,
}

/// One `pub const <IDENT>_NAMES: &[&str] = &[…];` registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub const_name: String,
    pub entries: Vec<RegistryEntry>,
}

// -----------------------------------------------------------------------
// Pure scanners (stubs — filled in by the TDD steps that follow)
// -----------------------------------------------------------------------

/// The `pdoccover:allow` escape token. **Prefixed form only** — the bare
/// `doccover:allow` of earlier PRD drafts was renamed 2026-07-25 and is
/// deliberately not consumed by any code path.
const ALLOW_TOKEN: &str = "pdoccover:allow";

/// The identifier suffix that marks a const as a builtin-name registry.
const REGISTRY_SUFFIX: &str = "_NAMES";

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the alphabet for
/// the hand-rolled `\b` word-boundary checks, matching `ptodo::is_word_byte`
/// so `union` is never satisfied by `disunion` / `union_all` / `reunion`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Strip a trailing `// …` line comment from `line`, returning the code part.
///
/// Quote-aware: a `//` inside a double-quoted string literal is NOT a comment
/// start. Without this, an entry line whose trailing comment itself contains a
/// quoted token (`"box", // renamed from "cube"`) would contribute a phantom
/// registry entry.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str => i += 1, // skip the escaped byte
            b'"' => in_str = !in_str,
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                return &line[..i];
            }
            _ => {}
        }
        i += 1;
    }
    line
}

/// Strip `/* … */` block-comment spans from `line`, tracking open state across
/// lines via `in_block`.
///
/// Returns the code outside any block comment, with the commented spans
/// replaced by a space so bracket depth and quoted-token extraction both see
/// only real code. Without this a `/* … "phantom" … */` span inside a registry
/// body would contribute a name that exists nowhere in the compiler.
///
/// Quote-aware in the same sense as [`strip_line_comment`]: a `/*` inside a
/// string literal does not open a block comment. Nested block comments (legal
/// in Rust) are depth-counted.
/// Operates on BYTES, accumulating into a `Vec<u8>` rather than slicing the
/// `&str`. `units.rs` doc-comments contain non-ASCII (`§`, `→`, em dashes), and
/// any `&line[i..i+1]`-style slice would panic on a multibyte char boundary.
/// Every retained byte is copied verbatim, so multibyte sequences survive
/// intact; every dropped span is delimited by ASCII `/*`/`*/`/`//`, so a drop
/// can never split a char.
fn strip_block_comments(line: &str, depth: &mut usize) -> String {
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        if *depth > 0 {
            if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                *depth += 1;
                i += 2;
                continue;
            }
            if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                *depth -= 1;
                out.push(b' ');
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        match bytes[i] {
            b'\\' if in_str => {
                // Copy the backslash and the first byte of the escaped char.
                // A multibyte escaped char's continuation bytes are copied by
                // the default arm on subsequent iterations.
                out.push(bytes[i]);
                if i + 1 < bytes.len() {
                    out.push(bytes[i + 1]);
                }
                i += 2;
                continue;
            }
            b'"' => in_str = !in_str,
            // A `//` line comment outside a string ends the line; everything
            // after it (including a `/*`) is comment text, never code.
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => break,
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                *depth += 1;
                out.push(b' ');
                i += 2;
                continue;
            }
            _ => {}
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Lossy is unreachable in practice (every retained byte comes from a valid
    // &str and sequences are never split), but it keeps this infallible rather
    // than panicking inside a fail-safe detector.
    String::from_utf8_lossy(&out).into_owned()
}

/// Byte offset of a word-boundary-delimited [`ALLOW_TOKEN`] in `line`.
///
/// Left-boundary-checked so the token is never matched as the tail of a longer
/// word (`xxpdoccover:allow`). The legacy unprefixed `doccover:allow` is a
/// *suffix* of the prefixed token, so it simply never matches — the 2026-07-25
/// rename means no code path consumes it.
fn find_allow_token(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut start = 0;
    loop {
        let rel = line[start..].find(ALLOW_TOKEN)?;
        let idx = start + rel;
        if idx == 0 || !is_word_byte(bytes[idx - 1]) {
            return Some(idx);
        }
        start = idx + ALLOW_TOKEN.len();
    }
}

/// `true` when `line` carries a `pdoccover:allow` marker at all — with or
/// without a reason body.
///
/// Paired with [`allow_marker_reason`]: present-but-reasonless is exactly the
/// `allow-missing-reason` trigger, and a reasonless marker confers no
/// exemption (PRD design decision 7 — no un-reviewable escape hatch).
fn allow_marker_present(line: &str) -> bool {
    find_allow_token(line).is_some()
}

/// `true` when `line` (already comment-stripped) is an attribute line such as
/// `#[cfg(feature = "x")]` or `#![allow(…)]`.
///
/// Attributes are skipped wholesale inside a registry body and while looking
/// for the opening `&[`: their `[`/`]` would corrupt bracket depth and their
/// string arguments (`"x"`) would enter the census as phantom names.
fn is_attribute_line(code: &str) -> bool {
    let t = code.trim_start();
    t.starts_with("#[") || t.starts_with("#![")
}

/// Extract every double-quoted token from `s`, honouring backslash escapes.
fn quoted_tokens(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j <= bytes.len() {
                out.push(s[start..j.min(bytes.len())].to_string());
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Registry-header match: returns the const identifier when `line` declares a
/// `[pub[(…)]] const <IDENT>_NAMES: &[&str] = …`.
///
/// Accepts `pub const`, `pub(crate) const` and bare `const`, and requires the
/// `&[&str]` element type so tuple-slice consts (the real `SI_PREFIXES:
/// &[(&str, f64)]`) are excluded — their quoted tokens are not builtin names.
fn registry_header(line: &str) -> Option<&str> {
    let s = line.trim_start();
    // Optional visibility prefix.
    let s = if let Some(rest) = s.strip_prefix("pub") {
        let rest = rest.trim_start();
        // `pub(crate)` / `pub(super)` / `pub(in …)` — skip the paren group.
        if rest.starts_with('(') {
            let close = rest.find(')')?;
            rest[close + 1..].trim_start()
        } else {
            rest
        }
    } else {
        s
    };
    let s = s.strip_prefix("const")?;
    // `const` must be a whole word.
    if !s.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let s = s.trim_start();
    let colon = s.find(':')?;
    let ident = s[..colon].trim();
    if ident.is_empty()
        || !ident.ends_with(REGISTRY_SUFFIX)
        || !ident.bytes().all(is_word_byte)
    {
        return None;
    }
    // Element type must be `&[&str]` — excludes `&[(&str, f64)]` etc.
    let rest: String = s[colon + 1..].chars().filter(|c| !c.is_whitespace()).collect();
    if !rest.starts_with("&[&str]") {
        return None;
    }
    Some(ident)
}

/// Extract every `*_NAMES` string-slice registry from `units_src`.
///
/// Pure `&str -> Vec<Registry>`, no IO, so the grammar is unit-testable
/// without disk access (the `pdssentinel.rs` `scan_content` split).
///
/// Line walk, no regex (the audit crate has no regex dep and must not gain
/// one). Every line is first reduced to its CODE part in one forward pass —
/// `/* … */` block-comment spans (tracked across lines), then the `// …` tail,
/// then attribute lines blanked wholesale. The header is recognised on that
/// code view — possibly with `&[` broken onto a following line, the real
/// `GEOMETRY_KINEMATIC_QUERY_NAMES` shape — after which quoted tokens are
/// accumulated per line until bracket depth returns to zero.
///
/// The comment/attribute reduction is what keeps a phantom name out of the
/// census: a quoted token inside a `//` comment, a `/* … */` span, or an
/// attribute's arguments (`#[cfg(feature = "x")]`) is not code, and a name
/// that entered the census from one of those could never be satisfied by any
/// chunk edit. Bracket depth is counted on the same reduced view, so an
/// attribute's `[`/`]` cannot desynchronise the body scan. Nested `&[…]`
/// inside an entry value is handled by the depth counter itself.
// G-allow: consumed by check()/baseline_candidates() in-module, and by the registry-path brittle-parse floor guard in tests/pdoccover.rs — a separate crate, so pub is required.
pub fn extract_registries(units_src: &str) -> Vec<Registry> {
    let lines: Vec<&str> = units_src.lines().collect();

    // One forward pass reducing every line to its code part. Block-comment
    // depth must carry across lines, so this cannot be done lazily per line.
    let mut block_depth = 0usize;
    let code_lines: Vec<String> = lines
        .iter()
        .map(|raw| {
            let no_block = strip_block_comments(raw, &mut block_depth);
            let code = strip_line_comment(&no_block).to_string();
            if is_attribute_line(&code) {
                String::new()
            } else {
                code
            }
        })
        .collect();

    let mut out: Vec<Registry> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let Some(ident) = registry_header(&code_lines[i]) else {
            i += 1;
            continue;
        };
        let const_name = ident.to_string();

        // Locate the opening `&[`. It is either on the header line (after the
        // `=`) or on a following line (the line-broken header form, possibly
        // with doc-comment or attribute lines between). Scan forward a bounded
        // distance so a malformed declaration cannot run away over the file.
        const HEADER_LOOKAHEAD: usize = 4;
        let mut open = None;
        for (k, code) in code_lines.iter().enumerate().skip(i).take(HEADER_LOOKAHEAD) {
            // On the header line, only look after the `=` — the `&[&str]` type
            // annotation's own brackets are not the registry's opening bracket.
            let hay = if k == i {
                match code.find('=') {
                    Some(eq) => &code[eq..],
                    None => continue,
                }
            } else {
                code.as_str()
            };
            if hay.contains('[') {
                open = Some(k);
                break;
            }
        }
        let Some(open_line) = open else {
            i += 1;
            continue;
        };

        // Accumulate entries until bracket depth returns to zero.
        let mut entries: Vec<RegistryEntry> = Vec::new();
        let mut depth: i32 = 0;
        let mut j = open_line;
        loop {
            if j >= lines.len() {
                break;
            }
            let raw = lines[j];
            // On the header line, restrict to the part after `=` so the
            // `&[&str]` type annotation's brackets are not counted.
            let code = if j == i {
                match code_lines[j].find('=') {
                    Some(eq) => &code_lines[j][eq..],
                    None => code_lines[j].as_str(),
                }
            } else {
                code_lines[j].as_str()
            };

            // The header line's own quoted tokens (single-line form) count as
            // entries; a comment-only or attribute line yields none because its
            // code view is empty. The allow marker is read from the RAW line —
            // markers live in comments, which the code view has removed.
            let reason = allow_marker_reason(raw);
            let has_marker = allow_marker_present(raw);
            for name in quoted_tokens(code) {
                if name.is_empty() {
                    continue;
                }
                entries.push(RegistryEntry {
                    name,
                    line: j + 1,
                    allow: reason.map(str::to_string),
                    allow_missing_reason: has_marker && reason.is_none(),
                });
            }

            depth += code.matches('[').count() as i32;
            depth -= code.matches(']').count() as i32;
            if depth <= 0 && j >= open_line {
                break;
            }
            j += 1;
        }

        out.push(Registry {
            const_name,
            entries,
        });
        i = j + 1;
    }

    out
}

/// Reason body of a `pdoccover:allow` marker on `line`, or `None` when the
/// line carries no marker or the body is blank after trimming.
///
/// Contract mirrors `ptodo::g_allow_marker_body`: locate the token, skip one
/// optional separator (em dash `—`, ASCII hyphen `-`, or colon `:`), and
/// return the trimmed remainder only when it is non-blank. A `None` return on
/// a line that DOES carry the token is what the caller turns into an
/// `allow-missing-reason` finding — a reasonless escape hatch is never
/// silently honoured (PRD design decision 7).
///
/// Only the prefixed [`ALLOW_TOKEN`] is recognised. Because the legacy
/// unprefixed `doccover:allow` is a suffix of the prefixed form, the match is
/// left-boundary-checked: the byte before the token must not be a word byte,
/// so `// doccover:allow — x` does NOT match `pdoccover:allow`… and neither
/// does any other suffix collision.
// G-allow: consumed in-module by extract_registries/chunk_call_mentions and by unit tests; pub for symmetry with ptodo::g_allow_marker_body, whose contract it mirrors.
pub fn allow_marker_reason(line: &str) -> Option<&str> {
    let idx = find_allow_token(line)?;
    let body = &line[idx + ALLOW_TOKEN.len()..];
    let body = body.trim_start();
    // One optional separator.
    let body = body
        .strip_prefix('—')
        .or_else(|| body.strip_prefix('-'))
        .or_else(|| body.strip_prefix(':'))
        .unwrap_or(body);
    let body = body.trim();
    if body.is_empty() { None } else { Some(body) }
}

/// Subset of `names` that is word-boundary-mentioned in ≥1 chunk source.
///
/// `chunk_sources` are pre-read `(path, content)` pairs so the matcher stays
/// pure and unit-testable without disk access.
// G-allow: consumed by check()/baseline_candidates() in-module and by unit tests.
pub fn documented_names(_names: &[String], _chunk_sources: &[(String, String)]) -> BTreeSet<String> {
    BTreeSet::new()
}

/// Existence oracle for the fabrication direction: every name that the
/// compiler/stdlib sources evidence as real.
// G-allow: consumed by check() in-module and by unit tests.
pub fn known_name_index(_sources: &[(String, String)]) -> BTreeSet<String> {
    BTreeSet::new()
}

/// Call-shaped API mentions in one chunk: `(name, 1-based line)`.
// G-allow: consumed by check() in-module, and by the chunk-path brittle-parse floor guard in tests/pdoccover.rs — a separate crate, so pub is required.
pub fn chunk_call_mentions(_content: &str) -> Vec<(String, usize)> {
    Vec::new()
}

// -----------------------------------------------------------------------
// check() — entry point
// -----------------------------------------------------------------------

/// Run both drift directions over the working tree.
///
/// Findings are deterministically ordered. Unreadable files are skipped
/// fail-safe (no finding, no panic).
pub fn check(_ctx: &AuditContext<'_>) -> Vec<Finding> {
    Vec::new()
}

/// The single shared derivation #5480's baseline regenerator consumes: the
/// sorted, deduped set of names that [`check`] reports as `undocumented-name:`.
///
/// Exported so generation and the ratchet can never disagree (PRD §6.6's
/// `ptodo-baseline-gen` lesson). This task ships NO `--emit-baseline` flag,
/// NO `pdoccover-baseline-gen` binary and NO baseline file — all three are
/// #5480's deliverables.
// G-allow: #5480's entry point (PRD open question 1); no in-repo caller yet by design.
pub fn baseline_candidates(_ctx: &AuditContext<'_>) -> Vec<String> {
    Vec::new()
}

// Silence unused-import warnings while the stubs are empty; every one of these
// is consumed once the emission steps land.
const _: fn() = || {
    let _ = |p: Pattern, s: Severity, e: EvidenceRef| (p, s, e);
};

// -----------------------------------------------------------------------
// Unit tests — pure scan grammar
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Names of the registries `extract_registries` found, in discovery order.
    fn const_names(regs: &[Registry]) -> Vec<&str> {
        regs.iter().map(|r| r.const_name.as_str()).collect()
    }

    /// `(name, line)` pairs of every entry in `reg`.
    fn entry_pairs(reg: &Registry) -> Vec<(&str, usize)> {
        reg.entries
            .iter()
            .map(|e| (e.name.as_str(), e.line))
            .collect()
    }

    /// Just the names of every entry in `reg`.
    fn entry_names(reg: &Registry) -> Vec<&str> {
        reg.entries.iter().map(|e| e.name.as_str()).collect()
    }

    // ── step-1 (a): single-line registry form ────────────────────────────

    /// The one-liner shape `pub const X_NAMES: &[&str] = &["nominal"];` (the
    /// real `TOLERANCING_MARKER_NAMES`) yields one registry with one entry,
    /// whose line is the declaration line.
    #[test]
    fn extract_registries_single_line_form() {
        let src = r#"
/// Tolerancing markers.
pub const TOLERANCING_MARKER_NAMES: &[&str] = &["nominal"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["TOLERANCING_MARKER_NAMES"],
            "single-line registry must be discovered; got: {regs:?}"
        );
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("nominal", 3)],
            "single-line registry must yield its one entry at the declaration \
             line; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (b): multi-line block form ────────────────────────────────

    /// The block shape (the real `GEOMETRY_FUNCTION_NAMES`) — one name per
    /// line, trailing comma — yields every name with correct 1-based lines.
    #[test]
    fn extract_registries_multi_line_block_form() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "cylinder",
    "sphere",
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "block-form registry must be discovered; got: {regs:?}"
        );
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("box", 2), ("cylinder", 3), ("sphere", 4)],
            "block-form entries must carry correct 1-based line numbers; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (c): line-broken header form ──────────────────────────────

    /// The real `GEOMETRY_KINEMATIC_QUERY_NAMES` shape puts `=` at the end of
    /// the declaration line and `&[` on the NEXT line. It must be parsed, not
    /// skipped — skipping it would silently drop a whole registry from the
    /// census, exactly the brittle-parse failure the step-3 guard defends.
    #[test]
    fn extract_registries_line_broken_header_form() {
        let src = r#"pub const GEOMETRY_KINEMATIC_QUERY_NAMES: &[&str] =
    &["interferes", "interferes_with", "min_clearance"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_KINEMATIC_QUERY_NAMES"],
            "line-broken header form must be discovered, not skipped; got: {regs:?}"
        );
        assert_eq!(
            entry_names(&regs[0]),
            vec!["interferes", "interferes_with", "min_clearance"],
            "line-broken header form must yield all three names; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (d): visibility prefixes ──────────────────────────────────

    /// `pub(crate) const` and bare `const` are accepted alongside `pub const`.
    #[test]
    fn extract_registries_accepts_visibility_prefixes() {
        let src = r#"pub(crate) const ALPHA_NAMES: &[&str] = &["a"];
const BETA_NAMES: &[&str] = &["b"];
pub const GAMMA_NAMES: &[&str] = &["c"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["ALPHA_NAMES", "BETA_NAMES", "GAMMA_NAMES"],
            "pub(crate)/bare/pub const prefixes must all be accepted; got: {regs:?}"
        );
    }

    // ── step-1 (e): non-`_NAMES` consts ignored ──────────────────────────

    /// A const whose identifier does not end in `_NAMES` is not a builtin-name
    /// registry and must be ignored — including tuple-slice consts like the
    /// real `SI_PREFIXES`, whose quoted tokens are not builtin names.
    #[test]
    fn extract_registries_ignores_non_names_consts() {
        let src = r#"pub const SI_PREFIXES: &[(&str, f64)] = &[("kilo", 1e3), ("milli", 1e-3)];
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &["box"];
pub const SOME_OTHER: &[&str] = &["not_a_registry"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "only `*_NAMES` string-slice consts are registries; got: {regs:?}"
        );
    }

    // ── step-1 (f): comment lines inside the block ───────────────────────

    /// `//` and `///` comment lines inside a registry block contribute no
    /// entry — including a comment that itself contains a quoted token, which
    /// must never be mistaken for a registry name.
    #[test]
    fn extract_registries_skips_comment_lines_in_block() {
        let src = r#"pub const GEOMETRY_QUERY_HELPER_NAMES: &[&str] = &[
    // Task 2320: the original trio (see "watertight" discussion).
    "is_watertight",
    /// doc-comment mentioning "is_phantom"
    "is_manifold",
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["is_watertight", "is_manifold"],
            "comment lines (and quoted tokens inside them) must contribute no \
             entries; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-1 (g): pdoccover:allow marker on an entry line ──────────────

    /// An entry line carrying `// pdoccover:allow — <reason>` records the
    /// trimmed reason body in `allow`, and the marker text itself is never
    /// mistaken for an entry.
    #[test]
    fn extract_registries_records_allow_reason() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    "lower_shim", // pdoccover:allow — geometry-only internal
];
"#;
        let regs = extract_registries(src);
        let entries = &regs[0].entries;
        assert_eq!(
            entries.len(),
            2,
            "allow-marked line yields exactly one entry (the name), not the \
             marker text; got: {entries:?}"
        );
        assert_eq!(entries[0].name, "box");
        assert_eq!(
            entries[0].allow, None,
            "unmarked entry must carry allow=None; got: {:?}",
            entries[0]
        );
        assert_eq!(entries[1].name, "lower_shim");
        assert_eq!(
            entries[1].allow.as_deref(),
            Some("geometry-only internal"),
            "allow-marked entry must carry the trimmed reason body; got: {:?}",
            entries[1]
        );
        assert!(
            !entries[1].allow_missing_reason,
            "a marker WITH a reason must not set allow_missing_reason; got: {:?}",
            entries[1]
        );
    }

    // ── step-1 (h): two names on one line ────────────────────────────────

    /// Two names on one line are both extracted, sharing that line number
    /// (the real `DYNAMICS_CONSTRUCTOR_NAMES` shape).
    #[test]
    fn extract_registries_two_names_one_line() {
        let src =
            r#"pub const DYNAMICS_CONSTRUCTOR_NAMES: &[&str] = &["mass_properties", "point_mass"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("mass_properties", 1), ("point_mass", 1)],
            "both names on one line must be extracted with that shared line \
             number; got: {:?}",
            regs[0].entries
        );
    }

    // ── step-2: allow_marker_reason grammar ──────────────────────────────

    /// The reason separator accepts an em dash, an ASCII hyphen or a colon —
    /// and a bare space also works. The body is returned trimmed.
    ///
    /// Three separators rather than only the PRD's literal em dash: a gate
    /// that fails on an invisible typographic difference would be a trap. The
    /// marker token plus a non-blank reason carry the normative weight.
    #[test]
    fn allow_marker_reason_accepts_three_separators() {
        for line in [
            r#"    "x", // pdoccover:allow — internal lowering shim"#,
            r#"    "x", // pdoccover:allow - internal lowering shim"#,
            r#"    "x", // pdoccover:allow: internal lowering shim"#,
            r#"    "x", // pdoccover:allow   internal lowering shim  "#,
        ] {
            assert_eq!(
                allow_marker_reason(line),
                Some("internal lowering shim"),
                "marker reason must be extracted and trimmed for line: {line:?}"
            );
        }
    }

    /// A reasonless marker (bare, or a separator with a blank body) yields
    /// `None` — which is what makes `allow-missing-reason` fall out naturally,
    /// mirroring `ptodo::g_allow_marker_body`'s blank-body contract.
    #[test]
    fn allow_marker_reason_none_for_blank_body() {
        for line in [
            r#"    "x", // pdoccover:allow"#,
            r#"    "x", // pdoccover:allow —"#,
            r#"    "x", // pdoccover:allow:   "#,
        ] {
            assert_eq!(
                allow_marker_reason(line),
                None,
                "a reasonless marker must yield None for line: {line:?}"
            );
        }
    }

    /// The legacy UNPREFIXED `doccover:allow` (earlier PRD drafts, renamed
    /// 2026-07-25 for uniformity with `ptodo:allow`) is NOT honoured — only
    /// the prefixed token is consumed.
    #[test]
    fn allow_marker_reason_rejects_legacy_unprefixed_token() {
        assert_eq!(
            allow_marker_reason(r#"    "x", // doccover:allow — legacy form"#),
            None,
            "the legacy unprefixed `doccover:allow` must confer nothing"
        );
    }

    /// A line with no marker at all yields `None`.
    #[test]
    fn allow_marker_reason_none_without_marker() {
        assert_eq!(allow_marker_reason(r#"    "box","#), None);
    }

    // ── step-4: hardening against the real units.rs shapes ──────────────

    /// A `#[…]` attribute line inside a registry body contributes NO entry and
    /// does not desynchronise bracket depth — its `[`/`]` are not the
    /// registry's, and its string arguments are not builtin names.
    #[test]
    fn extract_registries_ignores_attribute_lines() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    #[cfg(feature = "experimental")]
    "cylinder",
];
pub const AFTER_NAMES: &[&str] = &["sphere"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES", "AFTER_NAMES"],
            "an attribute's brackets must not desynchronise the body scan, so \
             the FOLLOWING registry is still discovered; got: {regs:?}"
        );
        assert_eq!(
            entry_names(&regs[0]),
            vec!["box", "cylinder"],
            "the attribute's own string argument (\"experimental\") must not \
             enter the census; got: {:?}",
            regs[0].entries
        );
    }

    /// A `/* … */` block comment inside a registry body contributes no entry,
    /// including when it spans multiple lines and contains a quoted token.
    #[test]
    fn extract_registries_skips_block_comments() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box",
    /* temporarily withdrawn:
       "phantom_op",
       "other_phantom", */
    "sphere",
    "cone", /* trailing "inline_phantom" */
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["box", "sphere", "cone"],
            "quoted tokens inside block comments must contribute no entries; \
             got: {:?}",
            regs[0].entries
        );
    }

    /// A trailing `//` comment containing a quoted token contributes no entry —
    /// the comment strip is quote-aware, so only real code is scanned.
    #[test]
    fn extract_registries_skips_quoted_token_in_trailing_comment() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "box", // renamed from "cube" in #1234
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["box"],
            "a quoted token in a trailing comment must contribute no entry; \
             got: {:?}",
            regs[0].entries
        );
    }

    /// A `//` sequence INSIDE a string literal does not start a comment — the
    /// rest of the line is still code.
    #[test]
    fn extract_registries_slashes_inside_string_are_not_a_comment() {
        let src = r#"pub const PATH_NAMES: &[&str] = &["a//b", "real_name"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["a//b", "real_name"],
            "a `//` inside a string literal must not truncate the line; got: {:?}",
            regs[0].entries
        );
    }

    /// A doc-comment line between the header and the opening `&[` does not
    /// prevent discovery (the bounded header lookahead skips it).
    #[test]
    fn extract_registries_doc_comment_between_header_and_bracket() {
        let src = r#"pub const GEOMETRY_KINEMATIC_QUERY_NAMES: &[&str] =
    // interference + clearance helpers
    &["interferes", "min_clearance"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_KINEMATIC_QUERY_NAMES"],
            "a comment between the header and `&[` must not prevent discovery; \
             got: {regs:?}"
        );
        assert_eq!(
            entry_names(&regs[0]),
            vec!["interferes", "min_clearance"],
            "entries must still be extracted; got: {:?}",
            regs[0].entries
        );
    }

    /// Non-ASCII text in doc-comments and entry values must not panic the
    /// byte-level comment stripper.
    ///
    /// Regression: the first block-comment implementation sliced the `&str`
    /// with `&line[i..i+1]`-style indices and panicked with "byte index N is
    /// not a char boundary" on the real `units.rs`, whose doc-comments contain
    /// `§`, `→` and em dashes. The hermetic fixtures above were all-ASCII and
    /// missed it; the step-3 floor guard against the real file caught it.
    #[test]
    fn extract_registries_handles_non_ascii_source() {
        let src = "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n\
             \x20   // See PRD \u{a7}8 \u{2014} the box \u{2192} solid lowering.\n\
             \x20   \"box\", // pdoccover:allow \u{2014} r\u{e9}serv\u{e9}\n\
             \x20   /* withdrawn \u{a7}9: \"phantom\" */\n\
             \x20   \"sphere\",\n\
             ];\n";
        let regs = extract_registries(src);
        assert_eq!(
            entry_names(&regs[0]),
            vec!["box", "sphere"],
            "non-ASCII comment text must be stripped without panicking and \
             without contributing entries; got: {:?}",
            regs[0].entries
        );
        assert_eq!(
            regs[0].entries[0].allow.as_deref(),
            Some("r\u{e9}serv\u{e9}"),
            "a non-ASCII allow reason must round-trip intact; got: {:?}",
            regs[0].entries[0]
        );
    }

    /// Two registries in sequence are both discovered — the scan resumes after
    /// the first one's closing bracket rather than swallowing the second.
    #[test]
    fn extract_registries_consecutive_registries_both_found() {
        let src = r#"pub const FIRST_NAMES: &[&str] = &[
    "a",
];

pub const SECOND_NAMES: &[&str] = &[
    "b",
];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["FIRST_NAMES", "SECOND_NAMES"],
            "both consecutive registries must be discovered; got: {regs:?}"
        );
    }

    /// `allow_missing_reason` is set on a registry entry whose line carries a
    /// reasonless marker (and the marker confers no reason).
    #[test]
    fn extract_registries_flags_reasonless_allow_marker() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[
    "lower_shim", // pdoccover:allow
];
"#;
        let regs = extract_registries(src);
        let e = &regs[0].entries[0];
        assert_eq!(e.name, "lower_shim");
        assert_eq!(
            e.allow, None,
            "a reasonless marker confers no reason; got: {e:?}"
        );
        assert!(
            e.allow_missing_reason,
            "a reasonless marker must set allow_missing_reason; got: {e:?}"
        );
    }
}
