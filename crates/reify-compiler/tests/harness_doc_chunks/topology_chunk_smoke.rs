//! Truth checks for the "topology" language-reference chunk
//! (`crates/reify-mcp/src/tools/chunks/topology.md`), served to the in-GUI
//! assistant via `reify_language_reference`: the selector catalogue it carries
//! must name exactly the selectors the compiler registers.
//!
//! Lives in `reify-compiler` (not `reify-mcp`, where the chunk itself lives)
//! because `reify-mcp` does not depend on `reify-compiler`, so only this crate
//! can read the compiler's registries. Split out of geometry.md by task 7344,
//! with the guard that covers it.
//!
//! What this file owns:
//!
//! - the catalogue table covers `reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`
//!   and names nothing outside it, read through `chunk_markdown.rs`'s catalogue
//!   scanners;
//! - every test the SYNC note cites resolves.
//!
//! The table's Call form, Result and Eval columns are NOT checked here. The
//! chunk's own SYNC note says so, and names the sources a reader must re-read
//! before trusting a cell.
//!
//! # The one doc-FORMAT pin this file does impose
//!
//! The selector catalogue must stay a markdown TABLE whose FIRST cell backticks
//! the selector each row is about; rewriting it into bullets is RED. Every other
//! column is free-form.
//!
//! Every `MINIMUM_*` floor here is re-measured by the protocol stated once next
//! to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.

use crate::callable_registries::{phantom_name_panic, registry_family};
use crate::chunk_cite_gate::assert_cited_paths_resolve;
use crate::chunk_io::{TOPOLOGY_CHUNK_PATH, read_chunk};
use crate::chunk_markdown::{catalogue_table_names, catalogue_table_rows, section_body};

/// Marker that OPENS the TOPOLOGY-SELECTOR catalogue section — the one that
/// documents [`reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`] as a table.
/// Matched BYTE-EXACTLY on the trimmed line, as every section marker in this
/// harness is, so the heading's wording stays free to change.
///
/// Scoping is what makes the scan mean anything for THIS family in particular:
/// `edges` and `faces` appear all over the geometry chunk as the fillet /
/// chamfer / shell ARGUMENT name (`fillet(solid, edges, radius)`), where they
/// say nothing about the selector that produces such a list. The catalogue is
/// the one place they are documented as callable selectors, so the catalogue is
/// what is scanned.
const TOPOLOGY_SECTION_MARKER: &str = "<!-- TOPOLOGY-SECTION -->";

/// Human-readable name of [`TOPOLOGY_SECTION_MARKER`]'s section. Panic text
/// only; nothing matches on it.
const TOPOLOGY_SECTION_TITLE: &str = "# Topology Selectors";

/// Minimum CATALOGUE ROWS the TOPOLOGY-SELECTOR table must carry.
///
/// The EXACT live length of `reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES`
/// (31 today), not a round number under it. The coverage half of
/// [`topology_selector_family_documented_in_topology_chunk`] cannot catch a
/// gutted table on its own — reformatting the table into prose bullets empties
/// [`catalogue_table_rows`] and the coverage loop then compares against an empty
/// set. The floor is what makes that RED.
///
/// A DERIVED value would be better and is deliberately not used: asserting
/// `rows.len() >= GEOMETRY_TOPOLOGY_SELECTOR_NAMES.len()` reads as tighter but is
/// strictly weaker as a floor, because a row documenting one selector under two
/// names (the shared-cell shape `catalogue_table_rows` supports) legitimately
/// makes rows FEWER than names. Keeping the number literal keeps the two claims
/// independent, which is the point of having both.
///
/// Raise this WITH the table; never lower it to go green.
const MINIMUM_TOPOLOGY_CATALOGUE_ROWS: usize = 31;

