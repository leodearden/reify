//! How a chunk's markdown divides into fenced code blocks, the sections those
//! fences cannot end, and the catalogue tables a section carries.

use std::collections::HashSet;

/// One fenced code block, as [`parse_fences`] reads it. An immutable parse
/// record: every field is read directly.
#[derive(Debug, Clone)]
pub(crate) struct Fence {
    /// 1-based position in document order across the whole file. This, not the
    /// line number, is what a violation message leads with: a reader counting
    /// fences down a rendered chunk can find "fence #4" without a line-numbered
    /// view of the source.
    pub(crate) ordinal: usize,
    /// 1-based line number of the OPENING delimiter.
    pub(crate) open_line: usize,
    /// 1-based line number of the CLOSING delimiter.
    pub(crate) close_line: usize,
    /// The info string with surrounding whitespace trimmed; `None` for a bare
    /// opening delimiter.
    pub(crate) tag: Option<String>,
    /// Fence content, excluding BOTH delimiter lines.
    pub(crate) body: String,
}

/// Parse every fenced code block in `content`, in document order.
///
/// This is the binary's SINGLE definition of a fence: every scan that needs to
/// know what is fenced — a tag selection, a section boundary, the prose model —
/// reads it through here, so no two harness modules can disagree about where a
/// block starts or ends.
///
/// A hand-rolled line-level state machine rather than a markdown crate: callers
/// need the OPENING line number and the raw info string of each block, and the
/// delimiter rule is stricter than CommonMark (below). Pulling in a markdown
/// dependency for a test-only scan of the chunk corpus would buy neither.
///
/// A delimiter is a run of THREE OR MORE of a single FENCE CHARACTER —
/// backtick or tilde, CommonMark allows both — starting at column 0. The run is
/// COUNTED, not assumed to be exactly three, and the raw line is deliberately
/// NOT `trim_start`-ed, so anything indented is body content. That is stricter
/// than CommonMark on the indentation axis and faithful to it on the
/// run-length and fence-character axes — and all three matter for one reason: a
/// misread delimiter inverts the open/close state for the entire rest of the
/// file, silently mislabelling every subsequent fence.
///
/// # Why the fence CHARACTER is tracked, not just backticks
///
/// No chunk uses `~~~` today, so this is latent rather than live — but the
/// fence gate's bare-fence ban is advertised over EVERY fence, and a
/// backtick-only scan is blind to a tilde one on both axes. An untagged `~~~`
/// would escape that ban silently. Worse, a `~~~text` block whose body contains a column-0
/// ```` ```reify ```` line — the natural way to write a chunk that DOCUMENTS
/// the fence-tag vocabulary — would be read as a genuine open `reify` fence,
/// desyncing the scan for the rest of the file in exactly the way the run-length
/// counting below exists to prevent. A closer must therefore match BOTH the
/// opener's character and at least its length; a run of the other character is
/// ordinary body content, whatever its length.
///
/// # Why the run length is counted rather than assumed
///
/// CommonMark requires a CLOSING delimiter to be a run of the OPENER'S OWN
/// character, at least as long as the opening one. That rule is what lets a
/// markdown file NEST a fence — and the obvious future chunk to do so is one
/// documenting the fence-tag vocabulary, which needs a four-backtick block
/// wrapping a three-backtick ```` ```reify ```` sample. Under a plain
/// `strip_prefix("```")` scan that opener parses with a BACKTICK captured into
/// its info string (tag `` `text `` rather than `text`) and the first INNER
/// three-backtick line closes the block, after which every fence in the file
/// is off by one and the bare-fence ban reports a fence at a line the author
/// never wrote one on. Counting the run makes the shorter inner lines ordinary
/// body content, which is what they are.
///
/// # A delimiter carrying an info string while a fence is OPEN is an `Err`
///
/// CommonMark says a closing fence carries no info string, so a tagged
/// delimiter appearing inside an already-open fence of the same run length is,
/// strictly, body content. In a hand-maintained doc corpus it is almost always
/// a MISSING closer instead. Both readings silently mislabel the rest of the
/// file, and the doc-chunk gates exist to catch exactly that class of drift, so
/// the parser refuses to guess: it returns `Err` naming both lines and lets a
/// human decide which one they meant.
///
/// Open/close state is what makes the bare-fence ban possible at all. In
/// markdown a CLOSING delimiter is bare by syntax, so a stateless scan for a
/// column-0 bare ``` would flag every well-formed fence in the corpus.
///
/// Returns `Err` if a fence is still open at EOF, naming its opening line.
/// Silently dropping it would be the worst outcome for an omission-drift gate:
/// the offending block would vanish from the scan and the corpus test would go
/// green *because* the file is malformed.
pub(crate) fn parse_fences(content: &str) -> Result<Vec<Fence>, String> {
    /// The leading run of a single CommonMark fence character at column 0:
    /// `(character, length)`, or `None` for a line that starts with neither.
    ///
    /// Both characters are ASCII, so the returned length is a valid byte AND
    /// char boundary and the caller can slice the info string off with it.
    fn delimiter_run(line: &str) -> Option<(u8, usize)> {
        let first = line.as_bytes().first().copied()?;
        if first != b'`' && first != b'~' {
            return None;
        }
        let run = line.bytes().take_while(|byte| *byte == first).count();
        Some((first, run))
    }

    /// The fence currently open, if any.
    ///
    /// A named struct rather than a tuple: five positional fields read as
    /// noise at every destructuring site, and the two `usize`s (a LINE and a
    /// RUN LENGTH) are trivially swappable by accident.
    struct Open<'a> {
        line: usize,
        /// The opener's fence character. A closer must match it — this is what
        /// keeps a ```` ``` ```` line inside a `~~~` block from closing it.
        fence_char: u8,
        /// The opener's run length. A closer must be at least this long — this
        /// is what lets a longer outer fence nest a shorter inner one.
        run: usize,
        tag: Option<String>,
        body: Vec<&'a str>,
    }

    fn name_of(fence_char: u8) -> &'static str {
        if fence_char == b'~' { "tilde" } else { "backtick" }
    }

    let mut fences: Vec<Fence> = Vec::new();
    let mut open: Option<Open<'_>> = None;

    for (index, line) in content.lines().enumerate() {
        let line_no = index + 1;
        let delimiter = delimiter_run(line);

        // Does this line close the fence currently open? Only a run of the
        // OPENER'S OWN character, at least as long as the opener's. A shorter
        // run — or a run of the other character, at any length — is body
        // content, which is what makes a nested fence parse correctly and what
        // keeps a ```` ```reify ```` line inside a `~~~` block from being read
        // as a genuine open `reify` fence.
        let closes = match (&open, delimiter) {
            (Some(state), Some((char_here, run))) => {
                char_here == state.fence_char && run >= state.run
            }
            _ => false,
        };

        if closes {
            let (_, run) = delimiter.expect("closes implies a delimiter");
            let rest = &line[run..];
            let state = open.as_ref().expect("closes implies open");
            if !rest.trim().is_empty() {
                return Err(format!(
                    "code fence delimiter at line {line_no} carries an info string \
                     ({info}) while the fence opened at line {open_line} (run of \
                     {open_run} {kind}s) is still OPEN. A closing delimiter must be \
                     bare, so this is either a MISSING closer above or a nested fence \
                     that needs a longer outer run; either way, guessing would \
                     mislabel every fence after it",
                    info = rest.trim(),
                    open_line = state.line,
                    open_run = state.run,
                    kind = name_of(state.fence_char)
                ));
            }
            let state = open.take().expect("closes implies open");
            fences.push(Fence {
                ordinal: fences.len() + 1,
                open_line: state.line,
                close_line: line_no,
                tag: state.tag,
                body: state.body.join("\n"),
            });
            continue;
        }

        match open.as_mut() {
            // Anything that did not close the open fence is its body — including
            // a run SHORTER than the opener's, and a run of the OTHER fence
            // character at any length.
            Some(state) => state.body.push(line),
            // Outside any fence, a run of >= 3 opens one; an empty info string
            // is the untagged case the bare-fence ban reports.
            None => {
                if let Some((fence_char, run)) = delimiter.filter(|(_, run)| *run >= 3) {
                    let info = line[run..].trim();
                    open = Some(Open {
                        line: line_no,
                        fence_char,
                        run,
                        tag: (!info.is_empty()).then(|| info.to_string()),
                        body: Vec::new(),
                    });
                }
            }
        }
    }

    if let Some(state) = open {
        return Err(format!(
            "unterminated code fence: the delimiter opened at line {open_line} \
             (run of {open_run} {kind}s, info string {}) is never closed, so \
             every fence after it would be mislabelled — the scan cannot be \
             trusted",
            state.tag.as_deref().unwrap_or("<none>"),
            open_line = state.line,
            open_run = state.run,
            kind = name_of(state.fence_char)
        ));
    }

    Ok(fences)
}

