//! Repo-wide fence gate for the MCP language-reference chunks
//! (`crates/reify-mcp/src/tools/chunks/*.md`).
//!
//! Task #5479 (PRD `docs/prds/v0_6/doc-chunk-truth-enforcement.md`, leaf β).
//!
//! # What this gate enforces
//!
//! 1. Every fence tagged EXACTLY ```` ```reify ```` compiles as a complete
//!    module with zero `Severity::Error` diagnostics.
//! 2. No fence is untagged. A bare opening delimiter is a violation naming
//!    `file:line`.
//! 3. Every `chunks/*.md` file on disk is reachable through the
//!    `reify_language_reference` MCP tool — i.e. both `include_str!`-ed AND
//!    listed in `TOPICS`.
//!
//! # Tag vocabulary
//!
//! - ```` ```reify ```` — **compiles STANDALONE as a complete module**. This is
//!   the one tag the gate compiles, and the meaning is deliberately the strong
//!   one: what the reader sees in the doc is exactly what the compiler was
//!   handed. No harness-side wrapper is applied, because a wrapper is an
//!   invisible privilege a reader of the doc cannot see.
//! - ```` ```reify-fragment ```` — genuine reify syntax that is member-level or
//!   otherwise context-dependent, so it cannot stand alone (a bare `let`, a
//!   `constraint`, a trait-body `fn` signature). A future content task could
//!   supply the missing context and PROMOTE it to ```` ```reify ````.
//! - ```` ```reify-schematic ```` — not reify source at all: signature
//!   listings, metavariable notation, `{ ... }` elisions. Never promotable.
//! - ```` ```reify-invalid ```` — a DELIBERATE-error teaching sample. Distinct
//!   from `reify-fragment`, which would falsely assert the body is valid.
//! - anything else (`ebnf`, `text`, …) — a non-reify language, exempt like any
//!   other explicit tag.
//!
//! The exempt list is **OPEN by design**: the gate only ever asks "is the tag
//! exactly `reify`". A closed allow-list would force this task to predict every
//! notation a future chunk might need; instead the tag itself is the sanction,
//! because retagging a fence away from `reify` is a one-line diff a reviewer
//! sees and can challenge.
//!
//! # Cross-harness contract (read before retagging anything)
//!
//! Two sibling modules in this same compile unit already scrape these chunks,
//! and they disagreed about what ```` ```reify ```` means:
//!
//! - `geometry_chunk_smoke.rs:617` matches ```` line.trim_end() == "```reify"
//!   ```` — EXACT, so `reify-fragment`/`reify-schematic` can never false-match
//!   it — compiles each hit VERBATIM (:704), and asserts its own anti-vacuity
//!   floor of >= 2 such fences (:661). Retagging either of geometry.md's two
//!   bare ```` ```reify ```` fences would therefore HOLLOW that suite rather
//!   than fail it loudly. `geometry_chunk_retains_bare_reify_fences_for_the_sibling_smoke_suite`
//!   below pins that coupling so it cannot happen silently.
//! - `enums_chunk_option_smoke.rs:106` selects fences TAG-AGNOSTICALLY via
//!   `strip_prefix("```")` and WRAPS each body in `structure def OptionDemo
//!   {{ … }}` (:132). Its comment at :96 explicitly defers tag discipline to
//!   this module by name.
//!
//! This gate settles the disagreement in favour of the standalone reading, so
//! `enums.md`'s `## Option Type` fence — which passes today only because of
//! that injected wrapper — is `reify-fragment`, not `reify`.

// ---------------------------------------------------------------------------
// Fence parser
// ---------------------------------------------------------------------------

/// One fenced code block, as this gate sees it.
#[derive(Debug, Clone)]
struct Fence {
    /// 1-based position in document order across the whole file. This, not the
    /// line number, is what a violation message leads with: a reader counting
    /// fences down a rendered chunk can find "fence #4" without a line-numbered
    /// view of the source.
    ordinal: usize,
    /// 1-based line number of the OPENING delimiter.
    open_line: usize,
    /// The info string with surrounding whitespace trimmed; `None` for a bare
    /// opening delimiter.
    tag: Option<String>,
    /// Fence content, excluding BOTH delimiter lines.
    body: String,
}

