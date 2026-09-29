//! The UNFENCED PROSE of a markdown chunk: what a reader sees outside code
//! blocks and maintainer notes, with every line and column still pointing into
//! the source.
//!
//! Fenced blocks come from [`parse_fences`], the binary's one fence model, and
//! every reader here sees comments only outside them. HTML comments and code
//! spans share CommonMark's one inline precedence — whichever STARTS first wins
//! — so backticks protect in one direction only. A `<!--` quoted in a prose code
//! span is literal text. Inside a comment, where markdown is not processed,
//! backticks protect nothing: a comment runs from `<!--` to its FIRST `-->`,
//! even one quoted in backticks. A maintainer note that quotes a full marker
//! therefore ends at the quote and leaks its tail into the rendered chunk — the
//! defect [`stray_comment_terminators`] reports. Code spans pair within one
//! block, as CommonMark scopes them ([`LineRole`]), so a stray backtick cannot
//! reach past its own paragraph, table row, list item or heading.

use std::ops::Range;

use crate::fence_gate::parse_fences;

/// The HTML comment grammar — ONE pair of delimiters for every comment reader in
/// this binary: the prose model here, the renderer-faithful
/// [`strip_html_comments`], and `oracle_xref_smoke.rs`'s debris check. Readers
/// that disagreed about what closes a note would each report on a document the
/// others never saw.
pub(crate) const HTML_COMMENT_OPEN: &str = "<!--";
pub(crate) const HTML_COMMENT_CLOSE: &str = "-->";

/// Why a maintainer note that quotes a full marker ends early, and the fix — ONE
/// wording for every gate that reports the debris, so what a fixer is told
/// cannot drift between them.
pub(crate) const EARLY_CLOSED_NOTE_FIX: &str = "HTML comments do not nest and HTML defines no \
    escape inside one, so backticks do not protect a quoted terminator — writing a marker out in \
    full is what ends the note. FIX: name the marker WITHOUT its closing bracket (e.g. \
    `ORACLE-XREF`, not the whole comment), or move that sentence out of the comment. A \
    terminator quoted on purpose in prose is reported too — it cannot be told from a note's \
    tail — so name it there as well.";

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
///
/// A `-->` quoted in a prose code span counts too. The tail an early close
/// leaves begins with the quote's own closing backtick, free to pair with any
/// later one, so a quoted terminator cannot be told from debris — and a false
/// report here is loud where a missed one is silent. Prose names a terminator
/// rather than quoting it.
pub(crate) fn stray_comment_terminators(markdown: &str) -> Result<Vec<usize>, String> {
    let prose = unfenced_prose(markdown)?;
    let mut lines: Vec<usize> = prose
        .match_indices(HTML_COMMENT_CLOSE)
        .map(|(at, _)| line_at(&prose, at))
        .collect();
    lines.dedup();
    Ok(lines)
}

/// `markdown` with every `<!-- … -->` comment outside a fence removed. A fenced
/// block is kept whole: a renderer shows a `<!--` inside one as code.
///
/// An UNTERMINATED comment consumes the remainder, which is exactly what a
/// markdown renderer does with it — so a region whose pointer has been swallowed
/// by a stray `<!--` reports as missing its call forms, which is the true
/// description of what the reader can now see. `Err` only on an unterminated
/// fence (the fence model's own message).
pub(crate) fn strip_html_comments(markdown: &str) -> Result<String, String> {
    let comments = scan_comments(&without_fences(markdown)?);
    let mut out = String::with_capacity(markdown.len());
    let mut kept_from = 0;
    for comment in comments.closed {
        out.push_str(&markdown[kept_from..comment.start]);
        kept_from = comment.end;
    }
    out.push_str(&markdown[kept_from..comments.unterminated.unwrap_or(markdown.len())]);
    Ok(out)
}

/// Every inline code span in `text`, in document order.
///
/// A backtick run of N opens a span that closes at the next run of EXACTLY N in
/// the same block; an opener with no such closer is literal text.
pub(crate) fn code_spans(text: &str) -> Vec<CodeSpan> {
    blocks(text)
        .into_iter()
        .flat_map(|(first_line, lines)| block_code_spans(first_line, &lines.join("\n")))
        .collect()
}

