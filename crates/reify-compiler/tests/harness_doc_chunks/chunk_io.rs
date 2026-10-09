//! Where the MCP language-reference chunk corpus lives, how a chunk is read and
//! listed, and how a corpus-wide gate over it reports what it found.
//!
//! `reify-mcp` does NOT depend on `reify-compiler`, so the chunks cannot be
//! reached through `language_chunks::get_chunk` from here; they are read BY PATH
//! at runtime instead, so an edit to the markdown is seen by `cargo test`
//! without a rebuild of this crate. Every path is REPO-RELATIVE — the currency a
//! violation message names a chunk in, so a failure reads as a path a developer
//! can open rather than an absolute build-machine one. A wrong path fails
//! loudly at read time; nothing here ever skips.

use std::path::{Path, PathBuf};

/// The chunk directory, repo-relative, as a macro so `concat!` can splice it
/// into every per-chunk path below — `concat!` expands a macro argument eagerly
/// but cannot read a `const`. [`CHUNKS_DIR`] is the one spelling everything
/// else uses.
macro_rules! chunks_dir {
    () => {
        "crates/reify-mcp/src/tools/chunks"
    };
}

/// The chunk directory, repo-relative.
pub(crate) const CHUNKS_DIR: &str = chunks_dir!();

/// `geometry.md` — primitives, profiles, zones, and the interference oracle.
pub(crate) const GEOMETRY_CHUNK_PATH: &str = concat!(chunks_dir!(), "/geometry.md");

/// `measurement.md` — the measurement and mass-property queries.
pub(crate) const MEASUREMENT_CHUNK_PATH: &str = concat!(chunks_dir!(), "/measurement.md");

/// `topology.md` — the topology-selector catalogue.
pub(crate) const TOPOLOGY_CHUNK_PATH: &str = concat!(chunks_dir!(), "/topology.md");

/// `stdlib.md` — the geometry-operation and curve tables.
pub(crate) const STDLIB_CHUNK_PATH: &str = concat!(chunks_dir!(), "/stdlib.md");

/// `enums.md` — including the `## Option Type` section.
pub(crate) const ENUMS_CHUNK_PATH: &str = concat!(chunks_dir!(), "/enums.md");

/// `units.md` — dimensioned literals and the rejected forms.
pub(crate) const UNITS_CHUNK_PATH: &str = concat!(chunks_dir!(), "/units.md");

/// `functions.md` — including the `## Overloading` listing.
pub(crate) const FUNCTIONS_CHUNK_PATH: &str = concat!(chunks_dir!(), "/functions.md");

/// `constraints.md` — where a designer writes a gate.
pub(crate) const CONSTRAINTS_CHUNK_PATH: &str = concat!(chunks_dir!(), "/constraints.md");

/// The EXACT number of `.md` files in [`CHUNKS_DIR`].
///
/// A live count, never a lower bound: slack is not a safety margin. At `>= 18`
/// against 19 files a whole chunk could be deleted with nothing going red and
/// no constant to lower. [`all_chunks`] compares with `>=` so a vacuous scan
/// fails fast and specifically; the EXACTNESS obligation is the separately
/// named `chunk_file_count_is_exact_not_slack`, so a diff that legitimately
/// adds a chunk gets a message telling it to re-measure rather than a vacuity
/// warning describing a bug that did not happen.
pub(crate) const CHUNK_FILE_COUNT: usize = 19;

/// Repo root, derived from this crate's manifest dir
/// (`<repo>/crates/reify-compiler`) — what every repo-relative path in this
/// binary resolves against.
pub(crate) fn repo_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!("CARGO_MANIFEST_DIR ({manifest:?}) must sit two levels under the repo root")
        })
        .to_path_buf()
}

/// The text of the chunk at the repo-relative `path`, panicking loudly (never
/// skipping) if it cannot be read.
pub(crate) fn read_chunk(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| {
        panic!("{path} must be readable ({e}) — update its path in chunk_io.rs if the chunk moved")
    })
}

/// The repo-relative path of the chunk named `stem`.
pub(crate) fn chunk_label(stem: &str) -> String {
    format!("{CHUNKS_DIR}/{stem}.md")
}

/// The text of the chunk named `stem`.
pub(crate) fn read_chunk_file(stem: &str) -> String {
    read_chunk(&chunk_label(stem))
}

/// Every `*.md` stem in the chunk dir, PATH-SORTED.
///
/// Sorted because `read_dir` order is filesystem-dependent: without this a
/// failure list would shuffle between machines and a diff of two runs would be
/// unreadable. Mirrors `pdoccover`'s sorted-corpus discipline.
pub(crate) fn discover_chunk_stems() -> Vec<String> {
    let entries = std::fs::read_dir(repo_root().join(CHUNKS_DIR)).unwrap_or_else(|e| {
        panic!("{CHUNKS_DIR} must be readable ({e}) — update CHUNKS_DIR if the chunk dir moved")
    });

    let mut stems: Vec<String> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|e| panic!("{CHUNKS_DIR}: unreadable dir entry ({e})"))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .collect();
    stems.sort();
    stems
}

/// Every chunk as `(stem, markdown)`, in stem order, for the corpus-wide check
/// `gate` names — after asserting the scan found the whole corpus, so a check
/// over a vacuous scan fails rather than passes.
pub(crate) fn all_chunks(gate: &str) -> Vec<(String, String)> {
    let stems = discover_chunk_stems();
    assert!(
        stems.len() >= CHUNK_FILE_COUNT,
        "the chunk-dir scan found only {} chunk(s), expected {CHUNK_FILE_COUNT} — {gate} would \
         be vacuous",
        stems.len()
    );
    stems
        .into_iter()
        .map(|stem| {
            let markdown = read_chunk_file(&stem);
            (stem, markdown)
        })
        .collect()
}

/// Render an accumulated violation list as one panic message.
pub(crate) fn report(check: &str, violations: &[String]) {
    assert!(
        violations.is_empty(),
        "{check}: {} violation(s)\n\n{}\n",
        violations.len(),
        violations.join("\n\n")
    );
}

/// `CHUNK_FILE_COUNT` must EQUAL the live corpus.
///
/// [`all_chunks`] compares with `>=` so a vacuous scan fails fast; without this
/// test that `>=` would be the only comparison, and the gap between floor and
/// live would be exactly the number of chunks that could disappear unremarked.
/// Growing the corpus is expected and makes this go red on purpose: raise the
/// constant in the diff that adds the file. What must not happen silently is
/// the other direction.
#[test]
fn chunk_file_count_is_exact_not_slack() {
    let stems = discover_chunk_stems();
    assert_eq!(
        stems.len(),
        CHUNK_FILE_COUNT,
        "{CHUNKS_DIR} holds {} `.md` file(s) while CHUNK_FILE_COUNT records \
         {CHUNK_FILE_COUNT}. Re-measure and record the live count in the SAME \
         diff that adds or removes a chunk — otherwise the difference is the \
         number of chunks that can later vanish with every test still green.",
        stems.len()
    );
}
