//! PCITE — capability-manifest cite detector.
//!
//! A capability manifest (`docs/prds/**/*.capability-manifest.md`) backs each
//! row with grep evidence — `grep: \`some_fn\` at \`path:line\``. A cited
//! symbol that occurs nowhere in the tree is a phantom cite: evidence nobody
//! can reproduce (the #6233 class). PCITE reports each one.
//!
//! ## Corpus, grammar, oracle
//!
//! - **Corpus** — tracked files under [`MANIFEST_ROOT`] ending
//!   [`MANIFEST_SUFFIX`].
//! - **Grammar** — [`cited_symbols`]: backtick spans after a line's first
//!   `grep:`, split into `::` segments. #6233's prototype over 138 manifests
//!   measured 620 cited segments in 60 manifests with 9 unresolved (5
//!   dark-factory symbols, 2 OCCT names, 2 unclear). Wider grammars — the
//!   whole row, or every bare snake_case word — left 55 to 231 unresolved,
//!   mostly planned fixture names and memory ids, and were rejected.
//! - **Oracle** — every identifier-shaped word of every tracked file that is
//!   neither under `docs/` nor markdown. Prose never vouches: a note quoting
//!   a cite is not evidence that the symbol exists.
//!
//! The oracle is a bag of words, not a declaration index, for PDOCCOVER's
//! asymmetry reason: a word that merely occurs in a comment lets a phantom
//! through, while a declaration parser that missed a real symbol would
//! accuse it. A miss is cheaper than a false accusation.
//!
//! ## Report-only
//!
//! Both categories — `fabricated-cite:` (one per manifest and name, at its
//! first line) and `allow-missing-reason:` — are [`Severity::Medium`]. The
//! exit code counts High findings only, so PCITE cannot move it even when a
//! gate selects the pattern. A legitimately external cite (a dark-factory or
//! OCCT symbol) is settled by `<!-- pcite:allow — <reason> -->` on its line;
//! a reasonless marker exempts nothing and is itself reported. A report-only
//! lane has nothing to ratchet, so there is no baseline.
//!
//! ## Limit
//!
//! Only backticked spans are cites. An un-backticked prose cite — the
//! parenthesised memory id that motivated #6233 — is outside this grammar,
//! and no precise grammar reaches it.

use crate::scan_util::{
    allow_marker_body, find_word_boundary_token, is_identifier_shaped, is_word_byte,
};
use crate::{AuditContext, EvidenceRef, Finding, Pattern, Severity};
use std::collections::HashSet;

/// The tree the capability manifests live under.
pub const MANIFEST_ROOT: &str = "docs/prds/";
/// The suffix that marks a file under [`MANIFEST_ROOT`] as a capability
/// manifest.
pub const MANIFEST_SUFFIX: &str = ".capability-manifest.md";

/// A manifest line cites grep evidence only after this marker.
const EVIDENCE_MARKER: &str = "grep:";

/// The per-line escape hatch: `pcite:allow — <reason>`.
const ALLOW_TOKEN: &str = "pcite:allow";

/// Tracked prose — never evidence that a symbol exists.
const PROSE_ROOT: &str = "docs/";
const PROSE_SUFFIX: &str = ".md";

/// Every phantom cite and malformed allow marker in the tracked manifests,
/// sorted by `(category, name, path)`.
pub fn check(ctx: &AuditContext<'_>) -> Vec<Finding> {
    let tracked = ctx.git.ls_files();
    let sources: Vec<(String, String)> = tracked
        .iter()
        .filter(|path| !path.starts_with(PROSE_ROOT) && !path.ends_with(PROSE_SUFFIX))
        .filter_map(|path| {
            ctx.read_relative(path)
                .map(|content| (path.clone(), content))
        })
        .collect();
    let known = symbol_index(&sources);

    let mut keyed: Vec<Keyed> = tracked
        .iter()
        .filter(|path| path.starts_with(MANIFEST_ROOT) && path.ends_with(MANIFEST_SUFFIX))
        .filter_map(|path| ctx.read_relative(path).map(|content| (path, content)))
        .flat_map(|(path, content)| manifest_findings(path, &content, &known))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, finding)| finding).collect()
}

