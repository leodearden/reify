//! How a chunk's markdown divides into fenced code blocks, and the sections
//! those fences cannot end.

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

/// Every ```` ```reify ````-tagged fence in the chunk, in document order, with
/// the fence delimiters stripped.
///
/// Only EXPLICITLY TAGGED fences are collected. The module doc above explains
/// why geometry.md cannot be scraped wholesale: it intermixes non-compilable
/// schematic notation (type params, trait lists, `-> Solid` return annotations)
/// with real call forms, and separating the two would need a fragile grammar.
/// An opt-in tag sidesteps that — the doc author marks exactly what is meant to
/// compile, and everything else stays free-form. ```` ```reify ```` is already
/// the in-repo convention (every fence in `chunks/traits.md` is tagged that
/// way).
///
/// Callers must anti-vacuity-check the result: a dropped tag or a renamed
/// section would otherwise empty the scan and pass trivially.
///
/// `tag` IS A PARAMETER, not the hardcoded `reify` this scanner started with
/// (task 5759). units.md carries a deliberately-INVALID rejected-forms block
/// tagged ```` ```reify-rejected ````, which a rejection-truth negative control
/// must scrape and a zero-Error compile gate must never sweep in. Parameterising
/// the tag lets both gates share this one scanner instead of the harness growing
/// its FIFTH near-identical scraper (see "Known duplication" above). Matching
/// stays BYTE-EXACT on the whole info string, so `reify` still excludes
/// `reify-rejected` in both directions.
///
/// `chunk_path` is threaded for the same reason [`assert_module_compiles`]
/// threads its own: a sibling chunk module's unterminated fence must be blamed
/// on ITS chunk, not on geometry.md.
pub(crate) fn tagged_fence_bodies(markdown: &str, tag: &str, chunk_path: &str) -> Vec<String> {
    let opener = format!("```{tag}");
    let mut fences: Vec<String> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let mut open = false;

    for line in markdown.lines() {
        if !open {
            // Exact tag match: `reify-something` is a different language and
            // must not be swept in.
            if line.trim_end() == opener {
                open = true;
                body.clear();
            }
            continue;
        }
        if line.trim_end() == "```" {
            fences.push(body.join("\n"));
            open = false;
            continue;
        }
        body.push(line);
    }

    assert!(
        !open,
        "{chunk_path} has an unterminated ```{tag} fence — the scrape cannot be trusted"
    );
    fences
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