/// Parse every fenced code block in `content`, in document order.
///
/// A hand-rolled line-level state machine rather than a markdown crate: the
/// gate needs the OPENING line number and the raw info string of each block,
/// and it must apply a stricter delimiter rule than CommonMark (below). Pulling
/// in a markdown dependency for a test-only scan of 17 files would buy neither.
///
/// A delimiter is a line whose RAW form — deliberately NOT `trim_start`-ed —
/// begins with three backticks at column 0. Anything indented is body content.
/// That is stricter than `enums_chunk_option_smoke.rs:106`'s
/// `trim_start().strip_prefix("```")`, and stricter on purpose: an indented
/// ``` inside a body would otherwise close the block and invert the open/close
/// state for the entire rest of the file, silently mislabelling every
/// subsequent fence.
///
/// Open/close state is what makes the bare-fence ban possible at all. In
/// markdown a CLOSING delimiter is bare by syntax, so a stateless scan for a
/// column-0 bare ``` would flag every well-formed fence in the corpus.
///
/// Returns `Err` if a fence is still open at EOF, naming its opening line.
/// Silently dropping it would be the worst outcome for an omission-drift gate:
/// the offending block would vanish from the scan and the corpus test would go
/// green *because* the file is malformed.
fn parse_fences(content: &str) -> Result<Vec<Fence>, String> {
    let mut fences: Vec<Fence> = Vec::new();
    // (opening line, tag, accumulated body lines)
    let mut open: Option<(usize, Option<String>, Vec<&str>)> = None;

    for (index, line) in content.lines().enumerate() {
        let line_no = index + 1;
        match line.strip_prefix("```") {
            Some(info) => match open.take() {
                // A delimiter seen while OPEN closes the block.
                Some((open_line, tag, body)) => fences.push(Fence {
                    ordinal: fences.len() + 1,
                    open_line,
                    tag,
                    body: body.join("\n"),
                }),
                // A delimiter seen while CLOSED opens one; an empty info
                // string is the untagged case the bare-fence ban reports.
                None => {
                    let info = info.trim();
                    let tag = if info.is_empty() {
                        None
                    } else {
                        Some(info.to_string())
                    };
                    open = Some((line_no, tag, Vec::new()));
                }
            },
            None => {
                if let Some((_, _, body)) = open.as_mut() {
                    body.push(line);
                }
            }
        }
    }

    if let Some((open_line, tag, _)) = open {
        return Err(format!(
            "unterminated code fence: the delimiter opened at line {open_line} \
             (info string {}) is never closed, so every fence after it would be \
             mislabelled — the scan cannot be trusted",
            tag.as_deref().unwrap_or("<none>")
        ));
    }

    Ok(fences)
}

// ---------------------------------------------------------------------------
// Check 2 — the bare-fence ban
// ---------------------------------------------------------------------------