/// The body of every fence whose info string is EXACTLY `tag`, in document
/// order, with both delimiter lines excluded.
///
/// A projection over [`parse_fences`], the one fence definition, so a sample
/// nested in a longer fence or quoted inside a `~~~` block is never mistaken for
/// a fence of its own. Matching on the whole info string keeps a hyphenated tag
/// distinct in both directions: `reify` never selects `reify-fragment`, and
/// `reify-rejected` selects only itself.
///
/// Only EXPLICITLY TAGGED fences are selected, so a chunk author marks exactly
/// what a caller's check applies to and everything else stays free-form.
/// Callers must anti-vacuity-check the result: a dropped tag would otherwise
/// empty the scan and pass trivially.
///
/// Panics if `markdown` does not parse, naming `chunk_path`, so a malformed
/// chunk is blamed on itself rather than scanned wrongly.
pub(crate) fn tagged_fence_bodies(markdown: &str, tag: &str, chunk_path: &str) -> Vec<String> {
    parse_fences(markdown)
        .unwrap_or_else(|e| panic!("{chunk_path}: {e}"))
        .into_iter()
        .filter(|fence| fence.tag.as_deref() == Some(tag))
        .map(|fence| fence.body)
        .collect()
}

/// The body of the section opened by `opener`, from the line after the opener
/// to the next `## ` heading (exclusive). `### ` subsections stay inside.
///
/// `opener` is a byte-exact whole trimmed line: either an HTML-comment marker
/// (`<!-- ORACLE-SECTION -->`) or a heading line such as `## Option Type`.
///
/// FENCE-AWARE through [`parse_fences`], tilde fences included: neither the
/// opener nor a `## ` section end is ever matched on a line inside a fence
/// (delimiters included), so a fenced comment such as `// ## note` can neither
/// open nor truncate a section. Inside the section every line, fenced or not,
/// is kept verbatim.
///
/// PANICS in two cases, each naming ITS OWN cause: a chunk [`parse_fences`]
/// rejects — an unterminated fence, or a tagged delimiter inside an open one —
/// which is reported before the opener is blamed, because a misread fence can
/// hide an opener that is present and intact; and an absent opener — the
/// anti-vacuity guard, and the failure a reader of a gutted section should see,
/// rather than an empty slice that makes every downstream assertion pass
/// trivially. `chunk_path` and `section_title` appear only in that panic text.
pub(crate) fn section_body(
    markdown: &str,
    opener: &str,
    chunk_path: &str,
    section_title: &str,
) -> String {
    let fenced_lines: HashSet<usize> = parse_fences(markdown)
        .unwrap_or_else(|e| panic!("{chunk_path}: {e}"))
        .iter()
        .flat_map(|fence| fence.open_line..=fence.close_line)
        .collect();

    let mut body: Vec<&str> = Vec::new();
    let mut in_section = false;
    for (index, line) in markdown.lines().enumerate() {
        let fenced = fenced_lines.contains(&(index + 1));
        if in_section {
            if !fenced && line.starts_with("## ") {
                break;
            }
            body.push(line);
        } else if !fenced && line.trim() == opener {
            in_section = true;
        }
    }

    assert!(
        in_section,
        "{chunk_path} carries no `{opener}` line — the line that opens the \
         `{section_title}` section was removed along with (or independently of) the \
         section itself. That section is what the in-GUI assistant retrieves when a designer \
         asks about the topic it covers; without it the assistant reads the capability as \
         MISSING and hand-rolls a substitute instead (task 5389, for the oracle section). \
         Restore the section WITH its opening line. Where the opener is an HTML-comment \
         marker directly under the heading, retitling the heading is free and needs no \
         change here — only the marker is matched."
    );
    body.join("\n")
}

