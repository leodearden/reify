//! PTODO — TODO-tracking-invariant detector (structural lane α + liveness lane β).
//!
//! Scans the working tree for TODO-family markers and classifies each through
//! two lanes, all emitting Medium-severity findings:
//!
//! - **Structural lane (α)** — markers not backed by a canonical `#NNNN` task
//!   citation: `untracked` / `malformed-cite` / `phantom-tracking` /
//!   `bare-ignore`. The grammar lives in pure `&str -> result` functions
//!   (mirroring P2's `line_matches_stub`/`scan_file_added_lines` split, no
//!   `regex` dependency per design §12).
//! - **Liveness lane (β)** — every canonical `#NNNN` cite the structural lane
//!   treats as "tracked" is resolved against `.taskmaster/tasks/tasks.db`
//!   (opened read-only): a cite whose status is terminal (done / cancelled) →
//!   `orphaned`; a cite resolving to a non-terminal task with
//!   `metadata.do_not_complete == true` (a permanently-parked anchor) →
//!   `parked-on-anchor` (Medium, advisory); a cite absent from the DB →
//!   `unknown-id`. Per §8.2 one genuinely-live cite (present, non-terminal,
//!   ¬do_not_complete) suffices to track a marker. The lane degrades fail-soft
//!   (§6.7): a missing/unreadable DB is skipped with a single stderr breadcrumb
//!   while the structural lane still runs in full.
//!
//! A single precedence-correct `scan_file` pass feeds both lanes so they
//! never drift. Only file enumeration (`GitOps::ls_files`), content reads
//! (`std::fs::read_to_string`), and the read-only task-DB open touch IO, inside
//! [`check`].
//!
//! Reference: `docs/prds/reify-audit-ptodo-detector.md` §8 (normative grammar),
//! §6.7 (liveness degradation contract).

use crate::{AuditContext, EvidenceRef, Finding, GitCommit, Pattern, Severity};
use reify_test_support::ignore_hygiene::extract_ignore_reason;
use rusqlite::OptionalExtension;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// -----------------------------------------------------------------------
// §8.1 marker recognition (pure, hand-rolled — no `regex` dep per design §12)
// -----------------------------------------------------------------------

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the alphabet for
/// the hand-rolled `\b` word-boundary checks in [`find_comment_marker`].
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `char`-level analogue of [`is_word_byte`] for the `\b` left-boundary check
/// in [`has_malformed_cite`] (which scans `char`s to recognise Greek cites).
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// §8.1 comment markers — canonical regex `\b(TODO|FIXME|HACK)\b\s*[(:]`.
///
/// Case-sensitive uppercase only: lowercase prose ("todo: someday") does not
/// fire (design decision — cuts false positives). The keyword must be a whole
/// word (non-word byte / line edge on both sides, so `XTODO`/`TODONE` miss),
/// optionally followed by whitespace, then `(` or `:`. Returns the matched
/// keyword, or `None`.
fn find_comment_marker(line: &str) -> Option<&'static str> {
    let bytes = line.as_bytes();
    for kw in ["TODO", "FIXME", "HACK"] {
        let klen = kw.len();
        let mut start = 0;
        while let Some(rel) = line[start..].find(kw) {
            let idx = start + rel;
            let after = idx + klen;
            let left_ok = idx == 0 || !is_word_byte(bytes[idx - 1]);
            let right_ok = after >= bytes.len() || !is_word_byte(bytes[after]);
            if left_ok && right_ok {
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] == b'(' || bytes[j] == b':') {
                    return Some(kw);
                }
            }
            start = idx + 1;
        }
    }
    None
}

/// §8.1 Rust stub macros: `todo!(` / `unimplemented!(`. Pure substring scan;
/// the `.rs`-only gating lives in [`scan_file`]. A line whose trimmed start
/// is a `//` comment (`//`, `///`, `//!`) is prose, not a real stub — a
/// commented-out or doc-comment mention (`// todo!() example`) does not fire
/// (mirrors the doc-comment skip in [`ignore_attr`]).
fn find_macro_stub(line: &str) -> bool {
    if line.trim_start().starts_with("//") {
        return false;
    }
    line.contains("todo!(") || line.contains("unimplemented!(")
}

/// The two §8.1 `#[ignore]` shapes the structural lane distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IgnoreForm {
    /// `#[ignore]` — no reason string (α emits `bare-ignore`).
    Bare,
    /// `#[ignore = "..."]` — carries a reason (α defers the reason policy to γ).
    WithReason,
}

/// §8.1 ignore attributes (`.rs` only — gating in [`scan_file`]): a trimmed
/// line that starts with `#[ignore`. `///`/`//!` doc-comment prose mentioning
/// the attribute does not fire. `]` immediately after → `Bare`; `=` →
/// `WithReason`.
fn ignore_attr(line: &str) -> Option<IgnoreForm> {
    let t = line.trim_start();
    if t.starts_with("///") || t.starts_with("//!") {
        return None;
    }
    let rest = t.strip_prefix("#[ignore")?.trim_start();
    if rest.starts_with(']') {
        Some(IgnoreForm::Bare)
    } else if rest.starts_with('=') {
        Some(IgnoreForm::WithReason)
    } else {
        None
    }
}

/// §8.1 δ-A anchor (`.rs` only — gating in [`scan_file`]): an
/// `#[allow(…dead_code…)]` attribute carrying a trailing `//` rationale.
/// Returns the trimmed rationale text, or `None` when the line is not such an
/// attribute or carries no rationale.
///
/// A direct structural sibling of [`ignore_attr`]: `trim_start`, early-return
/// on a doc-comment prefix, then prefix-strip and inspect.
///
/// **Ordering hazard.** The `///`/`//!` guard MUST fire before the `#[allow(`
/// search. `crates/reify-core/src/diagnostics.rs:4046` is a doc comment reading
/// ``/// This function is `#[allow(dead_code)]` pending the live wiring of`` —
/// prose that merely mentions the attribute, with the real attribute 12 lines
/// below. Without the guard, that doc paragraph would be reported as an
/// allow-rationale.
///
/// The lint list is SCANNED for a whole `dead_code` token rather than
/// string-equality-matched, so `#[allow(dead_code, unused_variables)]` is
/// recognised while `#[allow(dead_codex)]` is not.
///
/// Only the `//` rationale form is recognised; a `/* … */` trailing comment is
/// out of scope (unmeasured in the live corpus, so unpinned by evidence).
///
/// **Same-line only — a scope decision, not an oversight.** A rationale on the
/// PRECEDING line (the shape arm (4)'s stub-macro lookback handles for
/// `// #NNNN` \ `todo!()`) is deliberately NOT recognised by δ-A v1. Measured
/// over the live corpus under task #6087: the preceding-line population is **2
/// sites** (`crates/reify-stdlib/src/loads.rs:71`,
/// `crates/reify-stdlib/src/supports.rs:76`) and **both are `///` doc comments**
/// describing the item — the same class the `///`/`//!` guard above already
/// excludes on the same-line form, for the same reason (item documentation is
/// not an attribute rationale). The plain-`//` preceding-line form has **zero**
/// occurrences. Adding the lookback would therefore buy no signal while widening
/// the anchor to doc-comment prose; revisit only if that count moves.
fn allow_dead_code_attr(line: &str) -> Option<&str> {
    let t = line.trim_start();
    // Doc-comment prose mentioning the attribute is not the attribute.
    if t.starts_with("///") || t.starts_with("//!") {
        return None;
    }
    let rest = t.strip_prefix("#[allow(")?;
    let (lints, after) = rest.split_once(")]")?;
    // Whole-token scan of the lint list: split on the separators a lint list
    // can use, so `dead_codex` and a `dead_code` mention in the comment cannot
    // both be read as the lint.
    if !lints
        .split(|c: char| c == ',' || c.is_whitespace())
        .any(|tok| tok == "dead_code")
    {
        return None;
    }
    let comment = after.trim_start().strip_prefix("//")?;
    let rationale = comment.trim();
    (!rationale.is_empty()).then_some(rationale)
}

// -----------------------------------------------------------------------
// §8.2 citation resolution (canonical vs malformed)
// -----------------------------------------------------------------------

/// §8.2 PRD-relative index: `true` when the `#` at `cite_start` (with parsed
/// value `id`) names an index INSIDE a PRD document — a task/invariant/row/
/// open-question number local to some `docs/prds/**` file — rather than a
/// canonical task id.  Inspects ONLY the bytes to the LEFT of the `#`.
///
/// CLAUDE.md's TODO-citation convention already bans this register
/// ("PRD-relative indices (`task-5`) … resolve to `malformed-cite`"); this is
/// the recogniser that implements it for the `#N` spelling.  Three families:
///
/// 1. **Glued PRD-artifact namespace** — `§7#5`, or an uppercase artifact
///    abbreviation (`OQ`/`DD`/`Q`/`T`) with a left word boundary, so an
///    identifier merely ENDING in one of those letters (`ELEMENT#3`) does not
///    match.
/// 2. **Spaced PRD-local noun** — exactly one space between the `#` and an
///    `invariant(s)` / `row(s)` / `boundary` / `open-question` /
///    `design decision` token.
/// 3. **`task(s)`** — exactly one space between the `#` and a `task`/`tasks`
///    token.
///
/// Three properties are load-bearing; each is stated in the code below and
/// pinned by a test, and the live-corpus measurements behind them (per-family
/// histograms, the digit-split enumeration, the re-sweep predicates) live in
/// **PRD §8.2 and §16 Row 2**, which own that methodology:
///
/// - **The `id <= 99` bound is UNIFORM across all three families**, applied
///   once as an early return rather than per family.  It is a property of the
///   PRD-relative REGISTER — a document-local index is small — not of the
///   `task` noun, so a fourth family added later inherits it instead of having
///   to remember it.  It keys on DIGIT COUNT, not on a `PRD` left-context
///   window, because a window measurably fails in BOTH directions (a long path
///   pushes `PRD` out of range; a symmetric window kills the genuine
///   `task #333 per PRD §Slice B`).  Pinned by `prd_relative_cite_negatives`.
/// - **An UNBOUNDED family is fail-DANGEROUS in the one direction §6.6's
///   ratchet cannot see.**  A real task id in family-2 register
///   (`invariant #5238`, `done`) is either DOWNGRADED from a High `orphaned`
///   finding to the Medium advisory `malformed-cite`, or ERASED outright in the
///   cite-anchored δ-B lane — purely on which noun precedes the `#`.  The
///   ratchet asserts only `live ⊆ baseline`, which catches a GAINED finding and
///   never a LOST one, so nothing downstream would report it.  Pinned by
///   `prd_relative_families_are_digit_bounded_end_to_end`.
/// - **Classification is per-`#N`-OCCURRENCE, never per-line.**  Live lines
///   carry both idioms at once, so a per-line verdict would either drop a real
///   cite or resurrect a PRD-relative one.  Pinned by
///   `prd_relative_cite_is_per_occurrence_not_per_line`.
///
/// Widening the family list is deliberately gated: §14/§16's methodology is
/// that every widening arrives with a fresh live-corpus enumeration, a
/// hand-inspected FP count and a dated §16 row.  The non-PRD `#N` registers
/// considered and left out (`edge #N`, `suggestion #N`, `Gap #N`, `site #N`)
/// are listed with their measured exposure in PRD §8.2.
///
/// The G-allow owner-cite lane's own narrower `PRD `-immediately-left check
/// ([`is_g_allow_cite_exempt`] rule (c)) is deliberately NOT refactored to
/// delegate here: it belongs to a different lane with its own
/// `g-allow-orphaned` baseline exposure, and widening it would silently change
/// which owner cites are exempt.  The two grammars are genuinely decoupled —
/// [`extract_g_allow_owner_cites`] runs an independent scan and never calls
/// [`extract_cites`] — so the duplication is contained and is pinned by the
/// pre-existing `extract_g_allow_owner_cites_*` tests staying green.
fn prd_relative_cite(bytes: &[u8], cite_start: usize, id: u32) -> bool {
    // The shared digit bound, applied ONCE for all three families — see the
    // "UNIFORM" and "fail-DANGEROUS" paragraphs above.  Placed here rather than
    // inside a family arm so a family added later cannot forget it.
    if id > 99 {
        return false;
    }

    /// The token alphabet for the spaced-noun families: [`is_word_byte`] plus
    /// `-`, so a hyphenated PRD noun (`open-question`) is read as ONE token.
    fn is_token_byte(b: u8) -> bool {
        is_word_byte(b) || b == b'-'
    }

    /// The whole token ending at `end` (exclusive), scanning left over
    /// [`is_token_byte`].  Returns the token slice and its start offset; the
    /// token is empty when `end` is preceded by a non-token byte.
    fn token_before(bytes: &[u8], end: usize) -> (&[u8], usize) {
        let mut s = end;
        while s > 0 && is_token_byte(bytes[s - 1]) {
            s -= 1;
        }
        (&bytes[s..end], s)
    }

    let left = &bytes[..cite_start];

    // ---- family 1: glued PRD-artifact namespace ------------------------
    // `§<digits/dots>#N` — scan back over the section number, then require the
    // section sign itself (U+00A7 = 0xC2 0xA7).
    let mut s = left.len();
    while s > 0 && (left[s - 1].is_ascii_digit() || left[s - 1] == b'.') {
        s -= 1;
    }
    if s >= 2 && left[s - 2] == 0xC2 && left[s - 1] == 0xA7 {
        return true;
    }
    // `OQ#N` / `DD#N` / `Q#N` / `T#N` — an uppercase artifact abbreviation with
    // a left word boundary.  All candidates are tried: `OQ#1` would fail the
    // boundary check as `Q` (preceded by the word byte `O`) yet passes as `OQ`.
    for abbrev in [b"OQ".as_slice(), b"DD".as_slice(), b"Q".as_slice(), b"T".as_slice()] {
        if let Some(head) = left.strip_suffix(abbrev)
            && head.last().is_none_or(|&b| !is_word_byte(b))
        {
            return true;
        }
    }

    // ---- families 2 and 3: exactly one space, then a PRD-local noun -----
    // "Exactly one space" is the conservative reading: a wider separator rule
    // would classify MORE cites as PRD-relative, and every such classification
    // suppresses a cite, so the narrow form is the fail-safe direction.
    if left.last() != Some(&b' ') || (left.len() >= 2 && left[left.len() - 2] == b' ') {
        return false;
    }
    // Families 2 and 3 share one noun table: family 3's `task`/`tasks` is safe
    // here only because of the hoisted `id > 99` early return at the top of
    // this fn, which is not re-spelled per family.  Compared on the raw bytes
    // (`eq_ignore_ascii_case`) rather than via a lowercased `String`, so
    // classifying a cite allocates nothing — the `decision` arm below always
    // did this, and the two halves now agree.
    const PRD_LOCAL_NOUNS: [&[u8]; 9] = [
        b"invariant",
        b"invariants",
        b"row",
        b"rows",
        b"boundary",
        b"open-question",
        b"design_decision",
        b"task",
        b"tasks",
    ];
    let (token, token_start) = token_before(left, left.len() - 1);
    if PRD_LOCAL_NOUNS.iter().any(|n| token.eq_ignore_ascii_case(n)) {
        return true;
    }
    // `design decision #5` — the bare noun `decision` is only PRD-local when
    // `design` immediately qualifies it (one space, as above).
    token.eq_ignore_ascii_case(b"decision")
        && token_start > 0
        && left[token_start - 1] == b' '
        && token_before(left, token_start - 1)
            .0
            .eq_ignore_ascii_case(b"design")
}

/// §8.2 cite occurrences — the SINGLE `#`+digit-run scanner. Yields
/// `(byte_offset_of_the_hash, id)` for every numerically well-formed cite on
/// the line, in source order.
///
/// [`has_canonical_cite`], [`extract_cites`] and [`has_malformed_cite`]'s `#N`
/// pass are all expressed over this one iterator, so the grammar they share —
/// a run of 1..=5 ASCII digits (a 6-digit number is *not* matched on its
/// 5-digit prefix) whose value is ≥1 (`#0`/`#00` is not a task id) — holds by
/// CONSTRUCTION. It was previously three hand-rolled copies required to stay
/// lock-step by three doc comments, which is a promise rather than a guarantee.
/// The three callers now differ in exactly one thing: what each does with a
/// [`prd_relative_cite`] occurrence — skip it, drop it, or report it.
///
/// A consumed digit run is skipped whole; a malformed run (a bare `#`, `#abc`,
/// `#123456`) advances one byte, so a `#N` immediately after it is still seen.
///
/// The G-allow owner-cite lane deliberately does NOT scan through here — see
/// [`extract_g_allow_owner_cites`], which carries its own exemption grammar.
fn cite_occurrences(line: &str) -> impl Iterator<Item = (usize, u32)> + '_ {
    let bytes = line.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            if bytes[i] != b'#' {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if !(1..=5).contains(&(j - (i + 1))) {
                i += 1;
                continue;
            }
            let at = i;
            i = j; // skip past the consumed digit run
            // A 1..=5-digit ASCII run always fits in u32 (max 99999), so the
            // parse cannot fail. `#0`/`#00` parses to 0 — task ids start at 1,
            // so it is not a cite and is dropped here for every caller.
            if let Ok(id) = line[at + 1..j].parse::<u32>()
                && id >= 1
            {
                return Some((at, id));
            }
        }
        None
    })
}

/// §8.2 canonical citation: `true` when the line carries at least one
/// [`cite_occurrences`] cite (`#` + 1..=5 digits, value ≥1) that is NOT a
/// PRD-relative index.
///
/// A `#N` that [`prd_relative_cite`] recognises names a position inside a PRD
/// document, not a task, so it cannot anchor tracking. The filter is applied
/// per-OCCURRENCE, so a line carrying both idioms still reports the genuine
/// cite. An all-PRD-relative (or all-zero) line falls through to the structural
/// `untracked` / `malformed-cite` classification.
fn has_canonical_cite(line: &str) -> bool {
    let bytes = line.as_bytes();
    cite_occurrences(line).any(|(at, id)| !prd_relative_cite(bytes, at, id))
}

/// §8.2 cite extraction (β liveness lane): every canonical id on the line, in
/// source order.
///
/// Shares [`cite_occurrences`] with [`has_canonical_cite`] and applies the same
/// [`prd_relative_cite`] filter, so the two are lock-step by construction: a
/// cite that is not canonical is also not extracted.
fn extract_cites(line: &str) -> Vec<u32> {
    let bytes = line.as_bytes();
    cite_occurrences(line)
        .filter(|&(at, id)| !prd_relative_cite(bytes, at, id))
        .map(|(_, id)| id)
        .collect()
}

/// `true` when `c` is a Greek-block letter (U+0370..=U+03FF) — the banned
/// Greek-cite alphabet (`task δ`, `task α`).
fn is_greek(c: char) -> bool {
    ('\u{0370}'..='\u{03FF}').contains(&c)
}