/// Every fence in `content` that carries no language tag, one message each, in
/// document order.
///
/// Deliberately does NOT validate the tag against an allow-list. Task rule 3
/// makes the vocabulary open: the gate only ever asks "is the tag exactly
/// `reify`" (that is `reify_fence_violations`' job), so any explicit tag
/// exempts here. The point is not to police notation — it is to force the doc
/// author to make a CLAIM about the fence, which then shows up as a
/// one-line diff a reviewer can challenge.
///
/// A `parse_fences` failure is reported as a violation rather than panicking,
/// so a malformed file lands in the same accumulate-then-report-all output as
/// everything else.
fn untagged_fence_violations(path: &str, content: &str) -> Vec<String> {
    let fences = match parse_fences(content) {
        Ok(fences) => fences,
        Err(error) => return vec![format!("{path}: {error}")],
    };

    fences
        .iter()
        .filter(|fence| fence.tag.is_none())
        .map(|fence| {
            format!(
                "{path}:{} — fence #{} has NO language tag. Every fence must \
                 carry an explicit one: `reify` ONLY if the body compiles \
                 STANDALONE as a complete module; else `reify-fragment` (real \
                 reify syntax that is member-level or context-dependent), \
                 `reify-schematic` (not reify source at all — signature \
                 listing, metavariable notation, `{{ ... }}` elision), \
                 `reify-invalid` (a deliberate-error teaching sample), or the \
                 language it actually is (`ebnf`, `text`, …).",
                fence.open_line, fence.ordinal
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Hermetic parser tests
//
// Every case below runs on SYNTHETIC in-memory markdown. No chunk file on disk
// is read or mutated, so the parser's own contract is pinned independently of
// whatever the real corpus happens to contain today.
// ---------------------------------------------------------------------------

/// A bare ``` opening delimiter yields `tag == None`, and its bare closing
/// delimiter is consumed as a delimiter rather than mistaken for a second
/// untagged opening.
///
/// This open/close state discrimination is the whole reason the gate cannot be
/// a `grep`: in markdown a CLOSING delimiter is bare by syntax, so a stateless
/// scan for `^```$` would flag every well-tagged fence in the corpus.
#[test]
fn bare_opening_fence_parses_untagged_and_its_closer_is_not_a_second_block() {
    let md = "intro prose\n\
              ```\n\
              bare body\n\
              ```\n\
              trailing prose\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the closing ``` must be consumed as a delimiter, not parsed as a \
         second untagged block; got {fences:#?}"
    );
    assert_eq!(fences[0].ordinal, 1, "ordinals are 1-based");
    assert_eq!(fences[0].open_line, 2, "open_line is 1-based");
    assert_eq!(fences[0].tag, None, "a bare opening delimiter carries no tag");
    assert_eq!(
        fences[0].body, "bare body",
        "body excludes BOTH delimiter lines"
    );
}

/// The bare closing delimiter of a TAGGED fence never surfaces as an untagged
/// block. Stated separately from the case above because this is the shape the
/// bare-fence ban must not false-positive on: every compliant fence in the
/// corpus ends with a bare ```.
#[test]
fn the_bare_closer_of_a_tagged_fence_is_not_reported_as_untagged() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
    assert!(
        fences.iter().all(|f| f.tag.is_some()),
        "the closing delimiter must not appear as an untagged fence"
    );
}

/// EXACT tag semantics: `reify-fragment` is its own tag and must never be read
/// as a bare `reify` by prefix matching.
///
/// This is the single most load-bearing parser property. `reify_fence_violations`
/// selects on `tag.as_deref() == Some("reify")`; if the tag were captured (or
/// compared) by prefix, every `reify-fragment` / `reify-schematic` fence in the
/// corpus would be trial-compiled, and the whole exempt-tag vocabulary would
/// collapse.
#[test]
fn hyphenated_tags_are_exact_and_never_collapse_to_bare_reify() {
    for tag in ["reify-fragment", "reify-schematic", "reify-invalid"] {
        let md = format!("```{tag}\nlet x = 1mm\n```\n");
        let fences = parse_fences(&md).expect("well-formed markdown must parse");

        assert_eq!(fences.len(), 1, "tag `{tag}`: got {fences:#?}");
        assert_eq!(
            fences[0].tag.as_deref(),
            Some(tag),
            "tag `{tag}` must be captured verbatim"
        );
        assert_ne!(
            fences[0].tag.as_deref(),
            Some("reify"),
            "tag `{tag}` must NOT be readable as bare `reify` — prefix matching \
             here would trial-compile every exempt fence in the corpus"
        );
    }
}

/// Trailing whitespace after an info string is trimmed, so a fence tagged
/// `` ```reify `` with a stray trailing space is still exactly `reify`.
#[test]
fn trailing_whitespace_after_a_tag_is_trimmed() {
    let md = "```reify   \nstructure def S { let n = 1 }\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
}

/// A delimiter-looking line that is INDENTED is body content, not a delimiter.
///
/// The gate's rule is "fences at line start" — deliberately stricter than
/// `enums_chunk_option_smoke.rs`'s `line.trim_start().strip_prefix("```")`, so
/// an indented ``` inside a body cannot silently close the block and desync the
/// parser for the whole rest of the file.
#[test]
fn an_indented_delimiter_is_body_content_not_a_delimiter() {
    let md = "```text\n\
              outer\n\
              \x20   ```\n\
              still outer\n\
              ```\n\
              after\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        1,
        "the indented ``` must not open or close a block; got {fences:#?}"
    );
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
    assert_eq!(fences[0].body, "outer\n    ```\nstill outer");
}

/// Ordinals are assigned in document order across the whole file, and each
/// fence records its own 1-based opening line.
#[test]
fn ordinals_and_open_lines_follow_document_order() {
    let md = "# Title\n\
              ```reify\n\
              a\n\
              ```\n\
              prose\n\
              ```\n\
              b\n\
              ```\n\
              ```reify-fragment\n\
              c\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    let seen: Vec<(usize, usize, Option<&str>)> = fences
        .iter()
        .map(|f| (f.ordinal, f.open_line, f.tag.as_deref()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (1, 2, Some("reify")),
            (2, 6, None),
            (3, 9, Some("reify-fragment")),
        ],
        "ordinals must be 1-based and in document order, open_line 1-based"
    );
}

/// An unterminated final fence is an `Err` naming the opening line — never a
/// silently dropped block.
///
/// Dropping it would be the worst possible failure mode for an omission-drift
/// gate: the offending fence would vanish from the scan and the corpus test
/// would go green precisely because the file is malformed.
#[test]
fn an_unterminated_final_fence_is_an_error_naming_its_opening_line() {
    let md = "prose\n\
              ```reify\n\
              structure def S { let n = 1 }\n";

    let err = parse_fences(md).expect_err("an unterminated fence must not parse clean");

    assert!(
        err.contains('2'),
        "the error must name the OPENING line (2) of the unterminated fence, got: {err}"
    );
    assert!(
        err.to_lowercase().contains("unterminated"),
        "the error must say what went wrong, got: {err}"
    );
}

/// An empty fence body is `""`, not a parse failure.
#[test]
fn an_empty_fence_body_parses_as_the_empty_string() {
    let md = "```reify-schematic\n```\n";
    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].body, "");
}