/// How one line bounds the CommonMark block around it — the scope within which
/// code spans pair and a backtick can protect a comment opener.
#[derive(PartialEq)]
enum LineRole {
    /// Ends the block above it.
    Blank,
    /// Opens a block of its own, ending the one above: an HTML comment or a list
    /// item.
    Opens,
    /// A whole block on one line, ending the one above it and itself: a table row
    /// or an ATX heading.
    Whole,
    /// Continues the block above it.
    Continues,
}

fn line_role(line: &str) -> LineRole {
    let content = line.trim_start();
    if content.is_empty() {
        LineRole::Blank
    } else if content.starts_with('|') || is_atx_heading(content) {
        LineRole::Whole
    } else if content.starts_with(HTML_COMMENT_OPEN) || is_list_item(content) {
        LineRole::Opens
    } else {
        LineRole::Continues
    }
}

/// One to six `#`, then a space, a tab or the end of the line.
fn is_atx_heading(content: &str) -> bool {
    let level = content.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&level) && matches!(content.as_bytes().get(level), None | Some(b' ' | b'\t'))
}

/// A bullet (`-`, `+`, `*`) or an ordinal (`1.`, `2)`), then a space or a tab.
fn is_list_item(content: &str) -> bool {
    let digits = content.bytes().take_while(u8::is_ascii_digit).count();
    let marker_len = match content.as_bytes().get(digits) {
        Some(b'.' | b')') if (1..=9).contains(&digits) => digits + 1,
        Some(b'-' | b'+' | b'*') if digits == 0 => 1,
        _ => return false,
    };
    matches!(content.as_bytes().get(marker_len), Some(b' ' | b'\t'))
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
    while let Some(open) = next_comment_open(text, from) {
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

/// The first `<!--` at or after `from` that no code span opened before it.
fn next_comment_open(text: &str, from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        let open = at + text[at..].find(HTML_COMMENT_OPEN)?;
        match text[at..open].find('`') {
            None => return Some(open),
            Some(tick) => at = code_span_end(text, at + tick),
        }
    }
}

/// Where the backtick run starting at `tick` stops protecting what follows it:
/// the end of the code span it opens — closed, as [`code_spans`] pairs them, by
/// the next run of the same length in its block — or, with no such closer, the
/// end of the run itself, which is then literal text.
fn code_span_end(text: &str, tick: usize) -> usize {
    let runs = backtick_runs(&text[tick..block_end(text, tick)]);
    let opener = &runs[0];
    let closer = runs[1..].iter().find(|run| run.len() == opener.len());
    tick + closer.unwrap_or(opener).end
}

/// Where the block holding `offset` ends: after its own line when that line is
/// a whole block, else before the next line that does not continue it — so no
/// backtick above a note, or in another table row, can pair with one past it.
fn block_end(text: &str, offset: usize) -> usize {
    let line_start = text[..offset].rfind('\n').map_or(0, |newline| newline + 1);
    let mut end = next_line_start(text, offset);
    if line_role(first_line(&text[line_start..])) == LineRole::Whole {
        return end;
    }
    while end < text.len() && line_role(first_line(&text[end..])) == LineRole::Continues {
        end = next_line_start(text, end);
    }
    end
}

fn next_line_start(text: &str, offset: usize) -> usize {
    text[offset..]
        .find('\n')
        .map_or(text.len(), |newline| offset + newline + 1)
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
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

/// The blocks of `text` that inline content is scoped to, as [`LineRole`]
/// bounds them — each its lines, with the 1-based number of the first.
fn blocks(text: &str) -> Vec<(usize, Vec<&str>)> {
    let mut out: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut current: Option<(usize, Vec<&str>)> = None;
    for (index, line) in text.lines().enumerate() {
        let role = line_role(line);
        if role != LineRole::Continues {
            out.extend(current.take());
        }
        if role != LineRole::Blank {
            current
                .get_or_insert_with(|| (index + 1, Vec::new()))
                .1
                .push(line);
        }
        if role == LineRole::Whole {
            out.extend(current.take());
        }
    }
    out.extend(current);
    out
}

fn block_code_spans(first_line: usize, block: &str) -> Vec<CodeSpan> {
    let runs = backtick_runs(block);
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
            line: first_line + line_at(block, opener.start) - 1,
            text: span_content(&block[opener.end..closer.start]),
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

fn comments_of(md: &str) -> Vec<(usize, String)> {
    html_comments(md)
        .expect("well-formed markdown must list its comments")
        .into_iter()
        .map(|comment| (comment.line, comment.body))
        .collect()
}

/// The other direction of the one precedence rule: a code span that opens
/// FIRST makes the `<!--` inside it literal text.
#[test]
fn a_comment_opener_quoted_in_a_prose_code_span_opens_nothing() {
    let md = "Write `<!--` to open a note.\n\
              `sig(a)` stays prose.\n\
              <!-- a real note -->\n\
              after\n";

    let prose = unfenced_prose(md).expect("a quoted opener leaves the chunk readable");

    assert_eq!(
        prose.lines().take(2).collect::<Vec<_>>(),
        md.lines().take(2).collect::<Vec<_>>(),
        "the quoted opener must not swallow the prose up to the next note's terminator"
    );
    assert_eq!(comments_of(md), vec![(3, " a real note ".to_string())]);
}

/// A line that opens a comment ends the paragraph above it — an HTML block
/// interrupts a paragraph — so a backtick left unpaired there cannot pair with
/// one inside the note and hide it.
#[test]
fn a_stray_backtick_above_a_note_cannot_pair_into_it() {
    let md = "a stray ` backtick\n\
              <!-- a note quoting a ` backtick -->\n\
              prose\n";

    assert_eq!(
        comments_of(md),
        vec![(2, " a note quoting a ` backtick ".to_string())]
    );
}

/// Each table row is a block of its own, so a backtick left unpaired in one row
/// cannot pair with one in the next and hide the note between them.
#[test]
fn a_stray_backtick_in_one_table_row_cannot_hide_a_note_in_the_next() {
    let md = "| stray ` tick | x |\n\
              | <!-- a note --> | `y` |\n";

    assert_eq!(comments_of(md), vec![(2, " a note ".to_string())]);
}

/// The early-close check stays conservative: a quoted terminator cannot be
/// told from a closed-early note's tail, so it is reported.
#[test]
fn a_terminator_quoted_in_a_prose_code_span_is_still_reported() {
    assert_eq!(
        stray_comment_terminators("Close a note with `-->`.\n").expect("well-formed markdown"),
        vec![1]
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
    let fence_error = parse_fences(md).expect_err("fixture: the fence is unterminated");

    let outcomes = [
        ("unfenced_prose", unfenced_prose(md).map(drop)),
        ("strip_html_comments", strip_html_comments(md).map(drop)),
    ];
    for (entry_point, outcome) in outcomes {
        assert_eq!(
            outcome.expect_err("an unterminated fence must not read as prose"),
            fence_error,
            "{entry_point} passes the fence model's message through, not re-worded"
        );
    }
}

/// A blank line in a fence body leaves the opening delimiter nothing to pair
/// with, so only the fence model keeps a `<!--` in that body from opening a
/// comment that swallows the prose after the fence.
#[test]
fn strip_html_comments_keeps_a_fence_whole_and_strips_only_the_notes_outside_it() {
    let md = "```text\n\
              <!-- a fenced opener\n\
              \n\
              still fenced\n\
              ```\n\
              prose <!-- a note --> after\n";

    assert_eq!(
        strip_html_comments(md).expect("the fence is closed"),
        "```text\n<!-- a fenced opener\n\nstill fenced\n```\nprose  after\n"
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

/// A table row and a heading are each a whole block, and a list item opens one,
/// so a backtick left unpaired above any of them cannot pair into it.
#[test]
fn a_stray_backtick_cannot_pair_past_its_own_table_row_list_item_or_heading() {
    let cases = [
        (
            "| stray ` tick | x |\n| `some(v, w)` | form |\n",
            "a table row below a table row",
        ),
        (
            "a stray ` tick\n- `some(v, w)` is an item\n",
            "a bullet item below a paragraph",
        ),
        (
            "a stray ` tick\n1. `some(v, w)` is an item\n",
            "an ordinal item below a paragraph",
        ),
        (
            "a stray ` tick\n## `some(v, w)` heads a section\n",
            "a heading below a paragraph",
        ),
        (
            "## A stray ` tick\n`some(v, w)` follows the heading\n",
            "a paragraph below a heading",
        ),
    ];
    for (markdown, case) in cases {
        assert_eq!(
            spans_of(markdown),
            vec![(2, "some(v, w)".to_string())],
            "{case}: the stray backtick must stay literal"
        );
    }
}

#[test]
fn a_span_wrapped_inside_a_list_item_stays_one_span() {
    assert_eq!(
        spans_of("- see `rotate(geo,\n  angle)` here\n- next item\n"),
        vec![(1, "rotate(geo, angle)".to_string())],
        "a list item's continuation line is part of its block"
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
