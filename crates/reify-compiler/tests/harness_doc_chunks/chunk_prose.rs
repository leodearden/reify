//! The UNFENCED PROSE of a markdown chunk: what a reader sees outside code
//! blocks and maintainer notes, with every line and column still pointing into
//! the source.
//!
//! Fenced blocks come from [`parse_fences`], the binary's one fence model.
//! HTML comments are read as HTML reads them: a comment runs from `<!--` to its
//! FIRST `-->`, even one quoted in backticks, because markdown is not processed
//! inside a comment. A maintainer note that quotes a full marker therefore ends
//! at the quote and leaks its tail into the rendered chunk — the defect
//! [`stray_comment_terminators`] reports, measured in five notes of
//! `geometry.md` and `units.md` when this module landed. Code spans pair per
//! CommonMark within one paragraph, so a stray backtick cannot swallow the
//! paragraphs after it.

use std::ops::Range;

use crate::fence_gate::parse_fences;

/// The HTML comment grammar — ONE pair of delimiters for every comment reader in
/// this binary: the prose model here, the renderer-faithful
/// [`strip_html_comments`], and `oracle_xref_smoke.rs`'s debris check. Readers
/// that disagreed about what closes a note would each report on a document the
/// others never saw.
pub(crate) const HTML_COMMENT_OPEN: &str = "<!--";
pub(crate) const HTML_COMMENT_CLOSE: &str = "-->";

/// One HTML comment outside any fence.
#[derive(Debug)]
pub(crate) struct HtmlComment {
    /// 1-based line of the comment's `<!--`.
    pub(crate) line: usize,
    /// The text between the two delimiters.
    pub(crate) body: String,
}

/// One inline code span.
#[derive(Debug)]
pub(crate) struct CodeSpan {
    /// 1-based line of the opening backtick run.
    pub(crate) line: usize,
    /// The span's content, a line break inside it read as one space.
    pub(crate) text: String,
}

/// `markdown` with every fenced block — both delimiter lines included — and
/// every HTML comment outside a fence blanked to spaces. Every other byte, and
/// every line break, is kept.
///
/// `Err` on an unterminated fence (the fence model's own message) or an
/// unterminated comment (naming its line): either would silently hide the rest
/// of the chunk from whatever reads this prose.
pub(crate) fn unfenced_prose(markdown: &str) -> Result<String, String> {
    let unfenced = without_fences(markdown)?;
    let comments = closed_comments(&unfenced)?;
    let mut prose = unfenced.into_bytes();
    for comment in comments {
        blank(&mut prose[comment]);
    }
    Ok(into_text(prose))
}

/// Every HTML comment outside a fence, in document order.
pub(crate) fn html_comments(markdown: &str) -> Result<Vec<HtmlComment>, String> {
    let unfenced = without_fences(markdown)?;
    let comments = closed_comments(&unfenced)?;
    Ok(comments
        .into_iter()
        .map(|comment| {
            let body_start = comment.start + HTML_COMMENT_OPEN.len();
            let body_end = (comment.end - HTML_COMMENT_CLOSE.len()).max(body_start);
            HtmlComment {
                line: line_at(&unfenced, comment.start),
                body: markdown[body_start..body_end].to_string(),
            }
        })
        .collect())
}

/// The 1-based lines of every `-->` left in unfenced prose: the tail of a
/// comment that closed before its author meant it to.
pub(crate) fn stray_comment_terminators(markdown: &str) -> Result<Vec<usize>, String> {
    let prose = unfenced_prose(markdown)?;
    let mut lines: Vec<usize> = prose
        .match_indices(HTML_COMMENT_CLOSE)
        .map(|(at, _)| line_at(&prose, at))
        .collect();
    lines.dedup();
    Ok(lines)
}

/// `markdown` with every `<!-- … -->` comment removed.
///
/// An UNTERMINATED comment consumes the remainder, which is exactly what a
/// markdown renderer does with it — so a region whose pointer has been swallowed
/// by a stray `<!--` reports as missing its call forms, which is the true
/// description of what the reader can now see.
pub(crate) fn strip_html_comments(markdown: &str) -> String {
    let comments = scan_comments(markdown);
    let mut out = String::with_capacity(markdown.len());
    let mut kept_from = 0;
    for comment in comments.closed {
        out.push_str(&markdown[kept_from..comment.start]);
        kept_from = comment.end;
    }
    out.push_str(&markdown[kept_from..comments.unterminated.unwrap_or(markdown.len())]);
    out
}