// ---------------------------------------------------------------------------
// Check 2 — the bare-fence ban
// ---------------------------------------------------------------------------

/// An untagged OPENING fence is one violation, and the message carries both the
/// file path and `:<line>` of the opening delimiter.
///
/// `file:line` specifically, not just a count: the whole value of this check is
/// that a doc author who trips it can jump straight to the offending line. A
/// bare "3 untagged fences" would send them re-counting delimiters by hand.
#[test]
fn an_untagged_opening_fence_is_reported_with_file_and_line() {
    let md = "# Collections\n\
              \n\
              ```\n\
              let xs = [1, 2, 3]\n\
              ```\n";

    let violations = untagged_fence_violations("chunks/collections.md", md);

    assert_eq!(violations.len(), 1, "got {violations:#?}");
    assert!(
        violations[0].contains("chunks/collections.md"),
        "the violation must name the file, got: {}",
        violations[0]
    );
    assert!(
        violations[0].contains(":3"),
        "the violation must name `:<opening line>` (here `:3`), got: {}",
        violations[0]
    );
}

/// The bare CLOSING delimiter of a tagged fence is never a violation.
///
/// This is the false positive that would make the check unusable: in markdown
/// every fence closes with a bare ```, so a stateless scan would report a
/// violation for every single compliant fence in the corpus. Only the parser's
/// open/close state distinguishes them.
#[test]
fn the_bare_closing_delimiter_of_a_tagged_fence_is_not_a_violation() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```\n";

    assert!(
        untagged_fence_violations("chunks/whatever.md", md).is_empty(),
        "a compliant tagged fence closes with a bare ``` and must stay clean"
    );
}

/// ANY explicit tag exempts — the vocabulary is deliberately OPEN.
///
/// The check never validates the tag against an allow-list. A closed list would
/// force this task to predict every notation a future chunk might need; instead
/// the tag itself is the sanction, because retagging a fence away from `reify`
/// is a one-line diff a reviewer sees.
#[test]
fn every_explicit_tag_exempts_including_ones_this_task_never_anticipated() {
    for tag in [
        "reify",
        "reify-fragment",
        "reify-schematic",
        "reify-invalid",
        "text",
        "ebnf",
        "json",
        "some-future-notation",
    ] {
        let md = format!("```{tag}\nbody\n```\n");
        assert!(
            untagged_fence_violations("chunks/x.md", &md).is_empty(),
            "tag `{tag}` is explicit and must exempt the fence — the allow-list \
             is open by design"
        );
    }
}

/// Several offending fences produce one violation each, in document order, so a
/// single run surfaces the whole backlog rather than one fence at a time.
#[test]
fn multiple_untagged_fences_are_each_reported_in_document_order() {
    let md = "```\n\
              first\n\
              ```\n\
              prose\n\
              ```reify-schematic\n\
              exempt\n\
              ```\n\
              more prose\n\
              ```\n\
              second\n\
              ```\n";

    let violations = untagged_fence_violations("chunks/units.md", md);

    assert_eq!(violations.len(), 2, "got {violations:#?}");
    assert!(
        violations[0].contains(":1"),
        "first violation must be the line-1 fence, got: {}",
        violations[0]
    );
    assert!(
        violations[1].contains(":9"),
        "second violation must be the line-9 fence, got: {}",
        violations[1]
    );
}

/// The remedy is spelled out in the message, so the fix does not require
/// reading this module first.
#[test]
fn the_violation_message_points_at_the_tag_vocabulary() {
    let violations = untagged_fence_violations("chunks/x.md", "```\nbody\n```\n");

    let message = &violations[0];
    for expected in ["reify-fragment", "reify-schematic"] {
        assert!(
            message.contains(expected),
            "the remedy must name `{expected}` so the fix needs no source dive, \
             got: {message}"
        );
    }
}
