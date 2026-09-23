use crate::fence_gate::parse_fences;

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
        ("stray_comment_terminators", stray_comment_terminators(md).map(drop)),
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
