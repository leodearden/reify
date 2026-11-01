// Language reference chunks — embedded markdown topic files

/// The one table of the language reference: each topic paired with the chunk
/// file named after it, topics strictly ascending. `available_topics()` and
/// `get_chunk` both derive from it.
pub const CHUNKS: &[(&str, &str)] = &[
    ("collections", include_str!("chunks/collections.md")),
    ("connect", include_str!("chunks/connect.md")),
    ("constraints", include_str!("chunks/constraints.md")),
    ("enums", include_str!("chunks/enums.md")),
    ("fields", include_str!("chunks/fields.md")),
    ("functions", include_str!("chunks/functions.md")),
    ("geometry", include_str!("chunks/geometry.md")),
    ("guards", include_str!("chunks/guards.md")),
    ("measurement", include_str!("chunks/measurement.md")),
    ("occurrences", include_str!("chunks/occurrences.md")),
    ("parameters", include_str!("chunks/parameters.md")),
    ("purposes", include_str!("chunks/purposes.md")),
    ("stdlib", include_str!("chunks/stdlib.md")),
    ("structures", include_str!("chunks/structures.md")),
    ("syntax", include_str!("chunks/syntax.md")),
    ("topology", include_str!("chunks/topology.md")),
    ("traits", include_str!("chunks/traits.md")),
    ("types", include_str!("chunks/types.md")),
    ("units", include_str!("chunks/units.md")),
];

static TOPICS: [&str; CHUNKS.len()] = topic_names();

const fn topic_names() -> [&'static str; CHUNKS.len()] {
    let mut names = [""; CHUNKS.len()];
    let mut i = 0;
    while i < CHUNKS.len() {
        names[i] = CHUNKS[i].0;
        i += 1;
    }
    names
}

/// Look up a language reference chunk by topic name.
///
/// Returns `None` if the topic is not recognized.
pub fn get_chunk(topic: &str) -> Option<&'static str> {
    CHUNKS
        .iter()
        .find(|(name, _)| *name == topic)
        .map(|(_, chunk)| *chunk)
}

/// Return the list of all available topic names, in alphabetical order.
pub fn available_topics() -> &'static [&'static str] {
    &TOPICS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn all_topics_return_non_empty_content() {
        for topic in available_topics() {
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
    fn chunk_topics_are_strictly_ascending() {
        let out_of_order: Vec<(&str, &str)> = CHUNKS
            .windows(2)
            .filter(|w| w[0].0 >= w[1].0)
            .map(|w| (w[0].0, w[1].0))
            .collect();
        assert!(
            out_of_order.is_empty(),
            "CHUNKS topics must be strictly ascending; offending adjacent (left, right) pairs: \
             {out_of_order:?}. Sorted keeps the `reify_language_reference` help listing (which \
             joins `available_topics()` in order) byte-identical; strict rules out a duplicate \
             topic whose second row `get_chunk` would never serve."
        );
    }

    #[test]
    fn every_topic_serves_the_chunk_file_named_after_it() {
        let chunks_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools/chunks");
        let mismatched: Vec<&str> = CHUNKS
            .iter()
            .filter(|(topic, chunk)| {
                let path = chunks_dir.join(format!("{topic}.md"));
                let on_disk = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
                on_disk != *chunk || get_chunk(topic) != Some(on_disk.as_str())
            })
            .map(|(topic, _)| *topic)
            .collect();
        assert!(
            mismatched.is_empty(),
            "topic(s) whose served chunk is not byte-identical to src/tools/chunks/<topic>.md: \
             {mismatched:?}. Each CHUNKS row must pair a topic with the file named after it."
        );
    }

    const MAX_CHUNK_BYTES: usize = 36 * 1024;

    /// `get_chunk` serves a whole file per retrieval, so every byte of a chunk is
    /// paid on every lookup of its topic. A chunk that outgrows this ceiling
    /// should have a section split into its own topic — the way task 7344 split
    /// `measurement` and `topology` out of `geometry` — rather than the ceiling
    /// raised. Raising it is a deliberate, diff-visible edit.
    #[test]
    fn every_topic_fits_one_retrieval_budget() {
        let oversized: Vec<(&str, usize)> = CHUNKS
            .iter()
            .map(|(topic, chunk)| (*topic, chunk.len()))
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
