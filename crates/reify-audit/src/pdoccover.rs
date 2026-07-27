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

/// `true` when `needle` occurs in `haystack` delimited by word boundaries on
/// BOTH sides — a hand-rolled `\b<needle>\b`.
///
/// The boundary alphabet is [`is_word_byte`]'s `[A-Za-z0-9_]`, matching
/// `ptodo.rs`. Both sides matter: `union`, `union_all` and `intersection` are
/// all real registry entries, so a one-sided match would let `union_all`'s
/// documentation silently vouch for `union` and under-report coverage.
///
/// Case-sensitive — Reify builtin names are snake_case, and a prose `Union` is
/// not the builtin. `needle` is assumed identifier-shaped (ASCII), so byte
/// indexing is safe even when `haystack` contains multibyte characters: a
/// match can only start and end at ASCII byte positions.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hb = haystack.as_bytes();
    let mut start = 0;
    while let Some(rel) = haystack[start..].find(needle) {
        let idx = start + rel;
        let after = idx + needle.len();
        let left_ok = idx == 0 || !is_word_byte(hb[idx - 1]);
        let right_ok = after >= hb.len() || !is_word_byte(hb[after]);
        if left_ok && right_ok {
            return true;
        }
        // Advance past this occurrence's first byte, not past the whole
        // needle: overlapping occurrences must still be considered.
        start = idx + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// Subset of `names` that is word-boundary-mentioned in ≥1 chunk source.
///
/// `chunk_sources` are pre-read `(path, content)` pairs so the matcher stays
/// pure and unit-testable without disk access.
///
/// Deliberately format-agnostic: no markdown parser, no heading/fence/table
/// awareness. A name mentioned anywhere in any chunk — code span, fence,
/// heading, table cell, bold-prefixed prose, bare prose — counts as
/// documented. PRD §(b) disposition 1 asks for exactly that, and it is what
/// makes the index immune to a chunk reformat.
// G-allow: consumed by check()/baseline_candidates() in-module and by unit tests.
pub fn documented_names(names: &[String], chunk_sources: &[(String, String)]) -> BTreeSet<String> {
    names
        .iter()
        .filter(|name| {
            chunk_sources
                .iter()
                .any(|(_, content)| contains_word(content, name))
        })
        .cloned()
        .collect()
}

/// `true` when `s` is identifier-shaped end-to-end: `[A-Za-z_][A-Za-z0-9_]*`.
///
/// The admission test for existence evidence. Without it a message template
/// (`"unresolved type: {}"`), a phrase or a chunk id would enter the oracle and
/// arbitrary prose could vouch for a fabricated call.
fn is_identifier_shaped(s: &str) -> bool {
    let mut bytes = s.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
        _ => return false,
    }
    bytes.all(is_word_byte)
}

/// Leading `[A-Za-z0-9_]` run of `tok` — the identifier at the head of a token
/// like `revolute(a:` or `MaterialFrame`.
fn leading_ident(tok: &str) -> &str {
    let end = tok
        .bytes()
        .position(|b| !is_word_byte(b))
        .unwrap_or(tok.len());
    &tok[..end]
}

/// Declaration keywords that introduce a name in `.ri` source.
///
/// `structure` and `occurrence` are the two-word forms (`structure def X`) and
/// are handled by consuming the `def` token before the name.
const RI_DECL_KEYWORDS: &[&str] = &[
    "fn",
    "unit",
    "type",
    "constraint",
    "purpose",
    "enum",
    "trait",
    "joint",
    "structure",
    "occurrence",
];

/// Name declared by a `.ri` line, if it is a declaration at all.
///
/// `[pub] <keyword> [def] <name>…`, where the name is the leading identifier
/// run of the following token — `joint revolute(a: Axis)` declares `revolute`.
fn ri_declared_name(code: &str) -> Option<&str> {
    let mut toks = code.split_whitespace();
    let mut kw = toks.next()?;
    if kw == "pub" {
        kw = toks.next()?;
    }
    if !RI_DECL_KEYWORDS.contains(&kw) {
        return None;
    }
    if (kw == "structure" || kw == "occurrence") && toks.next()? != "def" {
        return None;
    }
    let ident = leading_ident(toks.next()?);
    if is_identifier_shaped(ident) {
        Some(ident)
    } else {
        None
    }
}

