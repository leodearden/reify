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
//! Census: the `*_NAMES` string-slice registries declared in PRODUCTION scope
//! in `crates/reify-compiler/src/units.rs` (the PRD-pinned corpus). Registries
//! inside a `#[cfg(test)]` module are test fixtures and are excluded — see
//! [`extract_registries`]. Each name is compliant iff exactly one of:
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
//! ### The existence oracle is deliberately asymmetric
//!
//! It is much BROADER than the omission census: the union of every `*_NAMES`
//! registry entry, every identifier-shaped double-quoted string literal under
//! `crates/reify-compiler/src/**/*.rs` and `crates/reify-stdlib/src/**/*.rs`,
//! and every `.ri` declaration in the bundled stdlib. Using the omission
//! census as the oracle would false-flag legitimate builtins wholesale —
//! `clamp`, `lerp` and `dot` are real and documented, but live in
//! `math_signatures.rs`, not `units.rs`.
//!
//! The asymmetry is the design: a false NEGATIVE (a fabricated name that
//! happens to collide with an unrelated string literal) costs one missed
//! finding, while a false POSITIVE accuses a working builtin and makes the
//! lane untrustworthy. When the two trade off, take the miss.
//!
//! ### Known imprecision — the mention side, not the oracle
//!
//! The oracle is broad, but the *mention* side is only syntactically
//! selective, and that is where the residual noise lives. Two syntactic
//! filters are applied in [`chunk_call_mentions`] — a name that IS a
//! [`RI_KEYWORDS`] member (the language's full reserved-word set,
//! `Trait::fn(args)`, `auto(free)`), and the name a `.ri`
//! declaration line introduces (`fn lateral_area(self)`, `purpose
//! manufacturing_ready(…)`) — because each is decidable from the line alone
//! and each can only REMOVE an accusation, never add one.
//!
//! What survives them needs prose-vs-example-source context, which this
//! detector deliberately does not model: grammar metavariables
//! (`predicate(x)`), user-defined names CALLED inside example source that
//! the chunk never DECLARES at all (`compute_moi(...)` in traits.md — the
//! declaration filter is FILE-scoped as of #5647, so a name the chunk DOES
//! declare, anywhere in it, no longer survives on a later call), and
//! non-function call-shaped syntax (`auto(free)`, `@region(…)`). Narrowing
//! THAT is **#5647**. Until it lands
//! the omission lane is the trustworthy half, which is one more reason the CLI
//! arm is opt-in — and the reason #5480's gate must leave `fabricated-name:`
//! report-only (see "δ's gate must key on the OMISSION categories only").
//!
//! **No residual count is pinned in this comment.** It would be wrong within a
//! chunk edit, and there is no test behind it (the floor guards deliberately
//! assert floors, never exact counts — see below). The same caution applies in
//! the other direction to #5647's own problem statement, whose numbers predate
//! this filter and whose named example has since left the corpus. Re-measure:
//!
//! ```text
//! reify-audit --pattern PDOCCOVER --project-root . --no-jcodemunch \
//!             --tasks-file <file holding []> --runs-db <scratch path>
//! ```
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
//!
//! ## CLI posture — opt-in, for now
//!
//! `run_pdoccover` in `bin/reify-audit.rs` uses `is_some_and`, so PDOCCOVER
//! runs only under an explicit `--pattern PDOCCOVER`. Its two structural
//! siblings PTODO and PDSSENTINEL use `is_none_or` and ride the default sweep;
//! the difference is severity plus backlog. These findings are High and the
//! exit code is the High-severity count, so with an unseeded baseline and a
//! documentation backlog still to work through, joining the default sweep
//! today would turn every audit run non-zero.
//! It joins when #5480 seeds the baseline and the residual reaches zero — the
//! warn-first-then-ratchet path PTODO took.
//!
//! ## The #5480 seam
//!
//! This task ships the detector and [`baseline_candidates`]. It ships NO
//! baseline file, NO `--emit-baseline` flag, NO generator binary, NO
//! `tests/infra/` script and NO `run-all-classification.manifest` row — all of
//! those are #5480 (δ). [`baseline_candidates`] is the single derivation δ's
//! regenerator calls, sharing [`omission_dispositions`] with [`check`] so a
//! generated baseline cannot disagree with the ratchet that checks it.
//!
//! ### δ's gate must key on the OMISSION categories only, until #5647 lands
//!
//! Normative for #5480, recorded here because #5480's own text says this
//! pattern "needs no change" and would otherwise inherit the constraint
//! silently. The ratchet has ONE channel and it is omission-shaped: the
//! baseline file is a flat list of NAMES, and [`baseline_candidates`] selects
//! [`Disposition::Undocumented`] alone. A `fabricated-name:` finding therefore
//! has no ratchet channel at all — it is suppressible only by hand-editing a
//! `pdoccover:allow` marker onto the mentioning chunk line, one at a time.
//!
//! Combine that with the mention-side imprecision documented above — today's
//! residue on the real corpus is dominated by #5647-class false positives
//! (grammar metavariables, user names called inside example source,
//! call-shaped non-function syntax) — and a hard gate over ALL five categories
//! would land carrying unsuppressable false positives. So δ's gate keys on
//! `undocumented-name:`, `stale-baseline-entry:`, `stale-allow-entry:` and
//! `allow-missing-reason:`; `fabricated-name:` stays REPORT-ONLY. Two ways to
//! lift that, whichever comes first: #5647 narrows the mention side until the
//! residue is real, or the baseline format grows `path:name` rows and the
//! disposition logic covers fabrications so the ratchet absorbs them the way it
//! absorbs omissions. Either is a deliberate decision with a test behind it —
//! neither is a silent widening of the gate.
//!
//! ## Both scanners are deliberately format-agnostic
//!
//! Neither scanner models the syntax it reads. [`chunk_call_mentions`] runs a
//! byte walk over each line looking for `ident(` — it knows nothing of
//! headings, fence tags, indented fences, table pipes, bold-prefixed lists,
//! nested backticks or trailing whitespace, and therefore cannot be broken by
//! any of them. [`extract_registries`] is line-level for the same reason.
//!
//! This is a correctness property, not a shortcut. Both lanes are **silent on
//! failure**: a blind chunk scanner makes the fabrication lane report clean
//! (nothing is mentioned, so nothing can be fabricated), and a blind registry
//! scanner makes the omission lane report clean (an empty census is fully
//! covered). A markdown-aware or macro-aware parser would go dark the first
//! time someone reflowed a table or wrapped a registry in a macro, and would
//! announce it as GREEN. So the scanners are built to have as little to break
//! as possible, and two floor guards in `tests/pdoccover.rs` pin what remains:
//!
//! | Guard | Defends |
//! |---|---|
//! | `registry_extraction_floor_guard_against_real_units_rs` | the real `units.rs` still yields every PRD-named registry, non-empty, above a distinct-name floor |
//! | `chunk_call_mention_floor_guard_against_real_chunks` | the real chunk corpus still yields a call-shaped census above a floor, spanning several files, containing structural anchors |
//!
//! Both freeze floors, never exact counts, so ordinary source edits never flip
//! them — only an extraction regression does. When one fails, fix the scanner;
//! relaxing the floor restores the exact silent-false-clean failure the guard
//! exists to prevent.

use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};
use std::borrow::Cow;
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
///
/// Returns [`Cow::Borrowed`] for the overwhelmingly common line that has
/// nothing to strip. With no open block and no `/` anywhere, the loop below
/// provably copies the input byte-for-byte: depth can only rise through the
/// `/*` arm, the `//` break needs a `/`, and every other arm (including the
/// in-string `\` escape, which pushes both of its bytes) falls through to a
/// verbatim push. So the early-out is an identity, not an approximation — and
/// it matters, because [`known_name_index`] runs this over the whole oracle
/// corpus (~200k lines of compiler and stdlib source on the real tree) and
/// every allocation it makes there is immediately discarded by
/// [`strip_line_comment`]/[`quoted_tokens`].
fn strip_block_comments<'a>(line: &'a str, depth: &mut usize) -> Cow<'a, str> {
    if *depth == 0 && !line.contains('/') {
        return Cow::Borrowed(line);
    }
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
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
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

/// `true` when `code` is a `#[cfg(test)]` attribute (whitespace-insensitive).
///
/// Also matches the `#[cfg_attr(test, …)]`-free common forms `#[cfg(test)]` and
/// `#![cfg(test)]`. Deliberately narrow: a broader `cfg(…)` match would blank
/// production registries behind an ordinary feature gate.
fn is_cfg_test_attr(code: &str) -> bool {
    let squashed: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    squashed.starts_with("#[cfg(test)]") || squashed.starts_with("#![cfg(test)]")
}

