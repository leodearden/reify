//! PTODO lane δ-C grammar: decides whether a task cite in a comment line
//! names the OWNER of work still to be done, as opposed to mentioning it as
//! provenance. Normative grammar and measurements:
//! `docs/prds/reify-audit-ptodo-detector.md` §8.1 (δ-C), §21.

use crate::scan_util::is_word_byte;

/// Rule (L): a phrase that, ending immediately before a cite, makes the cited
/// task the owner of the work. Closed: widening it is gated by PRD §16's
/// standing rule, and the measured-and-declined candidates (`until`, `owns`,
/// …) are recorded in §21.
const OWNER_PHRASES: &[&str] = &[
    "owned by",
    "tracked by",
    "tracked in",
    "tracked under",
    "deferred to",
    "blocked on",
    "pending",
    "awaiting",
    "waiting on",
    "waits on",
];

/// Rule (S): the words that make a `#N lands` clause a condition on the
/// future rather than narration (PRD §21).
const SUBORDINATORS: &[&[u8]] = &[b"until", b"when", b"once", b"after", b"before"];

/// Rule (S): the most words that may separate a [`SUBORDINATORS`] word from
/// the cite it governs. The widest governing shape on the 2026-10-21 tree is
/// `after engine wiring (PRD #2947-#2949) lands` (PRD §21).
const SUBORDINATOR_REACH: usize = 4;

/// `true` when the cite spanning `line[head..end]` is in owner position.
/// `head` is the cite's first byte (the `#` of a canonical cite, the `t` of a
/// legacy `task N`); `end` is one past its last digit.
///
/// Every other cite is provenance — the default. Byte-level throughout, so no
/// non-ASCII text around the cite can make it slice inside a character.
pub(crate) fn is_owner_position(line: &str, head: usize, end: usize) -> bool {
    let bytes = line.as_bytes();
    let slot = owner_slot(&bytes[..head]);
    let right = right_of_cite(&bytes[end..]);
    owner_phrase_on_left(slot)
        || future_passive_on_left(slot)
        || will_on_right(right)
        || lands_in_subordinate_clause(line, head, right)
}

/// The text an (L) phrase must end: everything before the cite, minus
/// trailing whitespace, an optional `task(s)` token (the canonical
/// `task #N` spelling) and an optional `(`.
fn owner_slot(left: &[u8]) -> &[u8] {
    let slot = left.trim_ascii_end();
    let slot = strip_word_suffix(slot, b"tasks")
        .or_else(|| strip_word_suffix(slot, b"task"))
        .map_or(slot, <[u8]>::trim_ascii_end);
    slot.strip_suffix(b"(").map_or(slot, <[u8]>::trim_ascii_end)
}

/// The text after the cite, past one optional closing `)`.
fn right_of_cite(right: &[u8]) -> &[u8] {
    right.strip_prefix(b")").unwrap_or(right)
}

/// (L) `slot` ends with an [`OWNER_PHRASES`] entry that starts the text, or
/// follows whitespace, `(` or `/`. The whitelist rejects identifiers
/// (`mark_pending #N`), member paths, quotes and compounds by construction.
fn owner_phrase_on_left(slot: &[u8]) -> bool {
    OWNER_PHRASES.iter().any(|phrase| {
        suffix_ci(slot, phrase.as_bytes()).is_some_and(|rest| {
            rest.last()
                .is_none_or(|&b| b.is_ascii_whitespace() || b == b'(' || b == b'/')
        })
    })
}

/// (L) `slot`'s last four words are `will|to be <verb> by|in|under`.
fn future_passive_on_left(slot: &[u8]) -> bool {
    let mut words = slot
        .rsplit(u8::is_ascii_whitespace)
        .filter(|w| !w.is_empty());
    let (Some(prep), Some(verb), Some(be), Some(aux)) =
        (words.next(), words.next(), words.next(), words.next())
    else {
        return false;
    };
    is_one_of(prep, &[b"by", b"in", b"under"])
        && verb.iter().all(u8::is_ascii_alphabetic)
        && be.eq_ignore_ascii_case(b"be")
        && is_one_of(aux, &[b"will", b"to"])
}

/// (R) the cite is the subject of ` will`.
fn will_on_right(right: &[u8]) -> bool {
    next_word_is(right, b"will")
}

/// (S) the cite is the subject of ` land(s)` and a [`SUBORDINATORS`] word
/// governs it: one sits within [`SUBORDINATOR_REACH`] words before the cite,
/// in the same clause. The clause runs from the last sentence punctuation
/// followed by whitespace (or the line start) up to the cite.
fn lands_in_subordinate_clause(line: &str, head: usize, right: &[u8]) -> bool {
    if !(next_word_is(right, b"land") || next_word_is(right, b"lands")) {
        return false;
    }
    let bytes = line.as_bytes();
    let start = (1..head)
        .rev()
        .find(|&i| {
            matches!(bytes[i - 1], b'.' | b';' | b':' | b'!' | b'?')
                && bytes[i].is_ascii_whitespace()
        })
        .unwrap_or(0);
    bytes[start..head]
        .split(|&b| !is_word_byte(b))
        .filter(|word| !word.is_empty())
        .rev()
        .take(SUBORDINATOR_REACH + 1)
        .any(|word| is_one_of(word, SUBORDINATORS))
}