/// Existence oracle for the fabrication direction: every name that the
/// compiler/stdlib sources evidence as real.
///
/// ## Deliberately broader than the omission census
///
/// The census is the PRD-pinned `units.rs` registries; the oracle is every
/// shred of evidence across the compiler and the stdlib. It HAS to be broader:
/// `clamp`, `lerp` and `dot` are legitimately documented builtins declared in
/// `math_signatures.rs`, not in any `*_NAMES` registry, and flagging them as
/// fabrications would make the gate unusable. The trade is one-way and
/// intentional — a false NEGATIVE (a fabricated name colliding with an
/// unrelated literal) is acceptable; a false POSITIVE is not.
///
/// ## Two lanes, by source language
///
/// - **`.rs`** — every identifier-shaped double-quoted literal outside a
///   comment. This strictly SUBSUMES the `*_NAMES` registry entries (each is
///   such a literal in an in-scope file), so there is one code path rather
///   than a union that could drift. It also admits attribute arguments and
///   unrelated literals; that is over-broadness in the acceptable direction.
/// - **`.ri`** — declaration lines, because the language's own grammar has no
///   Rust literal to harvest.
///
/// Comments are excluded in both lanes: a name discussed in a doc-comment
/// ("formerly known as `offset_surface`") is precisely the fabrication case,
/// and letting the discussion vouch for it would disarm the lane for exactly
/// the names people write about.
// G-allow: consumed by check() in-module and by unit tests.
pub fn known_name_index(sources: &[(String, String)]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (path, content) in sources {
        let is_ri = path.ends_with(".ri");
        let mut block_depth = 0usize;
        for raw in content.lines() {
            let no_block = strip_block_comments(raw, &mut block_depth);
            let code = strip_line_comment(&no_block);
            if is_ri {
                if let Some(name) = ri_declared_name(code) {
                    out.insert(name.to_string());
                }
            } else {
                for tok in quoted_tokens(code) {
                    if is_identifier_shaped(&tok) {
                        out.insert(tok);
                    }
                }
            }
        }
    }
    out
}

/// `true` when `path` may carry evidence that a builtin name exists.
///
/// Three roots, and the exclusions matter as much as the inclusions:
/// - the chunks themselves are excluded, or every fabrication would vouch for
///   itself and the lane would report clean by construction;
/// - `tests/` and fixtures are excluded because they routinely name things
///   that were proposed and never implemented — the exact class the lane
///   exists to catch;
/// - docs are excluded for the same reason as chunks.
// Exercised by the unit tests now; `check()` consumes it when the fabrication
// lane lands, at which point this attribute goes away. Scoped to `not(test)`
// rather than a bare `allow(dead_code)` so the lint keeps working under
// `cargo test` — a blanket allow here would also mask a genuinely orphaned
// helper later.
#[cfg_attr(not(test), allow(dead_code))]
fn in_oracle_scope(path: &str) -> bool {
    (path.starts_with("crates/reify-compiler/src/") && path.ends_with(".rs"))
        || (path.starts_with("crates/reify-compiler/stdlib/") && path.ends_with(".ri"))
        || (path.starts_with("crates/reify-stdlib/src/") && path.ends_with(".rs"))
}

/// Call-shaped API mentions in one chunk: `(name, 1-based line)`.
// G-allow: consumed by check() in-module, and by the chunk-path brittle-parse floor guard in tests/pdoccover.rs — a separate crate, so pub is required.
pub fn chunk_call_mentions(_content: &str) -> Vec<(String, usize)> {
    Vec::new()
}

// -----------------------------------------------------------------------
// Working-tree inputs
// -----------------------------------------------------------------------

/// One distinct census name, merged across every registry that declares it.
///
/// A name may legitimately appear in more than one registry (a query helper
/// that is also a topology selector, say). The census is keyed by NAME, not by
/// declaration site, so such a name yields at most one finding: reporting the
/// same undocumented name twice would make the ratchet count depend on an
/// internal `units.rs` factoring detail.
#[derive(Debug, Clone)]
struct CensusName {
    name: String,
    /// First registry that declares it — provenance for the finding text.
    const_name: String,
    /// 1-based line of that first declaration.
    line: usize,
    /// Reason body from the first declaring line that carries a well-formed
    /// `pdoccover:allow — <reason>`.
    allow: Option<String>,
    /// `true` when ANY declaring line carries a reasonless marker.
    allow_missing_reason: bool,
}