/// Replace every string and character literal in `code` with spaces.
///
/// Used ONLY for brace counting: a `{` inside `"unknown unit: {}"` must not
/// move the module-nesting depth that decides what is inside `#[cfg(test)]`
/// scope. Char literals are recognised only in the exact `'x'` / `'\x'` shapes
/// so a lifetime (`&'static str`) is never mistaken for an unterminated one —
/// which would swallow every brace to the end of the line.
fn blank_literals(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                out.push(b' ');
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    out.push(b' ');
                    i += 1;
                }
                if i < bytes.len() {
                    out.push(b' ');
                    i += 1;
                }
            }
            // `'x'` (3 bytes) or `'\x'` (4 bytes) only — anything else is a
            // lifetime and is copied through untouched.
            b'\'' if i + 2 < bytes.len() && bytes[i + 2] == b'\'' => {
                out.extend_from_slice(b"   ");
                i += 3;
            }
            b'\'' if i + 3 < bytes.len() && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'' => {
                out.extend_from_slice(b"    ");
                i += 4;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
/// Accepts `pub const`, `pub(crate) const`, bare `const` and the `static`
/// spellings of each, and requires a `&[&str]` element type so tuple-slice
/// consts (the real `SI_PREFIXES: &[(&str, f64)]`) are excluded — their quoted
/// tokens are not builtin names.
///
/// The type match tolerates the two spellings that mean the same thing:
/// explicit `'static` lifetimes (`&'static [&'static str]`) are erased before
/// comparison, and a fixed-size array (`&[&str; N]`) is accepted alongside the
/// slice. A grammar that silently REJECTED those would be a false-clean surface
/// one level below the one PRD decision 5 chose pattern discovery to avoid: the
/// registry would leave the census with no signal. What this grammar does not
/// model is cross-checked by [`undiscovered_registry_idents`], which names the
/// missed ident instead of letting the census shrink quietly.
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
    let s = s
        .strip_prefix("const")
        .or_else(|| s.strip_prefix("static"))?;
    // The item keyword must be a whole word.
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
    // Element type must be `&[&str]` or `&[&str; N]` — excludes
    // `&[(&str, f64)]` etc. Whitespace is squashed first, then `'static`
    // lifetimes are erased, which is why the squash is safe: `&'static
    // [&'static str]` squashes to `&'static[&'staticstr]` and erases to
    // `&[&str]`, the same shape the unlifetimed spelling produces.
    let squashed: String = s[colon + 1..].chars().filter(|c| !c.is_whitespace()).collect();
    let rest = squashed.replace("'static", "");
    if !(rest.starts_with("&[&str]") || rest.starts_with("&[&str;")) {
        return None;
    }
    Some(ident)
}

/// `units.rs` reduced to what the registry scan may read: per line, the CODE
/// part (block comments, line comments and attributes removed) plus whether the
/// line sits inside a `#[cfg(test)]` item.
///
/// Shared by [`extract_registries`] and [`undiscovered_registry_idents`] so the
/// completeness cross-check reads exactly the same view the census does — a
/// cross-check computed over a different view could disagree with the thing it
/// is checking.
struct CodeView {
    code: Vec<String>,
    in_test: Vec<bool>,
}

