// Language reference chunks — embedded markdown topic files

const SYNTAX: &str = include_str!("chunks/syntax.md");
const STRUCTURES: &str = include_str!("chunks/structures.md");
const PARAMETERS: &str = include_str!("chunks/parameters.md");
const CONSTRAINTS: &str = include_str!("chunks/constraints.md");
const GEOMETRY: &str = include_str!("chunks/geometry.md");
const TRAITS: &str = include_str!("chunks/traits.md");
const COLLECTIONS: &str = include_str!("chunks/collections.md");
const FIELDS: &str = include_str!("chunks/fields.md");
const PURPOSES: &str = include_str!("chunks/purposes.md");
const OCCURRENCES: &str = include_str!("chunks/occurrences.md");
const CONNECT: &str = include_str!("chunks/connect.md");
const ENUMS: &str = include_str!("chunks/enums.md");
const GUARDS: &str = include_str!("chunks/guards.md");
const FUNCTIONS: &str = include_str!("chunks/functions.md");
const UNITS: &str = include_str!("chunks/units.md");
const TYPES: &str = include_str!("chunks/types.md");
const STDLIB: &str = include_str!("chunks/stdlib.md");

/// All available topic names, in alphabetical order.
pub const TOPICS: &[&str] = &[
    "collections",
    "connect",
    "constraints",
    "enums",
    "fields",
    "functions",
    "geometry",
    "guards",
    "occurrences",
    "parameters",
    "purposes",
    "stdlib",
    "structures",
    "syntax",
    "traits",
    "types",
    "units",
];

/// Look up a language reference chunk by topic name.
///
/// Returns `None` if the topic is not recognized.
pub fn get_chunk(topic: &str) -> Option<&'static str> {
    match topic {
        "syntax" => Some(SYNTAX),
        "structures" => Some(STRUCTURES),
        "parameters" => Some(PARAMETERS),
        "constraints" => Some(CONSTRAINTS),
        "geometry" => Some(GEOMETRY),
        "traits" => Some(TRAITS),
        "collections" => Some(COLLECTIONS),
        "fields" => Some(FIELDS),
        "purposes" => Some(PURPOSES),
        "occurrences" => Some(OCCURRENCES),
        "connect" => Some(CONNECT),
        "enums" => Some(ENUMS),
        "guards" => Some(GUARDS),
        "functions" => Some(FUNCTIONS),
        "units" => Some(UNITS),
        "types" => Some(TYPES),
        "stdlib" => Some(STDLIB),
        _ => None,
    }
}

/// Return the list of all available topic names.
pub fn available_topics() -> &'static [&'static str] {
    TOPICS
}

/// Every embedded chunk as `(topic, content)`, in the same order as
/// [`TOPICS`]. Backs [`chunk_corpus`].
const CORPUS: &[(&str, &str)] = &[
    ("collections", COLLECTIONS),
    ("connect", CONNECT),
    ("constraints", CONSTRAINTS),
    ("enums", ENUMS),
    ("fields", FIELDS),
    ("functions", FUNCTIONS),
    ("geometry", GEOMETRY),
    ("guards", GUARDS),
    ("occurrences", OCCURRENCES),
    ("parameters", PARAMETERS),
    ("purposes", PURPOSES),
    ("stdlib", STDLIB),
    ("structures", STRUCTURES),
    ("syntax", SYNTAX),
    ("traits", TRAITS),
    ("types", TYPES),
    ("units", UNITS),
];

/// Every embedded chunk as `(topic, content)` pairs, read directly from the
/// `include_str!` constants above — never from the filesystem — so
/// corpus-wide invariants track exactly what the shipped binary serves
/// rather than whatever happens to be on disk.
///
/// This exists so corpus-wide invariants (e.g. "this vocabulary must survive
/// *somewhere* in the corpus") can be asserted in-crate at the DEFAULT test
/// gate, unlike opt-in PDOCCOVER (`crates/reify-audit/src/pdoccover.rs`),
/// which is not part of the default sweep. See
/// `docs/notes/angle-crossing-doctrine-placement-2026-08-19.md` for the
/// decision record this accessor was added to support.
pub fn chunk_corpus() -> &'static [(&'static str, &'static str)] {
    CORPUS
}

