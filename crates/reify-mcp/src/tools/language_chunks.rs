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
}