/// Build the [`CodeView`] in ONE forward pass. Block-comment depth and brace
/// depth both carry across lines, so neither can be done lazily per line.
fn code_view(units_src: &str) -> CodeView {
    let lines: Vec<&str> = units_src.lines().collect();
    let mut block_depth = 0usize;
    let mut brace_depth: i32 = 0;
    // A `#[cfg(test)]` attribute has been seen and its item not yet identified.
    let mut pending_cfg_test = false;
    // Brace depth OUTSIDE the currently-open `#[cfg(test)]` block, if any.
    let mut test_scope: Option<i32> = None;
    let mut code_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_test: Vec<bool> = Vec::with_capacity(lines.len());

    for raw in &lines {
        let no_block = strip_block_comments(raw, &mut block_depth);
        let code = strip_line_comment(&no_block).to_string();
        let is_attr = is_attribute_line(&code);

        if is_attr && is_cfg_test_attr(&code) {
            pending_cfg_test = true;
        }

        // Attributes contribute no braces; skipping them keeps a `cfg_attr`
        // argument from perturbing the nesting depth.
        let depth_before = brace_depth;
        if !is_attr {
            let counted = blank_literals(&code);
            brace_depth += counted.matches('{').count() as i32;
            brace_depth -= counted.matches('}').count() as i32;
        }

        let entering = test_scope.is_none() && pending_cfg_test && brace_depth > depth_before;
        if entering {
            test_scope = Some(depth_before);
            pending_cfg_test = false;
        } else if pending_cfg_test && !is_attr && !code.trim().is_empty() {
            // The `#[cfg(test)]` item opened no block (a gated `use`, `const`
            // or one-line `fn`) — it cannot be a module, so stop waiting for
            // one rather than capturing the next unrelated `{`.
            pending_cfg_test = false;
        }

        // The line carrying the closing brace still belongs to the block.
        let inside = test_scope.is_some();
        if test_scope.is_some_and(|outer| brace_depth <= outer) {
            test_scope = None;
        }

        // Attribute lines are blanked only AFTER the cfg(test) reading above,
        // so the body scan still never sees an attribute's brackets or its
        // quoted arguments.
        code_lines.push(if is_attr { String::new() } else { code });
        in_test.push(inside);
    }

    CodeView {
        code: code_lines,
        in_test,
    }
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
///
/// ## `#[cfg(test)]` scope is excluded
///
/// A registry declared inside a `#[cfg(test)]` module is a TEST FIXTURE, not a
/// builtin-name registry, and must never enter the census. The real `units.rs`
/// declares two (`AFFINE_ALGEBRA_NAMES`, `LIST_HELPER_NAMES` — "local fixtures
/// for name families that have no pub single-source slice"); today their
/// contents happen to be real builtin names, so the resulting findings were not
/// *wrong*, but the provenance string sent a reader to go document a
/// `#[cfg(test)]` const. Worse, a future negative-test fixture holding a
/// deliberately fake name would enter the census as an `undocumented-name:`
/// that no chunk edit could ever legitimately satisfy — an entry in the ratchet
/// with no correct resolution.
///
/// Scope is tracked by brace depth over the same forward pass, with string and
/// char literals blanked ([`blank_literals`]) so a `{` inside a message
/// template cannot shift it.
// G-allow: consumed by check()/baseline_candidates() in-module, and by the registry-path brittle-parse floor guard in tests/pdoccover.rs — a separate crate, so pub is required.
pub fn extract_registries(units_src: &str) -> Vec<Registry> {
    let lines: Vec<&str> = units_src.lines().collect();
    let CodeView {
        code: code_lines,
        in_test,
    } = code_view(units_src);

    let mut out: Vec<Registry> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if in_test[i] {
            i += 1;
            continue;
        }
        let Some(ident) = registry_header(&code_lines[i]) else {
            i += 1;
            continue;
        };
        let const_name = ident.to_string();

        // Locate the opening `&[`. It is either on the header line (after the
        // `=`) or on a following line (the line-broken header form, possibly
        // with doc-comment or attribute lines between). Scan forward a bounded
        // distance so a malformed declaration cannot run away over the file.
        //
        // The scan must ALSO stop when this declaration has no bracketed
        // literal at all — the alias form `pub const A_NAMES: &[&str] =
        // B_NAMES;`. Without a stop condition the lookahead latches onto the
        // NEXT declaration's `&[`, which is doubly wrong: the following real
        // registry's entries are harvested under the alias's `const_name` and
        // provenance line, and the real registry itself is then skipped
        // wholesale by `i = j + 1` — a name silently leaving the census, which
        // is exactly the false-clean mode the floor guards exist to prevent
        // (the guard cannot catch it: the mis-bound registry is non-empty).
        // Two stops, both cheap:
        //   - a `;` reached before any `[` ends the initialiser; and
        //   - a following line that is itself a registry header.
        // On either, `open` stays `None`, `i += 1`, and the next declaration is
        // re-examined normally under its own name.
        const HEADER_LOOKAHEAD: usize = 4;
        let mut open = None;
        for (k, code) in code_lines.iter().enumerate().skip(i).take(HEADER_LOOKAHEAD) {
            if k > i && registry_header(code).is_some() {
                break;
            }
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
            let bracket = hay.find('[');
            let terminator = hay.find(';');
            if bracket.is_some_and(|b| terminator.is_none_or(|e| b < e)) {
                // Bracket first (or no terminator at all) — the literal opens here.
                open = Some(k);
                break;
            }
            if terminator.is_some() {
                // Terminator first, or terminator with no bracket — the
                // initialiser is complete and holds no slice literal.
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

/// Every production-scope `*_NAMES` ident that LOOKS declared, whatever its
/// item keyword or element type — the completeness oracle for
/// [`extract_registries`]'s grammar.
///
/// ## Why a grammar needs a cross-check at all
///
/// PRD decision 5 chose pattern discovery (`*_NAMES`) over a hardcoded registry
/// list, because "a hardcoded registry list would itself be an omission-drift
/// surface". A discovery grammar that silently REJECTS an unmodelled
/// declaration shape is the same surface one level down: the registry leaves
/// the census, every name in it stops being checked, and PDOCCOVER reports
/// clean on it forever. The floor guard cannot see it either — it asserts that
/// the PRD-named registries are still found and that discovered registries are
/// non-empty, both of which stay true when a NEW registry is invisible.
///
/// ## What counts as "looks declared"
///
/// An `UPPER_SNAKE` ident ending in `_NAMES` in declaration position — followed
/// by `:` (and not `::`, which is a path) — on a line the [`CodeView`] keeps.
/// Deliberately blind to the item keyword and to the element type, because
/// those are exactly what the grammar might be too narrow about. Comments,
/// attribute lines and `#[cfg(test)]` scope are excluded by the shared code
/// view, so a fixture registry or a name discussed in a doc-comment is not a
/// hit.
///
/// ## The caller does the subtraction
///
/// This returns the DECLARED set, not the undiscovered difference, so the
/// caller can assert both directions: that nothing declared is missing from the
/// census (completeness) AND that everything discovered is in here
/// (non-vacuity). A function that returned only the difference could go blind —
/// scan nothing, find nothing missing — and its guard would pass silently,
/// which is the very failure mode the cross-check exists to close.
///
/// A declared-but-undiscovered ident is not automatically a bug: a genuinely
/// different KIND of const (a tuple slice, an alias of another registry) is
/// legitimately not a name registry. It means "a human must classify this",
/// which is why the caller is a test naming the ident rather than a finding
/// category. Consumed by `registry_extraction_floor_guard_against_real_units_rs`
/// in `tests/pdoccover.rs` — a separate crate, so `pub` is required.
// G-allow: consumed by the registry-path completeness cross-check in tests/pdoccover.rs.
pub fn declared_registry_idents(units_src: &str) -> Vec<String> {
    let view = code_view(units_src);
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (code, in_test) in view.code.iter().zip(view.in_test.iter()) {
        if *in_test {
            continue;
        }
        for ident in declaration_position_idents(code) {
            if ident.ends_with(REGISTRY_SUFFIX) {
                out.insert(ident.to_string());
            }
        }
    }
    out.into_iter().collect()
}

/// `UPPER_SNAKE` idents in `code` that sit in declaration position — followed,
/// after optional whitespace, by a single `:`.
///
/// A byte walk, like every other scanner here: `::` is a path separator and
/// never a declaration, and an ident preceded by `::` is a path segment. Case
/// is the cheap discriminator that keeps ordinary locals and fields out.
fn declaration_position_idents(code: &str) -> Vec<&str> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !is_word_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_word_byte(bytes[i]) {
            i += 1;
        }
        let ident = &code[start..i];
        // Screaming snake case only, and never a `::path::SEGMENT`.
        let shouty = ident
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
        // Byte comparison, never a `code[start - 2..start]` slice: the code
        // view can hold multibyte literals and a 2-byte back-step could land
        // mid-char.
        let after_path_sep = start >= 2 && bytes[start - 2] == b':' && bytes[start - 1] == b':';
        if !shouty || after_path_sep {
            continue;
        }
        // Next non-space byte must be a lone `:`.
        let mut j = i;
        while j < bytes.len() && bytes[j] == b' ' {
            j += 1;
        }
        if bytes.get(j) == Some(&b':') && bytes.get(j + 1) != Some(&b':') {
            out.push(ident);
        }
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
    // Drop a trailing comment terminator BEFORE looking at the separator.
    // A chunk-side marker naturally lives in an HTML comment (invisible in
    // rendered markdown), and `-->` starts with the ASCII-hyphen separator:
    // without this, `<!-- pdoccover:allow -->` would parse as a well-formed
    // marker whose reason is `->` and would silently suppress a real claim.
    // `*/` is the same hazard for a Rust block comment.
    let body = body.trim_end();
    let body = body
        .strip_suffix("-->")
        .or_else(|| body.strip_suffix("*/"))
        .unwrap_or(body);
    let body = body.trim();
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
/// not the builtin.
///
/// UTF-8-safe on BOTH arguments. Neither is assumed ASCII: `haystack` is chunk
/// prose (`§`, `→`, em dashes) and `needle` is whatever a `*_NAMES` registry
/// happens to hold. A match index is always a char boundary — the needle's
/// first byte is either ASCII or a UTF-8 lead byte, and neither can occur mid
/// char — but the boundary-rejected RETRY must still step by a whole char, or a
/// non-ASCII needle would slice into a continuation byte and panic. A detector
/// whose contract is "unreadable input is skipped fail-safe (no finding, no
/// panic)" must not have a panic reachable from corpus content.
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
        // Advance past this occurrence's first CHARACTER, not past the whole
        // needle: overlapping occurrences must still be considered, but a
        // one-BYTE step would land inside a multibyte char whenever `needle`
        // starts with one and the next `haystack[start..]` slice would panic.
        start = idx + haystack[idx..].chars().next().map_or(1, char::len_utf8);
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

/// Every reserved word in the language grammar — mention-side only.
///
/// = docs/reify-language-spec.md §2.10 "Keywords (Complete Post-Review List)"
/// (:191-219) / §17 alphabetical appendix (:2880-2897, "Total: 46 keywords" —
/// the two forms agree exactly) UNION [`RI_DECL_KEYWORDS`]. `joint` is a
/// grammar-only keyword that post-dates the spec table; the union keeps this
/// const a superset of `RI_DECL_KEYWORDS` by construction, so widening the
/// mention-side filter can never regress the narrower filter a63c892eea
/// already shipped (`ri_keywords_is_a_superset_of_ri_decl_keywords`).
///
/// DELIBERATELY EXCLUDED, both pinned by tests:
/// - the spec's "Removed keywords (not part of v0.1)" (`derived`, `require`,
///   `dimension`, :219) — none is part of the language at all;
/// - the spec's "Not keywords (standard library functions)" (`determined`,
///   `constrained`, `undetermined`, `partially_determined`, `point3`,
///   `vec3`, `point2`, `vec2`, `project`, `geo_equiv`, :221) — these are real
///   builtins, and admitting them here would silently drop every chunk claim
///   naming one (`ri_keywords_excludes_the_spec_carve_outs`, and against the
///   real `units.rs` census, `ri_keywords_never_collides_with_the_real_registry_census`
///   in tests/pdoccover.rs).
///
/// ## A separate const from `RI_DECL_KEYWORDS`, deliberately not unified
///
/// `RI_DECL_KEYWORDS` also drives [`ri_declared_name`], which feeds the
/// existence ORACLE ([`known_name_index`]) for `.ri` stdlib sources. Widening
/// *that* function to the full reserved-word set would make `let foo`,
/// `param foo`, `port foo` and `sub foo` parse as declarations of `foo`,
/// admitting local bindings into the oracle and vouching for names the
/// compiler never provides — turning the fabrication lane into a
/// false-negative machine. The mention side has the opposite polarity (a
/// keyword before `(` is grammar, never a claim), so the wider set is
/// correct there, and ONLY there. Do not fold the two consts together.
// G-allow: consumed by chunk_call_mentions in-module, and by the
// keyword-vs-builtin collision guard in tests/pdoccover.rs — a separate
// crate, so pub is required.
pub const RI_KEYWORDS: &[&str] = &[
    "and", "as", "auto", "chain", "connect", "constraint", "def", "else", "enum", "exists",
    "false", "field", "fn", "forall", "if", "implies", "import", "in", "let", "map", "match",
    "maximize", "meta", "minimize", "module", "none", "not", "occurrence", "or", "out", "param",
    "port", "pub", "purpose", "self", "set", "some", "structure", "sub", "then", "trait", "true",
    "type", "undef", "unit", "where",
    // From RI_DECL_KEYWORDS only — a grammar-only keyword that post-dates
    // the spec table (kept so this const is a superset by construction).
    "joint",
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
fn in_oracle_scope(path: &str) -> bool {
    (path.starts_with("crates/reify-compiler/src/") && path.ends_with(".rs"))
        || (path.starts_with("crates/reify-compiler/stdlib/") && path.ends_with(".ri"))
        || (path.starts_with("crates/reify-stdlib/src/") && path.ends_with(".rs"))
}

/// Call-shaped API mentions in one chunk: `(name, 1-based line)`.
///
/// A mention is an identifier immediately followed by `(` — the shape of a
/// claim that the compiler provides a callable of that name. Backticked tokens
/// with no parens are types, modules, units or constants (`Angle`, `std.math`,
/// `pi`); admitting them would flood the lane with every prose noun. The left
/// delimiter must not be `.` or `@`: `solid.volume()` is member access on a
/// value, and `pipe@region(outer)` (connect.md:52, "The `@` operator creates
/// ad-hoc ports by designating geometric regions"; docs/reify-language-spec.md:1488,
/// §D5) is an ad-hoc port/region designator on a value. Both are the same
/// rule, not a special case for `@`: the lane has no oracle for a selector on
/// a value, whichever delimiter introduces it, only for free-function claims.
///
/// **Format-agnostic by construction** — a byte walk over each line, with no
/// awareness of headings, fence tags, indented fences, table pipes,
/// bold-prefixed lists (`**Modify:** `fillet(...)`, `chamfer(...)``), nested
/// backticks or trailing whitespace. All of those shapes appear in the real
/// corpus and all of them fall out for free, because nothing here parses
/// markdown. That is what makes the chunk-path drift guard satisfiable at all:
/// a structure-aware extractor would go silently blind the first time someone
/// reflowed a table, and a silently-blind fabrication lane reports clean.
/// `chunk_call_mention_floor_guard_against_real_chunks` is what defends the
/// property (module header, "Both scanners are deliberately format-agnostic").
///
/// Chunk-side escape: a line carrying a well-formed `pdoccover:allow —
/// <reason>` documents something deliberately ahead of the implementation, and
/// its mentions are dropped. A REASONLESS marker drops nothing — the mention
/// stays visible so the caller can report the malformed marker rather than
/// silently honour it (PRD design decision 7).
///
/// ## Declaration sites are not claims
///
/// Two filters, both syntactic, neither modelling context:
///
/// 1. a name that IS a [`RI_KEYWORDS`] member — the language's full
///    reserved-word set (spec §2.10), not just declaration keywords.
///    `Trait::fn(args)` in prose about static dispatch reads as a call to
///    `fn`, and `auto(free)` reads as a call to `auto`; no keyword is ever a
///    builtin;
/// 2. the name a line DECLARES, per [`ri_declared_name`] — collected over
///    EVERY line of `content` first (FILE-scoped, not declaration-site-
///    scoped): a name this chunk's own example source declares anywhere in
///    it (`fn lateral_area(self)`, `purpose manufacturing_ready(subject)`,
///    or a trait body declaring `fn make_default()` a few lines above a
///    call to it) is example-local, not a claim that the compiler provides
///    it, however far from the declaration the call sits. A name the
///    content never declares at all is untouched by this filter — `fn
///    area(self) { rect_area(w, h) }` still yields `rect_area` as a claim,
///    because `rect_area` is declared nowhere in that content.
///
/// Both can only REMOVE accusations, never add one, which is the safe
/// direction for this lane (module header, "the existence oracle is
/// deliberately asymmetric"). Filter 2 reuses `ri_declared_name` — the oracle
/// lane's own grammar — rather than re-deriving one, so `pub fn`, `purpose`
/// and the two-word `structure def` / `occurrence def` forms are all covered
/// by a function that already has tests. FILE scope, not CORPUS scope: a
/// name declared in chunk A and called in chunk B is a cross-chunk reference
/// to a documented API, not an example-local definition, so `content` is the
/// unit — corpus scope would let one chunk's throwaway example silently
/// disarm every other chunk's claims about that name.
///
/// **Precision beyond that is deliberately deferred.** What remains needs
/// prose-vs-example-source context that no syntactic rule — line-local or
/// file-scoped — can decide: a grammar metavariable (`predicate(x)` in a
/// grammar production), and a user-defined name CALLED inside example
/// source that the chunk never declares AT ALL (`compute_moi(...)`, unlike
/// the now-handled `make_default`/`scaled` case above). Do not add a
/// context-modelling filter here without updating the floor guard's anchor
/// set. See the module header for the full, dated accounting of what
/// #5647 narrowed.
///
/// Line numbers are 1-based over `str::lines()`, which strips a trailing `\r`,
/// so CRLF chunks behave identically to LF ones.
// G-allow: consumed by check() in-module, and by the chunk-path brittle-parse floor guard in tests/pdoccover.rs — a separate crate, so pub is required.
pub fn chunk_call_mentions(content: &str) -> Vec<(String, usize)> {
    // Pass 1: every name this chunk's own example source declares, ANYWHERE
    // in the content — not just on the line currently being scanned in pass
    // 2. Borrowed from `content`, so no allocation per name.
    let declared_names: BTreeSet<&str> = content.lines().filter_map(ri_declared_name).collect();

    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        // A well-formed allow marker suppresses the whole line; a reasonless
        // one suppresses nothing.
        if allow_marker_reason(line).is_some() {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'(' {
                i += 1;
                continue;
            }
            // Walk back over the identifier immediately preceding the paren.
            let end = i;
            let mut start = end;
            while start > 0 && is_word_byte(bytes[start - 1]) {
                start -= 1;
            }
            i += 1;
            if start == end {
                continue; // `(` not preceded by an identifier
            }
            // `.foo(` is member access, `@foo(` is an ad-hoc port/region
            // designator — neither is a builtin call.
            if start > 0 && matches!(bytes[start - 1], b'.' | b'@') {
                continue;
            }
            let name = &line[start..end];
            if !is_identifier_shaped(name) {
                continue;
            }
            // A reserved word is never a builtin (`Trait::fn(args)`,
            // `auto(free)`), and a name this CHUNK declares anywhere is
            // defined by its own example source, not claimed of the
            // compiler.
            if RI_KEYWORDS.contains(&name) || declared_names.contains(name) {
                continue;
            }
            out.push((name.to_string(), idx + 1));
        }
    }
    out
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
///
/// ## Non-identifier tokens never enter the census
///
/// An entry that is not [`is_identifier_shaped`] cannot be a documentable
/// builtin call name, and admitting one would be harmful twice over. It would
/// enter the ratchet as an `undocumented-name:` that no chunk edit could
/// legitimately satisfy — the same "no correct resolution" defect the
/// `#[cfg(test)]` exclusion exists to prevent — and, being possibly non-ASCII
/// (`"µm"`, `"°C"` are entirely plausible tokens in a *units* file), it would
/// then be matched against ~8MB of chunk prose by [`contains_word`], whose
/// boundary alphabet is ASCII and which can therefore never report such a token
/// documented anyway.
///
/// This is the census's own admission test, deliberately distinct from the
/// oracle-side one in [`known_name_index`]: both use [`is_identifier_shaped`],
/// but for opposite reasons — there to keep prose from vouching for a
/// fabrication, here to keep an unsatisfiable entry out of the ratchet.
fn census_names(registries: &[Registry]) -> Vec<CensusName> {
    let mut by_name: std::collections::BTreeMap<String, CensusName> =
        std::collections::BTreeMap::new();
    for reg in registries {
        for entry in &reg.entries {
            if !is_identifier_shaped(&entry.name) {
                continue;
            }
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
    /// Sorted tracked-path list, retained so the fabrication lane's oracle read
    /// does not have to re-run `ls_files()`.
    tracked: Vec<String>,
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

/// Gather the inputs BOTH lanes need — deliberately not the oracle sources.
///
/// Path membership comes from `ctx.git.ls_files()` — content from `std::fs`,
/// mirroring `pdssentinel::check`. Only tracked files participate, so a stray
/// untracked `units.rs.orig` or a scratch chunk never perturbs the census.
///
/// The fabrication lane's ~8MB oracle read is split out into
/// [`load_oracle_sources`] so [`baseline_candidates`] — whose only consumer is
/// #5480's regenerator, and which touches nothing but `registries`,
/// `chunk_sources` and `baseline` — does not pay for it on every run.
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
        tracked,
        registries,
        chunk_sources,
        baseline,
    }
}

/// Read the fabrication lane's existence-oracle corpus.
///
/// Split out from [`load_inputs`] because it is by far the most expensive read
/// PDOCCOVER does — every tracked `.rs` under `crates/reify-compiler/src/` and
/// `crates/reify-stdlib/src/` plus the bundled `.ri` stdlib, ~8MB on the real
/// tree — and the omission lane never touches it.
///
/// Reuses `inputs.tracked` rather than re-running `ls_files()`, so the split
/// costs no extra git call.
fn load_oracle_sources(ctx: &AuditContext<'_>, inputs: &Inputs) -> Vec<(String, String)> {
    inputs
        .tracked
        .iter()
        .filter(|p| in_oracle_scope(p))
        .filter_map(|p| read_relative(ctx, p).map(|c| (p.clone(), c)))
        .collect()
}

// -----------------------------------------------------------------------
// Finding emission
// -----------------------------------------------------------------------

/// A finding plus its `(category, name, path)` sort key.
///
/// The key is carried alongside rather than re-parsed out of the summary so
/// the ordering contract cannot silently drift if a summary is ever reworded.
/// `path` is part of the key because one name can earn the same category from
/// both lanes — an `allow-missing-reason` in `units.rs` and another in a chunk
/// are two distinct defects in two distinct files.
struct Keyed {
    category: &'static str,
    name: String,
    path: String,
    finding: Finding,
}

/// Build one PDOCCOVER finding. `detail` is appended after `<category>: <name>`.
fn keyed(category: &'static str, name: &str, path: &str, detail: String) -> Keyed {
    Keyed {
        category,
        name: name.to_string(),
        path: path.to_string(),
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

/// What the three-way disposition — plus its two ratchet-honesty readings —
/// resolves one census name to. Exactly one variant per name, by construction:
/// making this an enum rather than a set of independent booleans is what
/// enforces the "at most one finding per name" rule below at the type level.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Disposition {
    /// Documented, with no suppression channel left pointing at it. Nothing to
    /// report — the goal state.
    Clean,
    /// Undocumented, but a well-formed allow marker or a baseline entry
    /// accounts for it. Nothing to report, and NOT a baseline candidate: it is
    /// already suppressed.
    Exempt,
    /// `pdoccover:allow` with no reason body. Confers no exemption, and is a
    /// defect on its own terms. Deliberately not a baseline candidate —
    /// baselining it would freeze a malformed marker into the ratchet instead
    /// of prompting the one edit that fixes it.
    AllowMissingReason,
    /// Documented, yet still allow-marked. Carries the now-obsolete reason so
    /// the finding can quote it back.
    StaleAllow(String),
    /// Documented, yet still listed in the baseline file.
    StaleBaseline,
    /// Undocumented with no exemption channel — the offender the lane exists to
    /// find, and the ONLY variant [`baseline_candidates`] selects.
    Undocumented,
}

/// Resolve every census name to exactly one [`Disposition`].
///
/// **The single source of truth for the omission lane.** Both consumers read
/// it: [`omission_findings`] renders the reportable dispositions as findings,
/// and [`baseline_candidates`] selects [`Disposition::Undocumented`]. Neither
/// re-derives anything, which is what makes #5480's generated baseline and the
/// ratchet that checks it structurally incapable of disagreeing.
///
/// ## Exemption precedence — at most ONE disposition per census name
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
fn omission_dispositions(inputs: &Inputs) -> Vec<(CensusName, Disposition)> {
    let census = census_names(&inputs.registries);
    let names: Vec<String> = census.iter().map(|c| c.name.clone()).collect();
    let documented = documented_names(&names, &inputs.chunk_sources);

    census
        .into_iter()
        .map(|c| {
            let name = c.name.as_str();

            // (1) A malformed escape hatch is a defect on its own terms.
            let d = if c.allow_missing_reason {
                Disposition::AllowMissingReason
            } else if documented.contains(name) {
                // (2) Documented: the name is covered, so any surviving
                // suppression channel is stale.
                match (&c.allow, inputs.baseline.contains(name)) {
                    (Some(reason), _) => Disposition::StaleAllow(reason.clone()),
                    (None, true) => Disposition::StaleBaseline,
                    (None, false) => Disposition::Clean,
                }
            } else if c.allow.is_some() || inputs.baseline.contains(name) {
                // (3) Undocumented, but a well-formed channel exempts it.
                Disposition::Exempt
            } else {
                Disposition::Undocumented
            };
            (c, d)
        })
        .collect()
}

/// Omission lane, including its two ratchet-honesty siblings.
///
/// Pure rendering of [`omission_dispositions`] — every disposition rule lives
/// there, so this function and [`baseline_candidates`] cannot drift apart.
fn omission_findings(inputs: &Inputs) -> Vec<Keyed> {
    let mut out = Vec::new();
    for (c, disposition) in omission_dispositions(inputs) {
        let name = c.name.as_str();
        let declared_at = format!("{} ({UNITS_PATH}:{})", c.const_name, c.line);
        match disposition {
            Disposition::Clean | Disposition::Exempt => {}
            Disposition::AllowMissingReason => out.push(keyed(
                "allow-missing-reason",
                name,
                UNITS_PATH,
                format!(
                    "— `{ALLOW_TOKEN}` on {declared_at} has no reason body, so it \
                     grants no exemption; write `// {ALLOW_TOKEN} — <reason>` or \
                     document the name under {CHUNKS_PREFIX}",
                ),
            )),
            Disposition::StaleAllow(reason) => out.push(keyed(
                "stale-allow-entry",
                name,
                UNITS_PATH,
                format!(
                    "— {declared_at} is documented under {CHUNKS_PREFIX}, so its \
                     `{ALLOW_TOKEN} — {reason}` marker is obsolete; delete the marker",
                ),
            )),
            Disposition::StaleBaseline => out.push(keyed(
                "stale-baseline-entry",
                name,
                BASELINE_PATH,
                format!(
                    "— documented under {CHUNKS_PREFIX} but still listed in \
                     {BASELINE_PATH}; delete the line so the ratchet keeps \
                     meaning residual debt",
                ),
            )),
            Disposition::Undocumented => out.push(keyed(
                "undocumented-name",
                name,
                UNITS_PATH,
                format!(
                    "— declared in {declared_at}, mentioned in no chunk under \
                     {CHUNKS_PREFIX}; document it, or mark the entry line \
                     `// {ALLOW_TOKEN} — <reason>`",
                ),
            )),
        }
    }
    out
}

/// Fabrication lane: call-shaped names a chunk claims that the compiler and
/// stdlib evidence nowhere.
///
/// Runs over the SAME `chunk_sources` the omission lane matched against — one
/// read of the corpus feeds both directions, which is the whole point of
/// making this one detector rather than two.
///
/// Deduped per (chunk file, name) at the FIRST occurrence: a name documented in
/// a heading, a table and a fence is one defect, not three, and reporting the
/// first mention keeps the reported line stable as later mentions come and go.
/// Dedup is per-file rather than global because each chunk that repeats a
/// fabricated name needs its own edit.
///
/// A reasonless `pdoccover:allow` marker is reported INSTEAD of any fabrication
/// verdict for the same name in the same file, and that precedence is
/// LINE-ORDER-INDEPENDENT: a pre-pass collects every name carrying a reasonless
/// marker anywhere in the file before the first finding is emitted. Resolving
/// it inline instead would make the outcome depend on where the marker sits —
/// a name mentioned plainly at line 10 and marked reasonlessly at line 50 would
/// emit the fabrication first, and the dedup set would then swallow the
/// malformed marker entirely. That is a hole in PRD design decision 7's
/// guarantee that the escape hatch can never become un-reviewable: the marker
/// would be invisible to the detector purely because of its position.
fn fabrication_findings(ctx: &AuditContext<'_>, inputs: &Inputs) -> Vec<Keyed> {
    let known = known_name_index(&load_oracle_sources(ctx, inputs));

    let mut out = Vec::new();
    for (path, content) in &inputs.chunk_sources {
        let lines: Vec<&str> = content.lines().collect();
        let mentions = chunk_call_mentions(content);

        // Pre-pass: name -> FIRST line in this file carrying a reasonless
        // marker for it. Position-independent, so the marker always wins.
        let mut reasonless: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for (name, line_no) in &mentions {
            let line = lines.get(line_no - 1).copied().unwrap_or("");
            if allow_marker_present(line) && allow_marker_reason(line).is_none() {
                reasonless.entry(name.clone()).or_insert(*line_no);
            }
        }

        let mut seen: BTreeSet<String> = BTreeSet::new();

        for (name, line_no) in mentions {
            // A reasonless marker suppresses nothing and is itself the defect —
            // reported once per name per file, and reported INSTEAD of any
            // fabrication verdict, so one malformed marker costs one finding.
            if let Some(&marker_line) = reasonless.get(&name) {
                if seen.insert(name.clone()) {
                    out.push(keyed(
                        "allow-missing-reason",
                        &name,
                        path,
                        format!(
                            "— `{ALLOW_TOKEN}` at {path}:{marker_line} has no reason body, so it \
                             grants no exemption; write `{ALLOW_TOKEN} — <reason>` or remove \
                             the claim",
                        ),
                    ));
                }
                continue;
            }

            if known.contains(&name) {
                continue;
            }
            if seen.insert(name.clone()) {
                out.push(keyed(
                    "fabricated-name",
                    &name,
                    path,
                    format!(
                        "— documented at {path}:{line_no} but declared nowhere in the \
                         compiler or stdlib sources; fix the chunk, or mark the line \
                         `{ALLOW_TOKEN} — <reason>` if it is deliberately ahead of the \
                         implementation",
                    ),
                ));
            }
        }
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
    keyed.extend(fabrication_findings(ctx, &inputs));
    keyed.sort_by(|a, b| {
        a.category
            .cmp(b.category)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    keyed.into_iter().map(|k| k.finding).collect()
}

/// The single shared derivation #5480's baseline regenerator consumes: the
/// sorted, deduped set of names that [`check`] reports as `undocumented-name:`.
///
/// Exported so generation and the ratchet can never disagree (PRD §6.6's
/// `ptodo-baseline-gen` lesson). The guarantee is structural, not a convention
/// two functions agree to keep: this and [`check`] both read
/// [`omission_dispositions`], selecting from one resolution rather than each
/// re-deriving it. `tests/pdoccover.rs` pins the two outputs equal as sets AND
/// as sequences.
///
/// Sorted and deduped for free — [`census_names`] is keyed by name in a
/// `BTreeMap`, so a name declared in several registries appears once and the
/// generated baseline file diffs cleanly against the next run.
///
/// Excludes, by design: documented names (not debt), allow-marked and
/// already-baselined names (debt already accounted for), reasonless markers (a
/// defect to fix, not debt to freeze) and fabrications (a chunk defect with no
/// registry entry to key on).
///
/// ## The fabrication exclusion is a constraint on #5480's gate
///
/// Because this is the ONLY ratchet channel and it is name-keyed, a
/// `fabricated-name:` finding cannot be baselined at all — only hand-marked
/// with `pdoccover:allow`, per chunk line. #5480's hard gate must therefore key
/// on the omission categories only, leaving `fabricated-name:` report-only
/// until #5647 narrows the mention side (or until the baseline format grows
/// `path:name` rows and [`omission_dispositions`] is generalised to cover
/// fabrications). Module header, "δ's gate must key on the OMISSION categories
/// only", has the full rationale.
///
/// Reads only what the omission lane needs — [`load_inputs`], not
/// [`load_oracle_sources`] — so a regenerator run does not pay for the
/// fabrication lane's ~8MB compiler/stdlib scan it would never consult.
///
/// **Scope** — this task ships NO `--emit-baseline` flag, NO
/// `pdoccover-baseline-gen` binary and NO baseline file. All three are #5480's
/// deliverables; this is the function they call.
// G-allow: #5480's entry point (PRD open question 1); no in-repo caller yet by design.
pub fn baseline_candidates(ctx: &AuditContext<'_>) -> Vec<String> {
    omission_dispositions(&load_inputs(ctx))
        .into_iter()
        .filter(|(_, d)| *d == Disposition::Undocumented)
        .map(|(c, _)| c.name)
        .collect()
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

    /// The comment stripper must BORROW the common line and allocate only when
    /// it actually rewrites something.
    ///
    /// `known_name_index` runs it over ~200k lines of compiler and stdlib
    /// source per fabrication-lane run; an unconditional `Vec<u8>` + `String`
    /// per line is pure waste on the one genuinely hot path this detector has.
    /// The early-out must also be an IDENTITY, so the same-output assertions
    /// below matter as much as the borrow ones.
    #[test]
    fn strip_block_comments_borrows_when_there_is_nothing_to_strip() {
        let mut depth = 0usize;
        for line in [
            "    \"is_watertight\",",
            "pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[",
            "",
            "    let msg = \"unresolved unit: {} — see §3\";",
            "    let escaped = \"a\\\"b\";",
        ] {
            let got = strip_block_comments(line, &mut depth);
            assert!(
                matches!(got, Cow::Borrowed(_)),
                "{line:?} has nothing to strip and must not allocate"
            );
            assert_eq!(got, line, "the early-out must be an identity");
            assert_eq!(depth, 0, "{line:?} must not open a block");
        }

        // Anything that actually rewrites still returns an owned String, and
        // still rewrites correctly.
        let mut depth = 0usize;
        let got = strip_block_comments("\"a\", /* \"phantom\" */ \"b\",", &mut depth);
        assert!(matches!(got, Cow::Owned(_)), "a stripped line is owned");
        assert_eq!(quoted_tokens(&got), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(depth, 0);

        // An OPEN block is stripped even with no `/` on the line — the
        // early-out must not fire on depth > 0.
        let mut depth = 1usize;
        let got = strip_block_comments("still inside the comment", &mut depth);
        assert!(
            matches!(got, Cow::Owned(_)) && got.is_empty(),
            "a line inside an open block must be dropped, got {got:?}"
        );
        assert_eq!(depth, 1, "the block is still open");
    }

    /// Three spellings that mean exactly what `pub const X_NAMES: &[&str]`
    /// means. Rejecting them would drop the whole registry out of the census
    /// with no signal — the false-clean mode the guards exist to prevent.
    #[test]
    fn extract_registries_accepts_static_lifetime_and_array_type_forms() {
        let src = r#"pub static ALPHA_NAMES: &[&str] = &["a_op"];
pub const BETA_NAMES: &'static [&'static str] = &["b_op"];
pub const GAMMA_NAMES: &[&str; 1] = &["c_op"];
static DELTA_NAMES: &'static [&'static str; 1] = &["d_op"];
"#;
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec![
                "ALPHA_NAMES",
                "BETA_NAMES",
                "GAMMA_NAMES",
                "DELTA_NAMES"
            ],
            "`static`, explicit `'static` lifetimes and `&[&str; N]` are all \
             the same kind of registry; got: {regs:?}"
        );
        for reg in &regs {
            assert_eq!(
                entry_names(reg).len(),
                1,
                "{} parsed to the wrong entry list: {reg:?}",
                reg.const_name
            );
        }
        // The tuple-slice exclusion must survive the widening.
        assert!(
            extract_registries("pub const SI_PREFIX_NAMES: &'static [(&str, f64)] = &[(\"k\", 1e3)];\n")
                .is_empty(),
            "a tuple slice is still not a name registry, lifetime or not"
        );
    }

    /// The completeness oracle sees `*_NAMES` declarations the grammar does not
    /// model, so the caller can name them instead of letting them leave the
    /// census silently.
    #[test]
    fn declared_registry_idents_sees_what_the_grammar_skips() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &["box"];
pub const SI_PREFIX_NAMES: &[(&str, f64)] = &[("kilo", 1e3)];
pub const ALIAS_NAMES: &[&str] = GEOMETRY_FUNCTION_NAMES;
"#;
        assert_eq!(
            declared_registry_idents(src),
            vec![
                "ALIAS_NAMES".to_string(),
                "GEOMETRY_FUNCTION_NAMES".to_string(),
                "SI_PREFIX_NAMES".to_string()
            ],
            "all three declarations must be seen, sorted — the discovered one \
             too, so the caller can prove the scan is not vacuous"
        );
        // The subtraction the floor guard performs.
        let discovered: Vec<String> = extract_registries(src)
            .into_iter()
            .map(|r| r.const_name)
            .collect();
        let undiscovered: Vec<String> = declared_registry_idents(src)
            .into_iter()
            .filter(|d| !discovered.contains(d))
            .collect();
        assert_eq!(
            undiscovered,
            vec!["ALIAS_NAMES".to_string(), "SI_PREFIX_NAMES".to_string()],
            "the tuple slice and the alias are the two the grammar does not \
             model; the real registry must not be flagged"
        );
    }

    /// Fixtures, prose and paths are not registry declarations.
    #[test]
    fn declared_registry_idents_ignores_test_scope_comments_and_paths() {
        let src = r#"pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &["box"];

// A comment about MISSING_NAMES: not code.
fn f() {
    let x = units::GEOMETRY_FUNCTION_NAMES::LEN;
    let names = vec![OTHER_NAMES];
    let _ = (x, names);
}

#[cfg(test)]
mod tests {
    const FIXTURE_NAMES: &[(&str, u8)] = &[("a", 1)];
}
"#;
        assert_eq!(
            declared_registry_idents(src),
            vec!["GEOMETRY_FUNCTION_NAMES".to_string()],
            "only the real declaration counts: a comment, a path segment, a \
             bare reference and a `#[cfg(test)]` fixture are none of them \
             registry declarations"
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

    /// A trailing comment terminator is not reason text.
    ///
    /// The chunk-side marker naturally lives in an HTML comment, and `-->`
    /// begins with the ASCII-hyphen separator: read naively,
    /// `<!-- pdoccover:allow -->` is a well-formed marker whose reason is
    /// `->`, and it would silently suppress a real claim. `*/` is the same
    /// hazard on the Rust side.
    #[test]
    fn allow_marker_reason_ignores_comment_terminators() {
        for line in [
            "- `planned_op(x)` <!-- pdoccover:allow -->",
            "- `planned_op(x)` <!-- pdoccover:allow — -->",
            r#"    "x", /* pdoccover:allow */"#,
        ] {
            assert_eq!(
                allow_marker_reason(line),
                None,
                "a comment terminator is not a reason body: {line:?}"
            );
        }
        assert_eq!(
            allow_marker_reason("- `planned_op(x)` <!-- pdoccover:allow — planned, see #5434 -->"),
            Some("planned, see #5434"),
            "a real reason inside an HTML comment survives, terminator stripped"
        );
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
    /// A NON-boundary occurrence that precedes the real one must not stop the
    /// search — the matcher retries from the next byte rather than giving up on
    /// the first hit.
    ///
    /// This is the branch that prevents a real false `undocumented-name:`.
    /// `union`, `union_all`, `intersection` and `intersection_all` are all real
    /// registry entries, so a corpus that documents the longer name FIRST is a
    /// live shape — and a single-`find` matcher would report the shorter name
    /// undocumented even though the very next line documents it.
    #[test]
    fn documented_names_retries_past_a_non_boundary_occurrence() {
        // Prose deliberately avoids a bare `union`, so the ONLY boundary-
        // delimited occurrence is the one on the second line — reached only by
        // retrying past the `union_all` hit on the first.
        let chunk = "\
- `union_all(list)` — n-ary boolean OR.
- `union(a, b)` — combine two solids.
";
        let names = vec!["union".to_string()];
        let got = documented_names(&names, &[("chunks/x.md".into(), chunk.into())]);
        assert!(
            got.contains("union"),
            "`union` IS documented on the second line; a matcher that stops at \
             the first (non-boundary) `union_all` hit would falsely report it \
             undocumented. Got: {got:?}"
        );

        // The reverse order must behave identically, and a name that only ever
        // occurs without boundaries is still undocumented.
        let only_longer = "- `intersection_all(list)` — n-ary boolean AND.\n";
        let got = documented_names(
            &["intersection".to_string()],
            &[("chunks/x.md".into(), only_longer.into())],
        );
        assert!(
            got.is_empty(),
            "`intersection_all` must not vouch for `intersection`; got {got:?}"
        );
    }

    /// The boundary-rejected retry must step by a whole CHARACTER.
    ///
    /// `units.rs` is a *units* file, so a non-ASCII registry entry (`"µm"`,
    /// `"°C"`) is entirely plausible; one such entry plus one non-boundary
    /// occurrence anywhere in ~8MB of chunk prose used to turn the whole
    /// detector into a `byte index is not a char boundary` panic — a
    /// fail-safe-by-contract scanner taken down by corpus content.
    #[test]
    fn contains_word_retry_is_char_stepped_not_byte_stepped() {
        // First occurrence is boundary-rejected (`a` on its left), so the
        // matcher retries from INSIDE the two-byte `µ`. A one-byte step panics
        // here; a char step finds the real occurrence that follows.
        assert!(
            contains_word("aµm µm", "µm"),
            "the standalone `µm` must be found after retrying past the \
             boundary-rejected `aµm`"
        );
        // Same retry path, nothing to find afterwards — must return false, not
        // panic.
        assert!(!contains_word("aµm", "µm"));
        // A multibyte needle whose only occurrence is boundary-clean.
        assert!(contains_word("size °C max", "°C"));
        // A multibyte HAYSTACK with an ASCII needle still matches normally.
        assert!(contains_word("§ union → ok", "union"));
    }

    /// A non-identifier registry token never reaches the census, so it can
    /// never become an `undocumented-name:` no chunk edit could satisfy.
    #[test]
    fn census_excludes_non_identifier_tokens() {
        let regs = vec![Registry {
            const_name: "UNIT_SYMBOL_NAMES".to_string(),
            entries: vec![
                RegistryEntry {
                    name: "µm".to_string(),
                    line: 1,
                    allow: None,
                    allow_missing_reason: false,
                },
                RegistryEntry {
                    name: "m/s".to_string(),
                    line: 2,
                    allow: None,
                    allow_missing_reason: false,
                },
                RegistryEntry {
                    name: "unresolved unit: {}".to_string(),
                    line: 3,
                    allow: None,
                    allow_missing_reason: false,
                },
                RegistryEntry {
                    name: "extrude".to_string(),
                    line: 4,
                    allow: None,
                    allow_missing_reason: false,
                },
            ],
        }];
        let census = census_names(&regs);
        let got: Vec<&str> = census.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            got,
            vec!["extrude"],
            "only the identifier-shaped entry may enter the census"
        );
    }

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

    /// The header lookahead is BOUNDED, and this pins the boundary explicitly
    /// so the limit is a documented behaviour rather than an accident.
    ///
    /// A registry whose opening `[` lands more than `HEADER_LOOKAHEAD` lines
    /// below its header is silently dropped. That is acceptable only because it
    /// is bounded and known: an unbounded forward scan would let a malformed
    /// declaration swallow the rest of the file, which is the worse failure. If
    /// a real registry ever adopts this shape, the registry-path floor guard in
    /// `tests/pdoccover.rs` fails RED (the registry goes missing from the
    /// PRD-named set) — widen the window there, do not relax the guard.
    #[test]
    fn extract_registries_header_lookahead_is_bounded() {
        // Within the window: 3 interposed lines, bracket on the 4th.
        let near = "pub const GEOMETRY_FUNCTION_NAMES: &[&str] =\n\
             \x20   // one\n\
             \x20   // two\n\
             \x20   &[\"box\"];\n";
        assert_eq!(
            const_names(&extract_registries(near)),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "a bracket inside the lookahead window must still be found"
        );

        // Beyond the window: 5 interposed lines. Dropped, by design.
        let far = "pub const GEOMETRY_FUNCTION_NAMES: &[&str] =\n\
             \x20   // one\n\
             \x20   // two\n\
             \x20   // three\n\
             \x20   // four\n\
             \x20   // five\n\
             \x20   &[\"box\"];\n";
        assert!(
            extract_registries(far).is_empty(),
            "a bracket beyond HEADER_LOOKAHEAD is silently dropped — pinned \
             here as a known, bounded limit rather than left to be discovered \
             by a future silent-false-clean census"
        );
    }

    /// A `*_NAMES` const with NO bracketed literal — the alias form
    /// `pub const A_NAMES: &[&str] = B_NAMES;` — must yield no registry, and
    /// must not consume the declaration that follows it.
    ///
    /// The lookahead's failure mode here is not a missing finding but a
    /// MIS-ATTRIBUTED one: latching onto the next declaration's `&[` harvests
    /// that registry's entries under the alias's name and line, and then
    /// `i = j + 1` skips the real declaration entirely — a name leaving the
    /// census silently, which no floor guard can catch (assertion (ii) there
    /// only rejects EMPTY registries, and the mis-bound one is non-empty).
    /// `units.rs` has no alias-form const today; this pins the behaviour before
    /// one arrives.
    #[test]
    fn extract_registries_alias_form_yields_nothing_and_consumes_nothing() {
        let src = "\
pub const ALIAS_NAMES: &[&str] = OTHER_NAMES;
pub const REAL_NAMES: &[&str] = &[
    \"real_one\",
    \"real_two\",
];
";
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["REAL_NAMES"],
            "the alias must contribute no registry AND must not swallow the \
             next declaration; got: {regs:?}"
        );
        assert_eq!(
            entry_pairs(&regs[0]),
            vec![("real_one", 3), ("real_two", 4)],
            "the following registry keeps its own name, entries and \
             provenance lines; got: {:?}",
            regs[0].entries
        );

        // Same shape with the alias LAST — nothing to latch onto, still no
        // registry, and the real one ahead of it is unaffected.
        let trailing = "\
pub const REAL_NAMES: &[&str] = &[\"real_one\"];
pub const ALIAS_NAMES: &[&str] = OTHER_NAMES;
";
        assert_eq!(
            const_names(&extract_registries(trailing)),
            vec!["REAL_NAMES"],
            "a trailing alias declares no names of its own"
        );
    }

    /// A registry declared inside a `#[cfg(test)]` module is a TEST FIXTURE and
    /// must NOT enter the census.
    ///
    /// The real `units.rs` declares two such consts ("local fixtures for name
    /// families that have no pub single-source slice"). Reporting them told a
    /// reader to go document a `#[cfg(test)]` const; worse, a negative-test
    /// fixture holding a deliberately fake name — the natural thing to write —
    /// would become an `undocumented-name:` that no chunk edit could ever
    /// legitimately satisfy.
    #[test]
    fn extract_registries_skips_cfg_test_module() {
        let src = "\
pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\"box\", \"sphere\"];

#[cfg(test)]
mod tests {
    use super::*;

    const BOGUS_NAMES: &[&str] = &[\"not_a_builtin\"];

    #[test]
    fn nested_braces_do_not_end_the_scope() {
        if true {
            let _ = format!(\"unbalanced brace in a literal: {{\");
        }
    }
}
";
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "a `*_NAMES` const inside `#[cfg(test)] mod tests` is a fixture, not \
             a builtin registry, and must not be discovered; got: {regs:?}"
        );
        let all: Vec<&str> = regs.iter().flat_map(|r| entry_names(r)).collect();
        assert!(
            !all.contains(&"not_a_builtin"),
            "no test-fixture name may enter the census; got: {all:?}"
        );
    }

    /// Scope tracking must END with the test module — a production registry
    /// declared AFTER one is still discovered.
    #[test]
    fn extract_registries_resumes_after_cfg_test_module() {
        let src = "\
#[cfg(test)]
mod tests {
    const BOGUS_NAMES: &[&str] = &[\"not_a_builtin\"];
}

pub const DYNAMICS_QUERY_NAMES: &[&str] = &[\"momentum\"];
";
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["DYNAMICS_QUERY_NAMES"],
            "the census must resume once the `#[cfg(test)]` block closes — a \
             scope tracker that never exits would blind the detector for the \
             whole rest of the file; got: {regs:?}"
        );
    }

    /// A `#[cfg(test)]` item that opens NO block (a gated `use`/`const`) must
    /// not capture the next unrelated `{` as if it were a test module.
    #[test]
    fn extract_registries_cfg_test_non_module_item_opens_no_scope() {
        let src = "\
#[cfg(test)]
use std::collections::BTreeSet;

fn helper() {
    let _ = 1;
}

pub const GEOMETRY_FUNCTION_NAMES: &[&str] = &[\"box\"];
";
        let regs = extract_registries(src);
        assert_eq!(
            const_names(&regs),
            vec!["GEOMETRY_FUNCTION_NAMES"],
            "a cfg(test) `use` opens no module, so the following `fn` body must \
             not be treated as test scope; got: {regs:?}"
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

    // -------------------------------------------------------------------
    // chunk_call_mentions — what a chunk CLAIMS the compiler provides
    //
    // Format-agnostic by construction: no heading, fence-tag or table
    // awareness anywhere. That is not laziness — it is what makes the
    // extractor survive a chunk reformat, and what makes the step-17 drift
    // guard satisfiable at all. A markdown-structure-aware extractor would go
    // silently blind the first time someone reflowed a table.
    // -------------------------------------------------------------------

    /// Names only, dropping line numbers.
    fn mention_names(content: &str) -> Vec<String> {
        chunk_call_mentions(content)
            .into_iter()
            .map(|(n, _)| n)
            .collect()
    }

    /// The canonical shape: a call in an inline code span.
    #[test]
    fn mentions_inline_code_span_call() {
        let hits = chunk_call_mentions("- `offset_surface(surface, distance)` — offsets.\n");
        assert_eq!(
            hits,
            vec![("offset_surface".to_string(), 1)],
            "an inline code-span call is a documented API claim"
        );
    }

    /// The real `stdlib.md` shape — several calls on one bold-prefixed line.
    /// All of them are claims, and all report that line.
    #[test]
    fn mentions_several_calls_on_one_line() {
        let hits = chunk_call_mentions(
            "intro\n**Modify:** `fillet(edges, r)`, `chamfer(edges, d)`, `shell(solid, t)`\n",
        );
        assert_eq!(
            hits,
            vec![
                ("fillet".to_string(), 2),
                ("chamfer".to_string(), 2),
                ("shell".to_string(), 2),
            ],
            "every call on the line is a separate claim, all at that line"
        );
    }

    /// Fenced blocks are the densest source of claims. Fence tag and
    /// indentation are both irrelevant — the extractor never looks at them.
    #[test]
    fn mentions_calls_inside_fences_of_any_tag_or_indent() {
        let content = "\
```reify
let s = extrude(profile, 10mm)
```

```reify-schematic
node_at(grid, 3)
```

    ```
    indented_fence_call(x)
    ```
";
        let names = mention_names(content);
        for want in ["extrude", "node_at", "indented_fence_call"] {
            assert!(
                names.contains(&want.to_string()),
                "{want:?} missing — fence tag/indent must not affect \
                 extraction; got {names:?}"
            );
        }
    }

    /// Table cells and ATX headings are ordinary text to this scanner.
    #[test]
    fn mentions_calls_in_table_cells_and_headings() {
        let content = "\
## `revolve(profile, axis, angle)`

| Call | Meaning |
|---|---|
| `loft(sections)` | lofted solid |
";
        let names = mention_names(content);
        for want in ["revolve", "loft"] {
            assert!(
                names.contains(&want.to_string()),
                "{want:?} missing from {names:?}"
            );
        }
    }

    /// CRLF chunks behave identically to LF ones — a line-ending difference
    /// must never change what the detector reports.
    #[test]
    fn mentions_are_line_ending_agnostic() {
        let lf = "intro\n- `fillet(edges, r)` — rounds.\n";
        let crlf = "intro\r\n- `fillet(edges, r)` — rounds.\r\n";
        assert_eq!(
            chunk_call_mentions(crlf),
            chunk_call_mentions(lf),
            "CRLF and LF must produce identical mentions"
        );
        assert_eq!(chunk_call_mentions(lf), vec![("fillet".to_string(), 2)]);
    }

    /// A backticked token with NO parens is a type, module, constant or unit —
    /// not a call. Flagging those would flood the lane with noise from every
    /// prose mention of `Angle` or `pi`.
    #[test]
    fn mentions_ignore_backticked_tokens_without_parens() {
        let content = "\
Import `std.math`. An `Angle` may be `Option` or `pi`.
See the section on booleans (union, difference) below.
";
        assert!(
            chunk_call_mentions(content).is_empty(),
            "only call-SHAPED mentions are claims; got {:?}",
            chunk_call_mentions(content)
        );
    }

    /// `.method(` is member access on a value, not a builtin call — the
    /// fabrication lane has no oracle for methods and would false-positive on
    /// every one of them.
    #[test]
    fn mentions_ignore_method_call_form() {
        let hits = chunk_call_mentions("- `solid.volume()` and `p.distance_to(q)`\n");
        assert!(
            hits.is_empty(),
            "a leading `.` marks member access, not a builtin; got {hits:?}"
        );
    }

    /// `@name(` designates an ad-hoc port on a value, exactly as `.name(`
    /// designates a member — the fabrication lane has no oracle for either, so
    /// `@` marks a selector on a value rather than a free-function claim. The
    /// real `connect.md:48-49` shapes (`docs/reify-language-spec.md:1488`,
    /// §D5: `@face(...)`/`@region(...)` are selector forms, not builtin
    /// calls) must yield nothing.
    #[test]
    fn mentions_ignore_ad_hoc_port_designator_form() {
        let content = "\
connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive
connect pipe@region(outer_surface, z = 0mm..50mm) -> clamp@region(inner_surface)
";
        let hits = chunk_call_mentions(content);
        assert!(
            hits.is_empty(),
            "a leading `@` marks an ad-hoc port designator, not a builtin; got {hits:?}"
        );

        // Pin the boundary: a BARE `face(...)` with no `@` is still a claim —
        // spec §D5 tells authors to replace the deprecated `@face("top")`
        // string form with function-call selectors, so the un-prefixed form
        // must stay visible to the lane.
        let bare = chunk_call_mentions("Use `face(top_surface)` to select it.\n");
        assert_eq!(
            bare,
            vec![("face".to_string(), 1)],
            "an un-prefixed call is still a claim; got {bare:?}"
        );

        // A line mixing both forms: the outer call is a real claim, the
        // `@region(...)` designator on its argument is not.
        let mixed = chunk_call_mentions("let x = translate(pipe@region(outer), 1mm)\n");
        assert_eq!(
            mixed,
            vec![("translate".to_string(), 1)],
            "only the real call survives; got {mixed:?}"
        );
    }

    /// A declaration KEYWORD is never a builtin. `Trait::fn(args)` is the real
    /// `traits.md` shape for explaining static dispatch, and read naively it is
    /// a call to `fn` — which the lane then accused the compiler of not
    /// providing.
    #[test]
    fn mentions_ignore_declaration_keywords_as_names() {
        let content = "\
**Static dispatch** — `Trait::fn(args)`: calls a trait-static function.
A `type(x)` or `unit(y)` in prose is grammar, not a call.
";
        assert!(
            chunk_call_mentions(content).is_empty(),
            "no RI_DECL_KEYWORDS member is a builtin name; got {:?}",
            chunk_call_mentions(content)
        );
    }

    /// Non-function call-shaped syntax that reads like a call but is grammar,
    /// not an API claim. `auto(free)` is the real `parameters.md:13` shape (a
    /// value literal/keyword, spec §2.10:207); `some(...)` is a language-level
    /// `Option` constructor intercepted before general function resolution
    /// (crates/reify-compiler/src/expr.rs:2223-2224, "some() is a
    /// language-level constructor, not a user-defined function" — `none` gets
    /// the same treatment at :1522-1525). Neither is ever a builtin the
    /// compiler could fail to provide.
    #[test]
    fn mentions_ignore_grammar_value_literal_forms() {
        let auto_line =
            "param wall_thickness : Length = auto(free)      // Free exploration mode\n";
        assert!(
            chunk_call_mentions(auto_line).is_empty(),
            "`auto(...)` is a value literal, not a builtin call; got {:?}",
            chunk_call_mentions(auto_line)
        );

        let some_line = "let c : Option<CoatingSpec> = some(spec)\n";
        assert!(
            chunk_call_mentions(some_line).is_empty(),
            "`some(...)` is a language-level Option constructor, not a \
             builtin call; got {:?}",
            chunk_call_mentions(some_line)
        );
    }

    /// The carve-out that stops [`RI_KEYWORDS`] rotting into a false-negative
    /// machine. docs/reify-language-spec.md:221 explicitly lists these as
    /// "Not keywords (standard library functions)" — every one must still be
    /// extracted as a claim when written call-shaped, and none may be a
    /// `RI_KEYWORDS` member. The spec's "Removed keywords" (`derived`,
    /// `require`, `dimension`, :219) are likewise absent — they are not part
    /// of v0.1 at all, so admitting them as keywords would silently disarm a
    /// chunk that (incorrectly) still calls them.
    #[test]
    fn ri_keywords_excludes_the_spec_carve_outs() {
        for name in [
            "determined",
            "constrained",
            "undetermined",
            "partially_determined",
            "point3",
            "vec3",
            "point2",
            "vec2",
            "project",
            "geo_equiv",
        ] {
            assert!(
                !RI_KEYWORDS.contains(&name),
                "{name:?} is a spec §2.10 standard-library function, not a \
                 keyword — it must not be in RI_KEYWORDS"
            );
            let hits = chunk_call_mentions(&format!("`{name}(x)`\n"));
            assert_eq!(
                hits,
                vec![(name.to_string(), 1)],
                "{name:?} must still be extracted as a claim when call-shaped; \
                 got {hits:?}"
            );
        }

        for removed in ["derived", "require", "dimension"] {
            assert!(
                !RI_KEYWORDS.contains(&removed),
                "{removed:?} is a REMOVED keyword (spec :219, not part of \
                 v0.1) — it must not be in RI_KEYWORDS"
            );
        }
    }

    /// `RI_KEYWORDS` must be a strict superset of [`RI_DECL_KEYWORDS`], so
    /// widening the mention-side filter can never regress the declaration-
    /// keyword filter a63c892eea already shipped.
    #[test]
    fn ri_keywords_is_a_superset_of_ri_decl_keywords() {
        for kw in RI_DECL_KEYWORDS {
            assert!(
                RI_KEYWORDS.contains(kw),
                "{kw:?} is in RI_DECL_KEYWORDS but not RI_KEYWORDS — the \
                 widening must never lose a keyword the narrower filter \
                 already had"
            );
        }
    }

    /// A `.ri` declaration line DEFINES a name; it does not claim the compiler
    /// provides one. Example source in a chunk is full of them, and every one
    /// was an accusation against a name the example itself introduced.
    ///
    /// Scoped to the DECLARED name only: a real call inside the declaration's
    /// body is still a claim.
    #[test]
    fn mentions_ignore_the_name_a_declaration_line_introduces() {
        for (label, line) in [
            ("plain fn", "    fn lateral_area(self) -> Scalar<Area>"),
            ("pub fn", "pub fn loss_factor(self) -> Real"),
            (
                "purpose",
                "purpose manufacturing_ready(subject : Structure) {",
            ),
            ("structure def", "structure def bracket(w : Length) {"),
        ] {
            let hits = chunk_call_mentions(&format!("{line}\n"));
            assert!(
                hits.is_empty(),
                "[{label}] a declaration site introduces the name, it does not \
                 claim it exists; got {hits:?}"
            );
        }

        // Only the DECLARED name is dropped — the body's call still counts.
        let hits = chunk_call_mentions("fn area(self) -> Real { rect_area(w, h) }\n");
        assert_eq!(
            hits,
            vec![("rect_area".to_string(), 1)],
            "the declared name goes, the call in its body stays"
        );
    }

    /// The declaration filter is FILE-scoped — a name declared ANYWHERE in
    /// this chunk's own content is example-local, wherever in the chunk it is
    /// later called.
    ///
    /// This is the real `traits.md:77-96` shape: `make_default` and `scaled`
    /// are declared in the example trait body (:78-79) and then CALLED a few
    /// lines further down to demonstrate static dispatch (:94-95). A
    /// declaration-SITE-scoped filter lets both survive on the call, which
    /// was exactly two of #5647's 7 live fabrication findings.
    #[test]
    fn mentions_ignore_a_name_declared_anywhere_in_the_chunk() {
        let content = "\
trait Defaultable {
    fn make_default() -> Length { 10mm }
    fn scaled(factor : Real) -> Length { 10mm * factor }
}

**Instance dispatch** — `obj.(Trait::fn)(args)`: resolves to the conformer's associated function (trait default or per-conformer override).

let wetted = pin.(Cylindrical::lateral_area)()

**Static dispatch** — `Trait::fn(args)`: calls a trait-static function directly; no receiver or conformance relationship required.

let gap : Length = Defaultable::make_default()
let wide : Length = Defaultable::scaled(3.0)
";
        assert!(
            chunk_call_mentions(content).is_empty(),
            "make_default and scaled are declared by this chunk's own \
             example source (lines 2-3); calling them later in the SAME \
             chunk is not a claim that the compiler provides them; got {:?}",
            chunk_call_mentions(content)
        );

        // Negative 1: a name only ever CALLED, never declared, in this
        // content is still a claim.
        let called_only = chunk_call_mentions("let s = extrude(profile, 10mm)\n");
        assert_eq!(
            called_only,
            vec![("extrude".to_string(), 1)],
            "a name this content never declares is still a claim; got {called_only:?}"
        );

        // Negative 2: the filter is per-`content` — a declaration in one
        // chunk must not suppress a call in another. Corpus scope would let
        // one chunk's throwaway example silently disarm every other chunk's
        // claims about that name, an unbounded false-negative amplifier.
        let declaring_chunk = "fn make_default() -> Length { 10mm }\n";
        let other_chunk = "let gap : Length = Defaultable::make_default()\n";
        let _ = chunk_call_mentions(declaring_chunk);
        let other_hits = chunk_call_mentions(other_chunk);
        assert_eq!(
            other_hits,
            vec![("make_default".to_string(), 1)],
            "a declaration in one chunk must not suppress a call in a \
             DIFFERENT chunk; got {other_hits:?}"
        );
    }

    /// An argument list that is literally `(...)` is a schematic placeholder
    /// standing for "any argument of this shape", not a concrete call — the
    /// real `geometry.md:126` shape `translate(primitive(...), 0, 0, -h/2)`
    /// names `primitive` as a metavariable for "any primitive constructor",
    /// not an accusation that the compiler provides a function literally
    /// named `primitive`.
    ///
    /// This is a deliberate false NEGATIVE: a real builtin documented only as
    /// `fillet(...)` would stop being seen. Accepted under the module's
    /// stated asymmetry ("take the miss" — module header, "the existence
    /// oracle is deliberately asymmetric"), and backstopped by
    /// `CHUNK_MENTION_ANCHORS` in tests/pdoccover.rs, which goes RED if the
    /// corpus ever drifts wholesale to the elided form.
    #[test]
    fn mentions_ignore_elided_argument_list() {
        let hits = chunk_call_mentions(
            "When in doubt, prefer the `_centered` variant over a manual \
             `translate(primitive(...), 0, 0, -h/2)`\n",
        );
        assert_eq!(
            hits,
            vec![("translate".to_string(), 1)],
            "the outer call is a real claim; the inner `primitive(...)` is a \
             metavariable, not a claim; got {hits:?}"
        );

        // Pin the boundary tightly — only an argument list that is EXACTLY
        // `...` elides. This is the narrowest and lowest-confidence of the
        // four rules and must not creep.
        for (label, line) in [
            ("empty argument list", "f()\n"),
            ("... plus a real arg", "f(..., x)\n"),
            ("single dot", "f(.)\n"),
            ("double dot", "f(..)\n"),
            // U+2026 HORIZONTAL ELLIPSIS: the real corpus uses only ASCII
            // `...`, so the rule stays keyed to that one byte sequence
            // rather than guessing at Unicode look-alikes.
            ("unicode ellipsis", "f(…)\n"),
        ] {
            let hits = chunk_call_mentions(line);
            assert_eq!(
                hits,
                vec![("f".to_string(), 1)],
                "[{label}] must still be a claim; got {hits:?}"
            );
        }
    }

    /// The chunk-side escape hatch: a line carrying a well-formed
    /// `pdoccover:allow — <reason>` documents something deliberately ahead of
    /// the implementation, and its claims are suppressed.
    #[test]
    fn mentions_suppressed_by_well_formed_allow_marker() {
        let content = "\
- `planned_op(x)` <!-- pdoccover:allow — planned, see #5434 -->
- `real_op(x)`
";
        assert_eq!(
            mention_names(content),
            vec!["real_op".to_string()],
            "the allow-marked line is suppressed, the next line is not"
        );
    }

    /// …but ONLY when the marker carries a reason. A reasonless marker must
    /// leave the mention visible, so the caller can report the malformed
    /// marker instead of silently honouring it (PRD design decision 7).
    #[test]
    fn mentions_not_suppressed_by_reasonless_allow_marker() {
        let content = "- `planned_op(x)` <!-- pdoccover:allow -->\n";
        assert_eq!(
            mention_names(content),
            vec!["planned_op".to_string()],
            "a reasonless marker suppresses nothing — it is itself the defect"
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
