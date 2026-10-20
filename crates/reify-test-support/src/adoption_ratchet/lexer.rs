#[cfg(test)]
mod tests {
    use super::*;

    fn lexed_code(source_lines: &[&str]) -> Vec<String> {
        lex(&source_lines.join("\n"))
            .lines
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
        let lines = lex(&["/// x", "//! y", "//// z", "// w"].join("\n")).lines;
        let docs: Vec<&str> = lines.iter().map(|line| line.doc.as_str()).collect();
        assert_eq!(docs, [" x", " y", "", ""]);
        for line in &lines {
            assert_eq!(line.code.trim(), "", "{:?}", line.raw);
        }
    }

    #[test]
    fn doc_view_captures_block_doc_comments_on_every_line_they_span() {
        let lines = lex(&["/** a", " * b /* c */ d", " */ fn f() {} /*! e */"].join("\n")).lines;
        let docs: Vec<&str> = lines.iter().map(|line| line.doc.as_str()).collect();
        assert_eq!(docs, [" a", " * b  c  d", "  e "]);
        let code: Vec<&str> = lines.iter().map(|line| line.code.trim()).collect();
        assert_eq!(code, ["", "", "fn f() {}"]);
    }

    #[test]
    fn plain_block_comments_are_not_doc_text() {
        let lines = lex(&["/*** x */", "/**/ fn g() {}", "/* y */"].join("\n")).lines;
        for line in &lines {
            assert_eq!(line.doc, "", "{:?}", line.raw);
        }
        assert_eq!(lines[1].code.trim(), "fn g() {}");
    }

    #[test]
    fn lex_reports_whether_the_text_ends_outside_every_literal_and_comment() {
        assert!(lex("fn f() {}").ends_in_code);
        for unclosed in ["let s = \"open;", "fn f() {}\n/* open", "let r = r#\"open;"] {
            assert!(!lex(unclosed).ends_in_code, "{unclosed:?}");
        }
    }
}