/// `true` when `b` is an ASCII word byte (`[A-Za-z0-9_]`) — the alphabet for
/// the word-boundary check in [`contains_word`]. Mirrors
/// `reify_audit::pdoccover::is_word_byte`
/// (`crates/reify-audit/src/pdoccover.rs:247`); reproduced rather than
/// linked because reify-mcp must not depend on reify-audit for a two-line
/// predicate.
///
/// `#[cfg(test)]`: every caller of this helper is a corpus-invariant guard
/// test (transitively, via [`contains_word`] and [`corpus_contains_word`]),
/// so it is cfg-gated the same way rather than shipping as unreachable code
/// in the binary.
#[cfg(test)]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `true` when `needle` occurs in `haystack` with a non-word byte (or a
/// string edge) on both flanks — so `curl` matches the comma-separated list
/// in units.md:92 but never `curling` or `uncurl`.
///
/// Mirrors `reify_audit::pdoccover::contains_word`
/// (`crates/reify-audit/src/pdoccover.rs:945`) exactly, including its
/// char-stepped retry: after a boundary-rejected candidate, the scan must
/// advance by a whole CHARACTER, not a byte, or a multi-byte character in
/// chunk prose (`—`, `·`, `π`, `θ`) can be sliced into and panic. See
/// `contains_word_retry_is_char_stepped_not_byte_stepped`
/// (`crates/reify-audit/src/pdoccover.rs:2343`) for the regression this
/// mirrors, and `contains_word_matches_word_boundaries_only` below for this
/// module's copy of that lesson.
///
/// `#[cfg(test)]`: see [`is_word_byte`] — this is test-only infrastructure.
#[cfg(test)]
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
        // Advance past this occurrence's first CHARACTER, not past a single
        // byte: a byte-stepped retry can land inside a multi-byte character
        // and panic on the next slice.
        start = idx + haystack[idx..].chars().next().map_or(1, char::len_utf8);
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// `true` when `needle` has a word-boundary mention (see [`contains_word`])
/// anywhere in [`chunk_corpus`].
///
/// `#[cfg(test)]`: see [`is_word_byte`] — this is test-only infrastructure.
#[cfg(test)]
fn corpus_contains_word(needle: &str) -> bool {
    chunk_corpus()
        .iter()
        .any(|(_, content)| contains_word(content, needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_topics_return_non_empty_content() {
        for topic in TOPICS {
            let content = get_chunk(topic);
            assert!(content.is_some(), "Topic '{topic}' returned None");
            assert!(
                content.unwrap().len() > 100,
                "Topic '{topic}' content too short"
            );
        }
    }

    #[test]
    fn unknown_topic_returns_none() {
        assert!(get_chunk("foobar").is_none());
    }

    #[test]
    fn available_topics_returns_17_entries() {
        assert_eq!(available_topics().len(), 17);
        assert_eq!(
            chunk_corpus().len(),
            TOPICS.len(),
            "chunk_corpus() must stay in sync with the TOPICS roster"
        );
    }

    /// The angle-crossing doctrine's `Hz`/`rad/s` vocabulary is a corpus
    /// SINGLETON — both terms occur exactly once, on the same line
    /// (`units.md:94`), so dropping them from `units.md` drops them
    /// everywhere. `docs/notes/angle-crossing-discoverability-2026-08-10.md`
    /// Q4 ("Hz to rad/s" / "angular frequency") measured this exact wording
    /// as RED, and the sharpest miss in the walk, on its first pass; esc-6267-3
    /// later caught a live removal of this vocabulary by hand, post-hoc, after
    /// it had already shipped. This guard makes that check executable instead
    /// of a hand-run grep. Asserted over the whole corpus (not just `UNITS`
    /// alone) so a future task may legitimately relocate the vocabulary to
    /// another chunk without tripping this guard. See
    /// `docs/notes/angle-crossing-doctrine-placement-2026-08-19.md` for the
    /// full decision record.
    #[test]
    fn angle_crossing_goal_vocabulary_survives_in_the_corpus() {
        for term in ["Hz", "rad/s"] {
            let found = chunk_corpus()
                .iter()
                .any(|(_, content)| content.contains(term));
            assert!(
                found,
                "goal-vocabulary term `{term}` is missing from the entire chunk \
                 corpus. `docs/notes/angle-crossing-discoverability-2026-08-10.md` \
                 Q4 (\"Hz to rad/s\" / \"angular frequency\") found this exact \
                 wording RED, and the sharpest miss in the walk, on its first \
                 pass; esc-6267-3 later caught a live removal of this vocabulary \
                 by hand, post-hoc. `{term}` is a corpus SINGLETON (units.md:94) \
                 — dropping it from units.md drops it everywhere."
            );
        }
    }

    /// PDOCCOVER tripwire: the field-operator names' ONLY corpus mention is
    /// `units.md:92`, inside the angle-crossing doctrine section. Losing it
    /// would silently ship a `pdoccover` regression — PDOCCOVER
    /// (`crates/reify-audit/src/pdoccover.rs`) would add one High
    /// `undocumented-name:` finding per name — but `pdoccover` is opt-in
    /// (`crates/reify-audit/pdoccover-baseline.txt` does not exist) and is
    /// not part of the default sweep, so the merge gate alone would not
    /// catch this. See
    /// `docs/notes/angle-crossing-doctrine-placement-2026-08-19.md`.
    #[test]
    fn field_operator_names_keep_a_chunk_mention() {
        // Mirrors `FIELD_OP_NAMES` at `crates/reify-compiler/src/units.rs:1041`.
        // Hard-coded rather than depending on reify-compiler from reify-mcp to
        // read the live registry: that would be a layering inversion for a
        // four-element list, and PDOCCOVER already owns the live-registry
        // direction.
        const FIELD_OP_NAMES: &[&str] = &["gradient", "divergence", "curl", "laplacian"];
        for name in FIELD_OP_NAMES {
            assert!(
                corpus_contains_word(name),
                "field-operator name `{name}` has no word-boundary mention \
                 anywhere in the chunk corpus. Its only corpus mention was \
                 `units.md:92`; losing it adds one High `undocumented-name:` \
                 PDOCCOVER finding (crates/reify-audit/src/pdoccover.rs). \
                 `crates/reify-audit/pdoccover-baseline.txt` does not exist \
                 and PDOCCOVER is opt-in, so the merge gate would not catch \
                 this."
            );
        }
    }

    /// Focused coverage for the word-boundary matcher the guard above
    /// depends on. Mirrors the shape (not the code) of
    /// `reify_audit::pdoccover`'s own `contains_word` coverage — see
    /// `crates/reify-audit/src/pdoccover.rs:2343` for the char-stepped-retry
    /// regression the last case here reproduces.
    #[test]
    fn contains_word_matches_word_boundaries_only() {
        // Exact match.
        assert!(contains_word("curl", "curl"));

        // Rejected substring inside a longer word: neither a longer prefix
        // nor a longer suffix vouches for the shorter word.
        assert!(!contains_word("curling stays outside INV-AD-2", "curl"));
        assert!(!contains_word("cannot uncurl a manifold edge", "curl"));

        // Punctuation-flanked, exactly as in units.md:92's comma-separated
        // list ("gradient, divergence, curl and laplacian").
        assert!(contains_word(
            "gradient, divergence, curl and laplacian stay pure quotient",
            "curl"
        ));

        // A multi-byte-char haystack whose first candidate is
        // boundary-rejected, forcing a retry that starts INSIDE the
        // multi-byte `µ`. A byte-stepped retry would panic here (slicing at
        // a non-char-boundary index); a char-stepped retry instead finds
        // the second, boundary-clean occurrence.
        assert!(contains_word("aµm µm", "µm"));
        assert!(!contains_word("aµm", "µm"));
    }
}
