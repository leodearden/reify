//! Splits Rust source into lines, each in a code view and a doc view.

pub(super) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One source line in two views: `code` has comments removed and string and
/// char literal contents blanked; `doc` is the text of the doc comments on it,
/// empty when there are none.
pub(super) struct Line {
    pub(super) raw: String,
    pub(super) code: String,
    pub(super) doc: String,
}

/// The lines of a text, and whether the text ends outside every literal and
/// comment (compilable Rust always does).
pub(super) struct Lexed {
    pub(super) lines: Vec<Line>,
    pub(super) ends_in_code: bool,
}

pub(super) fn lex(text: &str) -> Lexed {
    let mut carry = Carry::Code;
    let lines = text.lines().map(|raw| lex_line(&mut carry, raw)).collect();
    Lexed {
        lines,
        ends_in_code: matches!(carry, Carry::Code),
    }
}

/// What an unfinished construct at the end of a line carries into the next.
#[derive(Clone, Copy)]
enum Carry {
    Code,
    Str,
    RawStr { hashes: usize },
    Block { depth: usize, kind: BlockKind },
}

/// Whether a block comment is documentation (`/** */`, `/*! */`) or a plain one.
#[derive(Clone, Copy)]
enum BlockKind {
    Plain,
    Doc,
}

enum Scan {
    Advance(usize),
    LineComment { doc: String },
}

fn lex_line(carry: &mut Carry, raw: &str) -> Line {
    let chars: Vec<char> = raw.chars().collect();
    let mut code = String::new();
    let mut doc = String::new();
    let mut at = 0;
    while at < chars.len() {
        let scan = match *carry {
            Carry::Code => scan_code(carry, &chars, at, &mut code),
            Carry::Str => scan_str(carry, &chars, at, &mut code),
            Carry::RawStr { hashes } => scan_raw_str(carry, hashes, &chars, at, &mut code),
            Carry::Block { depth, kind } => scan_block(carry, depth, kind, &chars, at, &mut doc),
        };
        match scan {
            Scan::Advance(width) => at += width,
            Scan::LineComment { doc: text } => {
                doc.push_str(&text);
                break;
            }
        }
    }
    Line {
        raw: raw.to_string(),
        code,
        doc,
    }
}

fn starts_at(chars: &[char], at: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, c)| chars.get(at + offset) == Some(&c))
}

fn scan_code(carry: &mut Carry, chars: &[char], at: usize, code: &mut String) -> Scan {
    if starts_at(chars, at, "//") {
        return Scan::LineComment {
            doc: doc_text(&chars[at + 2..]).unwrap_or_default(),
        };
    }
    if starts_at(chars, at, "/*") {
        let (kind, opener_width) = block_opening(chars, at);
        *carry = Carry::Block { depth: 1, kind };
        return Scan::Advance(opener_width);
    }
    match chars[at] {
        '"' => {
            code.push('"');
            *carry = Carry::Str;
            Scan::Advance(1)
        }
        'r' => match raw_string_hashes(chars, at) {
            Some(hashes) => {
                code.push('"');
                *carry = Carry::RawStr { hashes };
                Scan::Advance(2 + hashes)
            }
            None => push_plain(code, 'r'),
        },
        '\'' => match char_literal_width(chars, at) {
            Some(width) => {
                code.push_str("' '");
                Scan::Advance(width)
            }
            None => push_plain(code, '\''),
        },
        c => push_plain(code, c),
    }
}

fn push_plain(code: &mut String, c: char) -> Scan {
    code.push(c);
    Scan::Advance(1)
}

/// The text after a `//` when it opens an outer (`///`) or inner (`//!`) doc comment.
fn doc_text(after_slashes: &[char]) -> Option<String> {
    let is_outer = after_slashes.first() == Some(&'/') && after_slashes.get(1) != Some(&'/');
    let is_inner = after_slashes.first() == Some(&'!');
    (is_outer || is_inner).then(|| after_slashes[1..].iter().collect())
}

/// The kind and width of the block comment whose `/*` sits at `at`: `/**` (but
/// not `/***` or `/**/`) and `/*!` open outer and inner doc comments.
fn block_opening(chars: &[char], at: usize) -> (BlockKind, usize) {
    let is_outer = starts_at(chars, at, "/**") && !matches!(chars.get(at + 3), Some('*' | '/'));
    let is_inner = starts_at(chars, at, "/*!");
    if is_outer || is_inner {
        (BlockKind::Doc, 3)
    } else {
        (BlockKind::Plain, 2)
    }
}

/// The `#` count of a raw string opening (`r"`, `r#"`, `br"`) whose `r` sits at `at`.
/// A raw identifier such as `r#type` is not an opening.
fn raw_string_hashes(chars: &[char], at: usize) -> Option<usize> {
    let before = |back: usize| at.checked_sub(back).map(|index| chars[index]);
    let starts_a_token = match before(1) {
        None => true,
        Some(c) if !is_ident_char(c) => true,
        Some('b' | 'c') => !before(2).is_some_and(is_ident_char),
        Some(_) => false,
    };
    let hashes = chars[at + 1..].iter().take_while(|&&c| c == '#').count();
    (starts_a_token && chars.get(at + 1 + hashes) == Some(&'"')).then_some(hashes)
}

/// The width of a char literal whose opening quote sits at `at`; `None` for a lifetime or label.
fn char_literal_width(chars: &[char], at: usize) -> Option<usize> {
    match chars.get(at + 1) {
        Some('\\') => (at + 3..chars.len())
            .find(|&index| chars[index] == '\'')
            .map(|closing| closing + 1 - at),
        Some(_) if chars.get(at + 2) == Some(&'\'') => Some(3),
        _ => None,
    }
}

fn scan_str(carry: &mut Carry, chars: &[char], at: usize, code: &mut String) -> Scan {
    match chars[at] {
        '\\' => Scan::Advance(2),
        '"' => {
            code.push('"');
            *carry = Carry::Code;
            Scan::Advance(1)
        }
        _ => Scan::Advance(1),
    }
}

fn scan_raw_str(
    carry: &mut Carry,
    hashes: usize,
    chars: &[char],
    at: usize,
    code: &mut String,
) -> Scan {
    let closes = chars[at] == '"' && (1..=hashes).all(|k| chars.get(at + k) == Some(&'#'));
    if !closes {
        return Scan::Advance(1);
    }
    code.push('"');
    *carry = Carry::Code;
    Scan::Advance(1 + hashes)
}

fn scan_block(
    carry: &mut Carry,
    depth: usize,
    kind: BlockKind,
    chars: &[char],
    at: usize,
    doc: &mut String,
) -> Scan {
    if starts_at(chars, at, "/*") {
        *carry = Carry::Block {
            depth: depth + 1,
            kind,
        };
        return Scan::Advance(2);
    }
    if starts_at(chars, at, "*/") {
        *carry = match depth {
            1 => Carry::Code,
            _ => Carry::Block {
                depth: depth - 1,
                kind,
            },
        };
        return Scan::Advance(2);
    }
    if matches!(kind, BlockKind::Doc) {
        doc.push(chars[at]);
    }
    Scan::Advance(1)
}

pub(super) fn bracket_step(c: char) -> isize {
    match c {
        '(' | '[' | '{' => 1,
        ')' | ']' | '}' => -1,
        _ => 0,
    }
}

/// The ()[]{} nesting depth after `c`; an unmatched closer leaves it at zero.
pub(super) fn nesting_after(depth: usize, c: char) -> usize {
    depth.saturating_add_signed(bracket_step(c))
}

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