/// A finding with its `(category, name, path)` sort key.
type Keyed = ((&'static str, String, String), Finding);

fn manifest_findings(path: &str, content: &str, known: &HashSet<&str>) -> Vec<Keyed> {
    let mut out = Vec::new();
    let mut reported = HashSet::new();
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        if find_word_boundary_token(line, ALLOW_TOKEN).is_some() {
            if allow_marker_body(line, ALLOW_TOKEN).is_some() {
                continue;
            }
            out.push(keyed(
                "allow-missing-reason",
                &format!("{path}:{line_no}"),
                path,
                &format!("— `{ALLOW_TOKEN}` with no reason body exempts nothing; write `{ALLOW_TOKEN} — <reason>`"),
            ));
        }
        for name in cited_symbols(line) {
            if !known.contains(name) && reported.insert(name) {
                out.push(keyed(
                    "fabricated-cite",
                    name,
                    path,
                    &format!(
                        "— cited as grep evidence at {path}:{line_no}, but no tracked source \
                         outside {PROSE_ROOT} and *{PROSE_SUFFIX} contains it"
                    ),
                ));
            }
        }
    }
    out
}

fn keyed(category: &'static str, name: &str, path: &str, detail: &str) -> Keyed {
    let finding = Finding {
        pattern: Pattern::PManifestCite,
        severity: Severity::Medium,
        task_id: path.to_string(),
        summary: format!("{category}: {name} {detail}"),
        evidence: vec![EvidenceRef::File {
            path: path.to_string(),
        }],
    };
    ((category, name.to_string(), path.to_string()), finding)
}

/// The symbols one capability-manifest line cites as grep evidence, in line
/// order.
///
/// A cite is a backtick span OPENING after the line's first `grep:`, with
/// backticks paired from the start of the line — pairing from the marker
/// instead would misalign every span when the marker sits inside one. A span
/// is a `::`-separated symbol path with an optional trailing `()`; it yields
/// each segment, or nothing when any segment is not identifier-shaped (a
/// file path, a phrase, `impl Trait`). Commit-SHA-shaped segments are
/// dropped. The grammar is pure: allow markers are the caller's business.
// G-allow: pub for the cross-crate real-corpus floor guard in tests/pcite.rs; the production caller, check(), is same-file, and the orphan audit counts only cross-file call sites
pub fn cited_symbols(line: &str) -> Vec<&str> {
    let Some(evidence_at) = line.find(EVIDENCE_MARKER) else {
        return Vec::new();
    };
    backtick_spans(line)
        .filter(|&(open, _)| open > evidence_at)
        .flat_map(|(_, span)| symbol_path_segments(span))
        .filter(|segment| !is_commit_sha_shaped(segment))
        .collect()
}

/// `(offset of the opening backtick, text between the pair)` for each
/// backtick pair on `line`, paired left to right; an unpaired trailing
/// backtick opens nothing.
fn backtick_spans(line: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut ticks = line.match_indices('`').map(|(at, _)| at);
    std::iter::from_fn(move || {
        let open = ticks.next()?;
        let close = ticks.next()?;
        Some((open, &line[open + 1..close]))
    })
}

fn symbol_path_segments(span: &str) -> Vec<&str> {
    let path = span.strip_suffix("()").unwrap_or(span);
    let segments: Vec<&str> = path.split("::").collect();
    if segments.iter().all(|segment| is_identifier_shaped(segment)) {
        segments
    } else {
        Vec::new()
    }
}

/// 7 to 40 lowercase hex digits including at least one decimal digit — an
/// abbreviated or full commit id, which a manifest cites as provenance, not
/// as a symbol.
fn is_commit_sha_shaped(segment: &str) -> bool {
    (7..=40).contains(&segment.len())
        && segment
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && segment.bytes().any(|b| b.is_ascii_digit())
}

/// Every identifier-shaped word in `sources`: each maximal `[A-Za-z0-9_]`
/// run that does not start with a digit. Any other byte, non-ASCII included,
/// is a boundary.
fn symbol_index(sources: &[(String, String)]) -> HashSet<&str> {
    sources
        .iter()
        .flat_map(|(_, content)| word_runs(content))
        .filter(|word| !word.starts_with(|c: char| c.is_ascii_digit()))
        .collect()
}

/// Each maximal run of [`is_word_byte`] bytes in `text`. A byte walk, not a
/// `char` split: the oracle is ~90MB, and the split measured 4x slower.
fn word_runs(text: &str) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    let mut at = 0;
    std::iter::from_fn(move || {
        while at < bytes.len() && !is_word_byte(bytes[at]) {
            at += 1;
        }
        let start = at;
        while at < bytes.len() && is_word_byte(bytes[at]) {
            at += 1;
        }
        (start < at).then(|| &text[start..at])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_without_grep_evidence_cites_nothing() {
        assert_eq!(
            cited_symbols("| cap | `check_expr_struct_ctor_args` in `lib.rs` | PASS |"),
            Vec::<&str>::new(),
            "backticked identifiers are cites only on a line carrying `grep:`"
        );
    }

    #[test]
    fn a_grep_cell_cites_its_identifier_span_and_not_its_path_span() {
        let line = "| cap | grep: `check_expr_struct_ctor_args` at \
                    `crates/x/src/y.rs:1344` | PASS |";
        assert_eq!(cited_symbols(line), vec!["check_expr_struct_ctor_args"]);
    }

    #[test]
    fn backticks_pair_from_line_start_not_from_the_grep_offset() {
        assert_eq!(
            cited_symbols("| `grep:crates/a.rs:12 wired` (`real_fn`, confirmed) |"),
            vec!["real_fn"],
            "slicing at `grep:` first would pair the span's CLOSING backtick \
             with `real_fn`'s opening one"
        );
        assert_eq!(
            cited_symbols("| `foo` | grep: `bar` |"),
            vec!["bar"],
            "a span opening before the first `grep:` is never cited"
        );
    }

    #[test]
    fn path_spans_yield_each_segment_and_call_parens_are_stripped() {
        assert_eq!(
            cited_symbols("grep: `PendingBoundCheck::TraitArgConformance`"),
            vec!["PendingBoundCheck", "TraitArgConformance"]
        );
        assert_eq!(cited_symbols("grep: `live_counts()`"), vec!["live_counts"]);
    }

    #[test]
    fn a_span_with_any_non_identifier_segment_yields_nothing() {
        for span in ["impl DiagnosticCode", "::COUNT", "a b", "Foo::", "x.y", ""] {
            let line = format!("grep: `{span}`");
            assert_eq!(
                cited_symbols(&line),
                Vec::<&str>::new(),
                "`{span}` is not a symbol path"
            );
        }
    }

    #[test]
    fn commit_sha_shaped_segments_are_dropped() {
        assert_eq!(cited_symbols("grep: `cdc501a3f1`"), Vec::<&str>::new());
        assert_eq!(
            cited_symbols("grep: `abcdef1` `deadbeefcafe0123456789abcdef0123456789ab` `real_fn`"),
            vec!["real_fn"],
            "7- and 40-hex-digit spans are both SHA-shaped"
        );
    }

    #[test]
    fn hex_words_that_are_not_sha_shaped_are_kept() {
        let too_long = "deadbeefcafe0123456789abcdef0123456789abc";
        assert_eq!(too_long.len(), 41);
        let line = format!("grep: `abcdef` `deadbeef` `abc123` `cdc501A3f1` `{too_long}`");
        assert_eq!(
            cited_symbols(&line),
            vec!["abcdef", "deadbeef", "abc123", "cdc501A3f1", too_long],
            "no digit, under 7 or over 40 hex digits, or an uppercase letter: \
             none is SHA-shaped"
        );
    }

    #[test]
    fn the_grammar_is_pure_an_allow_marker_does_not_suppress_here() {
        assert_eq!(
            cited_symbols("| grep: `ghost_symbol` | <!-- pcite:allow — dark-factory symbol -->"),
            vec!["ghost_symbol"],
            "suppression is check()'s job, so the marker report stays \
             independent of this grammar"
        );
    }

    #[test]
    fn symbol_index_holds_every_maximal_identifier_run() {
        let sources = vec![
            (
                "a.rs".to_string(),
                "fn x1() { let v = 1x; héllo_w }".to_string(),
            ),
            ("b.ri".to_string(), "structure Bracket_2".to_string()),
        ];
        let index = symbol_index(&sources);
        for present in [
            "fn",
            "x1",
            "let",
            "v",
            "h",
            "llo_w",
            "structure",
            "Bracket_2",
        ] {
            assert!(
                index.contains(present),
                "`{present}` must be indexed; got {index:?}"
            );
        }
        for absent in ["1x", "x", "héllo_w", "llo", "Bracket"] {
            assert!(
                !index.contains(absent),
                "`{absent}` must not be indexed; got {index:?}"
            );
        }
    }
}
