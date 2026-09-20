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
}
