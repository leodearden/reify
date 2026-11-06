//! The binary's ONE cite scanner and resolver, and the two corpus gates built on
//! it: every cited path resolves, and every maintainer note is whole with every
//! SYNC note naming a path.
//!
//! A cite is a repo path a chunk names — `docs/…/x.md`, `crates/…/y.rs`,
//! `examples/…/z.ri`, or a `name.rs::fn` test — read out of the markdown by
//! [`cited_source_paths`] and resolved against the tree by
//! [`audit_cited_paths`]. Resolution is EXISTENCE only: that the file is there
//! and, for a `::fn` cite, that it declares that fn — never that a cited test
//! still asserts what the prose claims.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::chunk_io::{all_chunks, chunk_label, repo_root, report};
use crate::chunk_prose::{
    EARLY_CLOSED_NOTE_FIX, HTML_COMMENT_CLOSE, HtmlComment, html_comments,
    stray_comment_terminators,
};

/// Every `.rs`/`.ri` file under `crates/` and `examples/`, keyed by basename.
type BasenameIndex = BTreeMap<String, Vec<PathBuf>>;

/// Index of every tracked-ish `.rs`/`.ri` file under `crates/` and `examples/`,
/// keyed by BASENAME, so the chunk may cite a test by bare file name (as its
/// prose already does) without this check hard-coding a directory.
///
/// Build artifacts are skipped by directory name rather than by path prefix, so a
/// nested `target/` cannot smuggle a stale duplicate into the index and make an
/// otherwise-unique basename ambiguous.
///
/// Walked at most ONCE per test process, when a bare-basename cite first needs
/// it: the tree does not change under a test run, and the corpus gate resolves
/// every chunk's cites against the same index.
fn source_files_by_basename() -> &'static BasenameIndex {
    fn walk(dir: &Path, out: &mut BasenameIndex) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if matches!(name.as_str(), "target" | ".git" | "node_modules") {
                    continue;
                }
                walk(&path, out);
            } else if name.ends_with(".rs") || name.ends_with(".ri") {
                out.entry(name).or_default().push(path);
            }
        }
    }

    static INDEX: OnceLock<BasenameIndex> = OnceLock::new();
    INDEX.get_or_init(|| {
        let root = repo_root();
        let mut out = BTreeMap::new();
        walk(&root.join("crates"), &mut out);
        walk(&root.join("examples"), &mut out);
        out
    })
}

/// Every repo-path cite in `markdown`, deduped, in document order: a `/`-bearing
/// path whose last segment has a LETTER-led extension, or a bare `.rs`/`.ri`
/// basename carrying a `::<fn>` half. The fn half is kept when it is a bare
/// identifier.
///
/// Scans maximal runs of path-ish characters, so markdown decoration (backticks,
/// parens, commas, the possessive `'s`) bounds a run rather than being swallowed,
/// and a nested `examples/` segment stays part of its whole path. A trailing
/// sentence period is not part of the path.
///
/// ANY extension, not a `.rs`/`.ri` allowlist: stale citations span many file
/// kinds, and an allowlist silently misses the next one.
/// LETTER-led, so a divided length (`width/2.0`) stays arithmetic.
///
/// A URL is NOT a cite, however file-like its path: each whitespace-separated
/// word is read only up to where a URL in it begins ([`before_url`]), so the
/// text of a `[crates/x.rs](https://…)` link is still read and its target is not.
///
/// A BARE basename in prose is NOT a cite. geometry.md is a designer-facing
/// tutorial whose whole subject is writing `.ri` files, so it will keep acquiring
/// illustrative filenames ("save the model as `my_bracket.ri`"); resolving those
/// would make an ordinary doc edit RED with a message that names neither the edit
/// nor its cause. The same rule keeps the chunks' C++ cites
/// (`BRepExtrema_DistShapeShape::InnerSolution()`) out without an exclusion list.
pub(crate) fn cited_source_paths(markdown: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();

    let runs = markdown
        .split_whitespace()
        .map(before_url)
        .flat_map(|text| text.split(|c: char| !is_path_char(c)));
    for run in runs {
        // Sentence punctuation that the run charset happens to include.
        let run = run.trim_end_matches(['.', '/', ':', '-']);
        let (path, fn_name) = match run.split_once("::") {
            Some((path, rest)) => (path, Some(rest)),
            None => (run, None),
        };
        // Tested on the RAW `::` split, before the identifier filter below: a
        // malformed fn half still marks the token as an intended cite, so its
        // path half stays subject to resolution.
        let is_cite = if path.contains('/') {
            has_letter_led_extension(path)
        } else {
            fn_name.is_some() && (path.ends_with(".rs") || path.ends_with(".ri"))
        };
        if !is_cite {
            continue;
        }
        let fn_name = fn_name
            .filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        let cite = (path.to_string(), fn_name.map(str::to_string));
        if !out.contains(&cite) {
            out.push(cite);
        }
    }
    out
}

fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '-')
}

/// `word` cut where a URL in it begins — at the scheme before its `://`. A URL
/// runs to the end of its word, through characters (`~`, `?`, `%`) that would
/// otherwise split its path into path-shaped pieces.
fn before_url(word: &str) -> &str {
    match word.find("://") {
        Some(separator) => word[..separator].trim_end_matches(|c: char| c.is_ascii_alphanumeric()),
        None => word,
    }
}

fn has_letter_led_extension(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    file_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.starts_with(|c: char| c.is_ascii_alphabetic()))
}

/// Resolve one cited path token to a real file, or explain why it did not.
fn resolve_cited_path(token: &str) -> Result<PathBuf, String> {
    let root = repo_root();
    if token.contains('/') {
        // Repo-relative, or crate-relative (the chunks write both forms).
        for candidate in [root.join(token), root.join("crates").join(token)] {
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
        return Err(format!(
            "no such file — tried {:?} and {:?}",
            root.join(token),
            root.join("crates").join(token)
        ));
    }
    match source_files_by_basename().get(token).map(Vec::as_slice) {
        None | Some([]) => Err(format!(
            "no file named `{token}` exists under crates/ or examples/"
        )),
        Some([only]) => Ok(only.clone()),
        Some(many) => Err(format!(
            "`{token}` is ambiguous — {} files share that basename ({many:?}); cite it by its \
             full repo-relative path instead",
            many.len()
        )),
    }
}

/// What resolving one chunk's cites found.
pub(crate) struct CiteAudit {
    /// Every cite [`cited_source_paths`] read, resolved or not.
    pub(crate) cites: Vec<(String, Option<String>)>,
    /// One line per cite that does not resolve, or whose file does not declare
    /// its `::fn`.
    pub(crate) violations: Vec<String>,
    /// `<path>::<fn>` cites whose path resolved.
    pub(crate) fn_cites: usize,
    /// Distinct resolved `.rs` files — distinct after resolution, so one file
    /// cited two ways (bare basename and full path) counts once.
    pub(crate) rs_files: BTreeSet<PathBuf>,
    /// Distinct resolved `.ri` files, likewise.
    pub(crate) ri_files: BTreeSet<PathBuf>,
}

/// Resolve every cite in `markdown`, accumulating every failure as a violation
/// line naming `chunk_path` and the cite — never panicking, so one run reports a
/// whole chunk's (or corpus's) dangling cites at once.
///
/// Only `.rs` and `.ri` files are bucketed; any other kind (a `docs/…md` cite)
/// counts as a cite and in neither bucket.
pub(crate) fn audit_cited_paths(chunk_path: &str, markdown: &str) -> CiteAudit {
    let mut audit = CiteAudit {
        cites: cited_source_paths(markdown),
        violations: Vec::new(),
        fn_cites: 0,
        rs_files: BTreeSet::new(),
        ri_files: BTreeSet::new(),
    };

    for (path_token, fn_name) in &audit.cites {
        let resolved = match resolve_cited_path(path_token) {
            Ok(resolved) => resolved,
            Err(why) => {
                audit.violations.push(format!(
                    "{chunk_path} cites `{path_token}`, which does not resolve: {why}. The chunk \
                     is served verbatim to the in-GUI assistant, so a dangling cite sends it to a \
                     file that is not there — and in a SYNC row it is a false claim that a real \
                     test pins something. Repoint the cite at the file's repo-relative path, or \
                     mark the row UNPINNED."
                ));
                continue;
            }
        };

        if path_token.ends_with(".rs") {
            audit.rs_files.insert(resolved.clone());
        } else if path_token.ends_with(".ri") {
            audit.ri_files.insert(resolved.clone());
        }

        let Some(fn_name) = fn_name else { continue };
        audit.fn_cites += 1;
        match std::fs::read_to_string(&resolved) {
            Ok(body) if body.contains(&format!("fn {fn_name}(")) => {}
            Ok(_) => audit.violations.push(format!(
                "{chunk_path} cites `{path_token}::{fn_name}` as pinning one of its claims, but \
                 {resolved:?} declares no `fn {fn_name}(`. The test was renamed or deleted, so \
                 that row now claims a pin that does not exist. Re-point the cite, or downgrade \
                 the row to UNPINNED."
            )),
            Err(e) => audit.violations.push(format!(
                "{chunk_path} cites `{path_token}::{fn_name}`, but {resolved:?} is unreadable ({e})"
            )),
        }
    }
    audit
}

/// Assert every cite in `markdown` resolves, and that the chunk still carries at
/// least `min_fn` / `min_rs` / `min_ri` of them.
///
/// SHARED BY EVERY CHUNK MODULE THAT FLOORS ITS CITES — among them
/// `geometry_chunk_smoke.rs`'s traps SYNC block and `units_chunk_smoke.rs`'s
/// PINNED/UNPINNED inventory — so the resolution and existence rule has one
/// copy.
///
/// The CHUNK-SPECIFIC "why this matters" prose lives in each caller's docstring,
/// not in the panic text here, so the shared message stays true for all of them. What
/// the panics do carry is the chunk path, the floor that was missed and the full
/// cite list, which is what a reader needs to act.
///
/// Floors are `>=`, so ADDING a cite is always safe; raise them WITH the chunk
/// when one is added, and never lower one to go green — a lowered floor is a
/// SYNC row that has quietly stopped claiming anything.
///
/// SCOPE — an EXISTENCE check, not a semantic one. It cannot tell that a
/// still-named test stopped asserting what the row claims, it says nothing about
/// rows marked UNPINNED, and it does not verify the fn is a `#[test]`.
pub(crate) fn assert_cited_paths_resolve(
    chunk_path: &str,
    markdown: &str,
    min_fn: usize,
    min_rs: usize,
    min_ri: usize,
) {
    let audit = audit_cited_paths(chunk_path, markdown);
    let cites = &audit.cites;

    assert!(
        audit.violations.is_empty(),
        "{} cite(s) in {chunk_path} do not hold:\n\n{}\n",
        audit.violations.len(),
        audit.violations.join("\n\n")
    );

    // Anti-vacuity. Reformatting a SYNC block into a shape this scan cannot read
    // — a path wrapped across two lines, or tabulated into two columns — would
    // otherwise empty the audit above and pass.
    assert!(
        audit.fn_cites >= min_fn,
        "only {} `<path>::<fn>` cite(s) found in {chunk_path} — expected at least {min_fn}. \
         CITES MUST BE WRITTEN WHOLE ON ONE LINE, never wrapped and never tabulated; a wrapped \
         path is invisible to this scan. Either the chunk was reformatted into a shape it cannot \
         read, or a row lost its cite while still claiming to pin something. Cites seen: \
         {cites:?}",
        audit.fn_cites
    );
    assert!(
        audit.rs_files.len() >= min_rs,
        "only {} distinct `.rs` FILE(s) cited in {chunk_path} (distinct after resolution — the \
         same file cited two ways counts once), expected at least {min_rs}. Losing one turns a \
         PINNED row into prose. Cites seen: {cites:?}",
        audit.rs_files.len()
    );
    assert!(
        audit.ri_files.len() >= min_ri,
        "only {} distinct `.ri` example FILE(s) cited in {chunk_path} (distinct after resolution \
         — the same example cited both bare and by full path counts once), expected at least \
         {min_ri}. The worked examples are what a designer is sent to next, so losing a cite is a \
         discoverability regression. Cites seen: {cites:?}",
        audit.ri_files.len()
    );
}

/// The SYNC notes in `markdown`: maintainer notes (HTML comments outside any
/// fence) whose body starts with `SYNC:`. Their cites are simply whatever
/// [`cited_source_paths`] reads in the body, so "verified by <path>" and
/// "<path> verifies" are the same to it.
pub(crate) fn sync_notes(markdown: &str) -> Result<Vec<HtmlComment>, String> {
    Ok(html_comments(markdown)?
        .into_iter()
        .filter(|comment| comment.body.trim_start().starts_with("SYNC:"))
        .collect())
}

/// Everything wrong with one chunk's maintainer notes, one line each: a SYNC
/// note that names no path, and — for EVERY note, SYNC or not — the `-->` a note
/// that closed early leaves in the rendered prose. A chunk whose notes cannot be
/// read at all is one violation.
pub(crate) fn note_violations(chunk_path: &str, markdown: &str) -> Vec<String> {
    let read = sync_notes(markdown)
        .and_then(|notes| stray_comment_terminators(markdown).map(|strays| (notes, strays)));
    let (notes, strays) = match read {
        Ok(read) => read,
        Err(error) => return vec![format!("{chunk_path}: {error}")],
    };

    let unnamed = notes
        .iter()
        .filter(|note| cited_source_paths(&note.body).is_empty())
        .map(|note| {
            format!(
                "{chunk_path}:{} — this SYNC note names no path. A SYNC note points a maintainer \
                 at what verifies the prose beside it, so one naming nothing is a claim nobody \
                 can check. FIX: cite the verifying file whole on one line (repo-relative, or \
                 `name.rs::fn`), or drop the `SYNC:` lead if the note makes no such claim.",
                note.line
            )
        });
    let closed_early = strays.into_iter().map(|line| {
        format!(
            "{chunk_path}:{line} — a maintainer note CLOSED EARLY: this `{HTML_COMMENT_CLOSE}` \
             survives into the rendered chunk, so the note's tail is text the reader sees, and a \
             SYNC note loses every cite past the early close. {EARLY_CLOSED_NOTE_FIX}"
        )
    });
    unnamed.chain(closed_early).collect()
}

// ---------------------------------------------------------------------------
// The real-corpus gate
// ---------------------------------------------------------------------------

/// Anti-vacuity floors for [`every_path_cited_by_any_chunk_resolves`]: the cites
/// read across every chunk, and the chunks carrying at least one. EXACT live
/// values — re-measure them by the protocol stated once next to
/// `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const MINIMUM_CORPUS_CITES: usize = 74;
const MINIMUM_CITING_CHUNKS: usize = 7;

/// Every repo path any chunk cites must exist — a `docs/…md` pointer, a
/// `crates/…rs` test, an `examples/…ri` walk or a `name.rs::fn` alike.
///
/// SCOPE: existence only, never that a cited test still asserts what the prose
/// claims. Complementary to the chunk-local cite floors in the per-chunk smoke
/// modules (`geometry_chunk_smoke.rs`, `units_chunk_smoke.rs`, …), which hold particular
/// SYNC inventories to their size, and to
/// `tests/infra/test_cited_test_paths_resolve.sh` (#7095), which reports only
/// MOVED `crates/*/tests/*.rs` cites — never a deleted target or a non-test path.
#[test]
fn every_path_cited_by_any_chunk_resolves() {
    let audits: Vec<CiteAudit> = all_chunks("the cite gate")
        .iter()
        .map(|(stem, markdown)| audit_cited_paths(&chunk_label(stem), markdown))
        .collect();

    let cites: usize = audits.iter().map(|audit| audit.cites.len()).sum();
    let citing_chunks = audits
        .iter()
        .filter(|audit| !audit.cites.is_empty())
        .count();
    assert!(
        cites >= MINIMUM_CORPUS_CITES,
        "the cite scan read only {cites} cite(s) across the chunks, expected at least \
         {MINIMUM_CORPUS_CITES} — either the scanner regressed and the gate below passes \
         vacuously, or cites were removed and MINIMUM_CORPUS_CITES must come down in the same diff"
    );
    assert!(
        citing_chunks >= MINIMUM_CITING_CHUNKS,
        "only {citing_chunks} chunk(s) carry a cite the scan can read, expected at least \
         {MINIMUM_CITING_CHUNKS} — either the scanner regressed, or a chunk's cites were removed \
         and MINIMUM_CITING_CHUNKS must come down in the same diff"
    );

    let violations: Vec<String> = audits
        .into_iter()
        .flat_map(|audit| audit.violations)
        .collect();
    report(
        "paths cited by the MCP language-reference chunks that do not resolve. A chunk is served \
         verbatim to the in-GUI assistant, so every dangling cite sends it looking for a file \
         that is not there",
        &violations,
    );
}

/// The PER-CHUNK SYNC-note floor: every chunk carrying a SYNC note, each entry
/// its EXACT live count — attributed per file, and TOTAL, in fence_gate's
/// `REIFY_FENCE_FLOORS` idiom. Re-measure by the protocol stated once next to
/// `geometry_chunk_smoke.rs`'s `MINIMUM_FN_CITES`.
const SYNC_NOTE_FLOORS: &[(&str, usize)] = &[
    ("geometry", 3),
    ("measurement", 1),
    ("stdlib", 1),
    ("topology", 1),
    ("units", 1),
];

/// Every maintainer note in every chunk must be WHOLE, and every SYNC note must
/// name a path.
///
/// A note that closes early leaks its tail into what the reader sees, and a SYNC
/// note truncated that way silently loses the cites past the early close — so
/// intactness is checked for every note, SYNC or not. Whether the named paths
/// RESOLVE is [`every_path_cited_by_any_chunk_resolves`]' job, so one defect
/// reds one test.
#[test]
fn every_maintainer_note_in_every_chunk_is_intact_and_every_sync_note_names_a_path() {
    let chunks = all_chunks("the note gate");

    for (stem, markdown) in &chunks {
        // An unreadable chunk is reported below, as a violation of its own.
        let Ok(notes) = sync_notes(markdown).map(|notes| notes.len()) else {
            continue;
        };
        let label = chunk_label(stem);
        match SYNC_NOTE_FLOORS
            .iter()
            .find(|(floor_stem, _)| floor_stem == stem)
        {
            Some((_, floor)) => assert!(
                notes >= *floor,
                "{label} carries {notes} SYNC note(s), expected at least {floor}. A SYNC note is \
                 what points a maintainer at the test verifying the prose beside it; losing one \
                 leaves that prose looking guarded while nothing says by what. If it was \
                 retired deliberately, lower its SYNC_NOTE_FLOORS entry in the same diff."
            ),
            None => assert_eq!(
                notes, 0,
                "{label} carries {notes} SYNC note(s) but has NO entry in SYNC_NOTE_FLOORS. Add \
                 `(\"{stem}\", {notes})` there, so that losing one later is RED."
            ),
        }
    }

    let violations: Vec<String> = chunks
        .iter()
        .flat_map(|(stem, markdown)| note_violations(&chunk_label(stem), markdown))
        .collect();
    report(
        "maintainer notes in the MCP language-reference chunks that closed early or, as SYNC \
         notes, name no path",
        &violations,
    );
}

// ---------------------------------------------------------------------------
// Hermetic tests — synthetic markdown; real repo files serve only as
// resolution targets.
// ---------------------------------------------------------------------------

#[test]
fn cited_source_paths_ignores_a_bare_illustrative_basename() {
    // geometry.md is a designer-facing tutorial about authoring `.ri` files, so
    // prose like this is ordinary content — not a claim that a repo file exists.
    // Resolving it would make an ordinary doc edit RED with a panic about SYNC
    // blocks and false PINNED claims.
    let md = "Save the model as `my_bracket.ri` and run `reify build my_bracket.ri`.\n\
              trap 5 — PINNED by\n\
              crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs::single_body_self_pair_excluded\n\
              See `examples/kinematic/dock_pickup.ri`, and `geometry_chunk_smoke.rs`, whose\n\
              `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve` resolves them.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            (
                "crates/reify-eval/tests/harness_mechanism/mechanism_interference_smoke.rs"
                    .to_string(),
                Some("single_body_self_pair_excluded".to_string()),
            ),
            ("examples/kinematic/dock_pickup.ri".to_string(), None),
            (
                "geometry_chunk_smoke.rs".to_string(),
                Some("cited_test_paths_in_the_chunk_resolve".to_string()),
            ),
        ],
        "only `/`-bearing paths and `::<fn>`-carrying tokens are cites; `my_bracket.ri` and the \
         bare `geometry_chunk_smoke.rs` mention are prose"
    );
}