/// Distinct census names in name-sorted order.
///
/// Sorting here (rather than at emission) is what makes the finding list
/// deterministic irrespective of declaration order inside `units.rs`.
fn census_names(registries: &[Registry]) -> Vec<CensusName> {
    let mut by_name: std::collections::BTreeMap<String, CensusName> =
        std::collections::BTreeMap::new();
    for reg in registries {
        for entry in &reg.entries {
            match by_name.get_mut(&entry.name) {
                Some(existing) => {
                    // Merge: the first well-formed reason wins; a reasonless
                    // marker anywhere is still worth reporting.
                    if existing.allow.is_none() {
                        existing.allow.clone_from(&entry.allow);
                    }
                    existing.allow_missing_reason |= entry.allow_missing_reason;
                }
                None => {
                    by_name.insert(
                        entry.name.clone(),
                        CensusName {
                            name: entry.name.clone(),
                            const_name: reg.const_name.clone(),
                            line: entry.line,
                            allow: entry.allow.clone(),
                            allow_missing_reason: entry.allow_missing_reason,
                        },
                    );
                }
            }
        }
    }
    by_name.into_values().collect()
}

/// Everything both lanes read from the working tree, gathered in ONE pass.
///
/// The chunk corpus is read exactly once and shared: the omission lane matches
/// census names against it, and the fabrication lane extracts call-shaped
/// mentions from the same strings.
struct Inputs {
    /// `*_NAMES` registries from `units.rs`; empty when it is untracked or
    /// unreadable (fail-safe: a missing census reports nothing, it does not
    /// report everything).
    registries: Vec<Registry>,
    /// Pre-read `(path, content)` for every tracked `chunks/*.md`, path-sorted.
    chunk_sources: Vec<(String, String)>,
    /// Names listed in the ratchet baseline; empty when the file is absent,
    /// untracked, unreadable or empty.
    baseline: BTreeSet<String>,
}

/// Names listed in a `pdoccover-baseline.txt`.
///
/// One name per line. Blank lines and `#` comment lines are skipped, so the
/// file can carry a regeneration header. An empty file yields an empty set —
/// indistinguishable from an absent one, which is exactly PRD leaf γ's
/// "baseline may be empty/absent at this stage" contract.
fn parse_baseline(content: &str) -> BTreeSet<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Read `path` under `ctx.project_root`, or `None` when unreadable.
///
/// Fail-safe by construction: an unreadable file is skipped, never escalated
/// into a finding and never a panic (`pdssentinel::check`'s contract).
fn read_relative(ctx: &AuditContext<'_>, path: &str) -> Option<String> {
    std::fs::read_to_string(ctx.project_root.join(path)).ok()
}

/// `true` when `path` is a documentation chunk in the MCP corpus.
fn is_chunk_path(path: &str) -> bool {
    path.starts_with(CHUNKS_PREFIX) && path.ends_with(".md")
}