/// Every inline code span in `text`, in document order.
///
/// A backtick run of N opens a span that closes at the next run of EXACTLY N in
/// the same paragraph; an opener with no such closer is literal text.
pub(crate) fn code_spans(text: &str) -> Vec<CodeSpan> {
    paragraphs(text)
        .into_iter()
        .flat_map(|(first_line, lines)| paragraph_code_spans(first_line, &lines.join("\n")))
        .collect()
}

/// The comments in `text` as byte ranges, delimiters included, in document
/// order — plus the offset of a final `<!--` that never closes.
struct CommentScan {
    closed: Vec<Range<usize>>,
    unterminated: Option<usize>,
}

fn scan_comments(text: &str) -> CommentScan {
    let mut closed = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find(HTML_COMMENT_OPEN).map(|at| from + at) {
        let Some(close) = text[open..].find(HTML_COMMENT_CLOSE) else {
            return CommentScan {
                closed,
                unterminated: Some(open),
            };
        };
        from = open + close + HTML_COMMENT_CLOSE.len();
        closed.push(open..from);
    }
    CommentScan {
        closed,
        unterminated: None,
    }
}

fn closed_comments(text: &str) -> Result<Vec<Range<usize>>, String> {
    let scan = scan_comments(text);
    match scan.unterminated {
        None => Ok(scan.closed),
        Some(open) => Err(format!(
            "unterminated HTML comment: the `{HTML_COMMENT_OPEN}` at line {} is never closed \
             by `{HTML_COMMENT_CLOSE}`, so a renderer hides everything after it. Close the note.",
            line_at(text, open)
        )),
    }
}

fn without_fences(markdown: &str) -> Result<String, String> {
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let mut text = markdown.as_bytes().to_vec();
    for fence in parse_fences(markdown)? {
        let start = line_starts[fence.open_line - 1];
        let end = line_starts
            .get(fence.close_line)
            .copied()
            .unwrap_or(markdown.len());
        blank(&mut text[start..end]);
    }
    Ok(into_text(text))
}

fn blank(bytes: &mut [u8]) {
    for byte in bytes.iter_mut().filter(|byte| **byte != b'\n') {
        *byte = b' ';
    }
}

fn into_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("blanking writes ASCII spaces over whole characters")
}

fn line_at(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

/// Maximal runs of non-blank lines, each with the 1-based number of its first
/// line.
fn paragraphs(text: &str) -> Vec<(usize, Vec<&str>)> {
    let mut out: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            out.extend(current.take());
        } else {
            current
                .get_or_insert_with(|| (index + 1, Vec::new()))
                .1
                .push(line);
        }
    }
    out.extend(current);
    out
}

fn paragraph_code_spans(first_line: usize, paragraph: &str) -> Vec<CodeSpan> {
    let runs = backtick_runs(paragraph);
    let mut spans = Vec::new();
    let mut next = 0;
    while let Some(opener) = runs.get(next) {
        let closer = runs[next + 1..]
            .iter()
            .position(|run| run.len() == opener.len());
        let Some(skip) = closer else {
            next += 1;
            continue;
        };
        let closer = &runs[next + 1 + skip];
        spans.push(CodeSpan {
            line: first_line + line_at(paragraph, opener.start) - 1,
            text: span_content(&paragraph[opener.end..closer.start]),
        });
        next += skip + 2;
    }
    spans
}

fn backtick_runs(text: &str) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for (at, byte) in text.bytes().enumerate() {
        if byte != b'`' {
            continue;
        }
        match runs.last_mut() {
            Some(run) if run.end == at => run.end += 1,
            _ => runs.push(at..at + 1),
        }
    }
    runs
}

/// A span's raw content as CommonMark reads it: a line break, with the next
/// line's indentation, becomes one space; then one space comes off each end
/// when both ends have one and the content is not all spaces.
fn span_content(raw: &str) -> String {
    let joined = raw
        .split('\n')
        .enumerate()
        .map(|(index, part)| if index == 0 { part } else { part.trim_start() })
        .collect::<Vec<_>>()
        .join(" ");
    match joined
        .strip_prefix(' ')
        .and_then(|rest| rest.strip_suffix(' '))
    {
        Some(inner) if !joined.trim().is_empty() => inner.to_string(),
        _ => joined,
    }
}