/// `text` is a single space followed by the whole word `word`
/// (case-insensitive).
fn next_word_is(text: &[u8], word: &[u8]) -> bool {
    text.strip_prefix(b" ").is_some_and(|rest| {
        rest.get(..word.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
            && rest.get(word.len()).is_none_or(|&b| !is_word_byte(b))
    })
}

/// `text` minus a case-insensitive `suffix`, when it ends with one.
fn suffix_ci<'a>(text: &'a [u8], suffix: &[u8]) -> Option<&'a [u8]> {
    let split = text.len().checked_sub(suffix.len())?;
    text[split..]
        .eq_ignore_ascii_case(suffix)
        .then_some(&text[..split])
}

/// [`suffix_ci`] for a whole word: the byte before `word` must not be a word
/// byte.
fn strip_word_suffix<'a>(text: &'a [u8], word: &[u8]) -> Option<&'a [u8]> {
    suffix_ci(text, word).filter(|rest| rest.last().is_none_or(|&b| !is_word_byte(b)))
}

fn is_one_of(word: &[u8], options: &[&[u8]]) -> bool {
    options.iter().any(|o| word.eq_ignore_ascii_case(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verdict for the first occurrence of `cite` in `line`.
    fn owner(line: &str, cite: &str) -> bool {
        let head = line.find(cite).expect("cite present in line");
        is_owner_position(line, head, head + cite.len())
    }

    #[test]
    fn owner_phrase_on_the_left() {
        for (line, cite) in [
            (
                "// production that does not exist; owned by task 4595.",
                "task 4595",
            ),
            ("/// family. Owned by #6006 (registry τ4)", "#6006"),
            (
                "// resolution semantics are deferred to task 3477/3558.",
                "task 3477",
            ),
            (
                "/// Auto-type resolution is deferred to task #3477",
                "#3477",
            ),
            (
                "/// during lowering — their full round-trip is tracked by task 3573.",
                "task 3573",
            ),
            (
                "/// gap documented in mechanism.rs and tracked in task 2538).",
                "task 2538",
            ),
            ("// type mismatch (ignored pending task 875)", "task 875"),
            ("/// hydration is blocked on (#2947)", "#2947"),
            ("// rho: re-home (waits on #5505 and #5500).", "#5505"),
        ] {
            assert!(
                owner(line, cite),
                "{cite} should be owner-position in: {line}"
            );
        }
    }

    #[test]
    fn future_passive_on_the_left() {
        for (line, cite) in [
            (
                "// Defensive: names that will be added by task 2699 but",
                "task 2699",
            ),
            ("// to be wired in #7097", "#7097"),
        ] {
            assert!(
                owner(line, cite),
                "{cite} should be owner-position in: {line}"
            );
        }
    }

    #[test]
    fn future_subject_on_the_right() {
        for (line, cite) in [
            (
                "//!   Task 4262 will add the descriptor-keyed store",
                "Task 4262",
            ),
            ("// and γ (task #5305) will legitimately", "#5305"),
            ("/// The follow-on docs task (#2538) will", "#2538"),
        ] {
            assert!(
                owner(line, cite),
                "{cite} should be owner-position in: {line}"
            );
        }
    }

    #[test]
    fn landing_clause() {
        for (line, cite) in [
            (
                "// node until task 4122 lands selector→mesh-node resolution",
                "task 4122",
            ),
            (
                "/// This is the exact case that fires when #5639 lands and",
                "#5639",
            ),
            ("// after engine wiring (PRD #2947-#2949) lands", "#2949"),
            ("/// Until #6874 lands, this binding", "#6874"),
        ] {
            assert!(
                owner(line, cite),
                "{cite} should be owner-position in: {line}"
            );
        }
    }

    /// Only the cite the rule reaches is the owner; its neighbour on the same
    /// line stays provenance.
    #[test]
    fn verdict_is_per_occurrence() {
        assert!(!owner(
            "// rho: re-home (waits on #5505 and #5500).",
            "#5500"
        ));
        assert!(!owner(
            "// after engine wiring (PRD #2947-#2949) lands",
            "#2947"
        ));
    }

    #[test]
    fn provenance_and_guards_stay_legal() {
        for (line, cite) in [
            ("// introduced by #4578", "#4578"),
            ("// landed in #4578", "#4578"),
            (
                "/// marker declared in this module (task 4578, per",
                "task 4578",
            ),
            ("// see #4578", "#4578"),
            ("// per #4578", "#4578"),
            ("/// #4578's ruling", "#4578"),
            ("// Task 3441: snapshot this template", "Task 3441"),
            (
                "/// was a second override until task 4168, and",
                "task 4168",
            ),
            ("/// shipped this form until #6890 renamed it", "#6890"),
            ("/// until task 6663 DID reinterpret", "task 6663"),
            ("//! Task #6082 lands `x.rs` here rather than", "#6082"),
            (
                "// when the cache is cold the fallback in #4578 lands first",
                "#4578",
            ),
            ("/// Grammar CAPABILITY, owned by the #5907 series", "#5907"),
            (
                "/// This test carried `#[ignore = \"blocked on #5951\"]`",
                "#5951",
            ),
            ("// mark_pending #4578", "#4578"),
            ("// self.pending #4578", "#4578"),
            ("// #4578 willfully", "#4578"),
            (
                "/// ALL FAILURES AT ONCE (reviewer amendment, task 5623) comes from",
                "task 5623",
            ),
        ] {
            assert!(!owner(line, cite), "{cite} must stay provenance in: {line}");
        }
    }

    #[test]
    fn non_ascii_context_is_safe() {
        assert!(owner("// κ — owned by #4578 ζ", "#4578"));
        assert!(owner("// ζ: once task 4578 lands — κ", "task 4578"));
        assert!(!owner("// κ — introduced by #4578 ζ", "#4578"));
        assert!(!owner("// —#4578— ζ", "#4578"));
    }
}