/// Gather the working-tree inputs.
///
/// Path membership comes from `ctx.git.ls_files()` — content from `std::fs`,
/// mirroring `pdssentinel::check`. Only tracked files participate, so a stray
/// untracked `units.rs.orig` or a scratch chunk never perturbs the census.
fn load_inputs(ctx: &AuditContext<'_>) -> Inputs {
    let mut tracked: Vec<String> = ctx.git.ls_files();
    tracked.sort();

    let registries = if tracked.iter().any(|p| p == UNITS_PATH) {
        read_relative(ctx, UNITS_PATH)
            .map(|src| extract_registries(&src))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let chunk_sources: Vec<(String, String)> = tracked
        .iter()
        .filter(|p| is_chunk_path(p))
        .filter_map(|p| read_relative(ctx, p).map(|c| (p.clone(), c)))
        .collect();

    let baseline = if tracked.iter().any(|p| p == BASELINE_PATH) {
        read_relative(ctx, BASELINE_PATH)
            .map(|c| parse_baseline(&c))
            .unwrap_or_default()
    } else {
        BTreeSet::new()
    };

    Inputs {
        registries,
        chunk_sources,
        baseline,
    }
}

// -----------------------------------------------------------------------
// Finding emission
// -----------------------------------------------------------------------

/// A finding plus its `(category, name)` sort key.
///
/// The key is carried alongside rather than re-parsed out of the summary so
/// the ordering contract cannot silently drift if a summary is ever reworded.
struct Keyed {
    category: &'static str,
    name: String,
    finding: Finding,
}

/// Build one PDOCCOVER finding. `detail` is appended after `<category>: <name>`.
fn keyed(category: &'static str, name: &str, path: &str, detail: String) -> Keyed {
    Keyed {
        category,
        name: name.to_string(),
        finding: Finding {
            pattern: Pattern::PDocCover,
            severity: Severity::High,
            task_id: path.to_string(),
            summary: format!("{category}: {name} {detail}"),
            evidence: vec![EvidenceRef::File {
                path: path.to_string(),
            }],
        },
    }
}

/// Omission lane, including its two ratchet-honesty siblings.
///
/// ## Exemption precedence — at most ONE finding per census name
///
/// A name that trips several conditions is reported once, under the category
/// naming the edit that resolves it. Emitting two findings for one defect
/// would inflate the ratchet count and make the same fix look like partial
/// progress:
///
/// 1. **Reasonless marker** → `allow-missing-reason:`, and the marker confers
///    no exemption. Checked FIRST because a malformed marker is a defect
///    whatever the name's documentation status, and because this finding
///    subsumes the `undocumented-name:` the name would otherwise earn — the
///    one edit that adds a reason resolves both readings.
/// 2. **Documented** → not an omission. But a suppression channel still
///    pointed at it is now dead weight, so an allow marker yields
///    `stale-allow-entry:` and a baseline entry yields
///    `stale-baseline-entry:`. Allow is checked first: it is the cheaper
///    deletion and lives next to the name. A name that is somehow both loses
///    the allow marker first and surfaces again next run for the baseline
///    entry — one finding, one edit, converging.
/// 3. **Undocumented** → a well-formed allow marker or a baseline entry
///    exempts it; otherwise `undocumented-name:`.
fn omission_findings(inputs: &Inputs) -> Vec<Keyed> {
    let census = census_names(&inputs.registries);
    let names: Vec<String> = census.iter().map(|c| c.name.clone()).collect();
    let documented = documented_names(&names, &inputs.chunk_sources);

    let mut out = Vec::new();
    for c in &census {
        let name = c.name.as_str();
        let declared_at = format!("{} ({UNITS_PATH}:{})", c.const_name, c.line);

        // (1) A malformed escape hatch is a defect on its own terms.
        if c.allow_missing_reason {
            out.push(keyed(
                "allow-missing-reason",
                name,
                UNITS_PATH,
                format!(
                    "— `{ALLOW_TOKEN}` on {declared_at} has no reason body, so it \
                     grants no exemption; write `// {ALLOW_TOKEN} — <reason>` or \
                     document the name under {CHUNKS_PREFIX}",
                ),
            ));
            continue;
        }

        // (2) Documented: the name is covered, so any surviving suppression
        // channel is stale.
        if documented.contains(name) {
            if let Some(reason) = &c.allow {
                out.push(keyed(
                    "stale-allow-entry",
                    name,
                    UNITS_PATH,
                    format!(
                        "— {declared_at} is documented under {CHUNKS_PREFIX}, so its \
                         `{ALLOW_TOKEN} — {reason}` marker is obsolete; delete the marker",
                    ),
                ));
            } else if inputs.baseline.contains(name) {
                out.push(keyed(
                    "stale-baseline-entry",
                    name,
                    BASELINE_PATH,
                    format!(
                        "— documented under {CHUNKS_PREFIX} but still listed in \
                         {BASELINE_PATH}; delete the line so the ratchet keeps \
                         meaning residual debt",
                    ),
                ));
            }
            continue;
        }

        // (3) Undocumented: a well-formed suppression channel exempts it.
        if c.allow.is_some() || inputs.baseline.contains(name) {
            continue;
        }
        out.push(keyed(
            "undocumented-name",
            name,
            UNITS_PATH,
            format!(
                "— declared in {declared_at}, mentioned in no chunk under \
                 {CHUNKS_PREFIX}; document it, or mark the entry line \
                 `// {ALLOW_TOKEN} — <reason>`",
            ),
        ));
    }
    out
}

// -----------------------------------------------------------------------
// check() — entry point
// -----------------------------------------------------------------------

/// Run both drift directions over the working tree.
///
/// Findings are deterministically ordered by `(category, name)` — the category
/// prefixes sort lexicographically, so the emitted list is byte-identical
/// between runs over an unchanged tree and diffs cleanly between runs over a
/// changed one. Unreadable files are skipped fail-safe (no finding, no panic).
pub fn check(ctx: &AuditContext<'_>) -> Vec<Finding> {
    let inputs = load_inputs(ctx);
    let mut keyed = omission_findings(&inputs);
    keyed.sort_by(|a, b| a.category.cmp(b.category).then_with(|| a.name.cmp(&b.name)));
    keyed.into_iter().map(|k| k.finding).collect()
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

    // ── step-5: chunk documentation-mention index ───────────────────────

    /// Build the `(path, content)` pair list the chunk matchers consume.
    fn sources(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect()
    }

    /// Convenience: is `name` documented by these chunk sources?
    fn documented(name: &str, srcs: &[(String, String)]) -> bool {
        documented_names(&[name.to_string()], srcs).contains(name)
    }

    /// A name counts as documented when it appears in an inline code span, in
    /// bold-prefixed prose (the real `stdlib.md` "**Booleans:** …" shape), in a
    /// heading, or inside a fenced block. PRD §(b) disposition 1 asks only for
    /// a word-boundary match somewhere in the chunk corpus.
    #[test]
    fn documented_names_matches_across_markdown_shapes() {
        let cases: &[(&str, &str)] = &[
            ("inline code span", "See `union(a, b)` for boolean ops."),
            (
                "bold-prefixed prose",
                "**Booleans:** `union(a, b)`, `difference(a, b)`",
            ),
            ("heading", "## The union operation"),
            (
                "fenced block",
                "```reify\nlet s = union(a, b)\n```",
            ),
            ("bare prose", "Use union to combine two solids."),
        ];
        for (label, content) in cases {
            let srcs = sources(&[("chunks/geometry.md", content)]);
            assert!(
                documented("union", &srcs),
                "`union` must be documented by the {label} shape: {content:?}"
            );
        }
    }

    /// Word boundaries on BOTH sides: a name must not be satisfied by a longer
    /// identifier that merely contains it. `union` is a real registry entry and
    /// so are `union_all` and `intersection` — treating `union_all` as
    /// documentation for `union` would silently under-report coverage.
    #[test]
    fn documented_names_requires_word_boundaries_on_both_sides() {
        for imposter in ["disunion", "union_all", "reunion", "unionize", "1union2"] {
            let srcs = sources(&[("chunks/geometry.md", imposter)]);
            assert!(
                !documented("union", &srcs),
                "`{imposter}` must NOT satisfy `union` — word boundary required \
                 on both sides"
            );
        }
    }

    /// Backtick, paren, comma, period, colon, asterisk, pipe and end-of-line
    /// all count as boundaries — the punctuation the chunk corpus actually
    /// uses around API names.
    #[test]
    fn documented_names_punctuation_counts_as_boundary() {
        for content in [
            "`union`",
            "union(a, b)",
            "a, union, b",
            "The op is union.",
            "union: combine",
            "**union**",
            "| union | boolean |",
            "trailing union",
            "union",
        ] {
            let srcs = sources(&[("chunks/geometry.md", content)]);
            assert!(
                documented("union", &srcs),
                "punctuation/EOL must count as a word boundary for: {content:?}"
            );
        }
    }

    /// Matching is case-sensitive — Reify builtin names are snake_case, and a
    /// prose `Union` is not the builtin.
    #[test]
    fn documented_names_is_case_sensitive() {
        let srcs = sources(&[("chunks/geometry.md", "Union and Difference are booleans.")]);
        assert!(
            !documented("union", &srcs),
            "`Union` must NOT satisfy `union` — matching is case-sensitive"
        );
    }

    /// A name mentioned in ANY of several chunk sources counts as documented —
    /// the corpus is searched as a whole, not per-file.
    #[test]
    fn documented_names_matches_any_source() {
        let srcs = sources(&[
            ("chunks/syntax.md", "no api names here"),
            ("chunks/types.md", "still nothing"),
            ("chunks/geometry.md", "`extrude(profile, direction, distance)`"),
        ]);
        assert!(
            documented("extrude", &srcs),
            "a mention in ANY chunk source must count as documented"
        );
    }

    /// The returned set contains exactly the documented subset of the input
    /// names — undocumented names are absent, and no name is invented.
    #[test]
    fn documented_names_returns_documented_subset_only() {
        let srcs = sources(&[("chunks/geometry.md", "`union(a, b)` and `extrude(p, d, l)`")]);
        let names = [
            "union".to_string(),
            "extrude".to_string(),
            "offset_solid".to_string(),
            "zone_annulus".to_string(),
        ];
        let got = documented_names(&names, &srcs);
        let want: BTreeSet<String> =
            ["union".to_string(), "extrude".to_string()].into_iter().collect();
        assert_eq!(
            got, want,
            "documented_names must return exactly the documented subset"
        );
    }

    /// An empty corpus documents nothing and does not panic (the fail-safe
    /// posture: a missing chunks dir must not crash the detector).
    #[test]
    fn documented_names_empty_corpus_documents_nothing() {
        let got = documented_names(&["union".to_string()], &[]);
        assert!(
            got.is_empty(),
            "an empty chunk corpus must document nothing; got: {got:?}"
        );
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

    // -------------------------------------------------------------------
    // known_name_index — the fabrication direction's existence oracle
    //
    // Deliberately ASYMMETRIC with the omission census. The census is the
    // PRD-pinned `units.rs` registries; the oracle is every shred of evidence
    // that a name is real, across the compiler and the stdlib. It has to be
    // broader, because legitimately-documented names live outside units.rs
    // (`clamp`, `lerp` and `dot` are in `math_signatures.rs`) and flagging
    // those as fabrications would make the gate unusable.
    //
    // The trade is explicit: a false NEGATIVE (a fabricated name that happens
    // to collide with an unrelated string literal) is acceptable; a false
    // POSITIVE is not.
    // -------------------------------------------------------------------

    /// Convenience: `known_name_index` over one `(path, content)` pair.
    fn oracle(path: &str, content: &str) -> BTreeSet<String> {
        known_name_index(&[(path.to_string(), content.to_string())])
    }

    /// (a) A `*_NAMES` registry entry is evidence the name exists.
    #[test]
    fn oracle_accepts_registry_entries() {
        let idx = oracle(
            "crates/reify-compiler/src/units.rs",
            "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\n    \"extrude\",\n];\n",
        );
        assert!(idx.contains("extrude"), "got: {idx:?}");
    }

    /// (b) The dispatch-match shape — `match name { "abs" => … }` — is how a
    /// large part of the stdlib surface is declared, and none of it is in a
    /// `*_NAMES` registry.
    #[test]
    fn oracle_accepts_match_arm_literals() {
        let idx = oracle(
            "crates/reify-stdlib/src/math.rs",
            "fn dispatch(name: &str) -> Option<Sig> {\n    \
             match name {\n        \"abs\" => Some(SIG_ABS),\n        \
             _ => None,\n    }\n}\n",
        );
        assert!(
            idx.contains("abs"),
            "a dispatch-match arm is existence evidence; got: {idx:?}"
        );
    }

    /// (c) A multi-name arm declares every alternative, not just the first.
    #[test]
    fn oracle_accepts_multi_name_match_arms() {
        let idx = oracle(
            "crates/reify-compiler/src/lower.rs",
            "        \"translate\" | \"rotate\" | \"scale\" => lower_affine(name),\n",
        );
        for want in ["translate", "rotate", "scale"] {
            assert!(
                idx.contains(want),
                "every alternative in a multi-name arm is evidence; \
                 {want:?} missing from {idx:?}"
            );
        }
    }

    /// (d) A struct-literal `name:` field is the third common declaration
    /// shape (signature tables built as arrays of structs).
    #[test]
    fn oracle_accepts_struct_literal_name_fields() {
        let idx = oracle(
            "crates/reify-compiler/src/math_signatures.rs",
            "    Builtin { name: \"clamp\", arity: 3 },\n",
        );
        assert!(idx.contains("clamp"), "got: {idx:?}");
    }

    /// (e) `.ri` stdlib sources declare names in the language's own grammar,
    /// where there is no Rust string literal to harvest at all. All ten
    /// keyword forms, with and without the optional `pub`.
    #[test]
    fn oracle_accepts_ri_declaration_forms() {
        let idx = oracle(
            "crates/reify-compiler/stdlib/dynamics.ri",
            "\
pub fn inverse_dynamics_at_snapshot(mechanism: Mechanism) -> List<JointForce> { x }
pub unit newton : Force = 1.0
pub type Stress = Pressure
    constraint von_mises_stress >= 0
pub purpose simulation_ready(subject : Structure) {
enum EulerConvention { XYZ, XZY }
trait AnalysisResult {
joint revolute(a: Axis, b: Axis)
structure def MaterialFrame {
occurrence def STEPOutput : Output {
",
        );
        for want in [
            "inverse_dynamics_at_snapshot",
            "newton",
            "Stress",
            "von_mises_stress",
            "simulation_ready",
            "EulerConvention",
            "AnalysisResult",
            "revolute",
            "MaterialFrame",
            "STEPOutput",
        ] {
            assert!(
                idx.contains(want),
                "`.ri` declaration form for {want:?} was not harvested; \
                 got: {idx:?}"
            );
        }
    }

    /// (f) Only identifier-shaped literals are evidence. A message template, a
    /// phrase, a chunk id or an empty string are not names — admitting them
    /// would let arbitrary prose vouch for a fabricated call.
    #[test]
    fn oracle_rejects_non_identifier_literals() {
        let idx = oracle(
            "crates/reify-compiler/src/entity.rs",
            "fn f() {\n    \
             let a = \"unresolved type: {}\";\n    \
             let b = \"a b\";\n    \
             let c = \"01-geometry\";\n    \
             let d = \"\";\n}\n",
        );
        for reject in ["unresolved type: {}", "a b", "01-geometry", ""] {
            assert!(
                !idx.contains(reject),
                "{reject:?} is not identifier-shaped and must not enter the \
                 oracle; got: {idx:?}"
            );
        }
    }

    /// (g) Comments are not code. A quoted word in a doc-comment or a `//`
    /// tail is prose — and prose that vouched for a name would silently
    /// disarm the fabrication lane for exactly the names people write about.
    #[test]
    fn oracle_rejects_quoted_words_in_comments() {
        let idx = oracle(
            "crates/reify-compiler/src/lower.rs",
            "/// Formerly known as \"offset_surface\" — removed in v0.5.\n\
             // see also \"chamfer_all\"\n\
             fn lower() {}\n",
        );
        assert!(
            !idx.contains("offset_surface"),
            "a doc-comment mention is not existence evidence; got: {idx:?}"
        );
        assert!(
            !idx.contains("chamfer_all"),
            "a line-comment mention is not existence evidence; got: {idx:?}"
        );
    }

    /// The oracle reads only sources that can carry a declaration.
    #[test]
    fn oracle_scope_covers_compiler_stdlib_rust_and_ri_sources() {
        for path in [
            "crates/reify-compiler/src/units.rs",
            "crates/reify-compiler/src/lower/affine.rs",
            "crates/reify-compiler/stdlib/dynamics.ri",
            "crates/reify-stdlib/src/lib.rs",
        ] {
            assert!(
                in_oracle_scope(path),
                "{path:?} declares builtins and must be in oracle scope"
            );
        }
        for path in [
            // Chunks are the thing being checked — treating them as evidence
            // would make every fabrication vouch for itself.
            "crates/reify-mcp/src/tools/chunks/stdlib.md",
            // Tests and fixtures name things that were never implemented.
            "crates/reify-compiler/tests/lowering.rs",
            "crates/reify-cli/tests/fixtures/affine_algebra.ri",
            "docs/prds/v0_6/doc-chunk-truth-enforcement.md",
        ] {
            assert!(
                !in_oracle_scope(path),
                "{path:?} must NOT be existence evidence"
            );
        }
    }
}