/// §8.2/§6.4 malformed citation: the case-insensitive token `task` immediately
/// followed — after an optional single space — by a Greek letter, OR
/// `task-`/`task_`/`task `+ ASCII digit (legacy forms), OR a `#N` sitting in
/// PRD-relative left-context ([`prd_relative_cite`]). Banned from day one; δ
/// migrates valid cites to canonical `#NNNN`.
///
/// The `#N` register is the second half of the §8.2 grammar fix and DELEGATES
/// to [`prd_relative_cite`] rather than re-spelling its three families, so the
/// two halves — "this `#N` is not a canonical cite" (`has_canonical_cite` /
/// `extract_cites`) and "this `#N` is a banned citation form" (here) — cannot
/// drift apart. Without it, a marker line whose only cite is PRD-relative loses
/// its canonical anchor and collapses into `untracked`, which §8.4 rates High
/// (hard gate) where a malformed cite is Medium (advisory) — over-reporting an
/// author who cited imprecisely as untracked debt. CLAUDE.md's TODO-citation
/// convention already mandates the `malformed-cite` disposition for
/// PRD-relative indices; this closes the `#N` spelling of it.
///
/// The `#N` register is scanned in a SEPARATE pass, via the shared
/// [`cite_occurrences`] iterator, rather than being fused into the `char` loop
/// below: the Greek arm needs `char` indices (Greek letters are multi-byte)
/// while [`prd_relative_cite`] takes a byte offset, and fusing the two would
/// require maintaining both cursors in lock-step for no gain. Both passes are
/// O(n) over a single line.
///
/// `is_g_allow_cite_exempt` rule (c) and [`extract_g_allow_owner_cites`] are
/// deliberately NOT refactored to delegate here — the G-allow owner-cite lane
/// has its own narrower `"PRD "`-immediately-left rule and its own
/// `g-allow-orphaned` baseline exposure, so widening it would perturb a
/// decoupled lane. The pre-existing `extract_g_allow_owner_cites_*` tests
/// staying green is the proof of that decoupling.
fn has_malformed_cite(line: &str) -> bool {
    // Pass 1 (§8.2 `#N` register): the shared [`cite_occurrences`] scan, with
    // the verdict INVERTED relative to `has_canonical_cite` — the line is
    // malformed when ANY occurrence lands in PRD-relative left-context. "Any",
    // not "all", because arm (3) consults this only AFTER `has_canonical_cite`
    // has already returned false, so reaching here means no occurrence on the
    // line was a genuine cite.
    let bytes = line.as_bytes();
    if cite_occurrences(line).any(|(at, id)| prd_relative_cite(bytes, at, id)) {
        return true;
    }

    // Pass 2 (Greek + legacy `task-N`/`task_N`/`task N` registers), unchanged.
    let chars: Vec<char> = line.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i + 4 <= n {
        let is_task = chars[i].eq_ignore_ascii_case(&'t')
            && chars[i + 1].eq_ignore_ascii_case(&'a')
            && chars[i + 2].eq_ignore_ascii_case(&'s')
            && chars[i + 3].eq_ignore_ascii_case(&'k');
        // Require a left word boundary so an embedded `task` (e.g. the one
        // inside `multitask 5`) is not misread as a malformed cite — mirrors
        // the `\b` logic in `find_comment_marker`.
        let left_ok = i == 0 || !is_word_char(chars[i - 1]);
        if is_task && left_ok {
            let after = i + 4;
            if after < n {
                let c = chars[after];
                // Greek immediately after `task`.
                if is_greek(c) {
                    return true;
                }
                // Digit form: `task` + (`-` | `_` | ` `) + ASCII digit.
                if (c == '-' || c == '_' || c == ' ')
                    && after + 1 < n
                    && chars[after + 1].is_ascii_digit()
                {
                    return true;
                }
                // Greek after a single space: `task δ`.
                if c == ' ' && after + 1 < n && is_greek(chars[after + 1]) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

// -----------------------------------------------------------------------
// G-allow owner-cite grammar
// -----------------------------------------------------------------------

/// Strip the `// G-allow:` prefix from a source line and return the trailing
/// body when non-blank (mirrors `audit-orphan-producers.sh` `G_ALLOW_RE`
/// `//\s*G-allow:\s*(.+)`, non-blank body). Leading whitespace on the line
/// is tolerated; the returned body has both leading and trailing whitespace
/// trimmed.
///
/// Returns `None` when:
/// - the line (after stripping leading whitespace) is not a `// G-allow:`
///   comment,
/// - or the body after the prefix is blank (whitespace-only or absent).
///
/// The caller passes ONE source line at a time; the returned slice borrows
/// from `line`.
// G-allow: test-facing pub fn (sole external caller: tests/engine_seam_g_allow_cites_live.rs, a separate crate; must stay pub). Pure grammar — no IO.
pub fn g_allow_marker_body(line: &str) -> Option<&str> {
    let s = line.trim_start();
    let s = s.strip_prefix("//")?;
    let s = s.trim_start();
    let s = s.strip_prefix("G-allow:")?;
    let s = s.trim();
    if s.is_empty() { None } else { Some(s) }
}

/// Extract the OWNER `#NNNN` task cites from a G-allow marker body (one
/// `// G-allow:` line's body text, or a joined `const PINS` per-entry
/// comment block).
///
/// Scans the body for canonical `#`+digit-run cites (1..=5 digits, id ≥ 1)
/// and classifies each as OWNER or provenance-EXEMPT:
///
/// - **(a)** The cite is exempt when a case-insensitive `done` or `cancelled`
///   **token** (matched at word boundaries via [`is_word_byte`], so subwords
///   like `"abandoned"` or `"undone"` do NOT match) appears in EITHER:
///   - **(a-following)** the depth-matched parenthetical group immediately
///     following the cite (after optional whitespace), e.g.
///     `"#3870 (κ — TOTS SQP, DONE)"` or `"#2949 (done)"` → exempt; OR
///   - **(a-enclosing)** the innermost parenthetical group enclosing the cite,
///     e.g. `"(task #1234, done)"` → exempt.
///
///   The two terminal strings deliberately mirror `is_terminal_status`'s
///   exact terminal set `{done, cancelled}`, so the textual vocabulary and
///   the DB-status notion of "terminal" stay in lockstep.
/// - **(b)** The text within a **bounded window** — from the last `';'`
///   separator before the cite to the start of the cite (or from the beginning
///   of the body when no `';'` precedes it) — contains a provenance keyword
///   (`re-homed`, `rehomed`, `cancelled`, `superseded`, `formerly`) case-
///   insensitively → **exempt**. The bounded window prevents a provenance
///   annotation on one cite from silently leaking exemption onto a later,
///   unrelated owner cite (e.g. `"re-homed from cancelled #OLD; live owner
///   #NEW"` correctly surfaces `#NEW` as an owner). Note: the keyword list
///   intentionally excludes `"provenance"` alone, because `"(done,
///   provenance)"` in a different cite's parenthetical does NOT exempt the
///   next cite — this is the grammar's known landmine for the
///   `loop_closure_value.rs` case, which is why the hard gate is scoped to
///   the engine-seam set only.
/// - **(c)** Immediately preceded by `"PRD "` (4 chars) — a PRD-section
///   number reference (e.g. `"PRD #2"`) → **exempt**.
///
/// Owner cites are returned in source order, de-duplicated. The caller passes
/// ONE unit (one marker-body line or one joined comment block) — provenance
/// state resets at every unit boundary; never call with a whole file.
// G-allow: test-facing pub fn (sole external caller: tests/engine_seam_g_allow_cites_live.rs, a separate crate; must stay pub). Pure grammar — no IO.
pub fn extract_g_allow_owner_cites(body: &str) -> Vec<u32> {
    let bytes = body.as_bytes();
    let n = bytes.len();
    // Hoist a single O(n) lowercased copy so is_g_allow_cite_exempt does not
    // rebuild it per-cite (was O(n²) with one allocation per cite).
    // `to_ascii_lowercase` maps ASCII→ASCII and leaves non-ASCII chars unchanged,
    // so byte positions are preserved and slicing body_lower at any ASCII boundary
    // (e.g. the byte offset of '#') remains valid.
    let body_lower: String = body.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut owners: Vec<u32> = Vec::new();
    let mut seen = HashSet::<u32>::new();
    let mut i = 0;
    while i < n {
        if bytes[i] == b'#' {
            let digit_start = i + 1;
            let mut digit_end = digit_start;
            while digit_end < n && bytes[digit_end].is_ascii_digit() {
                digit_end += 1;
            }
            let run = digit_end - digit_start;
            if (1..=5).contains(&run) {
                // Parse id; the digit slice is always ASCII so the parse never fails.
                if let Ok(id) = body[digit_start..digit_end].parse::<u32>()
                    && id >= 1
                    && !is_g_allow_cite_exempt(body, bytes, i, digit_end, &body_lower)
                    && seen.insert(id)
                {
                    owners.push(id);
                }
                i = digit_end;
                continue;
            }
        }
        i += 1;
    }
    owners
}

/// Return `true` if `token` (a lowercase ASCII string, e.g. `"done"` or
/// `"cancelled"`) appears as a **whole word** in `s_lower` (a pre-lowercased
/// string slice). Word boundaries are the `[A-Za-z0-9_]` alphabet of
/// [`is_word_byte`]; a token at the start/end of the slice has an implicit
/// boundary there.
///
/// Used by [`is_g_allow_cite_exempt`] rule (a) to test for `done` /
/// `cancelled` inside a parenthetical group without false-matching subwords:
/// `"abandoned"` contains `"done"` but not as a whole word (left boundary
/// fails), and `"undone"` similarly fails the left-boundary check.
fn contains_word_token(s_lower: &str, token: &str) -> bool {
    let bytes = s_lower.as_bytes();
    let n = bytes.len();
    let tlen = token.len();
    let mut start = 0;
    while start + tlen <= n {
        match s_lower[start..].find(token) {
            None => break,
            Some(rel) => {
                let idx = start + rel;
                let after = idx + tlen;
                let left_ok = idx == 0 || !is_word_byte(bytes[idx - 1]);
                let right_ok = after >= n || !is_word_byte(bytes[after]);
                if left_ok && right_ok {
                    return true;
                }
                start = idx + 1;
            }
        }
    }
    false
}

/// Depth-match the parenthetical group whose opening `(` is at `open_paren_idx`
/// in `bytes`, returning the byte index of the matching `)`.  Returns `None`
/// when the group is unclosed (EOF before depth returns to 0).
///
/// Used by [`is_g_allow_cite_exempt`] rule (a) to locate the group boundary so
/// the caller can slice `body_lower` for a terminal-token search.  Only `(` and
/// `)` are ASCII, so byte-offset arithmetic over a UTF-8 body is always safe.
fn find_group_close(bytes: &[u8], open_paren_idx: usize) -> Option<usize> {
    let n = bytes.len();
    let mut depth = 1i32;
    let mut k = open_paren_idx + 1;
    while k < n {
        match bytes[k] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

/// Return `true` if `group_lower` (a pre-lowercased slice of a paren group body
/// — the characters between `(` and `)`, exclusive, or a `;`-bounded sub-window
/// thereof) contains a whole-word `done` or `cancelled` token.  Word boundaries
/// are defined by [`is_word_byte`].
///
/// Called by [`is_g_allow_cite_exempt`] rule (a) via [`find_group_close`].
fn group_has_terminal_token(group_lower: &str) -> bool {
    contains_word_token(group_lower, "done") || contains_word_token(group_lower, "cancelled")
}

/// Internal helper — classify one `#NNNN` cite (hash at `cite_start`, digit
/// run ending at `cite_end`) as provenance-EXEMPT or not.
/// `body_lower` is a pre-computed lowercased copy of `body` (byte-positions
/// preserved for ASCII boundaries).
/// Rules (a)/(b)/(c) documented on [`extract_g_allow_owner_cites`].
fn is_g_allow_cite_exempt(
    _body: &str,
    bytes: &[u8],
    cite_start: usize,
    cite_end: usize,
    body_lower: &str,
) -> bool {
    // Rule (c): immediately preceded by "PRD " (4 ASCII bytes).
    if cite_start >= 4 && &bytes[cite_start - 4..cite_start] == b"PRD " {
        return true;
    }
    // Rule (a): exempt when a case-insensitive `done` or `cancelled` TOKEN
    // (whole-word match via is_word_byte — "abandoned"/"undone" do NOT match)
    // appears in either the FOLLOWING or the ENCLOSING parenthetical group.
    //
    // Both forms mirror is_terminal_status's terminal set {done, cancelled}.
    //
    // All paren-matching is done on raw bytes (parens are single-byte ASCII),
    // so byte-offset arithmetic is safe even over multi-byte UTF-8 bodies.
    // body_lower shares the same byte layout as body (to_ascii_lowercase
    // preserves byte widths), so slicing body_lower at any ASCII byte boundary
    // (paren byte offsets) yields a valid UTF-8 slice.
    let n = bytes.len();

    // (a-following) from cite_end, skip ASCII whitespace; if the next char is
    // '(', depth-match to its closing ')' and check the FULL group for
    // done/cancelled.  The following paren is attached to exactly this cite, so
    // no ';'-bounding is needed.
    {
        let mut j = cite_end;
        while j < n && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < n && bytes[j] == b'(' {
            // Safety: j is at '(' (ASCII 0x28); find_group_close returns Some(k)
            // where k is at ')' (ASCII 0x29) — both valid UTF-8 char boundaries.
            if let Some(close) = find_group_close(bytes, j) {
                let group_start = j + 1;
                if group_start <= close
                    && group_has_terminal_token(&body_lower[group_start..close])
                {
                    return true;
                }
            }
        }
    }

    // (a-enclosing) scan backwards from cite_start for the innermost unclosed
    // '(' (skip matched inner groups by tracking paren depth); if found,
    // depth-match forward to its closing ')'.
    //
    // Apply a ';'-bounded window WITHIN the enclosing group — the same
    // discipline rule (b) uses — so that a multi-cite annotation such as
    // "(re-homed from cancelled #OLD; live owner #NEW)" does NOT leak the
    // `cancelled` token from the #OLD segment onto the live #NEW owner cite.
    // Without this window, the full-group check would incorrectly exempt #NEW.
    {
        let mut depth = 0i32;
        let mut enclosing_open: Option<usize> = None;
        let mut m = cite_start;
        loop {
            if m == 0 {
                break;
            }
            m -= 1;
            match bytes[m] {
                b')' => depth += 1,
                b'(' => {
                    if depth == 0 {
                        enclosing_open = Some(m);
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
        if let Some(open_pos) = enclosing_open {
            // Safety: open_pos is at '(' (ASCII); find_group_close returns Some(k)
            // at ')' (ASCII) — both valid UTF-8 char boundaries.
            if let Some(close) = find_group_close(bytes, open_pos) {
                let group_start = open_pos + 1;
                if group_start <= close {
                    let group_lower = &body_lower[group_start..close];
                    // cite_start is guaranteed within [group_start, close) because
                    // open_pos is the innermost unclosed '(' before cite_start.
                    let cite_offset = cite_start - group_start;
                    // ';'-bounded window: from the last ';' before cite_offset (or
                    // group start) to the next ';' after cite_offset (or group end).
                    let win_start = group_lower[..cite_offset]
                        .rfind(';')
                        .map_or(0, |p| p + 1);
                    let win_end = group_lower[cite_offset..]
                        .find(';')
                        .map_or(group_lower.len(), |p| cite_offset + p);
                    if win_start <= win_end
                        && group_has_terminal_token(&group_lower[win_start..win_end])
                    {
                        return true;
                    }
                }
            }
        }
    }

    // Rule (b): the text within a BOUNDED WINDOW — from the last ';' separator
    // before cite_start to cite_start (or the start of the body when no ';'
    // precedes the cite) — contains a provenance keyword (case-insensitive).
    //
    // Using a bounded window (not the full preceding body) is critical: it
    // prevents a provenance annotation on an earlier cite from silently leaking
    // exemption onto a later, unrelated owner cite. For example, a marker written
    // as "re-homed from cancelled #OLD; live owner #NEW" must surface #NEW as an
    // owner, which requires the window for #NEW to start after the ';' separator.
    //
    // `cite_start` is the byte offset of '#' (ASCII), always a valid char boundary.
    // `body_lower` shares the same byte layout as `body` (to_ascii_lowercase
    // preserves non-ASCII byte widths), so slicing at `cite_start` is safe.
    let preceding_lower = &body_lower[..cite_start];
    let window_start = preceding_lower.rfind(';').map_or(0, |p| p + 1);
    let window = &preceding_lower[window_start..];
    window.contains("re-homed")
        || window.contains("rehomed")
        || window.contains("cancelled")
        || window.contains("superseded")
        || window.contains("formerly")
}

// -----------------------------------------------------------------------
// §8.3 phantom tracking / §6.8 inline escape, allowlist, swept extensions
// -----------------------------------------------------------------------

/// §8.3 phantom-tracking phrases — prose that *claims* a TODO is tracked
/// elsewhere without backing it with a canonical `#NNNN` cite. Matched
/// case-insensitively (lowercase copy) as substrings.
const PHANTOM_PHRASES: &[&str] = &[
    "tracked separately",
    "tracked as a follow-up",
    "tracked in project memory",
    "follow-up task will",
];

/// §8.3 phantom-tracking detection: `true` when the line contains any of the
/// [`PHANTOM_PHRASES`] (case-insensitive). The no-canonical-cite precondition
/// is applied by the caller ([`scan_file`]).
fn phantom_phrase(line: &str) -> bool {
    let lower = line.to_lowercase();
    PHANTOM_PHRASES.iter().any(|p| lower.contains(p))
}

/// §8.3 γ blocker-prose needles — matched case-insensitively (against a
/// lowercased copy of the reason), except `RED:` which is matched
/// case-sensitively against the original to avoid the `required:` false
/// positive (the substring `red:` appears in `required:` when lowercased).
///
/// Trailing spaces on `until ` and `once ` are part of the §8.3 grammar and
/// provide a crude word boundary (so `until` at end-of-string does not match).
const BLOCKER_PROSE: &[&str] = &["pending", "not yet", "until ", "once ", "blocked"];

/// §8.3 γ: `true` when `reason` contains a blocker-prose needle.
///
/// The check is applied to the EXTRACTED reason, not the whole `#[ignore]`
/// line. Five tokens are matched case-insensitively; `RED:` is matched
/// case-sensitively to guard against `required:` false positives.
fn has_blocker_prose(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    if BLOCKER_PROSE.iter().any(|n| lower.contains(n)) {
        return true;
    }
    // `RED:` case-sensitive — `required:` must not match.
    reason.contains("RED:")
}

/// §8.1 δ-A deferral-prose needles — the vocabulary that turns an
/// `#[allow(dead_code)]` rationale into a tracked-work marker.
///
/// Deliberately a SEPARATE const from [`BLOCKER_PROSE`], and deliberately
/// EXCLUDING that set's `once ` and `until `. `BLOCKER_PROSE` is applied to a
/// short EXTRACTED `#[ignore]` reason string, where those two are acceptable;
/// this set is applied to a whole trailing comment, where they explode ("run
/// once manually", "once the cell is built", "valid until the next commit").
/// Keeping the consts separate also guarantees the γ `#[ignore]` reason policy
/// stays byte-identical — mutating `BLOCKER_PROSE` would silently reclassify
/// existing `#[ignore]` findings and perturb the §6.6 baseline.
///
/// All needles are lowercase, and are matched case-SENSITIVELY (guard 1 below).
const DEFERRAL_PROSE: &[&str] = &["pending", "deferred to", "not yet", "blocked on", "awaiting"];

/// §8.1 δ-A: `true` when `text` carries deferral prose — i.e. an occurrence of a
/// [`DEFERRAL_PROSE`] needle that survives all three false-positive guards.
///
/// FP control here is load-bearing, not cosmetic. Every false-positive line
/// measured over the live corpus cites a task that is already `done`, so a
/// spurious match does not merely add noise: it resolves through the β liveness
/// lane to a **High `orphaned`** finding and hard-fails the merge gate. Each
/// guard was derived from a measured class (task #6087), and each is pinned by a
/// verbatim negative in `deferral_prose_negatives`:
///
/// 1. **Case-sensitive, lowercase-only.** No `to_lowercase()` — unlike
///    [`has_blocker_prose`]. `Pending` is the `NodeCache` freshness enum
///    VARIANT, and prose referring to it is not a deferral. Kills six of the
///    seven originally-pinned sites (`crates/reify-eval/src/cache.rs:977`,
///    `:1055`, `:1418`, `:3917`, `:4156`; `engine_demand.rs:110`). This
///    mirrors the existing `RED:` case-sensitivity precedent in
///    [`has_blocker_prose`].
/// 2. **Delimiter guard.** A needle whose immediately preceding or following
///    byte is `"` or a backtick is a quoted state name / code span, not prose.
///    Required for the seventh site, `gui/src-tauri/src/types.rs:1010`
///    (`/// "pending"` …`), which is lowercase and survives guard 1.
/// 3. **Identifier context.** A needle flanked by an ASCII word byte
///    ([`is_word_byte`]) is part of an IDENTIFIER, not prose —
///    `mark_pending_with_cause`, `mark_pruned_pending`
///    (`crates/reify-eval/src/cache.rs:968`, `:3651`). Measured as a real class
///    over the live corpus during task #6087. The same guard also rejects the
///    non-`snake_case` halves of that family, which the first cut missed: a
///    hyphenated compound (`the pending-queue path`) on either side, and a
///    member/path qualification (`self.pending`, `NodeCache::pending`) on the
///    LEFT. The `.`/`:` half is deliberately left-only — see
///    [`disqualifies_before`](has_deferral_prose::disqualifies_before).
///
/// Guards are per-OCCURRENCE, not per-line: a disqualified occurrence is
/// skipped and scanning continues, so a line that both names an identifier and
/// states a real deferral still matches on the latter.
///
/// Do NOT "simplify" this to a lowercased `contains` sweep: that reintroduces
/// class 1 wholesale, and PRD §14 rejected unanchored substring vocabularies at
/// an 89–100% false-positive rate for exactly this reason.
fn has_deferral_prose(text: &str) -> bool {
    /// A byte that disqualifies an adjacent needle occurrence from EITHER side:
    /// `"`/backtick (guard 2 — quoted state name or code span), an ASCII word
    /// byte (guard 3 — the occurrence is inside an identifier), or `-` (guard 3
    /// — a hyphenated compound such as `pending-queue` NAMES a thing rather than
    /// deferring work).
    ///
    /// The word-byte half delegates to the module-shared [`is_word_byte`], the
    /// documented alphabet for the hand-rolled `\b` checks, rather than
    /// re-spelling `is_ascii_alphanumeric() || b == b'_'` — so a future change to
    /// the module's notion of a word byte cannot silently skip this guard.
    fn disqualifies_either_side(b: u8) -> bool {
        b == b'"' || b == b'`' || b == b'-' || is_word_byte(b)
    }

    /// The byte immediately BEFORE a needle occurrence. Everything in
    /// [`disqualifies_either_side`], plus `.` and `:` — a needle qualified by a
    /// member or path separator is a SYMBOL reference, not prose
    /// (`self.pending`, `NodeCache::pending`).
    ///
    /// The `.`/`:` half is deliberately LEFT-ONLY. A trailing `.`/`:` is ordinary
    /// punctuation, and disqualifying it would silently kill the two most
    /// natural ways to write a real deferral — `wiring is pending.` and
    /// `pending: the morph rewrite`. Pinned by
    /// `deferral_prose_trailing_punctuation_still_matches`.
    ///
    /// Absent (start of string) never disqualifies.
    fn disqualifies_before(b: Option<u8>) -> bool {
        b.is_some_and(|b| b == b'.' || b == b':' || disqualifies_either_side(b))
    }

    /// The byte immediately AFTER a needle occurrence — the symmetric half of
    /// [`disqualifies_before`] MINUS `.`/`:` (see that doc). Absent (end of
    /// string) never disqualifies.
    fn disqualifies_after(b: Option<u8>) -> bool {
        b.is_some_and(disqualifies_either_side)
    }

    let bytes = text.as_bytes();
    DEFERRAL_PROSE.iter().any(|needle| {
        // Guard 1: match against the ORIGINAL text, not a lowercased copy.
        text.match_indices(needle).any(|(start, hit)| {
            let before = start.checked_sub(1).map(|i| bytes[i]);
            let after = bytes.get(start + hit.len()).copied();
            !disqualifies_before(before) && !disqualifies_after(after)
        })
    })
}

/// §6.8 inline escape: a line carrying the literal `ptodo:allow` opts out of
/// the whole sweep for that line (an intentional, reviewed marker).
fn line_escaped(line: &str) -> bool {
    line.contains("ptodo:allow")
}

/// §6.8 allowlist path prefixes — paths starting with any entry are exempt
/// from the sweep so the tool never flags its own machinery or test data.
const ALLOWLIST_PREFIXES: &[&str] = &[
    // The detector's own crate: its pattern string-literals (`TODO`/`FIXME`/
    // `HACK`, `task δ`, the phantom phrases, …) and the committed fixtures
    // under `tests/fixtures/ptodo/` would otherwise self-match.
    "crates/reify-audit/",
    // The `#[ignore]`-reason extraction tool: carries `#[ignore]` markers and
    // reason strings as the data it operates on.
    "crates/reify-test-support/src/ignore_hygiene.rs",
    // … and that tool's tests, which embed `#[ignore]` attributes as fixtures.
    "crates/reify-test-support/tests/ignore_reason_hygiene.rs",
    // δ migration sweep (task #4556) confirmed this set is FINAL: the ~198
    // swept findings from the pre-1 inventory all come from real non-self-
    // referential code sites. No additional prefix is warranted — scattered
    // legitimate pattern-string sites across other crates use the inline
    // `ptodo:allow` escape (§6.8) rather than a broad path-prefix exemption.
];

/// §6.8 allowlist check: `true` when `path` (root-relative) starts with any
/// [`ALLOWLIST_PREFIXES`] entry. Reused by `tests/ptodo_baseline.rs` (separate
/// crate — cannot use `pub(crate)`). Mirrors `resolve_liveness`/`fingerprint`.
// G-allow: reused by tests/ptodo_baseline.rs well-formedness test (separate crate; pub(crate) would break it).
pub fn is_allowlisted(path: &str) -> bool {
    ALLOWLIST_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

/// §6.8 swept extensions — the exact set the structural lane scans:
/// `.rs .ri .sh .py .ts .tsx .js`. Non-code/config files (`.md`, `.toml`,
/// `.yaml`, `.json`, …) carry prose, not tracked-work markers, and are skipped
/// (PRD §13 Q1 defers `.toml`/`.yml`/`.yaml` to θ).
// G-allow: reused by tests/ptodo_baseline.rs well-formedness test.
pub fn is_swept_ext(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".rs")
        || lower.ends_with(".ri")
        || lower.ends_with(".sh")
        || lower.ends_with(".py")
        || lower.ends_with(".ts")
        || lower.ends_with(".tsx")
        || lower.ends_with(".js")
}

// -----------------------------------------------------------------------
// §8.3 per-file classification
// -----------------------------------------------------------------------

/// The four structural-lane finding kinds α emits (all Medium severity). The
/// §8.3 `kind` token is carried as a stable summary prefix under the single
/// [`Pattern::PTodo`](crate::Pattern::PTodo) variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A TODO-family marker with no task citation at all.
    Untracked,
    /// A marker citing a task in a banned form (Greek / PRD-relative / legacy).
    MalformedCite,
    /// Prose claiming the work is tracked elsewhere, with no canonical cite.
    PhantomTracking,
    /// A bare `#[ignore]` attribute (no reason string).
    BareIgnore,
}

impl Kind {
    /// The §8.3 kind token, used as the finding summary prefix.
    fn as_str(self) -> &'static str {
        match self {
            Kind::Untracked => "untracked",
            Kind::MalformedCite => "malformed-cite",
            Kind::PhantomTracking => "phantom-tracking",
            Kind::BareIgnore => "bare-ignore",
        }
    }

    /// Per-kind severity mapping (task η, #4559): structural violations that
    /// represent actionable source-marker debt → High (non-zero exit, hard gate);
    /// advisory or citation-style findings → Medium.
    fn severity(self) -> Severity {
        match self {
            // Source-marker debt: a real untracked TODO or bare #[ignore] must
            // be fixed before the code is correct — these are High so they
            // hard-fail verify via reify-audit's exit-code = High-count gate.
            Kind::Untracked | Kind::BareIgnore => Severity::High,
            // Advisory: malformed cites and phantom-tracking phrases are noisy
            // but do not indicate code that is definitively broken — stay Medium.
            Kind::MalformedCite | Kind::PhantomTracking => Severity::Medium,
        }
    }
}

/// The unified per-line classification produced by [`scan_file`]. A given line
/// is either *structurally* offending (no canonical cite → α's domain) or
/// *cited* (a canonical `#NNNN` marker → β's liveness domain). At most one
/// variant per line; lines matching neither produce no entry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineClass {
    /// A structural finding kind (α) — constructed by [`scan_file`].
    Structural(Kind),
    /// A tracked marker carrying one or more canonical `#NNNN` cites (β) — the
    /// liveness lane resolves these ids against the task DB.
    Cited(Vec<u32>),
}

/// §8 per-file scan: walk `content` line-by-line and return one `(line_no,
/// class, marker_text)` entry per offending OR tracked line (1-based line
/// numbers, `marker_text` is the trimmed line). `is_rust` gates the `.rs`-only
/// macro and `#[ignore]` rules. This is the single precedence-correct pass
/// shared by the structural lane and the liveness lane (both driven from
/// [`check`]) so the two never drift.
///
/// Precedence per line (first match wins; at most one entry per line):
/// 1. `ptodo:allow` inline escape → the line is skipped entirely (§6.8).
/// 2. `#[ignore]` (`.rs`): bare → `Structural(BareIgnore)`; reason-bearing →
///    γ reason policy: extract reason via [`extract_ignore_reason`]; if it
///    contains a canonical `#NNNN` cite → `Cited(ids)` (step-8; β liveness);
///    else if it has blocker-prose → `Structural(Untracked)`; else (operational)
///    → no entry. Checked before comment markers so a reason string is not
///    misread as a marker.
/// 3. comment marker (all exts): canonical `#NNNN` → `Cited(on-line cites)`
///    (tracked → β liveness-checks); malformed cite → `Structural(MalformedCite)`;
///    else `Structural(Untracked)`.
/// 4. stub macro (`.rs`): a canonical cite on this line OR the line directly
///    above → `Cited(this-line ∪ above-line cites)` (above-line lookback for the
///    `// #NNNN` \ `todo!()` convention); else `Structural(Untracked)`.
/// 5. lane δ-A (`.rs`): an `#[allow(…dead_code…)]` attribute whose trailing
///    `//` rationale carries deferral prose → canonical cite → `Cited(ids)`;
///    else `Structural(Untracked)`.
/// 6. phantom phrase with no canonical cite → `Structural(PhantomTracking)`.
/// 7. lane δ-B (`.rs`): an ordinary comment line (trimmed, starts `//` — so
///    `//`, `///` and `//!` alike) that carries BOTH a canonical `#NNNN` cite
///    and deferral prose, and is not a `// G-allow:` marker →
///    `Cited(on-line cites)`. Cite-ANCHORED: it emits no structural kind, so an
///    uncited deferral comment produces no entry.
fn scan_file(content: &str, is_rust: bool) -> Vec<(usize, LineClass, String)> {
    let mut out = Vec::new();
    let mut prev: Option<&str> = None;
    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;

        // (1) inline escape — opt this line out of the whole sweep.
        if line_escaped(line) {
            prev = Some(line);
            continue;
        }

        let has_canon = has_canonical_cite(line);

        if is_rust && let Some(form) = ignore_attr(line) {
            // (2) #[ignore] (.rs only). γ reason policy (cite-first, §8.3):
            //   bare → Structural(BareIgnore);
            //   reason-bearing: extract reason;
            //     if it has a canonical cite → Cited(ids) (β liveness);
            //     else if it has blocker-prose → Structural(Untracked);
            //     else (operational) → no entry.
            match form {
                IgnoreForm::Bare => {
                    out.push((line_no, LineClass::Structural(Kind::BareIgnore), line.trim().to_string()));
                }
                IgnoreForm::WithReason => {
                    if let Some(reason) = extract_ignore_reason(line) {
                        if has_canonical_cite(reason) {
                            // cite-first (§8.3): reason contains a canonical #NNNN → β resolves it.
                            out.push((line_no, LineClass::Cited(extract_cites(reason)), line.trim().to_string()));
                        } else if has_blocker_prose(reason) {
                            out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
                        }
                        // else: operational reason → no entry (pass)
                    }
                    // extract_ignore_reason returned None (non-canonical form) → no entry
                }
            }
        } else if find_comment_marker(line).is_some() {
            // (3) comment markers (all swept exts).
            if has_canon {
                // canonical cite → tracked; β resolves the on-line cites. No
                // above-line lookback here (that is a stub-macro convention),
                // so an unrelated cite on the prior line cannot mask this one.
                out.push((line_no, LineClass::Cited(extract_cites(line)), line.trim().to_string()));
            } else if has_malformed_cite(line) {
                out.push((line_no, LineClass::Structural(Kind::MalformedCite), line.trim().to_string()));
            } else {
                out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
            }
        } else if is_rust && find_macro_stub(line) {
            // (4) stub macros (.rs only) with above-line cite lookback.
            let cited_above = prev.is_some_and(has_canonical_cite);
            if has_canon || cited_above {
                // tracked via on-line or above-line cite → β resolves the union.
                let mut ids = extract_cites(line);
                if let Some(p) = prev {
                    ids.extend(extract_cites(p));
                }
                dedup_in_place(&mut ids);
                out.push((line_no, LineClass::Cited(ids), line.trim().to_string()));
            } else {
                out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
            }
        } else if is_rust
            && let Some(rationale) = allow_dead_code_attr(line)
            && has_deferral_prose(rationale)
        {
            // (5) lane δ-A (.rs only): an #[allow(…dead_code…)] attribute whose
            // trailing rationale defers the work is a tracked-work marker.
            //
            // Placement is load-bearing, not stylistic. It sits AFTER arm (3)
            // so a line carrying both the attribute and a real TODO/FIXME
            // marker stays owned by the marker lane — the `else if` chain is
            // what guarantees at-most-one entry per line, which the
            // fingerprint/§6.6 baseline machinery assumes.
            //
            // Prose is matched against the RATIONALE, never the whole line, so
            // the `dead_code` token inside the attribute itself can never be
            // read as comment prose.
            //
            // Emits only the EXISTING classes (§8.3 taxonomy unchanged, no new
            // `Kind`), and the three-way split MIRRORS arm (3) rather than
            // inventing its own: canonical cite → the unmodified β liveness lane
            // (→ orphaned / unknown-id); legacy/Greek cite → malformed-cite;
            // otherwise unmarked debt → untracked.
            //
            // The malformed-cite branch is deliberate. §8.3 defines that trigger
            // LANE-INDEPENDENTLY, and three of this lane's live findings are
            // `// production wiring deferred to task 4050 (…)`
            // (crates/reify-eval/src/engine_build.rs:2199/2278/2292) — the legacy
            // `task NNNN` form. Collapsing them into `untracked` would report an
            // author who cited imprecisely at High (hard gate) where §8.4 rates a
            // malformed cite Medium (advisory).
            //
            // The γ `#[ignore]` arm (2) above does NOT have this branch. That is
            // a divergence, not a precedent to copy: γ's reason policy is
            // cite-first-then-blocker-prose and is byte-frozen (changing it would
            // reclassify existing `#[ignore]` findings and perturb the §6.6
            // baseline), so aligning it is out of scope here rather than
            // unnecessary. See PRD §8.3.
            if has_canonical_cite(rationale) {
                out.push((line_no, LineClass::Cited(extract_cites(rationale)), line.trim().to_string()));
            } else if has_malformed_cite(rationale) {
                out.push((line_no, LineClass::Structural(Kind::MalformedCite), line.trim().to_string()));
            } else {
                out.push((line_no, LineClass::Structural(Kind::Untracked), line.trim().to_string()));
            }
        } else if phantom_phrase(line) && !has_canon {
            // (6) phantom tracking — claim of tracking with no canonical cite.
            out.push((line_no, LineClass::Structural(Kind::PhantomTracking), line.trim().to_string()));
        } else if is_rust
            && line.trim_start().starts_with("//")
            && has_canon
            && has_deferral_prose(line)
            && g_allow_marker_body(line).is_none()
        {
            // (7) lane δ-B (.rs only): an ordinary FULL-LINE comment — no
            // TODO-family marker, no attribute — that both DEFERS work and
            // NAMES the task it is deferred to. The live deliverable is
            // `crates/reify-core/src/diagnostics.rs`'s `HexWedgeMeshOutcome`
            // rustdoc, "blocked on VolumeMesh realization (task #2947)", whose
            // cite is `cancelled`: a promise pinned to a task that will never
            // arrive, invisible to every other arm. Population enumeration, FP
            // measurements and the adoption ruling: PRD §16 Row 2.
            //
            // Five load-bearing choices:
            //
            // (i) APPENDED LAST, after the phantom arm (6), so every earlier arm
            // keeps every line it owned. The `else if` chain is what guarantees
            // at-most-one entry per line, which the `fingerprint` / §6.6
            // baseline machinery assumes.
            //
            // (ii) CITE-ANCHORED, hence NO structural kind. δ-A has an attribute
            // to anchor on and can afford to report the uncited case as
            // `Untracked`; δ-B has only the comment, so the cite IS the anchor —
            // which is what stops the lane firing on every prose comment
            // containing "pending". It therefore emits only `Cited` and reaches
            // ONLY the unchanged β liveness lane, leaving §8.3's taxonomy,
            // `VALID_KINDS` and the §8.4 severity map untouched.
            //
            // (iii) Both predicates match the WHOLE line, not a stripped comment
            // body. Safe here — unlike δ-A, which matches prose against the
            // extracted rationale so the attribute's own `dead_code` token
            // cannot be misread as prose — because on a δ-B line the entire line
            // IS comment text.
            //
            // (iv) The `g_allow_marker_body` guard delegates the ENTIRE
            // `// G-allow:` register to its owner lane, which runs an
            // independent `scan_g_allow_markers` →
            // `resolve_g_allow_owner_liveness` pass with its own
            // `g-allow-orphaned` kind; without it such a line emits TWO findings
            // under two kinds once its owner cite goes terminal (live today at
            // `crates/reify-ir/src/value.rs`, two sites citing #5235).
            //
            // The guard deliberately also covers the case that lane does NOT
            // report: a G-allow line whose cites are ALL provenance-exempt
            // (rules (a)/(b)/(c)) has no owner, so neither lane claims it. That
            // is two rules composing, not a hole — on such a line the owner-cite
            // grammar IS the cite grammar, and `extract_cites` is blind to its
            // exemptions, so admitting the line would anchor δ-B on exactly the
            // cites the sibling grammar classified as provenance (`#N (done)`,
            // `re-homed from cancelled #N`, `PRD #N`) and resurrect the
            // population that grammar exists to suppress — in the fail-dangerous
            // direction, since an FP here is a High `orphaned` hard-gate
            // finding. Measured (2026-08-31): ZERO live lines are
            // `// G-allow:` ∧ cite ∧ deferral prose ∧ owner-less, so the
            // narrower guard would buy no recall today. Pinned by
            // `scan_file_delta_b_negative_g_allow_owner_less`.
            //
            // (v) FULL-LINE comments only (`trim_start().starts_with("//")`): a
            // trailing comment after code (`let x = f(); // deferred to #1234`)
            // is out of scope. Decided, not overlooked — it is what makes the
            // whole-line predicates in (iii) sound, and measured (2026-08-31)
            // exactly one tracked `.rs` line is code-then-trailing-comment with
            // deferral prose and a four-digit cite, whose "code" is the
            // `#[allow(dead_code)]` attribute arm (5) already owns. δ-A reads a
            // trailing comment because its anchor is an attribute, which cannot
            // appear mid-expression. Pinned by
            // `scan_file_delta_b_negative_trailing_comment`.
            //
            // FP control is entirely inherited, not new: `has_deferral_prose`'s
            // guards kill the identifier class (`mark_pending_with_cause`) and
            // §8.2's `prd_relative_cite` kills the PRD-relative class
            // (`deferred to PRD task #10`). Task #6087 rejected this lane at a
            // 48% false-positive rate; those two guards are what changed.
            out.push((line_no, LineClass::Cited(extract_cites(line)), line.trim().to_string()));
        }

        prev = Some(line);
    }
    out
}

/// Order-preserving in-place dedup of cite ids. Cite lists are tiny (1–2
/// elements), so the O(n²) membership scan is cheaper than a `HashSet`.
fn dedup_in_place(ids: &mut Vec<u32>) {
    let mut seen: Vec<u32> = Vec::new();
    ids.retain(|id| {
        if seen.contains(id) {
            false
        } else {
            seen.push(*id);
            true
        }
    });
}

// -----------------------------------------------------------------------
// §6.7 liveness lane — task-DB path resolution
// -----------------------------------------------------------------------

/// §6.7 task-DB path resolution: the `REIFY_PTODO_TASKS_DB` env override (used
/// verbatim when set and non-empty), else `<project_root>/.taskmaster/tasks/
/// tasks.db`. `std::env::var_os` is a *read*, which is safe under edition 2024
/// (unlike `set_var`); tests exercise the override only via subprocess env.
// G-allow: pub for external test callers (tests/engine_seam_g_allow_cites_live.rs) that need the DB path for the live anti-drift guard. Mirrors the resolve_liveness/resolve_inverse pub-for-integration-test pattern.
pub fn tasks_db_path(project_root: &Path) -> PathBuf {
    if let Some(v) = std::env::var_os("REIFY_PTODO_TASKS_DB")
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    project_root.join(".taskmaster/tasks/tasks.db")
}

/// §6.7 read-only open of the task DB. `SQLITE_OPEN_READ_ONLY` never creates
/// the file and errors when it is absent (the degradation trigger), and dodges
/// the URI `file:…?mode=ro` path-escaping fragility on tempdir paths. An
/// existing-but-unreadable DB surfaces later as a prepare error in
/// [`resolve_liveness`], which also degrades.
// G-allow: pub for external test callers (tests/engine_seam_g_allow_cites_live.rs) that open the real tasks.db for the live anti-drift guard. Mirrors the resolve_liveness/resolve_inverse pub-for-integration-test pattern.
pub fn open_tasks_db(path: &Path) -> rusqlite::Result<rusqlite::Connection> {
    rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
}

/// §8.4 terminal statuses: a cite resolving to one of these is "dead" and
/// orphans its marker. Every other present status (pending / in-progress /
/// blocked / deferred) is nominally live — but see `metadata_do_not_complete`:
/// a non-terminal task carrying `do_not_complete == true` is classified as
/// `parked-on-anchor` (Medium) rather than live (task ι, #4644). η flips
/// `orphaned` to High; β keeps all other liveness kinds Medium.
fn is_terminal_status(status: &str) -> bool {
    status == "done" || status == "cancelled"
}

// -----------------------------------------------------------------------
// §6.3 inverse lane — non-terminal tasks citing git-deleted metadata.files paths
// -----------------------------------------------------------------------

/// §6.3 inverse-lane membership test: returns `true` when `path` (trailing-
/// slash-tolerant) is "present in the tracked set" — i.e. it equals a tracked
/// file OR is a directory prefix of some tracked file (a tracked file starts
/// with `path + "/"`). Strips at most one trailing `/` before the checks.
///
/// This guard suppresses the critical FP class where `metadata.files` names
/// a DIRECTORY that still exists (e.g. `crates/reify-audit/tests`): a
/// directory is never a member of the `git ls-files` set, yet
/// `git log -1 -- <dir>` returns non-empty — without this guard, every
/// directory citation would produce a false-positive finding.
fn path_present_in_tracked(path: &str, tracked: &std::collections::HashSet<String>) -> bool {
    // Strip at most one trailing slash for both exact-match and prefix checks.
    let path = path.trim_end_matches('/');
    if tracked.contains(path) {
        return true;
    }
    // Directory-prefix membership: some tracked file lives under `path/`.
    // O(n) scan over the tracked set — acceptable for current backlog sizes
    // because most cited paths hit the O(1) exact-match branch above and only
    // genuinely absent paths reach here. If the tracked set grows very large
    // (tens of thousands of files), consider a sorted Vec<String> +
    // `partition_point`-based prefix search to reduce this to O(log n).
    let prefix = format!("{}/", path);
    tracked.iter().any(|f| f.starts_with(&prefix))
}

/// §6.3 inverse lane: for each non-terminal master task, check each cited
/// `metadata.files` path. A path absent from `tracked` (not an exact tracked
/// file and not a directory prefix of one) is checked for git history via
/// [`crate::GitOps::last_commit_for_path`], giving a three-way outcome:
///
/// - `Some(commit)` **and** that commit renamed the path (per
///   [`crate::GitOps::rename_target_for_path`]) → emit a Medium
///   [`Pattern::PTodo`] `task-cites-renamed-path` finding carrying the task id,
///   BOTH paths, and the renaming commit — the cite is stale but repointable.
/// - `Some(commit)` otherwise → the path was deleted → emit a Medium
///   [`Pattern::PTodo`] `task-cites-deleted-path` finding carrying the task id,
///   the path, and the last-touching commit.
/// - `None` → path never existed → presumed to-be-created → pass (no finding).
///
/// The renamed classification is strictly the narrower one: any rename the git
/// seam cannot resolve (a merge commit, a git error — see
/// [`crate::GitOps::rename_target_for_path`]) degrades to the deleted kind, so
/// the lane never trades one misleading finding for another.
///
/// Fail-soft on DB errors (propagated as `Err` so the caller's
/// `and_then`-based degradation handles them alongside the liveness lane).
/// NULL/malformed/missing `metadata` → empty files list → graceful (no panic).
///
/// Findings are sorted by (task_id, path) for determinism; deleted paths are
/// by definition absent from `tracked` so they never share a key with the
/// structural/liveness (path, line) findings.
// G-allow: test-facing thin pub fn (mirrors resolve_liveness's pattern). MUST stay `pub`: its sole callers are the tests/ptodo.rs integration test binary (a SEPARATE crate — cannot see crate-private items) and check() (same module); `pub(crate)` would break the integration test, `#[cfg(test)]` would hide it from the same external caller.
pub fn resolve_inverse(
    conn: &rusqlite::Connection,
    git: &dyn crate::GitOps,
    tracked: &std::collections::HashSet<String>,
) -> rusqlite::Result<Vec<Finding>> {
    let mut stmt =
        conn.prepare("SELECT id, status, metadata FROM tasks WHERE tag = 'master'")?;

    let rows: Vec<(i64, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut out: Vec<Finding> = Vec::new();
    // Per-run caches: avoid redundant `git log` / `git show` spawns when
    // multiple tasks cite the same absent path (common in larger backlogs where
    // a single deleted or renamed file is referenced by several related tasks —
    // the measured live case was 2 renamed paths cited by 6 tasks).
    let mut git_cache: HashMap<String, Option<GitCommit>> = HashMap::new();
    // Keyed on (cited path, sha) rather than the path alone: within a run the
    // sha IS a pure function of the path (it comes from `git_cache`), but the
    // tuple key is correct without depending on that invariant, and matches the
    // `HashMap<(String, String), _>` shape the GitOps mock uses.
    let mut rename_cache: HashMap<(String, String), Option<String>> = HashMap::new();

    for (id, status, metadata_opt) in rows {
        if is_terminal_status(&status) {
            continue;
        }

        // Parse metadata.files: NULL / malformed / missing key → empty, graceful.
        let files: Vec<String> = metadata_opt
            .and_then(|m| serde_json::from_str::<serde_json::Value>(&m).ok())
            .and_then(|v| v.get("files").and_then(|a| a.as_array()).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        for path in files {
            if path_present_in_tracked(&path, tracked) {
                continue;
            }
            // Path absent from tracked set — check git history (fail-safe: None
            // on any git error → no false positive). Results are memoized to
            // avoid repeated subprocess spawns for the same path across tasks.
            let commit_opt = git_cache
                .entry(path.clone())
                .or_insert_with(|| git.last_commit_for_path(&path))
                .clone();
            if let Some(commit) = commit_opt {
                // Did that same commit RENAME the path rather than delete it?
                // Fail-safe: None on a merge commit, a genuine delete, or any
                // git error → the unchanged deleted-path finding below.
                let rename_target = rename_cache
                    .entry((path.clone(), commit.sha.clone()))
                    .or_insert_with(|| git.rename_target_for_path(&path, &commit.sha))
                    .clone()
                    // Only advertise a target the reader can actually go look
                    // at: a target that was itself renamed again or deleted
                    // falls back to the deleted kind. `path_present_in_tracked`
                    // (not a bare `tracked.contains`) keeps the trailing-slash
                    // and directory-prefix semantics identical to the cited-path
                    // check above, so the two membership tests cannot drift.
                    .filter(|new_path| path_present_in_tracked(new_path, tracked));
                if let Some(new_path) = rename_target {
                    out.push(Finding {
                        pattern: Pattern::PTodo,
                        severity: Severity::Medium,
                        task_id: id.to_string(),
                        summary: format!(
                            "task-cites-renamed-path: task #{id} cites renamed path '{path}' (renamed to '{new_path}' in {sha})",
                            sha = commit.sha,
                        ),
                        evidence: vec![
                            // Only the CITED path: `MetadataFiles` means
                            // "entries from a task's metadata.files", and it is
                            // this lane's path sort key (see the sort below).
                            EvidenceRef::MetadataFiles { entries: vec![path.clone()] },
                            EvidenceRef::File { path: new_path },
                            EvidenceRef::Commit { sha: commit.sha, subject: commit.subject },
                        ],
                    });
                } else {
                    out.push(Finding {
                        pattern: Pattern::PTodo,
                        severity: Severity::Medium,
                        task_id: id.to_string(),
                        summary: format!(
                            "task-cites-deleted-path: task #{id} cites deleted path '{path}' (last touched {sha})",
                            sha = commit.sha,
                        ),
                        evidence: vec![
                            EvidenceRef::MetadataFiles { entries: vec![path.clone()] },
                            EvidenceRef::Commit { sha: commit.sha, subject: commit.subject },
                        ],
                    });
                }
            }
            // None → path never existed → presumed to-be-created → pass.
        }
    }

    // Deterministic order: (task_id parsed as integer, path). Deleted paths are
    // absent from `tracked` so there is no cross-lane sort key collision.
    out.sort_by(|a, b| {
        let id_a = a.task_id.parse::<i64>().unwrap_or(i64::MAX);
        let id_b = b.task_id.parse::<i64>().unwrap_or(i64::MAX);
        let path_a = a
            .evidence
            .iter()
            .find_map(|e| {
                if let EvidenceRef::MetadataFiles { entries } = e {
                    entries.first().cloned()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let path_b = b
            .evidence
            .iter()
            .find_map(|e| {
                if let EvidenceRef::MetadataFiles { entries } = e {
                    entries.first().cloned()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        id_a.cmp(&id_b).then(path_a.cmp(&path_b))
    });

    Ok(out)
}

/// §8.2/§8.3 liveness resolution: per cited marker, resolve each `#NNNN` id's
/// status against the task DB and classify.
///
/// §8.2 multi-cite rule — "one genuinely-live cite suffices for tracking":
/// genuinely-live = present ∧ non-terminal ∧ ¬do_not_complete. If ANY cite
/// is genuinely-live the marker is tracked and emits nothing. Otherwise every
/// dead cite is explained:
/// - present + terminal (done / cancelled) → `orphaned` (High; task η, #4559)
/// - present + non-terminal + `do_not_complete == true` → `parked-on-anchor`
///   (Medium; task ι, #4644): permanently-parked anchor, "parked not promised"
/// - absent → `unknown-id` (Medium; DB-sync race must not hard-fail verify)
///
/// All findings are [`Pattern::PTodo`] with `task_id = path` and a single
/// [`EvidenceRef::File`] ref.
///
/// A statement-prepare error (missing `tasks` table / corrupt DB) is propagated
/// as `Err` so [`check`] degrades fail-soft (§6.7) instead of panicking.
// G-allow: test-facing thin wrapper over `resolve_liveness_keyed`. MUST stay `pub` (not `pub(crate)`/`#[cfg(test)]`): its sole caller is the tests/ptodo.rs integration test — a SEPARATE crate that cannot see crate-private or cfg(test)-gated items — while production `check` calls the keyed variant directly.
pub fn resolve_liveness(
    conn: &rusqlite::Connection,
    cited: &[(String, usize, Vec<u32>, String)],
) -> rusqlite::Result<Vec<Finding>> {
    Ok(resolve_liveness_keyed(conn, cited)?
        .into_iter()
        .map(|(_path, _line, finding)| finding)
        .collect())
}

/// Resolve G-allow OWNER cites for liveness, emitting distinct finding kinds.
///
/// Per `(path, line, owner_cites, text)` input tuple, queries each owner cite
/// against the master-tag `tasks` table:
///
/// - terminal (`is_terminal_status`) → `g-allow-orphaned` [`Severity::High`]
/// - absent from DB → `g-allow-unknown-id` [`Severity::Medium`] (fail-soft;
///   DB-sync race must not hard-fail verify)
/// - live (present and non-terminal) → no finding
///
/// **Every** terminal owner cite is flagged independently — there is no
/// "one genuinely-live cite suffices" exception (unlike [`resolve_liveness`]).
/// Owner semantics require ALL owner cites to be live.
///
/// A statement-prepare error is propagated as `Err`; callers degrade fail-soft.
// G-allow: test-facing pub fn (sole external caller: tests/ptodo.rs + tests/engine_seam_g_allow_cites_live.rs — separate crates that cannot see crate-private items).
pub fn resolve_g_allow_owner_liveness(
    conn: &rusqlite::Connection,
    cited: &[(String, usize, Vec<u32>, String)],
) -> rusqlite::Result<Vec<Finding>> {
    let mut stmt =
        conn.prepare("SELECT status FROM tasks WHERE tag = 'master' AND id = ?1")?;
    let mut out = Vec::new();
    for (path, line, owner_cites, text) in cited {
        for &id in owner_cites {
            let status: Option<String> = stmt
                .query_row(rusqlite::params![id], |row| row.get(0))
                .optional()?;
            match status {
                Some(s) if is_terminal_status(&s) => {
                    out.push(liveness_finding(
                        path,
                        Severity::High,
                        format!("g-allow-orphaned: line {line}: #{id} status={s}: {text}"),
                    ));
                }
                None => {
                    out.push(liveness_finding(
                        path,
                        Severity::Medium,
                        format!("g-allow-unknown-id: line {line}: #{id}: {text}"),
                    ));
                }
                Some(_) => { /* live — no finding */ }
            }
        }
    }
    Ok(out)
}

/// Parse the `metadata` TEXT column (a JSON string) from the `tasks` table and
/// return `true` iff the key `"do_not_complete"` is present and set to `true`.
///
/// Contract: `NULL` metadata (i.e. `None`) → `false`; malformed JSON → `false`;
/// key absent → `false`; `"do_not_complete": false` → `false`. Only the
/// precise structured flag fires — bare `"deferred"` status and
/// `"do_not_dispatch"` alone are both `false` (avoids false-positives on
/// genuine paused/human-owned tasks).
///
/// Mirrors the `resolve_inverse` serde_json parse pattern (ptodo.rs, near
/// `SELECT id, status, metadata FROM tasks WHERE tag='master'`).
fn metadata_do_not_complete(metadata_opt: Option<&str>) -> bool {
    metadata_opt
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        .and_then(|v| v.get("do_not_complete").and_then(|b| b.as_bool()))
        .unwrap_or(false)
}

/// Internal variant of [`resolve_liveness`] that tags each finding with its
/// `(path, line)` sort key, so [`check`] can merge the liveness findings with
/// the structural ones into a single deterministic `(path, line)`-ordered
/// stream. [`resolve_liveness`] is the thin public wrapper that drops the keys;
/// the findings and their order are identical either way.
///
/// **parked-on-anchor** (task ι, #4644): a cite whose task is non-terminal but
/// carries `metadata.do_not_complete == true` (a permanently-parked anchor) is
/// classified as `parked-on-anchor` (Severity::Medium, advisory) rather than
/// live. This preserves §8.2 — genuinely-live is redefined as:
/// present ∧ non-terminal ∧ ¬do_not_complete. A parked anchor co-cited with a
/// genuinely-live task still suppresses all findings (§8.2 one-live-suffices).
fn resolve_liveness_keyed(
    conn: &rusqlite::Connection,
    cited: &[(String, usize, Vec<u32>, String)],
) -> rusqlite::Result<Vec<(String, usize, Finding)>> {
    // §6.7 normative (PRD `reify-audit-ptodo-detector.md` line 181, "rows
    // filtered to `tag='master'`"): the reify task DB uses the single canonical
    // `master` tag context, so a cite is resolved ONLY there. Consequence — an id
    // that exists solely under a non-master tag is invisible to this query and
    // classifies as `unknown-id` (neither tracked nor orphaned); this is the
    // intended master-only semantics, pinned by the integration test
    // `liveness::non_master_tag_resolves_as_unknown_id` (tests/ptodo.rs). Should a
    // multi-tag task DB ever be introduced, revisit this filter alongside §8.2.
    //
    // Extend to read `metadata` alongside `status` so the parked-on-anchor lane
    // (task ι, #4644) can call `metadata_do_not_complete` per cite. The extra
    // column is nullable — a NULL metadata row decodes to `None`, which
    // `metadata_do_not_complete(None)` maps to false (no finding).
    let mut stmt = conn
        .prepare("SELECT status, metadata FROM tasks WHERE tag = 'master' AND id = ?1")?;
    let mut out = Vec::new();

    for (path, line, ids, text) in cited {
        // Resolve every cite once; remember each id's (status, dnc) pair.
        // `status` = None means the id is absent from the DB.
        // `dnc`    = metadata_do_not_complete flag (false when status is None).
        let mut resolved: Vec<(u32, Option<String>, bool)> = Vec::with_capacity(ids.len());
        let mut any_live = false;
        for &id in ids {
            let row: Option<(String, Option<String>)> = stmt
                .query_row(rusqlite::params![id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })
                .optional()?;
            let (status, dnc) = match row {
                Some((s, meta)) => {
                    let dnc = metadata_do_not_complete(meta.as_deref());
                    (Some(s), dnc)
                }
                None => (None, false),
            };
            // Genuinely-live: present AND non-terminal AND not a parked anchor.
            if status.as_deref().is_some_and(|s| !is_terminal_status(s)) && !dnc {
                any_live = true;
            }
            resolved.push((id, status, dnc));
        }

        // §8.2: a single genuinely-live cite tracks the whole marker → no finding.
        if any_live {
            continue;
        }

        for (id, status, dnc) in resolved {
            let finding = match status {
                Some(s) if is_terminal_status(&s) => {
                    // Present and terminal → orphaned.
                    // task η (#4559): orphaned is actionable source-marker debt → High.
                    liveness_finding(
                        path,
                        Severity::High,
                        format!("orphaned: line {line}: #{id} status={s}: {text}"),
                    )
                }
                Some(s) if dnc => {
                    // Present, non-terminal, but do_not_complete=true → parked-on-anchor.
                    // Advisory Medium: a parked anchor is "parked, not promised" —
                    // the cited debt will never resolve but it is not broken work
                    // (task ι, #4644; PRD §8.3/§8.4).
                    liveness_finding(
                        path,
                        Severity::Medium,
                        format!("parked-on-anchor: line {line}: #{id} status={s} (do_not_complete): {text}"),
                    )
                }
                Some(s) => {
                    // Non-terminal and NOT dnc — structurally unreachable: any such cite
                    // would have set `any_live = true` in the resolution loop above, and the
                    // §8.2 `if any_live { continue; }` guard would have skipped this entire
                    // emission loop before reaching here.
                    //
                    // Use debug_assert! so an invariant break surfaces immediately in
                    // debug/test builds while the release-mode audit sweep continues rather
                    // than aborting every running pattern. Fallback: skip this cite silently
                    // (emitting it as `unknown-id` would be confusing since that kind is
                    // documented to mean "absent from the DB"; omission is the safer
                    // release-mode degradation).
                    debug_assert!(
                        false,
                        "genuinely-live cite (present, non-terminal, not do_not_complete) \
                         should have set any_live and been skipped before emission; \
                         id={id}, status={s:?}"
                    );
                    continue;
                }
                // Absent → unknown-id.
                // Stays Medium: a DB-sync race (freshly-filed cite not yet in tasks.db)
                // must not hard-fail verify (PRD §8.4 D-unknown-id).
                None => liveness_finding(
                    path,
                    Severity::Medium,
                    format!("unknown-id: line {line}: #{id}: {text}"),
                ),
            };
            out.push((path.clone(), *line, finding));
        }
    }

    Ok(out)
}

/// Build a PTODO liveness [`Finding`] at `path` with the given severity and summary.
///
/// `severity` is caller-supplied per-kind (task η, #4559): `orphaned` → High;
/// `unknown-id` → Medium. The inverse-lane kinds (`task-cites-deleted-path`,
/// `task-cites-renamed-path`) are always Medium and built directly in
/// [`resolve_inverse`] without calling this helper.
fn liveness_finding(path: &str, severity: Severity, summary: String) -> Finding {
    Finding {
        pattern: Pattern::PTodo,
        severity,
        summary,
        task_id: path.to_string(),
        evidence: vec![EvidenceRef::File { path: path.to_string() }],
    }
}

// -----------------------------------------------------------------------
// §6.6 baseline fingerprint derivation
// -----------------------------------------------------------------------

/// §6.6 baseline fingerprint: the canonical one-line representation of a
/// PTODO finding used to key the committed `ptodo-baseline.txt` ratchet.
///
/// Shape: `{path} :: {kind} :: {text}`
///
/// - `path` = `finding.task_id` (root-relative file path for all PTODO kinds).
/// - `kind` = the summary prefix up to the first `':'` (e.g. `"untracked"`,
///   `"orphaned"`, `"unknown-id"`, `"phantom-tracking"`, …).
/// - `text` = the remainder of the summary after `"{kind}: "`, with an
///   optional leading `"line <digits>: "` segment removed, then internal runs
///   of whitespace folded to a single space and the result trimmed.
///
/// This is the SINGLE canonical derivation that both generates the committed
/// baseline (δ step-11) and computes live fingerprints for the ε ratchet
/// check — keeping the two lock-step and preventing the drift warned about
/// in PRD §6.6.
// G-allow: sole callers are tests/ptodo_baseline.rs (separate crate, cannot use pub(crate)) and check(); mirrors resolve_liveness/resolve_inverse pub-for-integration-test pattern.
pub fn fingerprint(finding: &Finding) -> String {
    let path = &finding.task_id;
    let summary = &finding.summary;

    // Extract `kind`: everything up to the first ':'.
    let (kind, after_kind) = match summary.split_once(':') {
        Some((k, rest)) => (k.trim(), rest),
        None => {
            // Malformed summary — return a best-effort fingerprint rather than
            // panicking; ε's well-formedness test will catch any ill-formed
            // baseline entry.
            return format!("{path} :: {summary} :: ");
        }
    };

    // Strip a leading space after the ':' separator.
    let after_kind = after_kind.strip_prefix(' ').unwrap_or(after_kind);

    // Strip an optional "line <digits>: " prefix (present in structural and
    // liveness findings; absent in the inverse lane's `task-cites-deleted-path`
    // / `task-cites-renamed-path` findings).
    let text_raw = if let Some(rest) = after_kind.strip_prefix("line ") {
        // Consume the digit run and the ": " that follows.
        let end = rest
            .bytes()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let after_digits = &rest[end..];
        after_digits.strip_prefix(": ").unwrap_or(after_digits)
    } else {
        after_kind
    };

    // Fold internal whitespace runs to a single space, then trim.
    let text = fold_whitespace(text_raw);

    format!("{path} :: {kind} :: {text}")
}

/// Fold every internal run of ASCII whitespace in `s` to a single space and
/// trim leading/trailing whitespace. Returns an owned `String`.
fn fold_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = true; // treat leading whitespace as if preceded by space
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    // Trim trailing space (produced when `s` ends with whitespace).
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Return `true` iff `f` is a G-allow finding (summary starts with
/// `"g-allow-"`).  The two G-allow kinds are `g-allow-orphaned` (High —
/// hard gate in both the engine-seam primitive and the repo-wide lane; task
/// η #4559 analogue) and `g-allow-unknown-id` (Medium).
///
/// Used by `ptodo-baseline-gen` and the `(B)` baseline ratchet to exclude the
/// G-allow advisory lane from the source-marker baseline, mirroring the ζ
/// inverse-lane exclusion: G-allow findings are a distinct
/// orphan-suppression-provenance taxonomy (path-keyed, `.rs` files) whose kind
/// strings (`g-allow-*`) are outside `baseline_is_well_formed`'s `VALID_KINDS`
/// set — including them in the baseline would make a regen fail the kind check.
// G-allow: pub for external callers (tests/ptodo_baseline.rs, src/bin/ptodo-baseline-gen.rs —
// separate crates / bins that cannot see crate-private items). Mirrors the
// resolve_liveness/resolve_inverse pub-for-integration-test pattern.
pub fn is_g_allow_finding(f: &Finding) -> bool {
    f.summary.starts_with("g-allow-")
}

// -----------------------------------------------------------------------
// §5 detector entry point — working-tree sweep
// -----------------------------------------------------------------------

/// Scan `content` for `// G-allow:` owner-cite lines, returning
/// `(line_no, owner_cites, line_text)` for every line that carries ≥1 owner
/// cite (after [`extract_g_allow_owner_cites`] classification). Line numbers
/// are 1-based (mirrors [`scan_file`]).
///
/// Used by [`check`] to collect the G-allow advisory lane input before the
/// task-DB open; the caller gates the call on `is_rust` (.rs files only).
fn scan_g_allow_markers(content: &str) -> Vec<(usize, Vec<u32>, String)> {
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let line_no = i + 1;
        if let Some(body) = g_allow_marker_body(line) {
            let owners = extract_g_allow_owner_cites(body);
            if !owners.is_empty() {
                out.push((line_no, owners, line.to_string()));
            }
        }
    }
    out
}

/// Extract the line number embedded in a G-allow finding's summary.
/// Format: `"g-allow-*: line {N}: ..."` — used to reconstruct the
/// `(path, line)` sort key when merging G-allow findings into `keyed`.
fn g_allow_finding_line(f: &Finding) -> usize {
    let parsed = f
        .summary
        .split_once(": line ")
        .and_then(|(_, rest)| rest.split_once(':').map(|(n, _)| n))
        .and_then(|n| n.trim().parse().ok());
    debug_assert!(
        parsed.is_some(),
        "g_allow_finding_line: unexpected summary format (no ': line N:' segment): {:?}",
        f.summary
    );
    parsed.unwrap_or(0)
}

/// RUN evidence for one [`check_with_stats`] sweep — proof that the detector
/// *executed and enumerated files*, independent of whether it *found* anything.
///
/// Counting boundary (pinned by `tests/ptodo.rs`):
/// * `files_scanned` — tracked paths that survived `is_swept_ext(path) &&
///   !is_allowlisted(path)` AND were read successfully. Paths skipped fail-safe
///   because `read_to_string` errored are excluded by construction; a swept file
///   carrying zero markers is INCLUDED.
/// * `markers_examined` — [`scan_file`]-classified marker lines
///   ([`LineClass::Structural`] and [`LineClass::Cited`] alike) across exactly
///   those files. Deliberately EXCLUDES the separate [`scan_g_allow_markers`]
///   advisory pass, so the number stays a property of the one structural sweep.
///
/// Both counters are derived inside the single existing walk — never a second
/// traversal, which would be a second derivation and exactly the drift PRD §6.6
/// exists to prevent.
///
/// These exist so the §6.6 vacuity floor can key on evidence the detector RAN
/// rather than on the live finding count: `files_scanned` is debt-independent
/// (a repo cannot have zero swept tracked files), so burning the debt down to
/// zero can never make the floor fire. Rationale:
/// `docs/prds/reify-audit-ptodo-detector.md` §6.6.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// Swept, non-allowlisted tracked paths that were read successfully.
    pub files_scanned: usize,
    /// [`scan_file`]-classified marker lines across those files.
    pub markers_examined: usize,
}

/// PTODO sweep (§5/§8) — see [`check_with_stats`], of which this is the
/// findings projection (`check_with_stats(ctx).0`). Callers that need the §6.6
/// scan evidence alongside the findings use [`check_with_stats`] directly.
pub fn check(ctx: &AuditContext) -> Vec<Finding> {
    check_with_stats(ctx).0
}

/// PTODO sweep (§5/§8) — structural (α), liveness (β), inverse (ζ), and
/// G-allow advisory (γ-advisory) lanes. Enumerates tracked files via the git
/// seam ([`GitOps::ls_files`](crate::GitOps::ls_files)), keeps only swept
/// extensions that are not allowlisted (§6.8), reads each file's
/// **working-tree** content directly (`std::fs::read_to_string` — only
/// enumeration is a git dependency; the lane "runs everywhere, including
/// worktrees"), and classifies each line via the single [`scan_file`] pass.
///
/// That one pass feeds the structural (α) and liveness (β) lanes:
/// [`LineClass::Structural`] lines become α structural findings;
/// [`LineClass::Cited`] markers are resolved against the task DB by β.
/// For `.rs` files, [`scan_g_allow_markers`] additionally collects
/// `// G-allow:` owner-cite lines for the G-allow owner-cite lane.
///
/// The task DB is opened read-only at [`tasks_db_path`]; when it is absent or
/// unreadable all three DB-backed lanes (β, ζ, G-allow) degrade together under
/// the single §6.7 breadcrumb.  `g-allow-orphaned` (a cite to a terminal task)
/// is a **hard gate** (`Severity::High`, non-zero exit) exactly mirroring PTODO
/// orphaned (task η #4559).  `g-allow-unknown-id` stays `Severity::Medium`
/// (DB-sync race — absent cite must not hard-fail verify).  When tasks.db is
/// absent or unreadable the whole DB-backed lane degrades fail-soft (zero
/// findings, one breadcrumb line on stderr).
///
/// Findings are returned in deterministic `(path, line)` order across all
/// path-keyed lanes; the ζ inverse lane appends as a trailing sorted block.
///
/// Also returns [`ScanStats`] — RUN evidence counted inside this one walk (see
/// that type for the exact counting boundary). [`check`] is the findings-only
/// projection.
// G-allow: pub for cross-crate test callers (tests/ptodo.rs pins the §6.6 scan-evidence counting contract); pub(crate) would break them. The in-file `check` delegation does not count as a caller (same-file callers are excluded by scripts/audit-orphan-producers.sh).
pub fn check_with_stats(ctx: &AuditContext) -> (Vec<Finding>, ScanStats) {
    // §6.6 RUN evidence, accumulated inside the single sweep below — no second
    // walk, no second derivation. See [`ScanStats`].
    let mut stats = ScanStats::default();
    // Structural offenders (α) and cited markers (β) from the single scan_file
    // pass, kept separate so each feeds its own lane.
    let mut struct_hits: Vec<(String, usize, Kind, String)> = Vec::new();
    let mut cited: Vec<(String, usize, Vec<u32>, String)> = Vec::new();
    // G-allow owner-cite lines for the advisory lane (.rs files only).
    let mut g_allow_cited: Vec<(String, usize, Vec<u32>, String)> = Vec::new();

    // Collect ls_files() once: the Vec drives the structural sweep; the HashSet
    // is reused by the ζ inverse-lane membership test without a second git call.
    let tracked_files: Vec<String> = ctx.git.ls_files();
    let tracked_set: HashSet<String> = tracked_files.iter().cloned().collect();

    for path in &tracked_files {
        if !is_swept_ext(path) || is_allowlisted(path) {
            continue;
        }
        // Read the working tree directly (only enumeration is a git seam). Skip
        // unreadable paths fail-safe.
        let content = match std::fs::read_to_string(ctx.project_root.join(path)) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Counted only after a successful read, so fail-safe skips above are
        // excluded from the §6.6 scan evidence by construction.
        stats.files_scanned += 1;
        let is_rust = path.ends_with(".rs");
        for (line_no, class, text) in scan_file(&content, is_rust) {
            stats.markers_examined += 1;
            match class {
                LineClass::Structural(kind) => struct_hits.push((path.clone(), line_no, kind, text)),
                LineClass::Cited(ids) => cited.push((path.clone(), line_no, ids, text)),
            }
        }
        // G-allow advisory lane: scan // G-allow: owner-cite lines (.rs only).
        // The walk already applies is_swept_ext + is_allowlisted, so reify-audit's
        // own markers (allowlisted) self-exclude — consistent with §6.8.
        if is_rust {
            for (line_no, owners, text) in scan_g_allow_markers(&content) {
                g_allow_cited.push((path.clone(), line_no, owners, text));
            }
        }
    }

    // α structural findings, each tagged with its (path, line) sort key.
    let mut keyed: Vec<(String, usize, Finding)> = struct_hits
        .into_iter()
        .map(|(path, line_no, kind, text)| {
            let finding = Finding {
                pattern: Pattern::PTodo,
                severity: kind.severity(),
                summary: format!("{}: line {}: {}", kind.as_str(), line_no, text),
                task_id: path.clone(),
                evidence: vec![EvidenceRef::File { path: path.clone() }],
            };
            (path, line_no, finding)
        })
        .collect();

    // β liveness lane + ζ inverse lane + G-allow advisory lane: open the task
    // DB read-only; on success resolve all three so they degrade together under
    // the single §6.7 breadcrumb.
    // A missing/unreadable DB (open error) OR a prepare/probe failure on an
    // existing-but-corrupt DB (resolve error) degrades all DB-backed lanes fail-soft.
    // The exit class is untouched (Medium-neutral) — 125 is reserved for genuine
    // arg/IO misconfig, never an absent optional substrate.
    let db_path = tasks_db_path(&ctx.project_root);
    let mut inverse_findings: Vec<Finding> = Vec::new();
    match open_tasks_db(&db_path).and_then(|conn| {
        let live = resolve_liveness_keyed(&conn, &cited)?;
        let inv = resolve_inverse(&conn, ctx.git, &tracked_set)?;
        let g_allow = resolve_g_allow_owner_liveness(&conn, &g_allow_cited)?;
        Ok((live, inv, g_allow))
    }) {
        Ok((live, inv, g_allow)) => {
            keyed.extend(live);
            inverse_findings = inv;
            // Insert G-allow findings into keyed so they sort with the other
            // path-keyed lanes.  Severities flow through unchanged from
            // resolve_g_allow_owner_liveness: g-allow-orphaned = Severity::High
            // (hard gate, exactly like PTODO orphaned; task η #4559) and
            // g-allow-unknown-id = Severity::Medium (DB-sync race, fail-soft;
            // same reasoning as PTODO unknown-id staying Medium — PRD §8.4).
            for f in g_allow {
                let line_no = g_allow_finding_line(&f);
                let path = f.task_id.clone();
                keyed.push((path, line_no, f));
            }
        }
        Err(_) => eprintln!(
            "reify-audit: tasks.db unreachable at '{}' — PTODO liveness (β), inverse (ζ), and G-allow advisory lanes degraded; structural checks still run",
            db_path.display()
        ),
    }

    // Deterministic merged order across structural + liveness + G-allow lanes:
    // (path, line). A given line yields at most one structural/liveness entry
    // (scan_file emits one LineClass per line) but may yield one G-allow advisory
    // entry (// G-allow: lines are inert to scan_file — not a TODO/FIXME/HACK
    // marker, #[ignore], or stub — so there is no double-counting between lanes).
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<Finding> = keyed.into_iter().map(|(_path, _line, finding)| finding).collect();
    // ζ inverse findings are already sorted by (task_id, path); append as a
    // deterministic trailing block. Deleted paths are absent from tracked_set
    // so they never share a (path,line) sort key with structural/liveness findings.
    out.extend(inverse_findings);
    (out, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// θ (#4560) ASSESS NO-decision: candidate softer vocabularies reviewed against
    /// the live corpus on 2026-06-15 and **rejected** as detector markers because each
    /// is dominated by legitimate technical usage — recognising them would replicate the
    /// P2/P5 alert-fatigue failure that PRD §6.2 exists to prevent.
    ///
    /// The authoritative per-vocabulary evidence table (occurrence counts, measured FP
    /// rates, dominant benign classes) and §13-Q1 reassessment resolutions are in
    /// `docs/prds/reify-audit-ptodo-detector.md` §14 — that is the single source of
    /// record.  Summary: `XXX`/`placeholder`/`stub` ≈100% FP; `"not yet implemented"`
    /// ≈89% FP; `"for now"`/`"workaround"` high FP.
    ///
    /// This const is the in-code witness that the non-recognition is deliberate, not an
    /// oversight.  Mirrors [`PHANTOM_PHRASES`] / [`BLOCKER_PROSE`] / [`ALLOWLIST_PREFIXES`]
    /// in form; test-scoped so no dead-code lint (the structural lane intentionally never
    /// consults this slice).
    const ASSESSED_REJECTED_VOCAB: &[&str] = &[
        "not yet implemented",
        "for now",
        "workaround",
        "XXX",
        "placeholder",
        "stub",
    ];

    /// Test-only derivation of the structural lane: [`scan_file`] filtered to its
    /// [`LineClass::Structural`] entries (the `Cited` markers — β's domain — drop
    /// out), yielding one `(line_no, kind, text)` per structurally-offending line.
    /// Production [`check`] drives [`scan_file`] directly (it needs the `Cited`
    /// markers this filter discards), so this "structural = scan_file ∩ Structural"
    /// view lives here purely to exercise α's precedence unit tests.
    fn classify_file(content: &str, is_rust: bool) -> Vec<(usize, Kind, String)> {
        scan_file(content, is_rust)
            .into_iter()
            .filter_map(|(line_no, class, text)| match class {
                LineClass::Structural(kind) => Some((line_no, kind, text)),
                LineClass::Cited(_) => None,
            })
            .collect()
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — comment markers
    // -------------------------------------------------------------------

    #[test]
    fn comment_marker_positives() {
        assert_eq!(find_comment_marker("// TODO: x"), Some("TODO"));
        assert_eq!(find_comment_marker("// FIXME(y)"), Some("FIXME"));
        assert_eq!(find_comment_marker("HACK : z"), Some("HACK"));
        assert_eq!(find_comment_marker("# TODO: sh-comment"), Some("TODO"));
    }

    #[test]
    fn comment_marker_negatives() {
        // Followed by space+word, not `(`/`:`.
        assert_eq!(find_comment_marker("// the TODO extractor"), None);
        // Lowercase — case-sensitive uppercase only.
        assert_eq!(find_comment_marker("// todo: someday"), None);
        // No left word boundary (preceded by `X`).
        assert_eq!(find_comment_marker("// XTODO: x"), None);
        // Right boundary violated — `TODONE` is not the whole word `TODO`.
        assert_eq!(find_comment_marker("// TODONE: x"), None);
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — macro stubs (.rs)
    // -------------------------------------------------------------------

    #[test]
    fn macro_stub_positives_and_negative() {
        assert!(find_macro_stub("    todo!()"));
        assert!(find_macro_stub("    unimplemented!(\"later\")"));
        assert!(!find_macro_stub("    let x = compute();"));
        // Commented-out / doc-comment mentions are prose, not real stubs.
        assert!(!find_macro_stub("// todo!() example"));
        assert!(!find_macro_stub("/// returns todo!() placeholder"));
    }

    // -------------------------------------------------------------------
    // §8.3 γ blocker-prose matching — has_blocker_prose
    // -------------------------------------------------------------------

    #[test]
    fn has_blocker_prose_positives() {
        // "pending" — case-insensitive
        assert!(has_blocker_prose("pending fillet binding"));
        assert!(has_blocker_prose("Pending upstream fix"));
        // "not yet" — case-insensitive
        assert!(has_blocker_prose("not yet implemented"));
        assert!(has_blocker_prose("Not Yet ready"));
        // "RED:" — case-SENSITIVE (must stay uppercase)
        assert!(has_blocker_prose("RED: awaiting impl"));
        // "until " — case-insensitive (trailing space is part of needle)
        assert!(has_blocker_prose("ignore until fillet lands"));
        assert!(has_blocker_prose("Until some later date"));
        // "once " — case-insensitive (trailing space is part of needle)
        assert!(has_blocker_prose("run once manually"));
        assert!(has_blocker_prose("Once fixed, remove this"));
        // "blocked" — case-insensitive
        assert!(has_blocker_prose("blocked on upstream"));
        assert!(has_blocker_prose("Blocked by refactor"));
    }

    #[test]
    fn has_blocker_prose_negatives() {
        // Operational reasons — none of the needles present
        assert!(!has_blocker_prose("requires OCCT"));
        assert!(!has_blocker_prose("probe: run manually"));
        assert!(!has_blocker_prose("timing/benchmark out of CI"));
        // Case-sensitivity guard: "required:" contains "red:" in lowercase
        // but must NOT match because RED: is matched case-sensitively.
        assert!(!has_blocker_prose("required: rebuild"));
        // Empty reason
        assert!(!has_blocker_prose(""));
    }

    // -------------------------------------------------------------------
    // §8.1 deferral-prose matching — has_deferral_prose (lane δ-A)
    // -------------------------------------------------------------------

    /// Verbatim rationale substrings from the real evidence sites the δ-A
    /// allow-attribute lane exists to surface. Pinning the exact in-tree source
    /// text (rather than a paraphrase) keeps the user-observable signal covered
    /// at the unit level: if a refactor stops matching any of these, the lane
    /// has silently stopped reporting the debt it was built for.
    #[test]
    fn deferral_prose_positives() {
        // crates/reify-eval/src/engine_build.rs:12891 — the δ-A rationale, and
        // the one cited-orphaned site this task delivers end-to-end.
        assert!(has_deferral_prose(
            "production wiring pending task #4744 (volume-mesh-realization-and-morph-wiring)"
        ));
        // crates/reify-eval/src/engine_build.rs:2199 — δ-A rationale (legacy
        // `task 4050` cite form; the cite grammar is not this function's job).
        assert!(has_deferral_prose(
            "production wiring deferred to task 4050 (in-realization conversion executor)"
        ));
        // Guard 2 is scoped to the bytes adjacent to the NEEDLE, not to any
        // backtick on the line: a rationale that code-spans a symbol name next
        // to (but not around) the needle still matches.
        assert!(has_deferral_prose(
            "`dispatch_volume_mesh` wiring is blocked on the realization rewrite"
        ));
        assert!(has_deferral_prose("awaiting the solver rewrite"));
        assert!(has_deferral_prose("not yet wired into the dispatcher"));
    }

    /// False-positive control. Every line in the first two groups is VERBATIM
    /// from the pinned false-positive set measured over the live corpus, and
    /// each cites a task that is already `done` — so a match would not merely be
    /// noisy, it would resolve through the β liveness lane to a High `orphaned`
    /// finding and hard-fail the merge gate. Guard 1 (case-sensitive,
    /// lowercase-only needles) kills six of the seven; guard 2 (quote/backtick
    /// delimiter) is required for the seventh; guard 3 (word boundary) kills the
    /// identifier class (`mark_pending_with_cause`), measured under task #6087.
    #[test]
    fn deferral_prose_negatives() {
        // --- guard 1: lowercase-only needles ---
        // `Pending` here is the NodeCache freshness enum VARIANT, not prose.
        // crates/reify-eval/src/cache.rs:977
        assert!(!has_deferral_prose(
            "Demand-prune **Pending producer** (task #4739 γ): flip every cached node"
        ));
        // crates/reify-eval/src/cache.rs:1055
        assert!(!has_deferral_prose(
            "builds (task #2592, parity with the Pending guard from task #2451)."
        ));
        // crates/reify-eval/src/cache.rs:1418
        assert!(!has_deferral_prose(
            "is enforced via `assert!` in all builds (task #2592, parity with the Pending"
        ));
        // crates/reify-eval/src/cache.rs:3917
        assert!(!has_deferral_prose(
            "--- set_freshness precondition: Pending is forbidden (task #2451, step-1) ---"
        ));
        // crates/reify-eval/src/cache.rs:4156
        assert!(!has_deferral_prose(
            "#2328): Failed and Pending inputs now produce a Pending output rather than"
        ));
        // crates/reify-eval/src/engine_demand.rs:110
        assert!(!has_deferral_prose(
            "Demand-prune **Pending producer** wired at the Engine facade (task #4739 γ)."
        ));

        // --- guard 2: quote / backtick delimiter ---
        // gui/src-tauri/src/types.rs:1010 — the needle IS lowercase, so guard 1
        // alone lets it through; it is a quoted state name, not deferral prose.
        assert!(!has_deferral_prose(
            "\"pending\"` (task #4739 γ): a hidden body's cell keeps its cached prior"
        ));
        // The BACKTICK half of guard 2, which the `"` case above does not cover:
        // a rationale that code-spans the state name is naming a symbol, not
        // deferring work. (`deferral_prose_positives` pins the converse — a
        // backtick elsewhere on the line does not disqualify.)
        assert!(!has_deferral_prose("uses the `pending` flag internally"));

        // --- guard 3: word boundary (the identifier class) ---
        // A needle occurrence flanked by an ASCII word byte or `_` is part of an
        // identifier, not prose. Both lines below are verbatim from
        // crates/reify-eval/src/cache.rs (:968, :3651); `mark_pending_with_cause`
        // and `mark_pruned_pending` are real symbols there (:926-:968, :1013).
        // Measured under task #6087: this class is why guard 3 exists.
        assert!(!has_deferral_prose(
            "cause via mark_pending_with_cause (task #2330 §9.2 invariant)."
        ));
        assert!(!has_deferral_prose(
            "--- mark_pruned_pending producer tests (task #4739 γ) ---"
        ));
        // Word-boundary symmetry: a trailing word byte disqualifies too, so a
        // needle used as an identifier PREFIX cannot match either.
        assert!(!has_deferral_prose("the pendingWrites queue is drained here"));
        // The non-`snake_case` halves of the same identifier family. These are
        // ordinary explanatory rationales an author would write without any
        // thought of debt, so matching them would hand a future author a red
        // merge gate (`untracked` is High) with no obvious cause.
        // Member access — `.` immediately before the needle.
        assert!(!has_deferral_prose("only set when self.pending is drained"));
        // Path qualification — `:` immediately before the needle.
        assert!(!has_deferral_prose("mirrors NodeCache::pending semantics"));
        // Hyphenated compound — `-` on either side names a thing, it does not
        // defer work.
        assert!(!has_deferral_prose("used by the pending-queue path"));
        assert!(!has_deferral_prose("the not-quite-pending state is transient"));

        // --- real allow-rationales that are NOT deferrals (the dominant benign
        // class among the 68 measured δ-A candidates) ---
        assert!(!has_deferral_prose(
            "used by some, but not all, test binaries that include this module"
        ));
        assert!(!has_deferral_prose("Phase-1 scaffold; consumed in later phases"));
        assert!(!has_deferral_prose(""));
    }

    /// Guard 3's `.`/`:` half is LEFT-ONLY, and that asymmetry is load-bearing:
    /// `.` and `:` are identifier context only when they PRECEDE the needle
    /// (`self.pending`, `NodeCache::pending`, pinned as negatives above). After
    /// it they are ordinary punctuation, and disqualifying them would silently
    /// kill the two most natural ways to write a real deferral. Assert the
    /// surviving direction directly so a "symmetry cleanup" fails loudly.
    #[test]
    fn deferral_prose_trailing_punctuation_still_matches() {
        assert!(has_deferral_prose("volume-mesh wiring is pending."));
        assert!(has_deferral_prose("pending: the morph rewrite"));
        assert!(has_deferral_prose("awaiting: the solver rewrite"));
        // Sanity: the same needles WITH the punctuation on the left do not match.
        assert!(!has_deferral_prose("mirrors NodeCache::pending"));
        assert!(!has_deferral_prose("only set when self.pending"));
    }

    /// Case-sensitivity is load-bearing, not incidental. `has_blocker_prose`
    /// lowercases its input; copying that here would reintroduce the entire
    /// `Pending`-enum-variant false-positive class pinned above. Assert the
    /// asymmetry directly so such a "simplification" fails loudly.
    #[test]
    fn deferral_prose_is_case_sensitive() {
        assert!(has_deferral_prose("pending"));
        assert!(!has_deferral_prose("Pending"));
        assert!(!has_deferral_prose("PENDING"));
        assert!(has_deferral_prose("blocked on"));
        assert!(!has_deferral_prose("Blocked on"));
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — ignore attributes (.rs)
    // -------------------------------------------------------------------

    #[test]
    fn ignore_attr_forms() {
        assert_eq!(ignore_attr("#[ignore]"), Some(IgnoreForm::Bare));
        assert_eq!(ignore_attr("#[ignore = \"r\"]"), Some(IgnoreForm::WithReason));
        // Indented bare form still recognised (trimmed-line start).
        assert_eq!(ignore_attr("    #[ignore]"), Some(IgnoreForm::Bare));
        // Doc-comment prose mentioning the attribute must NOT fire.
        assert_eq!(ignore_attr("/// #[ignore]"), None);
    }

    // -------------------------------------------------------------------
    // §8.1 marker recognition — allow(dead_code) attributes (.rs, lane δ-A)
    // -------------------------------------------------------------------

    /// Real in-tree shapes the δ-A anchor must recognise. The returned value is
    /// the trailing `//` RATIONALE, never the whole line — matching prose
    /// against the whole line would let the `dead_code` token inside the
    /// attribute itself be read as comment text.
    #[test]
    fn allow_dead_code_attr_positives() {
        // crates/reify-eval/src/engine_build.rs:12891 (shape).
        assert_eq!(
            allow_dead_code_attr(
                "#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh)"
            ),
            Some("production wiring pending task #4744 (volume-mesh)")
        );
        // Indented — the recogniser trims like `ignore_attr` does.
        assert_eq!(
            allow_dead_code_attr("    #[allow(dead_code)] // consumed by #4744 step-20"),
            Some("consumed by #4744 step-20")
        );
        // Multi-lint list: the bracketed lints must be SCANNED, not
        // string-equality-matched against `#[allow(dead_code)]`.
        assert_eq!(
            allow_dead_code_attr("#[allow(dead_code, unused_variables)] // pending #1234"),
            Some("pending #1234")
        );
        // Whitespace inside the lint list, and extra spacing before the comment.
        assert_eq!(
            allow_dead_code_attr("#[allow( unused_variables , dead_code )]   // pending #1234"),
            Some("pending #1234")
        );
    }

    /// Negative pins. The `///`/`//!` case is the load-bearing one: it is
    /// verbatim-shaped after `crates/reify-core/src/diagnostics.rs:4046`, a doc
    /// comment that merely MENTIONS the attribute in backticks (the real
    /// attribute is 12 lines below). Attributing that line to δ-A would report
    /// a doc paragraph as an allow-rationale.
    #[test]
    fn allow_dead_code_attr_negatives() {
        assert_eq!(
            allow_dead_code_attr("/// This function is `#[allow(dead_code)]` pending the wiring"),
            None
        );
        assert_eq!(
            allow_dead_code_attr("//! module prose about `#[allow(dead_code)]` pending work"),
            None
        );
        // Bare attribute, no trailing rationale → nothing to classify.
        assert_eq!(allow_dead_code_attr("#[allow(dead_code)]"), None);
        // Trailing whitespace only — still no rationale.
        assert_eq!(allow_dead_code_attr("#[allow(dead_code)]   "), None);
        // `dead_code` absent from the lint list.
        assert_eq!(allow_dead_code_attr("#[allow(unused)]  // pending #1234"), None);
        // `dead_code` appears only in the COMMENT, not the lint list.
        assert_eq!(
            allow_dead_code_attr("#[allow(deprecated)] // dead_code appears only in prose here"),
            None
        );
        // Whole-token match: `dead_codex` is a different lint name.
        assert_eq!(allow_dead_code_attr("#[allow(dead_codex)] // pending #1234"), None);
        // Not an attribute line at all.
        assert_eq!(allow_dead_code_attr("let x = 1; // pending #1234"), None);
    }

    // -------------------------------------------------------------------
    // §8.2 citation resolution — canonical `#NNNN`
    // -------------------------------------------------------------------

    #[test]
    fn canonical_cite_positives() {
        assert!(has_canonical_cite("// TODO(#42): x"));
        assert!(has_canonical_cite("see #4553"));
        assert!(has_canonical_cite("#1"));
        assert!(has_canonical_cite("#12345 five digits"));
    }

    #[test]
    fn canonical_cite_negatives() {
        assert!(!has_canonical_cite("bare # alone"));
        assert!(!has_canonical_cite("#abc not digits"));
        // 6-digit run exceeds the 1..=5 window — not a 5-digit prefix match.
        assert!(!has_canonical_cite("#123456 six digits"));
        // Space between `#` and digits.
        assert!(!has_canonical_cite("# 42"));
        // All-zero runs (`#0`, `#00`) are not valid task ids (ids start at 1).
        assert!(!has_canonical_cite("// TODO(#0): x"));
        assert!(!has_canonical_cite("see #00 here"));
    }

    // -------------------------------------------------------------------
    // §8.2 cite extraction (β liveness lane) — `extract_cites`
    // -------------------------------------------------------------------

    #[test]
    fn extract_cites_collects_all_canonical_ids() {
        // A single parenthesised cite.
        assert_eq!(extract_cites("// TODO(#42): x"), vec![42]);
        // Multiple bare cites in source order.
        assert_eq!(extract_cites("see #1 and #200"), vec![1, 200]);
        // Leading zeros are tolerated as long as the value is ≥1 (`#007` → 7).
        assert_eq!(extract_cites("// TODO(#007): x"), vec![7]);
    }

    #[test]
    fn extract_cites_rejects_non_cites() {
        // `#` followed by non-digits → no cite.
        assert_eq!(extract_cites("#abc"), Vec::<u32>::new());
        // A bare `#` at line end → no cite.
        assert_eq!(extract_cites("bare #"), Vec::<u32>::new());
        // A 6-digit run exceeds the 1..=5 window (consistent with
        // has_canonical_cite) → no cite (not a 5-digit prefix match).
        assert_eq!(extract_cites("#123456"), Vec::<u32>::new());
        // An all-zero run is not a valid task id (ids start at 1) → no cite, so
        // `#0` falls through to the structural `untracked` classification.
        assert_eq!(extract_cites("#0"), Vec::<u32>::new());
        assert_eq!(extract_cites("// TODO(#00): x"), Vec::<u32>::new());
    }

    // -------------------------------------------------------------------
    // §8.2 PRD-relative indices — the non-canonical `#N` register
    //
    // A PRD-relative `#N` (`PRD task #10`, `invariant #2`, `§7#5`) names an
    // index INSIDE a PRD document, not a task id.  CLAUDE.md's TODO-citation
    // convention already rules these out ("PRD-relative indices … resolve to
    // `malformed-cite`"); these tests pin that the shared cite grammar
    // implements it.  Measured basis for the population and the digit bound
    // lives on `prd_relative_cite`.
    // -------------------------------------------------------------------

    /// Every measured PRD-relative form must be invisible to BOTH halves of the
    /// cite grammar — `has_canonical_cite` false AND `extract_cites` empty —
    /// because the two are required to stay lock-step.
    ///
    /// The first six are the class-(b) false positives lane δ-B would otherwise
    /// report at High `orphaned` (all three ids resolve to REAL `done` tasks, so
    /// each spuriously orphans); they are pinned VERBATIM from the live corpus.
    /// The remainder are the sibling forms found by the same catalogue sweep.
    #[test]
    fn prd_relative_cite_positives() {
        let lines = [
            // ---- class (b), verbatim from the live corpus ----
            // crates/reify-stdlib/src/fea.rs
            "/// Diagnostic emission is deferred to PRD task #10 (Diagnostic mapping for",
            // crates/reify-stdlib/src/fea.rs
            "/// Diagnostic emission is deferred to PRD task #10.",
            // crates/reify-stdlib/src/fea.rs
            "///    is deferred to PRD task #10 (Diagnostic mapping for multi-case-",
            // crates/reify-stdlib/src/fea.rs
            "// Empty Map → Undef. Diagnostic emission deferred to PRD task #10",
            // crates/reify-solver-elastic/src/boundary/dirichlet.rs
            "/// a uniaxial-stretch scenario is deferred to the downstream PRD task #12",
            // crates/reify-eval/src/geometry_ops.rs
            "//   is not yet a hydrated Value::GeometryHandle (PRD invariant #2:",
            // ---- sibling forms from the catalogue re-sweep ----
            // Bare `invariant #N` needs no `PRD` token — all 52 repo-wide
            // occurrences are single-digit PRD-local (engine_build.rs).
            "///    `topology_attribute_table` debug_assert (invariant #4) never runs.",
            "//! …) — all §7 rows #1 pinned end-to-end.",
            // Glued PRD-artifact namespaces (`§X#N`, `Q#N`, `OQ#N`, `DD#N`, `T#N`).
            "//! Step 3 tests: §7#5 positive path + all-three-kind parity + non-regression.",
            "    // Repeatable per PRD §11 Open Q#4: each --purpose occurrence is one",
            "/// returns a VALUE (Type::Feature), not a Selector — PRD D1 OQ#1.",
            "/// subprocess (never FFI, PRD DD#4), composes a deterministic settings profile,",
            "//! consumer (PRD T#11). Output is a [`crate::assembly::ElementStiffness`]",
            // Spaced PRD-local nouns.
            "//!   task μ (PRD §10 open-question #2).",
            "//! lib.rs (see task 2035 design decision #5): both are used only by",
            "// PRD docs/prds/v0_6/stdlib-namespace.md §7 boundary #3. The observable the PRD",
            // Plural `tasks` under the ≤ 99 bound.
            "/// until PRD tasks #10 land the engine",
        ];
        for line in lines {
            assert!(
                !has_canonical_cite(line),
                "PRD-relative index must not be a canonical cite: {line}"
            );
            assert_eq!(
                extract_cites(line),
                Vec::<u32>::new(),
                "PRD-relative index must extract no ids: {line}"
            );
        }
    }

    /// The mirror image: the rule must not suppress a single GENUINE cite.
    /// Corpus-derived shapes plus synthetic four-digit controls — the
    /// `#4553` family below is deliberately synthetic, because family 1 has
    /// ZERO live four-digit exposure and pinning it is what makes the bound a
    /// property rather than luck.
    ///
    /// Provenance comments name a FILE, never a line: nothing verifies a line
    /// number here, and this test already caught one rotting (the
    /// `detectors.rs` entry, whose prose this branch itself rewrote).
    #[test]
    fn prd_relative_cite_negatives() {
        let cases: &[(&str, u32)] = &[
            // crates/reify-compute-contract/src/elastic_result.rs
            ("/// The `.ri` / gate exposure is deferred to consumer task #3787.", 3787),
            // crates/reify-core/src/diagnostics.rs
            ("/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring", 2947),
            // crates/reify-eval/src/compute_targets/elastic_static.rs
            ("/// resolution (deferred to P2 / task #4092): reads each support's raw", 4092),
            // crates/reify-eval/src/engine_build.rs
            ("/// deferred to task ζ (#3437, Manifold execute arm) + new cross-kernel", 3437),
            // crates/reify-eval/src/detectors.rs — the class-(c) line this
            // branch truth-corrected, quoted at its CURRENT text ("resolved
            // by", not the pre-fix "deferred to"). The correction changed the
            // prose, never the citation, which is what this case asserts.
            ("// comment for that drift-risk trade-off (resolved by task μ, #5062).", 5062),
            // crates/reify-eval/src/engine_edit.rs
            ("//      not yet in any `diff_*` helper (tracked by #4686);", 4686),
            // The three-digit legacy ids that must survive the `task #N ≤ 99`
            // guard — the only genuine sub-4-digit `task #N` cites in the repo.
            // crates/reify-compiler/src/stdlib_loader.rs
            ("        // Reconstruction of lost work from task #333 per PRD §Slice B.", 333),
            // crates/reify-lsp/tests/incremental_eval_benchmark.rs
            ("//! Re-establishes the deliverable from task #479 that was lost when commit 00a86da53", 479),
            // crates/reify-expr/tests/field_eval_tests.rs
            ("/// where inner_field is None (a separate task #630 adds FieldSourceKind::Gradient", 630),
            // ---- the digit bound must be UNIFORM across all three families ----
            // The one tracked line repo-wide that puts a REAL task id in
            // family-2 register: `git grep -nE '(invariants?|rows?|boundary)
            // #[0-9]{3,}'` over ALL tracked files returns exactly this hit, and
            // #5238 is `done` — terminal — and owns that very file.  An
            // unbounded family does not merely mute such a cite: on a marker
            // line it DOWNGRADES a High `orphaned` hard-gate finding to the
            // Medium advisory `malformed-cite`, and in the cite-anchored δ-B
            // lane it ERASES the candidate outright.  §6.6's ratchet cannot see
            // either — `live ⊆ baseline` catches a GAINED finding, never a LOST
            // one — so the bound has to be pinned here.
            // crates/reify-eval/tests/engine_eval_commit_migration.rs
            ("/// This is the invariant #5238 nearly lost: both let evaluators used to emit", 5238),
            // Synthetic four-digit forms for every family that carried no
            // bound of its own.  Family 1 has ZERO live four-digit exposure
            // (measure with the detector's own allowlisted crate excluded —
            // `':!crates/reify-audit/*'` — or the five lines directly below
            // match their own sweep), so pinning it here is what makes the
            // shape a PROPERTY rather than luck.  Method and counts: PRD §16
            // Row 2.
            ("//! superseded by §7#4553 in the ratchet", 4553),
            ("//! superseded by T#4553 in the ratchet", 4553),
            ("//! superseded by Q#4553 in the ratchet", 4553),
            ("//! superseded by OQ#4553 in the ratchet", 4553),
            ("//! superseded by DD#4553 in the ratchet", 4553),
            // Family 2 — every spaced PRD-local noun, four-digit.
            ("/// see row #4553 for the landed shape", 4553),
            ("/// see rows #4553 for the landed shape", 4553),
            ("/// see boundary #4553 for the landed shape", 4553),
            ("/// see open-question #4553 for the landed shape", 4553),
            ("/// see design decision #4553 for the landed shape", 4553),
        ];
        for (line, id) in cases {
            assert!(
                has_canonical_cite(line),
                "genuine cite must stay canonical: {line}"
            );
            assert_eq!(extract_cites(line), vec![*id], "genuine cite id: {line}");
        }
    }

    /// Classification is per-`#N`-OCCURRENCE, never per-line: six live lines
    /// carry BOTH idioms, so a per-line verdict would either lose a real cite or
    /// resurrect a PRD-relative one.
    #[test]
    fn prd_relative_cite_is_per_occurrence_not_per_line() {
        // crates/reify-mesh-morph/src/eligibility.rs
        let co_cite = "/// visibility scheme (PRD task #11, task #2948) maintain separate counters";
        assert!(has_canonical_cite(co_cite));
        assert_eq!(extract_cites(co_cite), vec![2948]);
        // crates/reify-mesh-morph/tests/chain_degradation.rs
        let provenance = "//! Provenance: task #2951 (PRD task #14).";
        assert!(has_canonical_cite(provenance));
        assert_eq!(extract_cites(provenance), vec![2951]);
    }

    /// The digit bound is a property of the PRD-relative REGISTER — a
    /// document-local index is small — NOT of the `task` noun.  Whether a `#N`
    /// resolves as a cite must therefore not depend on which PRD-local noun
    /// happens to precede it.
    ///
    /// Pinned END TO END rather than only on the grammar, because what an
    /// unbounded family actually costs is a DISPOSITION, and the disposition is
    /// invisible at `has_canonical_cite` level.  All three lines carry the same
    /// genuine cite, #5238 (`done` — terminal), so each would resolve to a High
    /// `orphaned` finding through the unchanged β liveness lane.
    #[test]
    fn prd_relative_families_are_digit_bounded_end_to_end() {
        // (i) Marker lane, arm (3).  A four-digit cite must anchor the line as
        // `Cited` (→ β liveness → High `orphaned`, the hard gate), NOT be
        // downgraded to the Medium advisory `malformed-cite` merely because
        // `invariant` precedes it.  Asserted on both faces: `scan_file` shows
        // the `Cited` anchor, `classify_file` shows the structural lane is
        // silent (it discards `Cited`), which is what excludes `MalformedCite`.
        let marker = "// TODO: the invariant #5238 nearly lost";
        assert_eq!(
            scan_file(marker, true),
            vec![(1, LineClass::Cited(vec![5238]), marker.to_string())],
            "family 2 must not swallow a four-digit cite on a marker line"
        );
        assert_eq!(
            classify_file(marker, true),
            vec![],
            "a genuine cite must not degrade to Kind::MalformedCite: {marker}"
        );

        // (ii) Lane δ-B, arm (7).  δ-B is cite-ANCHORED, so an unbounded family
        // does not downgrade the finding — it erases the candidate entirely.
        // Routed through `scan_file`, NOT `classify_file`: the latter discards
        // `Cited` entries and would report "nothing" either way, so only this
        // assertion can actually fail.
        let delta_b = "/// hydration is blocked on the invariant #5238 landing";
        assert_eq!(
            scan_file(delta_b, true),
            vec![(1, LineClass::Cited(vec![5238]), delta_b.to_string())],
            "cite-anchored δ-B must still see a four-digit cite in family-2 register"
        );

        // (iii) Control — family 3's own bound already saves this shape, so the
        // disposition must be IDENTICAL whichever PRD-local noun precedes the
        // cite.  That equality is the property under test.
        let control = "// TODO: see task #5238 nearly lost";
        assert_eq!(
            scan_file(control, true),
            vec![(1, LineClass::Cited(vec![5238]), control.to_string())],
            "family 3's existing bound must keep this line's cite canonical"
        );
    }

    // -------------------------------------------------------------------
    // §8.2/§6.4 malformed citations — Greek / PRD-relative / legacy
    // -------------------------------------------------------------------

    #[test]
    fn malformed_cite_positives() {
        assert!(has_malformed_cite("// TODO(task δ): migrate")); // Greek
        assert!(has_malformed_cite("tracked in task α")); // Greek, no space-after-paren
        assert!(has_malformed_cite("// TODO(task-5): later")); // PRD-relative
        assert!(has_malformed_cite("// TODO: see task 4553")); // legacy space form
        assert!(has_malformed_cite("// TODO: see task_4553")); // legacy underscore form
    }

    #[test]
    fn malformed_cite_negatives() {
        // Canonical-only line must not be reported malformed (no `task` token).
        assert!(!has_malformed_cite("// TODO(#4553): migrate"));
        // Ordinary prose, no task+cite shape.
        assert!(!has_malformed_cite("the multitasking scheduler runs"));
        // A bare canonical cite.
        assert!(!has_malformed_cite("resolved in #4553"));
        // `task` embedded in a larger word (no left boundary) must NOT match,
        // even when followed by a separator + digit (`multitask 5`).
        assert!(!has_malformed_cite("// TODO: schedule multitask 5 jobs"));
    }

    /// A PRD-relative index on a REAL marker line is a banned citation form —
    /// the behaviour CLAUDE.md's TODO-citation convention already mandates
    /// ("PRD-relative indices … resolve to `malformed-cite`") and which §8.2's
    /// grammar missed for the `#N` spelling.
    ///
    /// **Vacuous on the live corpus today, deliberately.** A sweep of every
    /// marker-lane line (`#[ignore]`, TODO/FIXME/HACK, stub macro, δ-A) found
    /// ZERO carrying a PRD-relative-shaped cite, under both a narrow and the
    /// full rule.  That is why this arm is pinned hermetically here rather than
    /// by a corpus assertion — and it is also why this change adds no new
    /// `malformed-cite` finding to the live corpus.
    #[test]
    fn malformed_cite_prd_relative() {
        // Family 3 — `task #N` under the ≤ 99 digit bound.
        assert!(has_malformed_cite("// TODO(#10): wire the diagnostic per PRD task #10"));
        // Family 2 — a spaced PRD-local noun.
        assert!(has_malformed_cite("// FIXME: blocked on PRD invariant #2"));
        // Family 1 — a glued PRD-artifact namespace.
        assert!(has_malformed_cite("// HACK: see §7#5"));
        // Mirror image: a genuine cite is NOT malformed, and neither are the
        // three-digit legacy ids the digit bound has to let through.
        assert!(!has_malformed_cite("// TODO(#4092): real task"));
        assert!(!has_malformed_cite(
            "        // Reconstruction of lost work from task #333 per PRD §Slice B."
        ));
        assert!(!has_malformed_cite(
            "//! Re-establishes the deliverable from task #479 that was lost when commit 00a86da53"
        ));
        assert!(!has_malformed_cite(
            "/// where inner_field is None (a separate task #630 adds FieldSourceKind::Gradient"
        ));
    }

    /// `scan_file` precedence for the same shape, end to end: a marker line
    /// whose only cite is PRD-relative must land in the `malformed-cite`
    /// branch of arm (3) — NOT `Cited` (step-2 removed the canonical anchor)
    /// and NOT `Untracked`.
    ///
    /// The `Untracked` collapse is the one that matters: §8.4 rates a malformed
    /// cite Medium/advisory while `untracked` is High and hard-fails the merge
    /// gate, so reporting an author who cited imprecisely as untracked debt
    /// would over-report at the gate.  Same reasoning task #6087 applied to
    /// lane δ-A's malformed branch.
    #[test]
    fn classify_file_marker_with_prd_relative_cite_is_malformed() {
        let got = classify_file("// TODO: deferred to PRD task #10", true);
        assert_eq!(
            got,
            vec![(
                1,
                Kind::MalformedCite,
                "// TODO: deferred to PRD task #10".to_string()
            )]
        );
    }

    // -------------------------------------------------------------------
    // §8.3 phantom-tracking phrases (case-insensitive)
    // -------------------------------------------------------------------

    #[test]
    fn phantom_phrase_positives() {
        // All four normative phrases.
        assert!(phantom_phrase("this is tracked separately"));
        assert!(phantom_phrase("// tracked as a follow-up task"));
        assert!(phantom_phrase("tracked in project memory for later"));
        assert!(phantom_phrase("a follow-up task will handle this"));
        // Mixed-case variant — matching is case-insensitive.
        assert!(phantom_phrase("// Tracked As A Follow-Up task"));
    }

    #[test]
    fn phantom_phrase_negative() {
        // Ordinary prose that mentions tracking but not a phantom phrase.
        assert!(!phantom_phrase("// the tracker walks the working tree"));
    }

    // -------------------------------------------------------------------
    // §6.8 inline escape — `ptodo:allow`
    // -------------------------------------------------------------------

    #[test]
    fn line_escaped_detects_marker() {
        assert!(line_escaped("// TODO: leave me  // ptodo:allow"));
        assert!(!line_escaped("// TODO: flag me"));
    }

    // -------------------------------------------------------------------
    // §6.8 allowlist prefixes
    // -------------------------------------------------------------------

    #[test]
    fn allowlist_membership() {
        // The detector's own crate (pattern strings + committed fixtures).
        assert!(is_allowlisted("crates/reify-audit/src/p2_consumer_stub.rs"));
        // The #[ignore]-extraction tool and its tests.
        assert!(is_allowlisted("crates/reify-test-support/src/ignore_hygiene.rs"));
        assert!(is_allowlisted(
            "crates/reify-test-support/tests/ignore_reason_hygiene.rs"
        ));
        // An ordinary crate source path is NOT allowlisted.
        assert!(!is_allowlisted("crates/reify-ast/src/decl.rs"));

        // δ migration sweep (pre-1) confirmed: no new ALLOWLIST_PREFIXES entries
        // are needed. All 198 swept findings come from real non-self-referential
        // code sites (stdlib/*.ri type-placeholders, legacy-cite Rust files,
        // phantom-tracking prose, uncited markers) — none carry the detector's
        // own pattern-strings programmatically in a way that would self-match.
        // Scattered legitimate sites use `ptodo:allow` inline (§6.8 escape) rather
        // than a broad path-prefix exemption. Regression pin: representative real
        // swept files below the migration surface are NOT allowlisted (they must
        // appear in detector findings, not be silently skipped).
        assert!(!is_allowlisted("crates/reify-compiler/stdlib/dynamics.ri"));
        assert!(!is_allowlisted("crates/reify-eval/src/dispatcher.rs"));
        assert!(!is_allowlisted("crates/reify-eval/src/geometry_ops.rs"));
        assert!(!is_allowlisted("gui/src-tauri/src/tests/engine_tests.rs"));
    }

    // -------------------------------------------------------------------
    // §6.8 swept extensions
    // -------------------------------------------------------------------

    #[test]
    fn swept_extension_membership() {
        for p in ["a.rs", "b.ri", "c.sh", "d.py", "e.ts", "f.tsx", "g.js"] {
            assert!(is_swept_ext(p), "{p} should be a swept extension");
        }
        for p in ["a.md", "b.toml", "c.yaml", "d.json"] {
            assert!(!is_swept_ext(p), "{p} should NOT be a swept extension");
        }
    }

    // -------------------------------------------------------------------
    // §8 per-file classification orchestration (precedence)
    // -------------------------------------------------------------------

    #[test]
    fn classify_file_precedence_rust() {
        // Each line exercises exactly one §8 precedence rule; line numbers are
        // 1-based. is_rust=true so macro/ignore rules are live.
        let lines = [
            "// TODO(#4553): cited",              // 1 (a) canonical cite -> no entry
            "// tracked as a follow-up task",     // 2 (b) phantom, no cite -> PhantomTracking
            "// TODO(task δ): migrate",           // 3 (c) marker + malformed -> MalformedCite
            "// TODO: wire this",                 // 4 (d) marker, no cite -> Untracked
            "    #[ignore]",                      // 5 (e) bare ignore -> BareIgnore
            "    #[ignore = \"blocked\"]",        // 6 (f) blocker-prose reason -> Untracked (γ)
            "// resolved in #4553",               // 7 canonical cite, no marker -> no entry (prev for 8)
            "    todo!()",                        // 8 (g) macro, canonical cite directly above -> no entry
            "// TODO: leave me  // ptodo:allow",  // 9 (h) inline escape -> skipped
            "    todo!(\"later\")",               // 10 macro, no cite above -> Untracked
        ];
        let content = lines.join("\n");
        let got = classify_file(&content, true);

        let expected: Vec<(usize, Kind, String)> = vec![
            (2, Kind::PhantomTracking, "// tracked as a follow-up task".to_string()),
            (3, Kind::MalformedCite, "// TODO(task δ): migrate".to_string()),
            (4, Kind::Untracked, "// TODO: wire this".to_string()),
            (5, Kind::BareIgnore, "#[ignore]".to_string()),
            // γ: blocker-prose reason "blocked" → Untracked (was "no entry" pre-γ).
            (6, Kind::Untracked, "#[ignore = \"blocked\"]".to_string()),
            (10, Kind::Untracked, "todo!(\"later\")".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §8.3 γ structural policy — blocker-prose vs operational
    // -------------------------------------------------------------------

    /// Blocker-prose reason (no cite) → Structural(Untracked).
    /// Operational reason (no cite, no blocker-prose) → no entry.
    /// Bare #[ignore] → Structural(BareIgnore) (regression).
    #[test]
    fn scan_file_ignore_with_reason_blocker_prose_and_operational() {
        let lines = [
            "#[ignore = \"pending fillet binding\"]", // 1 blocker-prose -> Structural(Untracked)
            "#[ignore = \"requires OCCT\"]",           // 2 operational -> no entry
            "#[ignore]",                              // 3 bare -> Structural(BareIgnore)
        ];
        let content = lines.join("\n");
        let got = scan_file(&content, true);

        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Structural(Kind::Untracked),
             "#[ignore = \"pending fillet binding\"]".to_string()),
            (3, LineClass::Structural(Kind::BareIgnore), "#[ignore]".to_string()),
        ];
        assert_eq!(got, expected);
    }

    /// Non-canonical `#[ignore="blocked"]` (no spaces around `=`) — identical
    /// to the canonical form from scan_file's perspective: extract_ignore_reason
    /// mirrors ignore_attr's tolerance of non-spaced forms.
    #[test]
    fn scan_file_ignore_non_canonical_form_blocker_prose() {
        let content = "#[ignore=\"pending fillet binding\"]";
        let got = scan_file(content, true);
        assert_eq!(
            got,
            vec![(
                1,
                LineClass::Structural(Kind::Untracked),
                "#[ignore=\"pending fillet binding\"]".to_string(),
            )]
        );
    }

    // -------------------------------------------------------------------
    // §8 unified scan — `scan_file` (Structural + Cited) (β liveness lane)
    // -------------------------------------------------------------------

    #[test]
    fn scan_file_emits_cited_and_structural() {
        // is_rust=true so the macro / #[ignore] rules are live.
        let lines = [
            "// TODO(#4553): x",          // 1 comment marker + canonical cite -> Cited([4553])
            "// #42",                     // 2 cite-only, no marker -> no entry (prev for 3)
            "    todo!()",                // 3 stub macro, cite directly above -> Cited([42])
            "    #[ignore = \"see #42\"]", // 4 reason-bearing with cite -> Cited([42]) (γ cite-first)
            "// TODO: wire this",         // 5 marker, no cite -> Structural(Untracked)
            "// TODO(#5): x  // ptodo:allow", // 6 inline escape on a cited line -> skipped
        ];
        let content = lines.join("\n");

        let got = scan_file(&content, true);
        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Cited(vec![4553]), "// TODO(#4553): x".to_string()),
            (3, LineClass::Cited(vec![42]), "todo!()".to_string()),
            // γ cite-first: reason "see #42" has a canonical cite → Cited([42]).
            (4, LineClass::Cited(vec![42]), "#[ignore = \"see #42\"]".to_string()),
            (5, LineClass::Structural(Kind::Untracked), "// TODO: wire this".to_string()),
        ];
        assert_eq!(got, expected);

        // Regression: classify_file is exactly scan_file filtered to its
        // Structural variants — the Cited markers (1, 3, 4) and the suppressed
        // lines (2, 6) drop out, leaving byte-identical α output.
        let classified = classify_file(&content, true);
        let expected_structural: Vec<(usize, Kind, String)> =
            vec![(5, Kind::Untracked, "// TODO: wire this".to_string())];
        assert_eq!(classified, expected_structural);
    }

    // -------------------------------------------------------------------
    // §8.1 lane δ-A — #[allow(dead_code)] + deferral rationale (.rs)
    // -------------------------------------------------------------------

    /// The user-observable signal at unit level: a cited deferral rationale on
    /// an allow-attribute becomes `Cited`, so the UNCHANGED β liveness lane
    /// resolves it (cite #4744 is `done` → High `orphaned`). Shaped after
    /// `crates/reify-eval/src/engine_build.rs:12891`.
    #[test]
    fn scan_file_allow_dead_code_cited() {
        let line = "#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh)";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Cited(vec![4744]), line.to_string())]
        );
    }

    /// SCOPE-1: a δ-A rationale with deferral prose but NO cite is unmarked
    /// debt → `Structural(Untracked)`. This is the bulk of the measured lane.
    #[test]
    fn scan_file_allow_dead_code_uncited() {
        let line = "#[allow(dead_code)] // wiring pending the morph rewrite";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Structural(Kind::Untracked), line.to_string())]
        );
    }

    /// A δ-A rationale whose cite is the LEGACY `task NNNN` form is
    /// `malformed-cite`, not `untracked` — the arm mirrors arm (3)'s three-way
    /// split, and §8.3 defines the malformed-cite trigger lane-independently.
    ///
    /// This is the live shape at `crates/reify-eval/src/engine_build.rs:2199`
    /// (also `:2278`, `:2292` — 3 of the lane's 14 live findings, 1 of the 5
    /// seeded baseline fingerprints), pinned VERBATIM. The kind drives severity:
    /// `malformed-cite` is Medium/advisory per §8.4 whereas `untracked` is High
    /// and hard-fails the merge gate, so a silent flip here would change what a
    /// merge does, not just what it prints.
    #[test]
    fn scan_file_allow_dead_code_legacy_cite() {
        let line =
            "#[allow(dead_code)] // production wiring deferred to task 4050 (in-realization conversion executor)";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Structural(Kind::MalformedCite), line.to_string())]
        );
    }

    /// The dominant BENIGN class among the 68 measured δ-A candidates: a real
    /// allow-rationale that explains rather than defers. It must stay silent —
    /// a lane that fired here would report ordinary documented code as debt.
    #[test]
    fn scan_file_allow_dead_code_no_deferral() {
        let lines = [
            "#[allow(dead_code)] // used by some, but not all, test binaries that include this",
            "#[allow(dead_code)] // Phase-1 scaffold; consumed in later phases",
            "#[allow(dead_code)]",
            // Guard 3 (word boundary) at the scan_file level: an identifier
            // mention is not a deferral. Symbols are real
            // (crates/reify-eval/src/cache.rs:968, :1013).
            "#[allow(dead_code)] // superseded by mark_pending_with_cause",
        ];
        assert_eq!(scan_file(&lines.join("\n"), true), vec![]);
    }

    /// SCOPE-3: the §6.8 inline escape is precedence arm (1), ahead of every
    /// classification arm, so it keeps working against δ-A with no code change.
    /// Pinned rather than assumed.
    #[test]
    fn scan_file_allow_dead_code_escape_wins() {
        let line = "#[allow(dead_code)] // pending #1234  // ptodo:allow";
        assert_eq!(scan_file(line, true), vec![]);
    }

    /// δ-A is `.rs`-gated, keeping the `.sh`/`.py`/`.ts`/`.ri` blast radius at
    /// zero (and removing the self-match hazard for the shell scenario's own
    /// fixture text).
    #[test]
    fn scan_file_allow_dead_code_non_rust() {
        let line = "#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh)";
        assert_eq!(scan_file(line, false), vec![]);
    }

    /// Precedence: a line carrying BOTH an allow-attribute and a real comment
    /// marker stays owned by the marker lane (arm 3) — ONE entry, no
    /// double-count. The `else if` chain is what the fingerprint/baseline
    /// machinery relies on for at-most-one-entry-per-line.
    #[test]
    fn scan_file_allow_dead_code_marker_lane_wins() {
        let line = "#[allow(dead_code)] // TODO(#1234): pending the rewrite";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Cited(vec![1234]), line.to_string())]
        );
    }

    // -------------------------------------------------------------------
    // §8.1 lane δ-B — cited deferral in an ordinary comment (.rs)
    // -------------------------------------------------------------------

    /// The deliverable signal at unit level: an ordinary `///` comment that
    /// states the work is deferred AND names the task becomes `Cited`, so the
    /// UNCHANGED β liveness lane resolves it.
    ///
    /// Both lines are lifted VERBATIM from the live corpus
    /// (`crates/reify-core/src/diagnostics.rs`, the `HexWedgeMeshOutcome`
    /// rustdoc). Cite #2947 is `cancelled` — a terminal status — so β turns each
    /// into a High `orphaned` finding. These two lines ARE the reason this lane
    /// exists: neither carries a TODO-family marker, neither sits on an
    /// attribute, so no existing arm can reach them. Note they are `///` doc
    /// comments with no attribute above, which is why δ-A provably cannot cover
    /// this shape.
    #[test]
    fn scan_file_delta_b_cited_deferral_positives() {
        for line in [
            "/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring",
            "/// `dispatch_volume_mesh` (blocked on task #2947).  The future dispatcher will",
        ] {
            assert_eq!(
                scan_file(line, true),
                vec![(1, LineClass::Cited(vec![2947]), line.trim().to_string())],
                "δ-B positive not classified Cited([2947]): {line}"
            );
        }
    }

    /// The lane fires on all three Rust comment openers, not just `///` — the
    /// predicate is `trim_start().starts_with("//")`, so a plain `//` and a
    /// `//!` module doc are equally in scope.
    #[test]
    fn scan_file_delta_b_all_comment_openers() {
        for line in [
            "    // hydration is blocked on task #2947 landing first",
            "//! Envelope assembly is deferred to task #2947.",
        ] {
            assert_eq!(
                scan_file(line, true),
                vec![(1, LineClass::Cited(vec![2947]), line.trim().to_string())],
                "δ-B opener not recognised: {line}"
            );
        }
    }

    /// Class (a) — the IDENTIFIER false-positive class, killed by
    /// [`has_deferral_prose`]'s guard 3. Every line is VERBATIM from
    /// `crates/reify-eval/src/cache.rs`; every one carries a genuine canonical
    /// cite, so nothing but the prose guard stands between them and a High
    /// `orphaned` finding. This class was 100% of what sank δ-B's first
    /// proposal (task #6087) and is now fully eliminated.
    #[test]
    fn scan_file_delta_b_negatives_identifier_class() {
        for line in [
            "            // cause via mark_pending_with_cause (task #2330 §9.2 invariant).",
            "    /// / `mark_pending_with_cause` (tasks #2326, #2335) and all Failed transitions",
            "    // --- pending_cause / mark_failed / mark_pending_with_cause tests (task #2330 step-3) ---",
            "    // --- mark_pruned_pending producer tests (task #4739 γ) ---",
        ] {
            assert_eq!(
                scan_file(line, true),
                vec![],
                "δ-B over-fired on an identifier-class line: {line}"
            );
        }
    }

    /// Class (b) — the PRD-RELATIVE false-positive class, killed by
    /// [`prd_relative_cite`] (§8.2). Every line is VERBATIM from the live
    /// corpus and every one carries BOTH deferral prose and a `#N`, so the
    /// prose guard alone does NOT save them — only the cite grammar does. That
    /// is precisely why SCOPE-1 is a prerequisite of this lane rather than an
    /// unrelated tidy-up: without it these six live lines would each become a
    /// spurious High `orphaned` finding naming a PRD document index.
    #[test]
    fn scan_file_delta_b_negatives_prd_relative_class() {
        for line in [
            // crates/reify-stdlib/src/fea.rs — family 3, `task #10`.
            "/// Diagnostic emission is deferred to PRD task #10 (Diagnostic mapping for",
            "/// Diagnostic emission is deferred to PRD task #10.",
            "///    is deferred to PRD task #10 (Diagnostic mapping for multi-case-",
            "    // Empty Map → Undef. Diagnostic emission deferred to PRD task #10",
            // crates/reify-solver-elastic/src/boundary/dirichlet.rs — `task #12`.
            "    /// a uniaxial-stretch scenario is deferred to the downstream PRD task #12",
            // crates/reify-eval/src/geometry_ops.rs — family 2, `invariant #2`.
            "            //   is not yet a hydrated Value::GeometryHandle (PRD invariant #2:",
            // Two more PRD-relative live shapes that carry no deferral prose —
            // belt and braces: they must stay silent on BOTH guards.
            "    /// (v0.3.x multi-load-case FEA PRD task #10).",
            "    /// rather than partially constructing a sub-handle (PRD invariant #2).",
        ] {
            assert_eq!(
                scan_file(line, true),
                vec![],
                "δ-B over-fired on a PRD-relative line: {line}"
            );
        }
    }

    /// Class (c) — δ-B is cite-ANCHORED. A deferral comment with NO canonical
    /// cite is not a δ-B candidate at all, so the lane emits no structural kind
    /// and cannot degenerate into "flag every comment containing `pending`".
    /// Contrast δ-A, which anchors on the attribute and therefore CAN emit
    /// `Untracked` for the uncited case.
    #[test]
    fn scan_file_delta_b_negative_uncited_deferral() {
        for line in [
            "/// wiring is pending the morph rewrite",
            "// the envelope path is blocked on the solver rewrite",
        ] {
            assert_eq!(
                scan_file(line, true),
                vec![],
                "δ-B is cite-anchored and must not emit a structural kind: {line}"
            );
        }
    }

    /// Class (d) — a `// G-allow:` line carrying both a cite and deferral prose
    /// belongs to the G-allow lane, which runs its own independent
    /// `scan_g_allow_markers` → `resolve_g_allow_owner_liveness` pass. Without
    /// the guard the same line would emit TWO findings under two different
    /// kinds (`orphaned` and `g-allow-orphaned`) once its owner cite goes
    /// terminal. VERBATIM from `crates/reify-ir/src/value.rs` (two live sites,
    /// both citing #5235, which is `pending` today — so this is latent, not
    /// live, which is exactly when it is cheapest to close).
    #[test]
    fn scan_file_delta_b_negative_g_allow_line() {
        let line = "// G-allow: shared display formatter input type (PRD display-unit-preference §6.2); the four surfaces route onto it in L4 task #5235 (pending) — no non-test caller until then";
        assert_eq!(
            scan_file(line, true),
            vec![],
            "δ-B must delegate G-allow lines to their owner lane"
        );
    }

    /// Class (d), the seam: a `// G-allow:` line whose cites are ALL
    /// provenance-EXEMPT (rules (a)/(b)/(c)) yields no owner, so the G-allow
    /// lane skips it for having nothing to resolve and δ-B skips it for being a
    /// G-allow line — NEITHER lane claims it. That is the composition of two
    /// rules, not an oversight, and this test pins it as a decision: on such a
    /// line the owner-cite grammar IS the cite grammar, and `extract_cites` is
    /// blind to its exemptions, so admitting the line into δ-B would anchor it
    /// on precisely the cites the sibling grammar classified as provenance.
    /// Both halves are asserted, because the property is about the PAIR.
    #[test]
    fn scan_file_delta_b_negative_g_allow_owner_less() {
        let line =
            "// G-allow: envelope assembly is deferred to #4092 (done); re-homed from cancelled #3429";
        // The owner lane is silent: every cite is provenance-exempt.
        let body = g_allow_marker_body(line).expect("a G-allow body");
        assert_eq!(
            extract_g_allow_owner_cites(body),
            Vec::<u32>::new(),
            "both cites must be provenance-exempt for this to be the seam case"
        );
        // …and so is δ-B, by the `g_allow_marker_body` guard.
        assert_eq!(
            scan_file(line, true),
            vec![],
            "an owner-less G-allow line stays delegated to the G-allow lane"
        );
        // The same prose WITHOUT the `// G-allow:` prefix is a δ-B candidate —
        // so the guard, not the prose or the cites, is what silences it.
        let plain = "// envelope assembly is deferred to #4092";
        assert_eq!(
            scan_file(plain, true),
            vec![(1, LineClass::Cited(vec![4092]), plain.to_string())]
        );
    }

    /// δ-B is scoped to FULL-LINE comments: a trailing comment after code is
    /// not a candidate, because both of the lane's predicates match the WHOLE
    /// line and are only sound while the whole line is comment text.
    ///
    /// Decided, not overlooked — and measured (tracked `.rs`, 2026-08-31):
    /// exactly ONE line repo-wide is code-then-trailing-comment carrying both
    /// deferral prose and a four-digit cite, and its "code" is the
    /// `#[allow(dead_code)]` attribute that arm (5) already owns
    /// (`scan_file_delta_b_allow_dead_code_lane_wins`). The restriction
    /// therefore costs no recall today. δ-A reads a trailing comment because
    /// its anchor is an attribute, which cannot appear mid-expression.
    #[test]
    fn scan_file_delta_b_negative_trailing_comment() {
        let trailing = "let x = f(); // wiring is deferred to task #2947";
        assert_eq!(
            scan_file(trailing, true),
            vec![],
            "a trailing comment after code is out of δ-B's scope"
        );
        // The control: the same comment as a full line IS a candidate, so the
        // code prefix is the only difference the assertion above turns on.
        let full_line = "// wiring is deferred to task #2947";
        assert_eq!(
            scan_file(full_line, true),
            vec![(1, LineClass::Cited(vec![2947]), full_line.to_string())]
        );
    }

    /// Precedence 1: a line carrying BOTH a comment marker and a cited deferral
    /// stays owned by arm (3) — ONE entry, no double-count. The `else if` chain
    /// is what the fingerprint/§6.6 baseline machinery relies on for
    /// at-most-one-entry-per-line.
    #[test]
    fn scan_file_delta_b_marker_lane_wins() {
        let line = "// TODO(#1234): blocked on task #2947 landing";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Cited(vec![1234, 2947]), line.to_string())]
        );
    }

    /// Precedence 2: a δ-A line stays owned by arm (5). δ-B is appended LAST,
    /// after the phantom arm, so it can never steal a line an earlier arm owns.
    /// (Independently, the attribute line does not start with `//`, so δ-B's
    /// own predicate would reject it too — belt and braces.)
    #[test]
    fn scan_file_delta_b_allow_dead_code_lane_wins() {
        let line = "#[allow(dead_code)] // production wiring pending task #4744 (volume-mesh)";
        assert_eq!(
            scan_file(line, true),
            vec![(1, LineClass::Cited(vec![4744]), line.to_string())]
        );
    }

    /// The §6.8 inline escape opts a δ-B line out of the whole sweep, like
    /// every other lane.
    #[test]
    fn scan_file_delta_b_escape_wins() {
        let line = "/// wiring is blocked on task #2947 // ptodo:allow";
        assert_eq!(scan_file(line, true), vec![]);
    }

    /// δ-B is `.rs`-only. Asserted through `scan_file` rather than
    /// `classify_file`, because `classify_file` discards `Cited` entries and so
    /// would report "nothing" even if the lane HAD fired — the assertion has to
    /// be able to fail.
    #[test]
    fn scan_file_delta_b_non_rust() {
        let line = "// wiring is blocked on task #2947";
        assert_eq!(scan_file(line, false), vec![]);
        assert_eq!(classify_file(line, false), vec![]);
    }

    /// δ-B emits NO structural kind, ever — the whole lane is invisible to α
    /// and reaches only the unchanged β liveness lane. Pinned as its own
    /// assertion because it is the property that keeps §8.3's taxonomy (and
    /// therefore `VALID_KINDS`, `fingerprint` and the §8.4 severity map)
    /// byte-unchanged by this lane.
    #[test]
    fn scan_file_delta_b_emits_no_structural_kind() {
        let content = [
            "/// — that wiring is blocked on VolumeMesh realization (task #2947), mirroring",
            "/// wiring is pending the morph rewrite",
            "//! Envelope assembly is deferred to task #2947.",
        ]
        .join("\n");
        assert_eq!(classify_file(&content, true), vec![]);
    }

    // -------------------------------------------------------------------
    // §8.3 γ cite-first path — reason with canonical cite → Cited (β lane)
    // -------------------------------------------------------------------

    /// `#[ignore = "blocked on #4444"]` — cite wins over blocker-prose.
    /// `#[ignore = "see #42"]`          — cite without blocker-prose.
    #[test]
    fn scan_file_ignore_reason_with_cite_emits_cited_entry() {
        let lines = [
            "#[ignore = \"blocked on #4444\"]", // 1 cite wins over "blocked" prose → Cited([4444])
            "#[ignore = \"see #42\"]",          // 2 cite, no blocker-prose → Cited([42])
        ];
        let content = lines.join("\n");
        let got = scan_file(&content, true);

        let expected: Vec<(usize, LineClass, String)> = vec![
            (1, LineClass::Cited(vec![4444]), "#[ignore = \"blocked on #4444\"]".to_string()),
            (2, LineClass::Cited(vec![42]),   "#[ignore = \"see #42\"]".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §6.7 task-DB path resolution (β liveness lane) — `tasks_db_path`
    // -------------------------------------------------------------------

    #[test]
    fn tasks_db_path_defaults_under_project_root() {
        // With REIFY_PTODO_TASKS_DB unset (the normal cargo-test env), the path
        // resolves to <project_root>/.taskmaster/tasks/tasks.db. The env-override
        // branch is covered end-to-end by the subprocess test (no unsafe set_var).
        assert_eq!(
            tasks_db_path(std::path::Path::new("/repo")),
            std::path::PathBuf::from("/repo/.taskmaster/tasks/tasks.db"),
        );
    }

    #[test]
    fn classify_file_non_rust_skips_macro_and_ignore() {
        // is_rust=false: comment markers and phantom phrases still fire (all
        // swept exts), but the .rs-only macro and #[ignore] rules do NOT.
        let lines = [
            "# TODO: wire this sh script", // 1 comment marker -> Untracked
            "todo!()",                     // 2 macro -> suppressed (is_rust=false)
            "#[ignore]",                   // 3 ignore -> suppressed (is_rust=false)
            "tracked separately",          // 4 phantom -> PhantomTracking
        ];
        let content = lines.join("\n");
        let got = classify_file(&content, false);

        let expected: Vec<(usize, Kind, String)> = vec![
            (1, Kind::Untracked, "# TODO: wire this sh script".to_string()),
            (4, Kind::PhantomTracking, "tracked separately".to_string()),
        ];
        assert_eq!(got, expected);
    }

    // -------------------------------------------------------------------
    // §6.6 fingerprint() — baseline fingerprint derivation
    // -------------------------------------------------------------------

    /// (a) Structural finding: line-N stripped, internal whitespace folded.
    #[test]
    fn fingerprint_structural_untracked() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/foo/bar.rs".to_string(),
            summary: "untracked: line 12:    // TODO: wire   this".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/foo/bar.rs :: untracked :: // TODO: wire this",
        );
    }

    /// (b) Malformed-cite finding: same stripping/folding rules as structural.
    #[test]
    fn fingerprint_structural_malformed_cite() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/src/dispatcher.rs".to_string(),
            summary: "malformed-cite: line 5: // TODO(task-3445): some  text".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/src/dispatcher.rs :: malformed-cite :: // TODO(task-3445): some text",
        );
    }

    /// (c) Liveness finding: kind up to first ':', `line N: ` stripped, rest kept verbatim
    /// modulo whitespace folding. The `orphaned` kind has additional structure
    /// (`#id status=done: <text>`) that must be preserved.
    #[test]
    fn fingerprint_liveness_orphaned() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/src/engine_purposes.rs".to_string(),
            summary: "orphaned: line 7: #4551 status=done: // FIXME(#4551): x".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/src/engine_purposes.rs :: orphaned :: #4551 status=done: // FIXME(#4551): x",
        );
    }

    /// Unknown-id liveness finding: `unknown-id` kind, `line N: #id: <text>`.
    #[test]
    fn fingerprint_liveness_unknown_id() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-solver/src/lib.rs".to_string(),
            summary: "unknown-id: line 99: #9999: // TODO(#9999): placeholder".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-solver/src/lib.rs :: unknown-id :: #9999: // TODO(#9999): placeholder",
        );
    }

    /// `phantom-tracking` taxonomy kind (structural lane, `line N:` prefix).
    #[test]
    fn fingerprint_phantom_tracking() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-core/src/primitives.rs".to_string(),
            summary: "phantom-tracking: line 59: // work   tracked separately".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-core/src/primitives.rs :: phantom-tracking :: // work tracked separately",
        );
    }

    /// `bare-ignore` taxonomy kind (structural lane, `line N:` prefix).
    #[test]
    fn fingerprint_bare_ignore() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/tests/connect_eval.rs".to_string(),
            summary: "bare-ignore: line 12: #[ignore]".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/tests/connect_eval.rs :: bare-ignore :: #[ignore]",
        );
    }

    /// Non-`line ` branch: a summary whose post-kind text does NOT carry a
    /// `line <digits>: ` prefix is folded and kept verbatim (no stripping).
    /// (Inverse `task-cites-deleted-path` findings take this branch; they are
    /// excluded from the source-marker baseline by the convergence test's
    /// swept-ext gate, but `fingerprint()` must still derive a stable string.)
    #[test]
    fn fingerprint_no_line_prefix() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/reify-eval/src/dispatcher.rs".to_string(),
            summary: "orphaned: #4592   status=done: x".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/reify-eval/src/dispatcher.rs :: orphaned :: #4592 status=done: x",
        );
    }

    /// Malformed (no-colon) summary: the best-effort branch returns
    /// `"{path} :: {summary} :: "` with an EMPTY text field. This fingerprint is
    /// intentionally ill-formed — `baseline_is_well_formed` (tests/ptodo_baseline.rs)
    /// rejects an empty text field, so such a finding can never silently enter the
    /// committed baseline. Pinning the contract here documents that boundary.
    #[test]
    fn fingerprint_no_colon_summary_yields_empty_text() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/foo/bar.rs".to_string(),
            summary: "weird summary with no colon".to_string(),
            evidence: vec![],
        };
        let fp = fingerprint(&finding);
        assert_eq!(fp, "crates/foo/bar.rs :: weird summary with no colon :: ");
        // The text field (after the second ` :: `) is empty by construction.
        assert!(fp.ends_with(" :: "), "no-colon branch must leave an empty text field");
    }

    // -------------------------------------------------------------------
    // fold_whitespace() — internal whitespace normalization
    // -------------------------------------------------------------------

    #[test]
    fn fold_whitespace_folds_internal_runs() {
        // Mixed internal whitespace (spaces, tab, newline) folds to single spaces.
        assert_eq!(fold_whitespace("a\t\n  b   c"), "a b c");
    }

    #[test]
    fn fold_whitespace_trims_leading_and_trailing() {
        // Leading whitespace is dropped; trailing whitespace is popped.
        assert_eq!(fold_whitespace("   abc"), "abc");
        assert_eq!(fold_whitespace("abc   "), "abc");
        assert_eq!(fold_whitespace("  abc  "), "abc");
    }

    #[test]
    fn fold_whitespace_all_whitespace_and_empty() {
        // All-whitespace input collapses to the empty string (no trailing space left).
        assert_eq!(fold_whitespace("    "), "");
        assert_eq!(fold_whitespace("\t\n "), "");
        assert_eq!(fold_whitespace(""), "");
    }

    // -------------------------------------------------------------------
    // θ (#4560) assess-NO regression guard — softer vocabularies
    // -------------------------------------------------------------------

    /// Regression guard for the task θ (#4560) ASSESS NO-decision: every
    /// vocabulary in [`ASSESSED_REJECTED_VOCAB`] must remain silent when
    /// embedded in a benign line that carries **no** TODO/FIXME/HACK marker,
    /// no `todo!()`/`unimplemented!()` macro, and no `#[ignore]` attribute.
    ///
    /// A future contributor who adds one of these vocabularies as a recognised
    /// marker will see this test fail, prompting them to revisit the θ evidence
    /// and update the PRD §14 record before proceeding.
    #[test]
    fn softer_vocabularies_remain_unrecognised() {
        // Each vocabulary embedded in an innocent comment — no TODO/FIXME/HACK
        // / todo!() / unimplemented!() / #[ignore] present.  scan_file must
        // return an empty vec for both Rust and non-Rust contexts.
        for vocab in ASSESSED_REJECTED_VOCAB {
            let rust_line = format!("// this uses {vocab} in a comment");
            assert_eq!(
                scan_file(&rust_line, true),
                vec![],
                "vocab {:?} must not trigger the detector in a Rust comment",
                vocab,
            );
            let non_rust_line = format!("# {vocab} mentioned here");
            assert_eq!(
                scan_file(&non_rust_line, false),
                vec![],
                "vocab {:?} must not trigger the detector in a non-Rust comment",
                vocab,
            );
        }

        // Also check each vocab in a *marker-like* position — the first word after `//`,
        // mirroring the TODO/FIXME/HACK syntax.  This catches a narrower regression where
        // a vocab is wired into the marker position but not yet into the generic comment
        // path (the loop above).
        for vocab in ASSESSED_REJECTED_VOCAB {
            let marker_like = format!("// {vocab}: some description");
            assert_eq!(
                scan_file(&marker_like, true),
                vec![],
                "vocab {:?} in marker-like position must not trigger the detector",
                vocab,
            );
        }

        // Concrete real-corpus benign forms that must also stay silent.

        // (a) mktemp XXXXXX template — the dominant "XXX" corpus class (~100% FP).
        //     Shell context (is_rust=false).
        let mktemp_line = "TMPDIR=$(mktemp -d /tmp/reify-XXXXXX)";
        assert_eq!(
            scan_file(mktemp_line, false),
            vec![],
            "mktemp XXXXXX template line must not trigger the detector",
        );

        // (b) Doc-comment with "ephemeral placeholder" — the dominant "placeholder"
        //     corpus class (type-system/UI vocabulary, ~100% FP).  Rust context.
        let placeholder_line = "/// Uses an ephemeral placeholder for the auto-generated type param.";
        assert_eq!(
            scan_file(placeholder_line, true),
            vec![],
            "doc-comment with 'placeholder' must not trigger the detector",
        );

        // (c) Doc-comment with "in stub mode" — the dominant "stub" corpus class
        //     (stub-mode architectural concept, ~100% FP).  Rust context.
        let stub_mode_line = "/// Returns `None` in stub mode (OCCT/OpenVDB absent builds).";
        assert_eq!(
            scan_file(stub_mode_line, true),
            vec![],
            "doc-comment with 'stub mode' must not trigger the detector",
        );
    }

    /// `parked-on-anchor` liveness finding: kind up to first ':' → `parked-on-anchor`;
    /// `line N:` prefix stripped; rest kept verbatim modulo whitespace folding.
    /// Pins the fingerprint so the empty-baseline ratchet can depend on it.
    #[test]
    fn fingerprint_parked_on_anchor() {
        let finding = Finding {
            pattern: Pattern::PTodo,
            severity: Severity::Medium,
            task_id: "crates/foo/bar.rs".to_string(),
            summary: "parked-on-anchor: line 7: #42 status=deferred (do_not_complete): // TODO(#42): perf".to_string(),
            evidence: vec![],
        };
        assert_eq!(
            fingerprint(&finding),
            "crates/foo/bar.rs :: parked-on-anchor :: #42 status=deferred (do_not_complete): // TODO(#42): perf",
        );
    }

    // -------------------------------------------------------------------
    // metadata_do_not_complete() — pure helper parser
    // -------------------------------------------------------------------

    /// Step-1 (RED): the helper does not exist yet → this test must fail to compile.
    #[test]
    fn metadata_do_not_complete_parsing() {
        // None → false (no metadata)
        assert!(!metadata_do_not_complete(None));
        // Malformed JSON → false (graceful)
        assert!(!metadata_do_not_complete(Some("{not json")));
        // Valid JSON, key missing → false
        assert!(!metadata_do_not_complete(Some(r#"{"files":[]}"#)));
        // do_not_complete: true → true (the signal)
        assert!(metadata_do_not_complete(Some(r#"{"do_not_complete":true}"#)));
        // do_not_complete: false → false
        assert!(!metadata_do_not_complete(Some(r#"{"do_not_complete":false}"#)));
        // do_not_dispatch only (no do_not_complete) → false (FP guard)
        assert!(!metadata_do_not_complete(Some(r#"{"do_not_dispatch":true}"#)));
    }

    // -------------------------------------------------------------------
    // G-allow owner-cite grammar — g_allow_marker_body / extract_g_allow_owner_cites
    // -------------------------------------------------------------------

    #[test]
    fn g_allow_marker_body_positives() {
        assert_eq!(g_allow_marker_body("// G-allow: foo #1"), Some("foo #1"));
        assert_eq!(g_allow_marker_body("    // G-allow: bar"), Some("bar"));
        assert_eq!(
            g_allow_marker_body("// G-allow: consumer pending task #4743 (volume-mesh §8)"),
            Some("consumer pending task #4743 (volume-mesh §8)"),
        );
    }

    #[test]
    fn g_allow_marker_body_negatives() {
        // blank body (whitespace-only or absent)
        assert_eq!(g_allow_marker_body("// G-allow: "), None);
        assert_eq!(g_allow_marker_body("// G-allow:"), None);
        // non-marker lines
        assert_eq!(g_allow_marker_body("// TODO: foo"), None);
        assert_eq!(g_allow_marker_body("fn dispatch_volume_mesh()"), None);
    }

    /// Real engine_build.rs marker body: consumer #4743 is OWNER; provenance
    /// #3429/#2947 exempt by rule (b) ("re-homed from cancelled", case-insensitive).
    #[test]
    fn extract_g_allow_owner_cites_real_engine_build_marker() {
        let body = "§3.2 realization-kind dispatch seam (VolumeMesh) per engine-integration-norm \
                    §3.2; consumer pending task #4743 (volume-mesh-realization-and-morph-wiring \
                    §8 task α — adds the execute_realization_ops→dispatch_volume_mesh call edge); \
                    re-homed from cancelled #3429/#2947";
        assert_eq!(extract_g_allow_owner_cites(body), vec![4743_u32]);
    }

    /// Real diagnostics.rs marker body: live wiring owner #4744; debug-RPC snapshot
    /// consumer #2949 exempt by rule (a) ("#2949 (done)"); re-homed from cancelled
    /// #3429 exempt by rule (b).
    #[test]
    fn extract_g_allow_owner_cites_diagnostics_marker() {
        let body = "live wiring owner: task #4744 (volume-mesh-realization-and-morph-wiring §8 \
                    task β — morph arm in dispatch_volume_mesh, engine_build.rs); \
                    debug-RPC snapshot consumer #2949 (done); re-homed from cancelled #3429";
        assert_eq!(extract_g_allow_owner_cites(body), vec![4744_u32]);
    }

    /// Joined PINS per-entry comment block: consumer #4743 is OWNER; #3429/#2947
    /// exempt by rule (b) ("Re-homed from cancelled", case-insensitive).
    #[test]
    fn extract_g_allow_owner_cites_pins_comment_block() {
        let body = "§3.2 realization-kind dispatch seam (VolumeMesh); consumer task #4743 \
                    (volume-mesh-realization-and-morph-wiring §8 task α). \
                    Re-homed from cancelled #3429/#2947.";
        assert_eq!(extract_g_allow_owner_cites(body), vec![4743_u32]);
    }

    /// PRD #2 is a PRD-section reference (rule c), NOT an owner task cite.
    #[test]
    fn extract_g_allow_owner_cites_prd_exemption() {
        let body = "producer for pending task #2997 \
                    (a-posteriori-error-estimation PRD #2: adaptive refinement loop)";
        assert_eq!(extract_g_allow_owner_cites(body), vec![2997_u32]);
    }

    /// Loop-closure-style body: #3843 exempt by rule (a) "(done"; #4428 surfaces as
    /// OWNER even though "provenance" appears before it — documents the grammar's
    /// landmine and justifies the scoped engine-seam gate (not repo-wide).
    #[test]
    fn extract_g_allow_owner_cites_loop_closure_landmine() {
        let body = "...; KCC-γ #3843 (done, provenance); \
                    live downstream closed-chain consumer: KIN-OFFSET batch #4428 (β1, in-progress)";
        assert_eq!(extract_g_allow_owner_cites(body), vec![4428_u32]);
    }

    /// Pins the bounded-window semantics of rule (b): a provenance keyword
    /// appearing before a ';' separator must NOT exempt an owner cite that
    /// appears AFTER the separator. Without the bounded window, rule (b) would
    /// scan the entire preceding body and incorrectly exempt #4744.
    ///
    /// This is the scenario the reviewer flagged: a future G-allow marker written
    /// as "re-homed from cancelled #OLD; live owner #NEW" must surface #NEW as
    /// an owner, not silently drop it.
    #[test]
    fn extract_g_allow_owner_cites_owner_after_provenance_keyword_in_earlier_segment() {
        // Provenance text + terminal cite, then ';', then a live owner cite.
        // Rule (b)'s window for #4744 starts after ';', so "cancelled" is not
        // in scope → #4744 is classified as OWNER.
        let body = "re-homed from cancelled #3429; live owner #4744";
        assert_eq!(extract_g_allow_owner_cites(body), vec![4744_u32]);

        // Same pattern with "re-homed" keyword and multiple provenance cites
        // before the separator.
        let body2 = "re-homed from cancelled #3429/#2947; consumer task #4743";
        assert_eq!(extract_g_allow_owner_cites(body2), vec![4743_u32]);
    }

    // -------------------------------------------------------------------
    // Broadened rule (a): following-paren / enclosing-paren DONE/CANCELLED
    // token (case-insensitive, word-boundary).  RED until step-2 widens
    // is_g_allow_cite_exempt.
    // -------------------------------------------------------------------

    /// Real corpus shape from tots.rs ×14:
    ///   `helper, task #3870 (κ — TOTS SQP, DONE)`
    /// The DONE token is INSIDE the FOLLOWING paren, uppercase.  Current rule
    /// (a) only does `starts_with("(done")` so this currently returns vec![3870].
    /// After broadening it must return vec![] (exempt).
    #[test]
    fn g_allow_cite_exempt_following_paren_uppercase_done() {
        // Following-paren uppercase DONE token (real corpus shape, tots.rs ×14)
        let body = "helper, task #3870 (\u{03ba} \u{2014} TOTS SQP, DONE)";
        assert_eq!(
            extract_g_allow_owner_cites(body),
            Vec::<u32>::new(),
            "uppercase DONE inside following paren must exempt the cite"
        );

        // Real corpus shape from simulate.rs ×4
        let body2 = "#3869 (\u{03b8} \u{2014} simulate_trajectory, DONE)";
        assert_eq!(
            extract_g_allow_owner_cites(body2),
            Vec::<u32>::new(),
            "uppercase DONE inside following paren must exempt simulate cite"
        );

        // Tensegrity shape: immediately-following paren with DONE
        let body3 = "#3796 (Tensegrity T2, DONE)";
        assert_eq!(
            extract_g_allow_owner_cites(body3),
            Vec::<u32>::new(),
            "uppercase DONE inside immediately-following paren must exempt"
        );
    }

    /// Enclosing-paren form: the cite itself sits inside a paren group that
    /// contains `done` (case-insensitive).  Current code never checks backwards,
    /// so `"(task #1234, done)"` currently returns vec![1234].
    #[test]
    fn g_allow_cite_exempt_enclosing_paren_done() {
        // Cite is enclosed in a paren that contains `done`
        assert_eq!(
            extract_g_allow_owner_cites("(task #1234, done)"),
            Vec::<u32>::new(),
            "cite inside a (…done…) group must be exempt"
        );
        // uppercase in enclosing paren
        assert_eq!(
            extract_g_allow_owner_cites("(task #1234, DONE)"),
            Vec::<u32>::new(),
            "cite inside a (…DONE…) group must be exempt (case-insensitive)"
        );
    }

    /// Un-annotated owner cites must NOT be exempted — they have no paren
    /// containing done/cancelled.
    #[test]
    fn g_allow_cite_owner_unannotated_stays_owner() {
        // Plain owner cite with no annotation
        assert_eq!(
            extract_g_allow_owner_cites("task #1234 const-slice registry; consumer same-file"),
            vec![1234_u32],
            "plain unannotated cite must stay as owner"
        );
    }

    /// (γ)-style following paren with no done/cancelled token — must NOT exempt.
    #[test]
    fn g_allow_cite_gamma_style_stays_owner() {
        assert_eq!(
            extract_g_allow_owner_cites("task #5678 (\u{03b3}) fn-pointer blind spot"),
            vec![5678_u32],
            "(γ) following paren has no done/cancelled token — must stay owner"
        );
    }

    /// WORD-BOUNDARY negatives: words that CONTAIN `done`/`cancelled` as a
    /// substring but are NOT the token must NOT exempt (rule: match whole-word).
    #[test]
    fn g_allow_cite_word_boundary_negatives() {
        // `abandoned` contains no `done` or `cancelled` token at word boundaries
        assert_eq!(
            extract_g_allow_owner_cites("#1234 (abandoned approach)"),
            vec![1234_u32],
            "'abandoned' must NOT exempt — not a done/cancelled token"
        );
        // `undone` is NOT the token `done` (it is `undone`)
        assert_eq!(
            extract_g_allow_owner_cites("#1234 (work undone later)"),
            vec![1234_u32],
            "'undone' must NOT exempt — 'done' must be a word-boundary token"
        );
    }

    /// ';'-bounded-window guard for (a-enclosing): in a multi-cite annotation
    /// enclosed in a single paren group, a `cancelled`/`done` token in one
    /// ';'-segment must NOT exempt a live owner cite in a different segment.
    ///
    /// Shape: `(re-homed from cancelled #OLD; live owner #NEW)`
    /// — #OLD is in the segment containing `cancelled` → exempt via rule (a-enclosing)
    ///   (and also via rule (b)'s bounded window).
    /// — #NEW is in the segment AFTER the ';', which has no terminal token
    ///   → must survive as an owner cite.
    ///
    /// Without the ';' bounding, the full-group check would incorrectly see
    /// `cancelled` and exempt #NEW.
    #[test]
    fn g_allow_cite_enclosing_paren_semicolon_bounded_window() {
        let body = "(re-homed from cancelled #1111; live owner #2222)";
        let owners = extract_g_allow_owner_cites(body);
        assert!(
            !owners.contains(&1111_u32),
            "#1111 is in the cancelled segment and must be exempt; owners={owners:?}"
        );
        assert!(
            owners.contains(&2222_u32),
            "#2222 is the live owner and must NOT be exempted by cancelled in a different \
             ';'-segment; owners={owners:?}"
        );
    }

    /// Regression guards: existing lowercase `(done)`/`(done, provenance)` forms
    /// that already pass rule (a) must continue to work after the broadening.
    #[test]
    fn g_allow_cite_exempt_regression_lowercase_done() {
        // Exact old rule (a) shape — must still exempt after broadening
        assert_eq!(
            extract_g_allow_owner_cites("#2949 (done)"),
            Vec::<u32>::new(),
            "lowercase (done) must still be exempt after grammar broadening"
        );
        assert_eq!(
            extract_g_allow_owner_cites("#3843 (done, provenance)"),
            Vec::<u32>::new(),
            "lowercase (done, …) must still be exempt after grammar broadening"
        );
    }

    // -------------------------------------------------------------------
    // is_g_allow_finding predicate — baseline-exclusion gate.
    // RED until step-6 adds the function.
    // -------------------------------------------------------------------

    /// Pin the `is_g_allow_finding` predicate used by the baseline gen + ratchet
    /// to exclude the advisory G-allow lane from the source-marker baseline.
    ///
    /// Rationale for the exclusion: g-allow-orphaned / g-allow-unknown-id are a
    /// distinct orphan-suppression-provenance taxonomy (path-keyed, .rs files).
    /// Including them in the baseline would (a) make the on-demand (B) ratchet
    /// RED against the intentionally-empty baseline and (b) make a future regen
    /// emit lines whose `kind` fails `VALID_KINDS` in `baseline_is_well_formed`.
    /// The `fingerprint()` check below documents WHY the exclusion is necessary.
    ///
    /// RED until step-6 adds `pub fn is_g_allow_finding`.
    #[test]
    fn is_g_allow_finding_predicate() {
        use crate::{EvidenceRef, Pattern, Severity};

        let make_finding = |summary: &str| -> Finding {
            Finding {
                pattern: Pattern::PTodo,
                severity: Severity::Medium,
                task_id: "crates/x/src/a.rs".to_string(),
                summary: summary.to_string(),
                evidence: vec![EvidenceRef::File {
                    path: "crates/x/src/a.rs".to_string(),
                }],
            }
        };

        // g-allow-orphaned → true
        let g_allow_orphaned = make_finding(
            "g-allow-orphaned: line 3: #1234 status=done: // G-allow: consumer task #1234",
        );
        assert!(
            is_g_allow_finding(&g_allow_orphaned),
            "g-allow-orphaned must be detected as a g-allow finding"
        );

        // g-allow-unknown-id → true (both kinds detected)
        let g_allow_unknown = make_finding(
            "g-allow-unknown-id: line 7: #9999: // G-allow: consumer task #9999",
        );
        assert!(
            is_g_allow_finding(&g_allow_unknown),
            "g-allow-unknown-id must be detected as a g-allow finding"
        );

        // source-marker orphaned → false (different taxonomy)
        let source_orphaned =
            make_finding("orphaned: line 3: #1234 status=done: // TODO(#1234): wire");
        assert!(
            !is_g_allow_finding(&source_orphaned),
            "source-marker 'orphaned' must NOT be detected as a g-allow finding"
        );

        // Demonstrate WHY exclusion is needed: fingerprint() extracts the kind
        // segment "g-allow-orphaned", which is NOT in the source-marker VALID_KINDS
        // taxonomy {untracked, malformed-cite, phantom-tracking, bare-ignore,
        // orphaned, unknown-id}.  A regen including it would fail
        // baseline_is_well_formed's kind check — so it must be excluded upstream.
        let fp = fingerprint(&g_allow_orphaned);
        let kind_segment = fp.split(" :: ").nth(1).unwrap_or("");
        const SOURCE_MARKER_VALID_KINDS: &[&str] = &[
            "untracked",
            "malformed-cite",
            "phantom-tracking",
            "bare-ignore",
            "orphaned",
            "unknown-id",
        ];
        assert!(
            !SOURCE_MARKER_VALID_KINDS.contains(&kind_segment),
            "fingerprint kind {kind_segment:?} must NOT be in the source-marker \
             VALID_KINDS — this documents why g-allow findings must be excluded \
             from the baseline ratchet"
        );
    }
}