// ---------------------------------------------------------------------------
// Hermetic tests — synthetic markdown only; no chunk file is read.
// ---------------------------------------------------------------------------

#[test]
fn unfenced_prose_blanks_every_fence_with_both_delimiters_and_keeps_prose_lines() {
    let md = "intro\n\
              ```reify\n\
              structure def S { let n = 1 }\n\
              ```\n\
              between\n\
              ~~~text\n\
              tilde body\n\
              ~~~\n\
              ````markdown\n\
              ```reify\n\
              nested sample\n\
              ```\n\
              ````\n\
              outro\n";
    let prose_lines = [1, 5, 14];

    let prose = unfenced_prose(md).expect("well-formed markdown must yield its prose");

    assert_eq!(prose.lines().count(), md.lines().count(), "got {prose:?}");
    for (index, (seen, original)) in prose.lines().zip(md.lines()).enumerate() {
        let line = index + 1;
        if prose_lines.contains(&line) {
            assert_eq!(seen, original, "prose line {line} must be byte-identical");
        } else {
            assert!(
                seen.trim().is_empty(),
                "fenced line {line} (delimiters included) must be blanked, got {seen:?}"
            );
        }
    }
}

#[test]
fn unfenced_prose_blanks_whole_line_inline_and_multi_line_comments_in_place() {
    let md = "intro\n\
              <!-- a whole-line note -->\n\
              before <!-- inline --> after\n\
              <!--\n\
              a multi-line note\n\
              -->\n\
              outro\n";

    let prose = unfenced_prose(md).expect("well-formed markdown must yield its prose");
    let lines: Vec<&str> = prose.lines().collect();

    assert_eq!(lines.len(), md.lines().count(), "got {prose:?}");
    assert_eq!(lines[0], "intro");
    assert!(lines[1].trim().is_empty(), "got {:?}", lines[1]);
    assert_eq!(
        lines[2],
        format!("before {} after", " ".repeat("<!-- inline -->".len())),
        "an inline comment is blanked in place, so every column after it keeps its position"
    );
    assert!(
        lines[3..6].iter().all(|line| line.trim().is_empty()),
        "got {:?}",
        &lines[3..6]
    );
    assert_eq!(lines[6], "outro");
}

/// HTML comments do not nest and backticks are markdown, which is not processed
/// inside a comment — so a quoted marker's terminator ends the note.
#[test]
fn a_comment_ends_at_its_first_terminator_even_inside_backticks() {
    let closed_early = "<!-- note `<!-- MARK -->";
    let md = format!("{closed_early}` tail -->\n");

    let prose = unfenced_prose(&md).expect("a closed comment must yield its prose");

    assert_eq!(
        prose,
        format!("{}` tail -->\n", " ".repeat(closed_early.len())),
        "only the text up to the FIRST terminator is comment; the rest is prose"
    );
}

#[test]
fn an_unterminated_comment_is_an_error_naming_its_opening_line() {
    let md = "prose\n\nstill prose <!-- never closed\nmore prose\n";

    let outcomes = [
        ("unfenced_prose", unfenced_prose(md).map(drop)),
        ("html_comments", html_comments(md).map(drop)),
        (
            "stray_comment_terminators",
            stray_comment_terminators(md).map(drop),
        ),
    ];
    for (entry_point, outcome) in outcomes {
        let err = outcome.expect_err("an unterminated comment must not read as prose");
        assert!(
            err.contains("line 3"),
            "{entry_point} must name the comment's opening line, got: {err}"
        );
    }
}

#[test]
fn an_unterminated_fence_is_the_fence_models_own_error() {
    let md = "prose\n```reify\nstructure def S { let n = 1 }\n";

    assert_eq!(
        unfenced_prose(md).expect_err("an unterminated fence must not read as prose"),
        parse_fences(md).expect_err("fixture: the fence is unterminated"),
        "the fence model's message is passed through, not re-worded"
    );
}