/// The part of a [`section_body`] that lies BEFORE `end_marker`.
///
/// A CLOSED region, for the one scan that needs one. [`section_body`] runs to
/// the next `## ` heading, which is right for a coverage scan — more text can
/// only help it — but wrong for a FORBIDDEN-direction scan, where every extra
/// line is another place correct prose can trip the assertion. Delegating keeps
/// the fence-awareness and the panics in one implementation.
///
/// PANICS when `end_marker` is absent, for the same reason `section_body` panics
/// on an absent opening marker: silently falling back to "the rest of the
/// section" would quietly widen a forbidden-direction scan back to the extent it
/// was narrowed away from, and the widening would first be noticed as a RED
/// against prose that is perfectly correct.
pub(crate) fn marker_closed_region(
    markdown: &str,
    marker: &str,
    end_marker: &str,
    chunk_path: &str,
    section_title: &str,
) -> String {
    let body = section_body(markdown, marker, chunk_path, section_title);
    let end = body
        .lines()
        .position(|line| line.trim() == end_marker)
        .unwrap_or_else(|| {
            panic!(
                "{chunk_path} opens the `{section_title}` region with `{marker}` but never closes \
                 it: no line is exactly `{end_marker}`. The closing marker is what bounds this \
                 region — without it the scan would run on to the next `##` heading and start \
                 judging text that was never in scope. Restore the closing marker on its own \
                 line where the region ends."
            )
        });
    body.lines().take(end).collect::<Vec<_>>().join("\n")
}

