//! An entry made by a test rather than read from tests/corpus/: how the
//! runner's own failures are seeded (docs/prds/instances-on-the-core.md
//! section 12.1 rule 10).

use std::path::PathBuf;

use crate::corpus_tree::{EntryId, EntrySources, LoadedEntry, SourceFile};
use crate::entry::EntryRecord;

/// A one-file entry in theme `other`, from its expectations and its source.
pub fn seeded(name: &str, expectations: &str, source: &str) -> LoadedEntry {
    let record = EntryRecord::from_toml(expectations)
        .unwrap_or_else(|error| panic!("the seeded entry `{name}` loads: {error}"));
    LoadedEntry {
        id: EntryId {
            theme: "other".to_string(),
            name: name.to_string(),
        },
        record,
        sources: EntrySources {
            files: vec![SourceFile {
                name: "seeded.ri".to_string(),
                text: source.to_string(),
            }],
            import_root: PathBuf::from("/nonexistent/seeded-corpus-entry"),
        },
    }
}
