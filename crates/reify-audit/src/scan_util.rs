//! Shared primitives for this crate's structural text scanners — PTODO,
//! PDSSENTINEL and PDOCCOVER.
//!
//! Each of those detectors hand-rolls the same two things over raw source
//! lines: a `\b` word-boundary match (the crate takes no `regex` dep, per
//! `f-infra-design.md` §12) and an inline `<token> — <reason>` marker
//! grammar. Before task #6036 each had its own copy, so a boundary-semantics
//! fix — such as the char-stepped retry that stopped a multibyte registry
//! entry from panicking PDOCCOVER on ~8MB of chunk prose — reached exactly
//! one detector and had to be rediscovered in the next. They share one
//! implementation here so such a fix reaches all of them at once.
//!
//! What this module deliberately does NOT own is argued on
//! [`allow_marker_body`]: two sibling marker grammars resemble this one and
//! are measurably not it.

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the single
/// alphabet for every hand-rolled `\b` word-boundary check in this crate's
/// scanners, so `union` is never satisfied by `disunion` / `union_all` /
/// `reunion` and `done` is never satisfied by `abandoned` / `undone`.
///
/// A byte predicate over arbitrary UTF-8: every non-ASCII byte reads as a
/// boundary, which is what lets the callers do byte-offset arithmetic over
/// text they never validated.
pub(crate) fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `true` when `needle` occurs in `haystack` delimited by word boundaries on
/// BOTH sides — a hand-rolled `\b<needle>\b` over [`is_word_byte`]'s
/// alphabet. An empty `needle` never matches; it would otherwise match at
/// every position.
///
/// Case-SENSITIVE. A caller needing case-insensitive matching pre-lowercases
/// `haystack` and passes a lowercase ASCII `needle` — which is what PTODO's
/// terminal-token lane does (`ptodo.rs` lowercases at the two sites that
/// build the slices, and its needles are the literals `done` / `cancelled`).
///
/// UTF-8-safe on BOTH arguments; neither is assumed ASCII. A match index is
/// always a char boundary — the needle's first byte is either ASCII or a
/// UTF-8 lead byte, and neither can occur mid-char — but the
/// boundary-rejected RETRY must still step by a whole CHARACTER, or a
/// non-ASCII needle would re-slice from inside a continuation byte and
/// panic. That is not hypothetical: `units.rs` is a units registry, so a
/// `"µm"` / `"°C"` entry is ordinary, and one boundary-rejected occurrence
/// anywhere in PDOCCOVER's ~8MB of chunk prose was enough to take the whole
/// detector down. A scanner whose contract is "unreadable input is skipped
/// fail-safe (no finding, no panic)" must have no panic reachable from
/// corpus content.
pub(crate) fn contains_word(haystack: &str, needle: &str) -> bool {
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

/// Byte offset of a word-boundary-delimited `token` in `line`, or `None` when
/// the line carries no such occurrence.
///
/// Left-boundary only — the caller decides what, if anything, may follow, so
/// this is a hand-rolled `\b<token>` rather than a whole-word match. That is
/// what keeps a token from being matched as the tail of a longer word
/// (`xxpdoccover:allow`), and what makes a legacy unprefixed spelling that is
/// a SUFFIX of the current one (`doccover:allow`) simply never match.
///
/// A rejected occurrence advances by the token's whole length. Unlike
/// [`contains_word`]'s haystack-driven retry, that step is safe as a byte
/// step: `token` is caller-supplied ASCII marker syntax, so its length is its
/// char count and `idx + token.len()` is always a char boundary.
pub(crate) fn find_word_boundary_token(line: &str, token: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut start = 0;
    loop {
        let rel = line[start..].find(token)?;
        let idx = start + rel;
        if idx == 0 || !is_word_byte(bytes[idx - 1]) {
            return Some(idx);
        }
        start = idx + token.len();
    }
}

/// Reason body of an inline `token` marker on `line`, or `None` when the line
/// carries no marker or the body is blank after trimming.
///
/// Grammar, in the order it is applied — the order is load-bearing:
/// locate `token`, take the rest of the line, `trim_end`, strip a trailing
/// comment terminator (`-->`, else `*/`), trim, strip ONE optional separator
/// (em dash `—`, ASCII hyphen `-`, or colon `:`), trim, reject blank.
/// Terminator before separator, because `-->` BEGINS with the ASCII-hyphen
/// separator: the other order reads `<!-- pdoccover:allow -->` as a
/// well-formed marker whose reason is `->`, silently suppressing a real
/// claim. `*/` is the same hazard inside a Rust block comment.
///
/// A `None` return on a line that DOES carry the token is what a caller turns
/// into an `allow-missing-reason`-style finding; a reasonless escape hatch is
/// never silently honoured.
///
/// Deliberately NOT the home for the crate's two other marker readers, each
/// for a measured reason (both pinned by tests below, so a later convergence
/// pass reds instead of changing a detector):
/// - `ptodo::g_allow_marker_body` requires the marker to own the whole line
///   after a literal `//`, and strips neither separator nor terminator. This
///   grammar is free-floating, so it also finds the TRAILING markers PTODO
///   does not — which would newly admit them into the hard-gated G-allow
///   owner-cite lane and the arm-(7) delta-B guard.
/// - `pdssentinel::has_allow_marker` is a bare presence check with no reason
///   requirement, and this function rejects a blank body — so routing it here
///   would flip every reasonless `// ds-sentinel:allow` from suppressing to
///   not suppressing.
///
/// The explicit `'a` is required: with two `&str` parameters and no `&self`,
/// elision cannot choose the return lifetime. The body borrows from `line`.
pub(crate) fn allow_marker_body<'a>(line: &'a str, token: &str) -> Option<&'a str> {
    let idx = find_word_boundary_token(line, token)?;
    let body = &line[idx + token.len()..];
    let body = body.trim_end();
    let body = body
        .strip_suffix("-->")
        .or_else(|| body.strip_suffix("*/"))
        .unwrap_or(body);
    let body = body.trim();
    let body = body
        .strip_prefix('—')
        .or_else(|| body.strip_prefix('-'))
        .or_else(|| body.strip_prefix(':'))
        .unwrap_or(body);
    let body = body.trim();
    if body.is_empty() { None } else { Some(body) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The boundary alphabet is ASCII-only, on purpose: it is a *byte*
    /// predicate applied to arbitrary UTF-8, so every non-ASCII byte — lead
    /// or continuation — must read as a boundary rather than as a word byte.
    #[test]
    fn is_word_byte_alphabet_is_ascii_alnum_underscore() {
        for b in [b'a', b'Z', b'0', b'_'] {
            assert!(is_word_byte(b), "{} must be a word byte", b as char);
        }
        for b in [b'-', b' ', b':', b'.'] {
            assert!(!is_word_byte(b), "{} must not be a word byte", b as char);
        }
        // A UTF-8 lead byte (here `§`'s 0xC2) is not a word byte: the
        // alphabet is ASCII-only, so multibyte text always delimits.
        assert!(
            !is_word_byte(0xC2),
            "a UTF-8 lead byte must not be a word byte",
        );
    }

    /// Both sides matter. `union`, `union_all` and `intersection` are all real
    /// registry entries, so a one-sided match would let `union_all`'s
    /// documentation silently vouch for `union` and under-report coverage.
    #[test]
    fn contains_word_requires_both_boundaries() {
        assert!(
            !contains_word("union_all(list)", "union"),
            "`union_all` must not vouch for `union` — right boundary fails",
        );
        assert!(
            contains_word("- `union(a, b)` — combine", "union"),
            "a backtick-delimited `union(` mention is a whole-word match",
        );
        assert!(
            !contains_word("intersection_all", "intersection"),
            "`intersection_all` must not vouch for `intersection`",
        );
    }

    /// The subword rejections PTODO's terminal-token lane relies on: its
    /// `done` / `cancelled` needles must never be satisfied by a subword.
    #[test]
    fn contains_word_rejects_subword_occurrences() {
        assert!(
            !contains_word("abandoned", "done"),
            "`abandoned` contains `done` but not as a whole word",
        );
        assert!(
            !contains_word("undone", "done"),
            "`undone` contains `done` but not as a whole word",
        );
        assert!(contains_word("task is done", "done"));
        assert!(contains_word("(k — tots sqp, cancelled)", "cancelled"));
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

    /// An empty needle would otherwise match at every position, so a registry
    /// entry that trims to nothing could vouch for itself everywhere.
    #[test]
    fn contains_word_empty_needle_never_matches() {
        assert!(!contains_word("anything", ""));
    }

    /// Left-boundary-checked, and a rejected occurrence does not end the
    /// search: the token is never matched as the tail of a longer word, but a
    /// glued occurrence must not mask a clean one later on the same line.
    #[test]
    fn find_word_boundary_token_skips_glued_occurrences() {
        assert_eq!(
            find_word_boundary_token("// pdoccover:allow — x", "pdoccover:allow"),
            Some(3),
            "a boundary-clean occurrence is located at its byte offset",
        );
        assert_eq!(
            find_word_boundary_token("// xxpdoccover:allow — x", "pdoccover:allow"),
            None,
            "an occurrence glued to a preceding word byte is not the token",
        );
        assert_eq!(
            find_word_boundary_token("xxpdoccover:allow and pdoccover:allow", "pdoccover:allow"),
            Some(22),
            "the glued occurrence is skipped, not treated as the answer",
        );
    }

    /// One optional separator, in any of the three spellings a marker is
    /// written with in the wild — plus the no-separator form.
    #[test]
    fn allow_marker_body_accepts_each_separator_form() {
        for line in [
            "// pdoccover:allow — reason",
            "// pdoccover:allow - reason",
            "// pdoccover:allow: reason",
            "// pdoccover:allow reason",
        ] {
            assert_eq!(
                allow_marker_body(line, "pdoccover:allow"),
                Some("reason"),
                "`{line}` must yield the trimmed body",
            );
        }
    }

    /// The comment terminator is stripped BEFORE the separator, and the order
    /// is load-bearing: `-->` begins with the ASCII-hyphen separator, so the
    /// other order reads `<!-- pdoccover:allow -->` as a well-formed marker
    /// whose reason is `->` and silently suppresses a real claim.
    #[test]
    fn allow_marker_body_strips_terminator_before_separator() {
        assert_eq!(
            allow_marker_body(
                "<!-- pdoccover:allow — planned, see #5434 -->",
                "pdoccover:allow"
            ),
            Some("planned, see #5434"),
            "an HTML-comment marker's reason excludes the `-->` terminator",
        );
        assert_eq!(
            allow_marker_body("/* pdoccover:allow — reason */", "pdoccover:allow"),
            Some("reason"),
            "a Rust block-comment marker's reason excludes the `*/` terminator",
        );
        assert_eq!(
            allow_marker_body("<!-- pdoccover:allow -->", "pdoccover:allow"),
            None,
            "a reasonless HTML-comment marker must not parse a reason of `->`",
        );
    }

    /// A reasonless marker is not a marker with a reason: the caller turns
    /// `None`-with-token into its own finding rather than honouring an
    /// un-reviewable escape hatch.
    #[test]
    fn allow_marker_body_rejects_blank_body_and_absent_token() {
        assert_eq!(
            allow_marker_body("// pdoccover:allow", "pdoccover:allow"),
            None
        );
        assert_eq!(
            allow_marker_body("// pdoccover:allow ", "pdoccover:allow"),
            None
        );
        assert_eq!(
            allow_marker_body("// an ordinary comment", "pdoccover:allow"),
            None
        );
    }

    /// DIVERGENCE PIN — `ptodo::g_allow_marker_body` is deliberately NOT this
    /// grammar, and this is the measurement that says so.
    ///
    /// The line below is real: `crates/reify-stdlib/src/dynamics/mass_props.rs`
    /// carries it verbatim, as a TRAILING comment after an attribute.
    /// `g_allow_marker_body` requires the marker to own the whole line (it
    /// strips leading whitespace and then demands a literal `//`), so today it
    /// reads this line as carrying no marker at all. The free-floating shared
    /// grammar finds one. Converging the two would therefore newly admit this
    /// line — and every other trailing-comment G-allow marker on the real
    /// tree — into PTODO's G-allow owner-cite lane and into the arm-(7)
    /// delta-B guard. That lane is hard-gated repo-wide, so this is a
    /// behaviour change on a gating detector, not a mechanical relocation.
    #[test]
    fn g_allow_grammar_deliberately_diverges_from_shared_marker_body() {
        let corpus_line = "#[allow(dead_code)] // G-allow: test-only analytic \
                           ground-truth closed form; KGQ wiring into \
                           body_mass_props landed via #3829 (done) + #4237 \
                           dynamics_ops seam (done); fn is permanent test-only \
                           helper, zero production callers by design";
        assert_eq!(
            crate::ptodo::g_allow_marker_body(corpus_line),
            None,
            "PTODO's grammar requires a whole-line `//` comment, so a trailing \
             marker carries no G-allow body for it",
        );
        let shared = allow_marker_body(corpus_line, "G-allow:");
        assert!(
            shared.is_some_and(|b| b.starts_with("test-only analytic")),
            "the shared grammar is free-floating and DOES find a body here — \
             routing g_allow_marker_body through it would change what the \
             hard-gated G-allow lane sees; got {shared:?}",
        );
        // The second flip, in the other direction: the shared grammar strips
        // one leading separator, so a body that is only a separator collapses
        // to blank — where PTODO keeps it and suppresses on it.
        assert_eq!(
            crate::ptodo::g_allow_marker_body("// G-allow: -"),
            Some("-")
        );
        assert_eq!(allow_marker_body("// G-allow: -", "G-allow:"), None);
    }

    /// DIVERGENCE PIN — `pdssentinel::has_allow_marker` is deliberately NOT
    /// this grammar either: it is a bare PRESENCE check with no reason
    /// requirement, so a reasonless marker suppresses.
    ///
    /// `allow_marker_body` rejects a blank body, so routing the sentinel
    /// through it would flip a bare `// ds-sentinel:allow` from suppressing to
    /// NOT suppressing — silently turning every reasonless marker on the
    /// scoped compiler files into a finding.
    #[test]
    fn ds_sentinel_presence_check_deliberately_diverges_from_shared_marker_body() {
        assert_eq!(
            allow_marker_body("    let x = y; // ds-sentinel:allow", "ds-sentinel:allow"),
            None,
            "the shared grammar rejects a reasonless marker",
        );

        let suppressed = "\
fn resolve(name: &str) -> Type {
    diagnostics.push(
        Diagnostic::error(format!(\"unresolved type: {}\", name))
            .with_code(DiagnosticCode::UnresolvedType)
    );
    Type::dimensionless_scalar() // ds-sentinel:allow
}
";
        assert!(
            crate::pdssentinel::scan_content(suppressed).is_empty(),
            "a REASONLESS ds-sentinel:allow still suppresses — which is why \
             the sentinel does not route through allow_marker_body",
        );
        // Non-vacuity: the same shape without the marker is a live hit, so the
        // emptiness above is the marker's doing and not the fixture's shape.
        let unsuppressed = suppressed.replace(" // ds-sentinel:allow", "");
        assert_eq!(crate::pdssentinel::scan_content(&unsuppressed).len(), 1);
    }
}