#[test]
fn cited_source_paths_keeps_a_malformed_fn_half_as_a_path_cite() {
    // `::` marks the token as an INTENDED cite even when the fn half is not a bare
    // identifier, so the path half stays subject to resolution rather than being
    // dropped as if it were a prose basename.
    assert_eq!(
        cited_source_paths("geometry_chunk_smoke.rs::not-an-ident"),
        vec![("geometry_chunk_smoke.rs".to_string(), None)]
    );
}

#[test]
fn cited_source_paths_leaves_a_cxx_cite_alone() {
    // The chunk cites OCCT's C++ API for the containment behaviour; the path half
    // does not end in `.rs`/`.ri`, so no resolution is attempted.
    assert!(
        cited_source_paths("BRepExtrema_DistShapeShape::InnerSolution()").is_empty(),
        "a C++ `Type::method()` cite is not a source-file cite"
    );
}

#[test]
fn cited_source_paths_reads_a_slash_bearing_path_with_any_letter_led_extension() {
    let md = "Rule: `docs/prds/v0_6/doc-chunk-truth-enforcement.md`; runner: scripts/gui-test.sh.\n\
              Fixture crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri and\n\
              `geometry_chunk_smoke.rs::cited_test_paths_in_the_chunk_resolve`.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            (
                "docs/prds/v0_6/doc-chunk-truth-enforcement.md".to_string(),
                None
            ),
            ("scripts/gui-test.sh".to_string(), None),
            (
                "crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri".to_string(),
                None
            ),
            (
                "geometry_chunk_smoke.rs".to_string(),
                Some("cited_test_paths_in_the_chunk_resolve".to_string())
            ),
        ],
        "any `/`-bearing path whose last segment has a letter-led extension is a cite; a bare \
         basename still needs `.rs`/`.ri` AND a `::fn` half"
    );
}