/// The names a CATALOGUE TABLE's rows are about — one entry per markdown row, in
/// document order. geometry.md carries two: the length-argument catalogue and
/// the topology-selector catalogue.
///
/// FIRST COLUMN ONLY, and that is the whole point of the scan rather than a
/// simplification of it. A catalogue's claim is made by the row's subject: the
/// later columns are prose that legitimately backticks argument NAMES
/// (`degree`, `n_points`), which are not callables and must not be fed to a
/// registry lookup. Taking column one keeps "every name this asserts is real" a
/// true statement instead of one needing an allowlist to stay green.
///
/// A cell may name more than one callable (the length catalogue's
/// ``| `interp` / `bezier` |`` row), so a row yields a Vec. Rows that yield
/// nothing — the header, the `|---|---|---|` delimiter, any `|`-leading line
/// without a backticked identifier — are dropped, so `.len()` counts CATALOGUE
/// rows and nothing else.
///
/// A backticked span is accepted only if it is a bare identifier, optionally
/// followed by a call form: ``` `helix` ``` and ``` `helix(radius, pitch,
/// height)` ``` both yield `helix`. Anything else (a prose span, a unit
/// literal) is skipped rather than guessed at.
pub(crate) fn catalogue_table_rows(section: &str) -> Vec<Vec<String>> {
    fn is_ident(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    let mut rows: Vec<Vec<String>> = Vec::new();

    for line in section.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('|') else {
            continue;
        };
        // The FIRST cell: everything up to the next `|`, or the whole remainder
        // for a (malformed) single-column row.
        let first_cell = rest.split('|').next().unwrap_or_default();

        let mut names: Vec<String> = Vec::new();
        for span in first_cell.split('`').skip(1).step_by(2) {
            // Trim a trailing call form, so `helix` and
            // `helix(radius, pitch, height)` are the same claim.
            let head = span.split('(').next().unwrap_or_default().trim();
            if !head.is_empty()
                && head.chars().all(is_ident)
                && !head.starts_with(|c: char| c.is_ascii_digit())
                && !names.contains(&head.to_string())
            {
                names.push(head.to_string());
            }
        }
        if !names.is_empty() {
            rows.push(names);
        }
    }
    rows
}

