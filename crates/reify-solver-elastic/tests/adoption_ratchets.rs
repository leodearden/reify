//! Adoption-ratchet host for reify-solver-elastic (PRD
//! `docs/prds/v0_6/solver-crates-layering.md` D20, §9 α, BT14).
//!
//! Each row counts the inline copies of a primitive that a later leaf extracts,
//! and the count must equal the row's committed baseline: above it is a re-added
//! copy, below it means the migrating leaf has to lower the baseline.

struct Line {
    raw: String,
    code: String,
    doc: Option<String>,
}

fn lex(text: &str) -> Vec<Line> {
    text.lines()
        .map(|raw| Line {
            raw: raw.to_string(),
            code: raw.to_string(),
            doc: None,
        })
        .collect()
}

fn lexed_code(source_lines: &[&str]) -> Vec<String> {
    lex(&source_lines.join("\n"))
        .into_iter()
        .map(|line| line.code)
        .collect()
}

#[test]
fn code_view_strips_line_comments_but_not_slashes_inside_strings() {
    let code = lexed_code(&[r#"let u = "https://x"; let det_bb = 1; // MpcRow {"#]).remove(0);
    assert!(code.contains("let det_bb ="), "{code:?}");
    assert!(!code.contains("MpcRow {"), "{code:?}");
    assert!(!code.contains("https"), "{code:?}");
}

#[test]
fn code_view_masks_multi_line_and_raw_string_contents() {
    let code = lexed_code(&[
        r#"let s = "begin"#,
        "MpcRow { middle of the string",
        r#"end"; let det_bb = 1;"#,
        r##"let r = r#"a " MpcRow { "#; let det_bb = 2;"##,
    ]);
    for line in &code {
        assert!(!line.contains("MpcRow {"), "{line:?}");
    }
    assert!(code[2].contains("let det_bb ="), "{:?}", code[2]);
    assert!(code[3].contains("let det_bb ="), "{:?}", code[3]);
}

#[test]
fn char_literals_and_lifetimes_do_not_desync_string_tracking() {
    let code = lexed_code(&[
        r#"let q = '"'; let det_bb = 0;"#,
        r#"let e = '\''; let det_bb = 1;"#,
        "fn f<'a>(x: &'a str) { let det_bb = 2; }",
        r#"let s = "MpcRow {"; let det_bb = 3;"#,
    ]);
    for line in &code {
        assert!(line.contains("let det_bb ="), "{line:?}");
        assert!(!line.contains("MpcRow {"), "{line:?}");
    }
    assert!(!code[0].contains('"'), "{:?}", code[0]);
    assert!(!code[1].contains('\\'), "{:?}", code[1]);
    assert!(code[2].contains("<'a>(x: &'a str)"), "{:?}", code[2]);
}

#[test]
fn nested_block_comments_span_lines() {
    let code = lexed_code(&[
        "/* a /* b */ MpcRow { */ let det_bb = 3;",
        "/* opens",
        "MpcRow { inside the comment",
        "closes */ let det_bb = 4;",
    ]);
    assert_eq!(code[0].trim(), "let det_bb = 3;");
    assert_eq!(code[1].trim(), "");
    assert_eq!(code[2].trim(), "");
    assert_eq!(code[3].trim(), "let det_bb = 4;");
}

#[test]
fn doc_view_captures_outer_and_inner_doc_comments_only() {
    let lines = lex(&["/// x", "//! y", "//// z", "// w"].join("\n"));
    let docs: Vec<Option<&str>> = lines.iter().map(|line| line.doc.as_deref()).collect();
    assert_eq!(docs, [Some(" x"), Some(" y"), None, None]);
    for line in &lines {
        assert_eq!(line.code.trim(), "", "{:?}", line.raw);
    }
}