#[test]
fn cited_source_paths_ignores_words_that_only_look_path_shaped() {
    let md = "Save it as `my_bracket.ri`; designs live under examples/, and/or the tolerancing/\n\
              subdir. OCCT's `BRepExtrema_DistShapeShape::InnerSolution()` decides it, and a\n\
              length divided as width/2.0 stays a length. The API is documented at\n\
              https://docs.rs/foo/latest/foo/index.html and <https://example.com/~me/a.md?f=docs/x.md>.\n";

    assert_eq!(
        cited_source_paths(md),
        Vec::<(String, Option<String>)>::new(),
        "a bare basename, a directory word, an extension-less slash token, a C++ cite, a \
         digit-led `.0` and a URL — even one whose `~`/`?` would split it into path-shaped \
         pieces — are prose, not file cites"
    );
}

#[test]
fn cited_source_paths_reads_a_link_text_cite_but_not_its_url_target() {
    let md = "See [crates/reify-compiler/tests/harness_doc_chunks.rs](https://example.com/blob/main/crates/x.rs).\n";

    assert_eq!(
        cited_source_paths(md),
        vec![(
            "crates/reify-compiler/tests/harness_doc_chunks.rs".to_string(),
            None
        )],
        "a word is cut where its URL begins, not skipped whole, so the link text stays a cite"
    );
}

