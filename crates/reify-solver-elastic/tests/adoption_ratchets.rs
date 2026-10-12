//! Adoption-ratchet host for reify-solver-elastic (PRD
//! `docs/prds/v0_6/solver-crates-layering.md` D20, §9 α, BT14).
//!
//! Each row counts the inline copies of a primitive that a later leaf extracts,
//! and the count must equal the row's committed baseline: above it is a re-added
//! copy, below it means the migrating leaf has to lower the baseline.

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One source line in two views: `code` has comments removed and string and
/// char literal contents blanked; `doc` is the text of a `///` or `//!` line.
struct Line {
    raw: String,
    code: String,
    doc: Option<String>,
}

/// What an unfinished construct at the end of a line carries into the next.
#[derive(Clone, Copy)]
enum Carry {
    Code,
    Str,
    RawStr { hashes: usize },
    Block { depth: usize },
}

enum Scan {
    Advance(usize),
    LineComment { doc: Option<String> },
}

fn lex(text: &str) -> Vec<Line> {
    let mut carry = Carry::Code;
    text.lines()
        .map(|raw| lex_line(&mut carry, raw))
        .collect()
}

fn lex_line(carry: &mut Carry, raw: &str) -> Line {
    let chars: Vec<char> = raw.chars().collect();
    let mut code = String::new();
    let mut doc = None;
    let mut at = 0;
    while at < chars.len() {
        let scan = match *carry {
            Carry::Code => scan_code(carry, &chars, at, &mut code),
            Carry::Str => scan_str(carry, &chars, at, &mut code),
            Carry::RawStr { hashes } => scan_raw_str(carry, hashes, &chars, at, &mut code),
            Carry::Block { depth } => scan_block(carry, depth, &chars, at),
        };
        match scan {
            Scan::Advance(width) => at += width,
            Scan::LineComment { doc: text } => {
                doc = text;
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
            doc: doc_text(&chars[at + 2..]),
        };
    }
    if starts_at(chars, at, "/*") {
        *carry = Carry::Block { depth: 1 };
        return Scan::Advance(2);
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

fn scan_block(carry: &mut Carry, depth: usize, chars: &[char], at: usize) -> Scan {
    if starts_at(chars, at, "/*") {
        *carry = Carry::Block { depth: depth + 1 };
        return Scan::Advance(2);
    }
    if starts_at(chars, at, "*/") {
        *carry = match depth {
            1 => Carry::Code,
            _ => Carry::Block { depth: depth - 1 },
        };
        return Scan::Advance(2);
    }
    Scan::Advance(1)
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

// ── item spans ───────────────────────────────────────────────────────────────

const TEST_ITEMS: &[&str] = &["#[cfg(test)]"];

/// An item being skipped: the nesting depth it began at, and whether its body
/// brace has opened yet (an item without one ends at a `;` at that depth).
struct CurrentItem {
    start_depth: usize,
    opened: bool,
}

struct ItemTracker {
    depth: usize,
    current: Option<CurrentItem>,
}

impl ItemTracker {
    fn begin_item(&mut self) {
        self.current = Some(CurrentItem {
            start_depth: self.depth,
            opened: false,
        });
    }

    fn advance(&mut self, c: char) {
        let depth = self.depth;
        match c {
            '{' => {
                if let Some(item) = self.current.as_mut().filter(|item| item.start_depth == depth) {
                    item.opened = true;
                }
                self.depth += 1;
            }
            '(' | '[' => self.depth += 1,
            '}' | ')' | ']' => {
                self.depth = depth.saturating_sub(1);
                let ended = self.current.as_ref().is_some_and(|item| {
                    item.opened && item.start_depth == self.depth
                });
                if ended {
                    self.current = None;
                }
            }
            ';' => {
                let ended = self
                    .current
                    .as_ref()
                    .is_some_and(|item| !item.opened && item.start_depth == depth);
                if ended {
                    self.current = None;
                }
            }
            _ => {}
        }
    }
}

/// Whether `code` begins an item named by `prefix`; a prefix ending in an
/// identifier character must not be the start of a longer identifier.
fn starts_item(code: &str, prefix: &str) -> bool {
    let Some(rest) = code.trim_start().strip_prefix(prefix) else {
        return false;
    };
    let ends_in_ident = prefix.chars().last().is_some_and(is_ident_char);
    let continues_ident = rest.chars().next().is_some_and(is_ident_char);
    !(ends_in_ident && continues_ident)
}

/// Marks every line of each item whose first code line starts with one of
/// `item_prefixes`, through its closing brace (or its `;` for a braceless item).
fn skipped_lines(lines: &[Line], item_prefixes: &[&str]) -> Vec<bool> {
    let mut tracker = ItemTracker {
        depth: 0,
        current: None,
    };
    lines
        .iter()
        .map(|line| {
            let starts_listed_item = item_prefixes
                .iter()
                .any(|prefix| starts_item(&line.code, prefix));
            if tracker.current.is_none() && starts_listed_item {
                tracker.begin_item();
            }
            let skipped = tracker.current.is_some();
            line.code.chars().for_each(|c| tracker.advance(c));
            skipped
        })
        .collect()
}

fn skipped_indices(source: &[&str], item_prefixes: &[&str]) -> Vec<usize> {
    let lines = lex(&source.join("\n"));
    skipped_lines(&lines, item_prefixes)
        .iter()
        .enumerate()
        .filter_map(|(index, &skipped)| skipped.then_some(index))
        .collect()
}

#[test]
fn cfg_test_gated_single_item_skips_only_that_item() {
    let skipped = skipped_indices(
        &[
            "#[cfg(test)]",
            "fn helper(coord: Coord) -> [f64; 2] {",
            "    let mut b = [0.0; 2];",
            "    for k in 0..2 {",
            "        b[k] = coord.get(k);",
            "    }",
            "    b",
            "}",
            "pub fn later() {",
            "    body();",
            "}",
        ],
        TEST_ITEMS,
    );
    assert_eq!(skipped, (0..=7).collect::<Vec<_>>());
}

#[test]
fn cfg_test_out_of_line_mod_declaration_skips_one_item() {
    let skipped = skipped_indices(
        &["#[cfg(test)]", "mod tests;", "pub fn prod() {}"],
        TEST_ITEMS,
    );
    assert_eq!(skipped, [0, 1]);
}

#[test]
fn cfg_test_mod_with_stacked_attributes_skips_to_its_closing_brace() {
    let skipped = skipped_indices(
        &[
            "#[cfg(test)]",
            "#[allow(clippy::x)]",
            "mod tests {",
            "    fn a() { if x { } }",
            "}",
            "fn after() {}",
        ],
        TEST_ITEMS,
    );
    assert_eq!(skipped, [0, 1, 2, 3, 4]);
}

#[test]
fn semicolon_inside_brackets_does_not_end_a_gated_item() {
    let skipped = skipped_indices(
        &[
            "#[cfg(test)]",
            "fn f(a: [f64; 3]) {",
            "    body();",
            "}",
            "fn prod() {}",
        ],
        TEST_ITEMS,
    );
    assert_eq!(skipped, [0, 1, 2, 3]);
}

#[test]
fn named_impl_block_is_skipped_but_not_a_longer_name() {
    let skipped = skipped_indices(
        &[
            "impl MpcRow {",
            "    fn new() -> Self { MpcRow { dofs: vec![] } }",
            "}",
            "impl MpcRowBuilder {",
            "    fn build() {}",
            "}",
        ],
        &["impl MpcRow"],
    );
    assert_eq!(skipped, [0, 1, 2]);
}

#[test]
fn braces_inside_strings_and_comments_do_not_shift_depth() {
    let skipped = skipped_indices(
        &[
            "#[cfg(test)]",
            "fn gated() {",
            r#"    let s = "}";"#,
            "    // }",
            "    let c = '}';",
            "    body();",
            "}",
            "fn prod() {}",
        ],
        TEST_ITEMS,
    );
    assert_eq!(skipped, [0, 1, 2, 3, 4, 5, 6]);
}