/// The DISTINCT names carried by `rows`, in document order.
///
/// A SIBLING of [`catalogue_table_rows`] rather than a replacement for it,
/// because a catalogue scan asks a table two different questions: how many ROWS
/// it still has (the anti-vacuity floor) and which NAMES it claims (the registry
/// assertions). Callers want both answers, so the rows are parsed once and
/// flattened here — one dedup rule, so every catalogue scan compares against
/// the same set.
pub(crate) fn catalogue_table_names(rows: &[Vec<String>]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in rows.iter().flatten() {
        if !out.contains(name) {
            out.push(name.clone());
        }
    }
    out
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
/// This open/close state discrimination is the whole reason the parser cannot
/// be a `grep`: in markdown a CLOSING delimiter is bare by syntax, so a stateless
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
/// This is the single most load-bearing parser property. The fence gate
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
/// The rule is "fences at line start" — deliberately stricter than CommonMark,
/// so an indented ``` inside a body cannot silently close the block and desync
/// the parser for the whole rest of the file.
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

/// A FOUR-backtick fence nests a three-backtick sample as body content.
///
/// This is the CommonMark way to show a fenced block inside a fenced block —
/// exactly what a future chunk documenting the fence-tag vocabulary would
/// need. A naive `strip_prefix("```")` scan mis-parses it twice over:
/// the opener's tag becomes `` `text `` (a backtick swallowed into the info
/// string) and the first INNER delimiter closes the block, leaving every
/// later fence in the file off by one and the bare-fence ban reporting a
/// violation at a line the author never wrote a bare fence on.
#[test]
fn a_four_backtick_fence_nests_a_three_backtick_sample_as_body() {
    let md = "````text\n\
              ```reify\n\
              structure def S { let n = 1 }\n\
              ```\n\
              ````\n\
              after\n";

    let fences = parse_fences(md).expect("a nested fence is well-formed markdown");

    assert_eq!(
        fences.len(),
        1,
        "the inner three-backtick lines are BODY of the four-backtick block, not \
         delimiters of their own; got {fences:#?}"
    );
    assert_eq!(
        fences[0].tag.as_deref(),
        Some("text"),
        "the info string is what follows the COUNTED run; a `strip_prefix(\"```\")` \
         scan would report the tag as `` `text `` and no exact-match check would \
         ever recognise it again"
    );
    assert_eq!(
        fences[0].body, "```reify\nstructure def S { let n = 1 }\n```",
        "both inner delimiter lines belong to the body verbatim"
    );
}

/// A tagged `~~~` fence carries its info string, and its bare closer is not a
/// second block — the tilde mirror of the backtick contract above.
#[test]
fn a_tagged_tilde_fence_carries_its_info_string() {
    let md = "~~~text\n\
              plain prose sample\n\
              ~~~\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");
    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
}

/// THE DESYNC CASE. A column-0 ```` ```reify ```` line inside a `~~~` block is
/// BODY, never a fence.
///
/// This is the failure mode a backtick-only parser cannot see and the reason
/// the fence CHARACTER is tracked rather than assumed. Under a backtick-only
/// scan the inner line opens a `reify` fence the author never wrote, the outer
/// `~~~` closer is not a backtick run so the block never closes, and the parse
/// either errors at EOF or mislabels every fence after it — while the fence
/// gate tries to compile a body that is really a chunk of prose. The one shape
/// most likely to hit this is a chunk documenting the fence-tag vocabulary,
/// which needs to show a ```` ```reify ```` line without it being one.
#[test]
fn a_backtick_fence_line_inside_a_tilde_block_is_body_not_a_fence() {
    let md = "~~~text\n\
              ```reify\n\
              structure def NotReallyAFence { let n = 1 }\n\
              ```\n\
              ~~~\n\
              ```reify\n\
              structure def GenuinelyAFence { let n = 1 }\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    assert_eq!(
        fences.len(),
        2,
        "the tilde block is ONE fence and the trailing backtick block is the \
         other — the inner ```reify line is body. Got {fences:#?}"
    );
    assert_eq!(fences[0].tag.as_deref(), Some("text"));
    assert!(
        fences[0].body.contains("```reify"),
        "the inner delimiter line must survive INTO the body verbatim, got: {}",
        fences[0].body
    );
    assert_eq!(
        fences[1].tag.as_deref(),
        Some("reify"),
        "the scan must still be in sync after the tilde block — a desync here \
         mislabels every fence in the rest of the file"
    );
    assert_eq!(
        fences[1].open_line, 6,
        "and the open line of the genuine fence must be exact, got {fences:#?}"
    );
}