#[test]
fn cited_source_paths_keeps_a_nested_examples_segment_whole_and_dedupes_in_document_order() {
    let md = "Worked example: `examples/tolerancing/gdt_zones.ri`.\n\
              See `docs/examples/foo.ri`, examples/tolerancing/gdt_zones.ri and examples/half_space.ri.\n";

    assert_eq!(
        cited_source_paths(md),
        vec![
            ("examples/tolerancing/gdt_zones.ri".to_string(), None),
            ("docs/examples/foo.ri".to_string(), None),
            ("examples/half_space.ri".to_string(), None),
        ],
        "a nested `examples/` segment is part of its whole path, a repeated cite is listed \
         once, and the period ending a sentence is not part of the path"
    );
}

#[test]
fn audit_reports_every_dangling_cite_and_undeclared_fn_without_panicking() {
    let md = "See `docs/prds/no_such_prd.md` and\n\
              crates/reify-compiler/tests/no_such_harness.rs::anything\n\
              crates/reify-compiler/tests/harness_doc_chunks.rs::no_such_test_fn\n";

    let audit = audit_cited_paths("chunks/synthetic.md", md);

    assert_eq!(audit.violations.len(), 3, "got {:#?}", audit.violations);
    let expected_cites = [
        "docs/prds/no_such_prd.md",
        "crates/reify-compiler/tests/no_such_harness.rs",
        "harness_doc_chunks.rs::no_such_test_fn",
    ];
    for (violation, cite) in audit.violations.iter().zip(expected_cites) {
        assert!(
            violation.contains("chunks/synthetic.md") && violation.contains(cite),
            "each violation must name the chunk and the cite `{cite}`, got: {violation}"
        );
    }
}