#[test]
fn a_comment_opener_inside_a_fence_is_code_not_a_comment() {
    let md = "```text\n<!-- never closed, but fenced\n```\nprose\n";

    let prose = unfenced_prose(md).expect("a fenced `<!--` opens nothing");
    let lines: Vec<&str> = prose.lines().collect();

    assert!(
        lines[..3].iter().all(|line| line.trim().is_empty()),
        "got {prose:?}"
    );
    assert_eq!(lines[3], "prose");
}

#[test]
fn html_comments_lists_only_comments_outside_fences_with_line_and_body() {
    let md = "prose <!-- inline note --> tail\n\
              ```reify\n\
              // <!-- SYNC: inside a fence -->\n\
              ```\n\
              <!-- SYNC: signatures verified by crates/x/tests/y.rs -->\n\
              <!--\n\
              SYNC: an inventory note\n\
              -->\n";

    let comments = html_comments(md).expect("well-formed markdown must list its comments");
    let seen: Vec<(usize, &str)> = comments
        .iter()
        .map(|comment| (comment.line, comment.body.as_str()))
        .collect();

    assert_eq!(
        seen,
        vec![
            (1, " inline note "),
            (5, " SYNC: signatures verified by crates/x/tests/y.rs "),
            (6, "\nSYNC: an inventory note\n"),
        ],
        "a comment inside a fence is fence content, not a comment"
    );
    assert!(comments[2].body.trim().starts_with("SYNC:"));
}

#[test]
fn a_note_that_quotes_a_full_marker_leaves_a_stray_terminator_where_it_meant_to_end() {
    let md = "prose\n\
              <!-- SYNC: scoped by the `<!-- MARK -->` marker\n\
              on the line above. -->\n\
              more prose\n";

    assert_eq!(
        stray_comment_terminators(md).expect("the note is closed, just early"),
        vec![3]
    );
}

#[test]
fn a_whole_note_and_a_fenced_arrow_leave_no_stray_terminator() {
    let md = "<!-- SYNC: scoped by the `MARK` marker\n\
              on the line above. -->\n\
              ```reify-rejected\n\
              box(20, 20, 10)  -->  box(20mm, 20mm, 10mm)\n\
              ```\n";

    assert_eq!(
        stray_comment_terminators(md).expect("well-formed markdown"),
        Vec::<usize>::new()
    );
}

fn spans_of(text: &str) -> Vec<(usize, String)> {
    code_spans(text)
        .into_iter()
        .map(|span| (span.line, span.text))
        .collect()
}

#[test]
fn code_spans_are_returned_in_document_order_with_their_opening_line() {
    assert_eq!(
        spans_of("first `a(x)` and `b`\nsecond line `c(y, z)`\n"),
        vec![
            (1, "a(x)".to_string()),
            (1, "b".to_string()),
            (2, "c(y, z)".to_string()),
        ]
    );
}

#[test]
fn a_span_wrapped_across_two_lines_is_one_span_on_its_opening_line() {
    assert_eq!(
        spans_of("prose\nsee `rotate(geo, ax, ay,\naz, angle)` here\n"),
        vec![(2, "rotate(geo, ax, ay, az, angle)".to_string())],
        "the line break inside a span reads as one space"
    );
}

#[test]
fn a_double_backtick_span_may_contain_a_single_backtick() {
    assert_eq!(
        spans_of("quote `` a`b `` then `c`\n"),
        vec![(1, "a`b".to_string()), (1, "c".to_string())],
        "a run of N closes only at the next run of EXACTLY N"
    );
}

#[test]
fn an_unmatched_backtick_run_is_literal_and_never_pairs_across_a_blank_line() {
    assert_eq!(
        spans_of("a stray ` backtick\n\nnext paragraph `real(x)` span\n"),
        vec![(3, "real(x)".to_string())],
        "a stray backtick must not swallow the next paragraph"
    );
    assert_eq!(
        spans_of("``unclosed then `single(x)` span\n"),
        vec![(1, "single(x)".to_string())],
        "an opener with no closer of its own length is literal text"
    );
}

#[test]
fn several_spans_in_one_table_row_are_each_returned() {
    assert_eq!(
        spans_of("| `faces` | `faces(solid)` | `Selector(Face)` |\n"),
        vec![
            (1, "faces".to_string()),
            (1, "faces(solid)".to_string()),
            (1, "Selector(Face)".to_string()),
        ]
    );
}
