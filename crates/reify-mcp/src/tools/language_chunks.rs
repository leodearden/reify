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
    fn available_topics_returns_19_entries() {
        assert_eq!(available_topics().len(), 19);
    }

    const MAX_CHUNK_BYTES: usize = 36 * 1024;

    /// `get_chunk` serves a whole file per retrieval, so every byte of a chunk is
    /// paid on every lookup of its topic. A chunk that outgrows this ceiling
    /// should have a section split into its own topic — the way task 7344 split
    /// `measurement` and `topology` out of `geometry` — rather than the ceiling
    /// raised. Raising it is a deliberate, diff-visible edit.
    #[test]
    fn every_topic_fits_one_retrieval_budget() {
        let oversized: Vec<(&str, usize)> = TOPICS
            .iter()
            .map(|topic| (*topic, get_chunk(topic).unwrap().len()))
            .filter(|(_, len)| *len > MAX_CHUNK_BYTES)
            .collect();
        assert!(
            oversized.is_empty(),
            "topic(s) over the {MAX_CHUNK_BYTES}-byte per-retrieval ceiling, as (topic, bytes): \
             {oversized:?}. Split a section of each into its own topic rather than raising the \
             ceiling."
        );
    }
}