#[test]
fn audit_passes_real_cites_and_buckets_only_rs_and_ri_files() {
    let md = "crates/reify-compiler/tests/harness_doc_chunks.rs\n\
              `examples/half_space.ri`\n\
              `docs/prds/v0_6/doc-chunk-truth-enforcement.md`\n\
              crates/reify-eval/src/geometry_ops.rs::expected_arity\n";

    let audit = audit_cited_paths("chunks/synthetic.md", md);

    assert_eq!(audit.violations, Vec::<String>::new());
    assert_eq!(audit.cites.len(), 4, "got {:#?}", audit.cites);
    assert_eq!(audit.fn_cites, 1);
    assert_eq!(
        (audit.rs_files.len(), audit.ri_files.len()),
        (2, 1),
        "a `.md` cite is counted as a cite but in NEITHER file bucket, so the chunk-local \
         `.rs`/`.ri` floors stay exact"
    );
}

#[test]
fn a_sync_note_names_its_verifier_in_either_order() {
    let md = "<!-- SYNC: signatures verified by crates/reify-compiler/tests/harness_doc_chunks.rs -->\n\
              prose\n\
              <!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks.rs verifies, for all\n     \
              five query names, that each is documented. -->\n";

    let notes = sync_notes(md).expect("well-formed markdown must list its SYNC notes");

    assert_eq!(
        notes.iter().map(|note| note.line).collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert_eq!(
        note_violations("chunks/synthetic.md", md),
        Vec::<String>::new(),
        "\"verified by <path>\" and \"<path> verifies\" both name a path"
    );
}

/// The explanatory inventory shape: `<!--` alone on its line, a `SYNC:` lead,
/// a FORMAT note quoting the cite template, then one real cite per line.
const INVENTORY_NOTE: &str = "\
<!--
SYNC: which claim below is pinned by an executable test, and where.

FORMAT IS LOAD-BEARING. Every cite is written WHOLE on ONE line as `<path>::<fn_name>`, never
wrapped and never tabulated.

  claim 1 — PINNED by
    crates/reify-compiler/tests/harness_doc_chunks/chunk_prose.rs::code_spans_are_returned_in_document_order_with_their_opening_line
    crates/reify-eval/src/geometry_ops.rs::expected_arity
-->
";

#[test]
fn an_inventory_note_is_one_note_whose_cite_lines_are_read_and_whose_template_is_not() {
    let notes = sync_notes(INVENTORY_NOTE).expect("well-formed markdown must list its SYNC notes");

    assert_eq!(notes.len(), 1, "got {notes:#?}");
    assert_eq!(notes[0].line, 1);
    assert_eq!(
        cited_source_paths(&notes[0].body),
        vec![
            (
                "crates/reify-compiler/tests/harness_doc_chunks/chunk_prose.rs".to_string(),
                Some(
                    "code_spans_are_returned_in_document_order_with_their_opening_line".to_string()
                )
            ),
            (
                "crates/reify-eval/src/geometry_ops.rs".to_string(),
                Some("expected_arity".to_string())
            ),
        ],
        "the `<path>::<fn_name>` template is not a cite; the per-line cites are"
    );
    assert_eq!(
        note_violations("chunks/synthetic.md", INVENTORY_NOTE),
        Vec::<String>::new()
    );
}

#[test]
fn a_sync_note_naming_no_path_is_reported_at_its_line() {
    let md = "prose\n\
              <!-- ORACLE-XREF -->\n\
              ```text\n\
              <!-- SYNC: fenced, so not a note -->\n\
              ```\n\
              <!-- SYNC: this section is checked by a guard somewhere -->\n";

    let notes = sync_notes(md).expect("well-formed markdown must list its SYNC notes");
    assert_eq!(
        notes.iter().map(|note| note.line).collect::<Vec<_>>(),
        vec![6],
        "a marker comment is not a SYNC note, and neither is `SYNC:` text inside a fence"
    );

    let violations = note_violations("chunks/synthetic.md", md);
    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/synthetic.md:6"),
        "the violation must name chunk:line, got: {}",
        violations[0]
    );
}

#[test]
fn an_unterminated_note_is_a_violation_not_a_panic_or_a_skip() {
    let md = "prose\n\
              <!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks.rs verifies\n\
              the rest of the chunk\n";

    let violations = note_violations("chunks/synthetic.md", md);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/synthetic.md") && violations[0].contains("line 2"),
        "the violation must name the chunk and the note's opening line, got: {}",
        violations[0]
    );
}

#[test]
fn a_note_quoting_a_full_marker_is_reported_at_its_stray_terminator_with_the_shared_fix() {
    let md = "<!-- MARK -->\n\
              <!-- SYNC: crates/reify-compiler/tests/harness_doc_chunks.rs scans from the\n\
              `<!-- MARK -->` marker on the line above. -->\n\
              prose\n";

    let violations = note_violations("chunks/synthetic.md", md);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/synthetic.md:3"),
        "the violation must name the line of the stray terminator, got: {}",
        violations[0]
    );
    assert!(
        violations[0].contains(EARLY_CLOSED_NOTE_FIX),
        "the fix is the ONE wording oracle_xref_smoke.rs's debris check also gives, got: {}",
        violations[0]
    );
}