/// A `~~~` run never closes a backtick fence, whatever its length.
///
/// The converse of the case above, and the property that makes the closer rule
/// symmetric: mismatching characters are body content in both directions.
#[test]
fn a_tilde_run_does_not_close_a_backtick_fence() {
    let md = "```text\n\
              ~~~~~~\n\
              still inside the backtick fence\n\
              ```\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");
    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert!(
        fences[0].body.contains("~~~~~~"),
        "the long tilde run is body content, got: {}",
        fences[0].body
    );
}

/// A closing run LONGER than the opening one still closes it (CommonMark: the
/// closer must be *at least* as long, not exactly as long).
#[test]
fn a_closing_run_longer_than_the_opening_one_still_closes_it() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ````\n";

    let fences = parse_fences(md).expect("a longer closing run is well-formed");

    assert_eq!(fences.len(), 1, "got {fences:#?}");
    assert_eq!(fences[0].tag.as_deref(), Some("reify"));
    assert_eq!(fences[0].body, "structure def S { let n = 1 }");
}

/// A TAGGED delimiter appearing while a fence of the same run length is still
/// open is a named `Err`, not a silent guess.
///
/// CommonMark would read it as body; a hand-maintained corpus almost always
/// means a missing closer on the line above. Both readings mislabel every
/// fence after it, which is the precise drift the doc-chunk gates exist to
/// catch, so the parser refuses and names BOTH lines.
#[test]
fn a_tagged_delimiter_inside_an_open_fence_is_a_named_error() {
    let md = "```reify\n\
              structure def S { let n = 1 }\n\
              ```reify-fragment\n\
              let n = 1\n\
              ```\n";

    let err = parse_fences(md).expect_err("an info string on a closer must not parse clean");

    assert!(
        err.contains('1') && err.contains('3'),
        "the error must name BOTH the open line (1) and the offending delimiter \
         line (3), got: {err}"
    );
    assert!(
        err.contains("reify-fragment"),
        "the error must quote the info string that made the line ambiguous, got: {err}"
    );
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

/// `close_line` is the 1-based line of the CLOSING delimiter — the one line a
/// fence's extent cannot be derived from its body without.
#[test]
fn close_line_is_the_one_based_line_of_the_closing_delimiter() {
    let md = "prose\n\
              ```reify-schematic\n\
              ```\n\
              between\n\
              ```reify\n\
              structure def S { let n = 1 }\n\
              ````\n";

    let fences = parse_fences(md).expect("well-formed markdown must parse");

    let extents: Vec<(usize, usize)> = fences
        .iter()
        .map(|fence| (fence.open_line, fence.close_line))
        .collect();
    assert_eq!(
        extents,
        vec![(2, 3), (5, 7)],
        "an empty-bodied fence closes on the line after it opens; a longer closing run \
         still closes its fence"
    );
}

// ---------------------------------------------------------------------------
// Hermetic tag-selection tests
// ---------------------------------------------------------------------------

/// A column-0 ```` ```reify ```` line inside a `~~~` block is that block's
/// body, so selecting by tag must not return it as a fence of its own.
#[test]
fn tagged_fence_bodies_ignores_a_reify_line_inside_a_tilde_block() {
    let md = "~~~text\n\
              ```reify\n\
              structure def NotAFence { let n = 1 }\n\
              ```\n\
              ~~~\n\
              ```reify\n\
              structure def Real { let n = 1 }\n\
              ```\n";

    assert_eq!(
        tagged_fence_bodies(md, "reify", "demo.md"),
        vec!["structure def Real { let n = 1 }".to_string()],
        "only the genuine ```reify fence may be selected; the one inside the tilde block is \
         a sample, and compiling it would hold prose to the standalone-module claim"
    );
}

/// A three-backtick sample nested in a longer fence is body content, never a
/// fence of its own.
#[test]
fn tagged_fence_bodies_ignores_a_sample_nested_in_a_longer_fence() {
    let md = "````markdown\n\
              ```reify\n\
              let g = 1\n\
              ```\n\
              ````\n";

    assert!(
        tagged_fence_bodies(md, "reify", "demo.md").is_empty(),
        "the inner ```reify lines belong to the four-backtick block's body"
    );
}

/// Selection is by the EXACT info string: a hyphenated tag is its own tag in
/// both directions.
#[test]
fn tagged_fence_bodies_matches_the_info_string_exactly() {
    let md = "```reify-fragment\n\
              let a = 1\n\
              ```\n\
              ```reify-schematic\n\
              box(width, depth, height)\n\
              ```\n\
              ```reify-invalid\n\
              let c = 1 + 1mm\n\
              ```\n\
              ```reify-rejected\n\
              let d = 1 rad\n\
              ```\n";

    assert!(
        tagged_fence_bodies(md, "reify", "demo.md").is_empty(),
        "no hyphenated tag may be selected as bare `reify`"
    );
    assert_eq!(
        tagged_fence_bodies(md, "reify-rejected", "demo.md"),
        vec!["let d = 1 rad".to_string()],
        "a hyphenated tag selects exactly its own fences"
    );
}

/// An unterminated fence is the parser's named error, blamed on the chunk.
#[test]
#[should_panic(expected = "unterminated code fence")]
fn tagged_fence_bodies_panics_on_an_unterminated_fence() {
    let _ = tagged_fence_bodies("```reify\nlet g = 1\n", "reify", "demo.md");
}

// ---------------------------------------------------------------------------
// Hermetic section tests
// ---------------------------------------------------------------------------

#[test]
fn section_body_reads_a_shorter_fence_run_as_content_of_a_longer_one() {
    // A 4-backtick fence displaying a 3-backtick one — the routine shape in a doc
    // that shows markdown, and the shape a toggle-based scanner desyncs on: the
    // inner ``` lines would flip fence state twice more, leaving the opener line
    // "inside a fence" and unreachable.
    let md = "# Chunk\n\
              ````markdown\n\
              ```reify\n\
              let g = box(1mm, 1mm, 1mm)\n\
              ```\n\
              ````\n\
              <!-- M -->\n\
              body line\n\
              ## Next section\n\
              not in the body\n";

    assert_eq!(
        section_body(md, "<!-- M -->", "demo.md", "## Demo"),
        "body line",
        "the inner ``` run is shorter than the ```` that opened the fence, so it is CONTENT — \
         only a bare run of >= 4 closes"
    );
}

#[test]
fn section_body_keeps_a_fenced_heading_out_of_the_section_boundary() {
    // A `## ` line inside a fence is content, not the end of the section.
    let md = "<!-- M -->\n\
              ```reify\n\
              // ## not a heading\n\
              ```\n\
              tail\n\
              ## Real heading\n\
              gone\n";

    let body = section_body(md, "<!-- M -->", "demo.md", "## Demo");
    assert!(body.contains("// ## not a heading"), "got {body:?}");
    assert!(body.contains("tail"), "got {body:?}");
    assert!(!body.contains("gone"), "got {body:?}");
}

#[test]
#[should_panic(expected = "unterminated code fence")]
fn section_body_blames_an_unterminated_fence_rather_than_the_marker() {
    // The opener is PRESENT here. Blaming it (which a toggle-based scanner's
    // anti-vacuity panic does, because the swallowed tail leaves `in_section`
    // false) sends the reader to a line that is not the defect.
    let md = "```reify\n\
              let g = box(1mm, 1mm, 1mm)\n\
              <!-- M -->\n\
              body\n";
    let _ = section_body(md, "<!-- M -->", "demo.md", "## Demo");
}

/// A `~~~` fence is a fence for section purposes too: a `## ` line inside one
/// is content, not the end of the section.
#[test]
fn section_body_keeps_a_heading_inside_a_tilde_fence_as_content() {
    let md = "<!-- M -->\n\
              ~~~text\n\
              ## not a heading\n\
              ~~~\n\
              tail\n\
              ## Real\n\
              gone\n";

    let body = section_body(md, "<!-- M -->", "demo.md", "## Demo");
    assert!(body.contains("## not a heading"), "got {body:?}");
    assert!(body.contains("tail"), "got {body:?}");
    assert!(!body.contains("gone"), "got {body:?}");
}

/// A tagged delimiter while a fence is open is ambiguous — a missing closer or
/// a nesting error — and a section scan must refuse it, as [`parse_fences`]
/// does, rather than silently read it as content.
#[test]
#[should_panic(expected = "still OPEN")]
fn section_body_rejects_a_tagged_delimiter_inside_an_open_fence() {
    let md = "<!-- M -->\n\
              ```reify\n\
              let a = 1\n\
              ```reify-fragment\n\
              let b = 2\n\
              ```\n";
    let _ = section_body(md, "<!-- M -->", "demo.md", "## Demo");
}

/// A heading line is a valid opener: the section is the lines after it up to
/// the next `## ` heading, `### ` subsections included.
#[test]
fn section_body_opens_on_a_heading_line() {
    let md = "# Enums\n\
              ## Option Type\n\
              line a\n\
              ### sub\n\
              line b\n\
              ## Next\n\
              gone\n";

    assert_eq!(
        section_body(md, "## Option Type", "demo.md", "## Option Type"),
        "line a\n### sub\nline b"
    );
}

/// An absent opener is a loud failure, never an empty section every downstream
/// assertion would pass over.
#[test]
#[should_panic(expected = "carries no")]
fn section_body_panics_when_the_opener_is_absent() {
    let _ = section_body("# Chunk\nprose\n", "<!-- M -->", "demo.md", "## Demo");
}

// ---------------------------------------------------------------------------
// Hermetic catalogue-table tests
// ---------------------------------------------------------------------------

/// The catalogue scan reads the FIRST cell only, and takes both spellings of a
/// constructor name.
///
/// The first-column rule is what keeps "every name this asserts is real" true:
/// `n_points` here is a backticked ARGUMENT name in a later column, and feeding
/// it to a registry lookup would force an allowlist entry for a name that is not
/// a constructor at all.
#[test]
fn catalogue_table_rows_reads_the_first_cell_and_strips_a_call_form() {
    let table = "\
| Constructor | Length-semantic arguments | Stays dimensionless |
|---|---|---|
| `helix` | **all three** — `helix(radius, pitch, height)` | — |
| `nurbs` | the control points | leading `n_points` counts |
";
    assert_eq!(
        catalogue_table_rows(table),
        vec![vec!["helix".to_string()], vec!["nurbs".to_string()]],
        "the header, the delimiter row, the trailing call form and the later-column \
         `n_points` must all be absent"
    );
}

/// A cell naming two constructors yields both, and a `|`-leading line with no
/// backticked identifier yields no ROW at all.
///
/// The row count is an anti-vacuity floor in `geometry_chunk_smoke.rs`'s
/// catalogue checks, so what counts as a row is load-bearing: a header or
/// delimiter line that slipped into the count would let a real catalogue row be
/// deleted while the floor stayed satisfied.
#[test]
fn catalogue_table_rows_splits_a_shared_cell_and_drops_a_rowless_line() {
    let table = "\
|---|---|
| `interp` / `bezier` | every argument |
| no backticks here | so this is not a catalogue row |
prose, not a table row at all
";
    assert_eq!(
        catalogue_table_rows(table),
        vec![vec!["interp".to_string(), "bezier".to_string()]]
    );
}

/// The flatten keeps DOCUMENT ORDER and drops a repeat, across rows as well as
/// within one.
///
/// Both directions of the catalogue assertions read this list — coverage reports
/// it back in its panic text, and registry-truth iterates it — so an order that
/// wandered or a duplicate that survived would show up as a confusing panic
/// rather than a wrong verdict. Pinned here because no caller carries a copy of
/// the rule to read.
#[test]
fn catalogue_table_names_flattens_in_document_order_without_repeats() {
    let rows = vec![
        vec!["interp".to_string(), "bezier".to_string()],
        vec!["helix".to_string()],
        vec!["bezier".to_string()],
    ];
    assert_eq!(
        catalogue_table_names(&rows),
        vec![
            "interp".to_string(),
            "bezier".to_string(),
            "helix".to_string()
        ],
        "the second `bezier` is the same claim as the first, and the surviving order is the \
         order a reader scans the table in"
    );
}