/// Every member of the topology-selector registry must have a CATALOGUE ROW, and
/// every catalogue row must name a real selector.
///
/// BOTH DIRECTIONS, because each catches a different rot. Coverage (registry →
/// table) catches a selector landing in `units.rs` that the chunk never learns
/// about — the in-GUI assistant then cannot reach it and falls back to indexing
/// `faces(...)` by hand. Registry truth (table → registry) catches the failure
/// that has actually happened in this repo: the 2026-07-24 language review found
/// `rotate(geo, axis, angle)` and `translate(geo, vector)` documented at
/// signatures the compiler had never been shown (tasks #5347 / #5364). A phantom
/// selector is worse than a missing one, because the reader has no reason to
/// doubt it.
///
/// A TABLE rather than the measurement chunk's call-form prose, and that is a
/// scanner decision rather than a style one: [`catalogue_table_rows`] already
/// exists to read exactly this shape (it backs geometry.md's length-argument
/// catalogue), and a 31-entry family rendered as prose is unreadable for the
/// human as well as unscannable for the test. The registry is iterated DIRECTLY
/// here for the same reason `measurement_chunk_smoke.rs`'s
/// `measurement_query_family_documented_in_measurement_chunk` iterates its own:
/// at 31 names a hand-copied mirror is a bigger drift surface than the thing it
/// guards.
#[test]
fn topology_selector_family_documented_in_topology_chunk() {
    let markdown = read_chunk(TOPOLOGY_CHUNK_PATH);
    // Panics if the marker is gone, so deleting the section is RED rather than
    // vacuously green.
    let section = section_body(
        &markdown,
        TOPOLOGY_SECTION_MARKER,
        TOPOLOGY_CHUNK_PATH,
        TOPOLOGY_SECTION_TITLE,
    );

    let rows = catalogue_table_rows(&section);
    assert!(
        rows.len() >= MINIMUM_TOPOLOGY_CATALOGUE_ROWS,
        "only {} catalogue row(s) found in {TOPOLOGY_CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` \
         table — expected at least {MINIMUM_TOPOLOGY_CATALOGUE_ROWS}. Either rows were deleted, \
         or the table was reformatted into a shape this scan cannot read: a catalogue row is a \
         `|`-leading line whose FIRST cell backticks the selector it is about. Without the rows \
         the coverage check below compares against an empty set and protects nothing. Rows seen: \
         {rows:?}",
        rows.len()
    );

    let table_names = catalogue_table_names(&rows);

    // (a) COVERAGE — registry → table.
    for name in reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES {
        assert!(
            table_names.iter().any(|n| n == name),
            "{TOPOLOGY_CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has no row for the \
             topology selector `{name}`. The chunk is what the in-GUI assistant retrieves, so a \
             selector missing from this table reads to it as a MISSING CAPABILITY: it will index \
             `faces(...)` positionally, or hand-roll a filter, instead of calling the selector \
             that exists (task 5581). Add a row whose FIRST cell backticks `{name}`, or — if the \
             builtin itself is gone — remove it from \
             reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES, which is iterated directly here \
             and is the sole source of this list. Names the table does carry: {table_names:?}"
        );
    }

    // (b) REGISTRY TRUTH — table → registry. A row naming something that is not
    // a selector sends an author to write a call the compiler will not accept,
    // and — because a `structure def` body types an unresolved call from its
    // first argument — often will not even say so.
    for name in &table_names {
        if reify_compiler::GEOMETRY_TOPOLOGY_SELECTOR_NAMES.contains(&name.as_str()) {
            continue;
        }
        // Split by WHY it is not a selector, so the panic names the actual
        // remedy. `phantom_name_panic` is the shared voice for "no registry has
        // this name at all"; a name that IS real but lives in a sibling family
        // is a different (and more confusing) mistake, and gets told so.
        match registry_family(name) {
            None => panic!(
                "{}",
                phantom_name_panic(
                    TOPOLOGY_CHUNK_PATH,
                    "the topology-selector catalogue table's first column",
                    name
                )
            ),
            Some(family) => panic!(
                "{TOPOLOGY_CHUNK_PATH}'s `{TOPOLOGY_SECTION_TITLE}` catalogue has a row for \
                 `{name}`, which is a real builtin but belongs to {family}, NOT \
                 GEOMETRY_TOPOLOGY_SELECTOR_NAMES. The families are disjoint by construction and \
                 dispatch differently, so documenting one here teaches a reader the wrong \
                 result type and the wrong resolution stage. Move the row to the chunk that \
                 owns {family}, or drop it."
            ),
        }
    }
}

/// Cite floors for [`cited_test_paths_in_the_chunk_resolve`]: `<path>::<fn>`
/// cites, then distinct `.rs` files, then distinct `.ri` files. The EXACT live
/// counts, never round numbers under them — why exact, and how to re-measure
/// one, is stated once next to `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
/// Raise them WITH the chunk; never lower one to go green.
const MINIMUM_FN_CITES: usize = 3;
const MINIMUM_RS_FILES: usize = 3;
const MINIMUM_RI_FILES: usize = 1;

/// Every source the chunk cites must still exist.
///
/// The chunk's SYNC note names the guard above and the compiler and eval
/// sources its unpinned columns were transcribed from, and points a reader at a
/// runnable worked example. A renamed or deleted target would leave the note
/// sending a maintainer to a file that is not there.
///
/// The mechanism is shared with every other chunk module that floors its cites;
/// see [`assert_cited_paths_resolve`] for what it checks and what it does NOT
/// (an existence check, never a semantic one).
#[test]
fn cited_test_paths_in_the_chunk_resolve() {
    assert_cited_paths_resolve(
        TOPOLOGY_CHUNK_PATH,
        &read_chunk(TOPOLOGY_CHUNK_PATH),
        MINIMUM_FN_CITES,
        MINIMUM_RS_FILES,
        MINIMUM_RI_FILES,
    );
}
