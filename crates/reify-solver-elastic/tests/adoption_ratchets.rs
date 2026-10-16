//! Adoption-ratchet host for reify-solver-elastic (PRD
//! `docs/prds/v0_6/solver-crates-layering.md` D20, §9 α, BT14).
//!
//! Each row counts the inline copies of a primitive that a later leaf extracts,
//! and the count must equal the row's committed baseline: above it is a re-added
//! copy, below it means the migrating leaf has to lower the baseline.
//!
//! These rows are not `tests/infra/adoption-ratchet.manifest` rows: that engine
//! reds only above a ceiling, so a migrated copy would go unrecorded, and it
//! matches line by line, so it cannot skip `#[cfg(test)]` items or string contents.

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One source line in two views: `code` has comments removed and string and
/// char literal contents blanked; `doc` is the text of the doc comments on it,
/// empty when there are none.
struct Line {
    raw: String,
    code: String,
    doc: String,
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

/// The lines of `text` together with the lexer's state at the end of the text.
fn lex_to_end(text: &str) -> (Vec<Line>, Carry) {
    let mut carry = Carry::Code;
    let lines = text.lines().map(|raw| lex_line(&mut carry, raw)).collect();
    (lines, carry)
}

fn lex(text: &str) -> Vec<Line> {
    lex_to_end(text).0
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
    let docs: Vec<&str> = lines.iter().map(|line| line.doc.as_str()).collect();
    assert_eq!(docs, [" x", " y", "", ""]);
    for line in &lines {
        assert_eq!(line.code.trim(), "", "{:?}", line.raw);
    }
}

#[test]
fn doc_view_captures_block_doc_comments_on_every_line_they_span() {
    let lines = lex(&["/** a", " * b /* c */ d", " */ fn f() {} /*! e */"].join("\n"));
    let docs: Vec<&str> = lines.iter().map(|line| line.doc.as_str()).collect();
    assert_eq!(docs, [" a", " * b  c  d", "  e "]);
    let code: Vec<&str> = lines.iter().map(|line| line.code.trim()).collect();
    assert_eq!(code, ["", "", "fn f() {}"]);
}

#[test]
fn plain_block_comments_are_not_doc_text() {
    let lines = lex(&["/*** x */", "/**/ fn g() {}", "/* y */"].join("\n"));
    for line in &lines {
        assert_eq!(line.doc, "", "{:?}", line.raw);
    }
    assert_eq!(lines[1].code.trim(), "fn g() {}");
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

fn bracket_step(c: char) -> isize {
    match c {
        '(' | '[' | '{' => 1,
        ')' | ']' | '}' => -1,
        _ => 0,
    }
}

/// The ()[]{} nesting depth after `c`; an unmatched closer leaves it at zero.
fn nesting_after(depth: usize, c: char) -> usize {
    depth.saturating_add_signed(bracket_step(c))
}

impl ItemTracker {
    fn begin_item(&mut self) {
        self.current = Some(CurrentItem {
            start_depth: self.depth,
            opened: false,
        });
    }

    fn end_item_when(&mut self, ends: impl Fn(&CurrentItem) -> bool) {
        if self.current.as_ref().is_some_and(ends) {
            self.current = None;
        }
    }

    fn advance(&mut self, c: char) {
        let before = self.depth;
        let after = nesting_after(before, c);
        self.depth = after;
        match c {
            '{' => {
                if let Some(item) = self
                    .current
                    .as_mut()
                    .filter(|item| item.start_depth == before)
                {
                    item.opened = true;
                }
            }
            '}' | ')' | ']' => {
                self.end_item_when(|item| item.opened && item.start_depth == after);
            }
            ';' => self.end_item_when(|item| !item.opened && item.start_depth == before),
            _ => {}
        }
    }
}

/// Whether `code` begins an item named by `prefix`: the prefix opens the item or
/// is one of the attributes in front of it (`#[allow(dead_code)] #[cfg(test)] mod t {`).
fn starts_item(code: &str, prefix: &str) -> bool {
    let (attributes, item) = split_attributes(code);
    attributes
        .into_iter()
        .chain([item])
        .any(|part| starts_with_name(part, prefix))
}

/// Whether `text` starts with `prefix`; a prefix ending in an identifier
/// character must not be the start of a longer identifier.
fn starts_with_name(text: &str, prefix: &str) -> bool {
    let Some(rest) = text.strip_prefix(prefix) else {
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
fn cfg_test_behind_another_attribute_on_its_line_skips_its_item() {
    let skipped = skipped_indices(
        &[
            "#[allow(dead_code)] #[cfg(test)] mod t {",
            "    fn a() { if x { } }",
            "}",
            "fn after() {}",
        ],
        TEST_ITEMS,
    );
    assert_eq!(skipped, [0, 1, 2]);
}

#[test]
fn named_item_behind_an_attribute_on_its_line_is_skipped_but_not_a_longer_name() {
    let skipped = skipped_indices(
        &[
            "#[derive(Debug)] pub struct MpcRow {",
            "    dofs: Vec<usize>,",
            "}",
            "#[derive(Debug)] pub struct MpcRowBuilder {",
            "}",
        ],
        &["pub struct MpcRow"],
    );
    assert_eq!(skipped, [0, 1, 2]);
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

// ── ratchet rows ─────────────────────────────────────────────────────────────

/// A way a file can be shown to be one the matcher cannot follow: compilable
/// Rust ends outside every literal and comment with ()[]{} nested to zero, the
/// item tracker models one test gate only, the bare `#[cfg(test)]`, and a test
/// item's body is in the same file.
#[derive(Debug, PartialEq)]
enum LexDefect {
    EndsInsideLiteralOrComment,
    UnbalancedBrackets,
    UnsupportedTestCfg { line: usize },
    OutOfLineTestModule { line: usize },
}

struct SourceFile {
    path: String,
    lines: Vec<Line>,
    defect: Option<LexDefect>,
}

fn lex_defect(lines: &[Line], end: Carry) -> Option<LexDefect> {
    if !matches!(end, Carry::Code) {
        return Some(LexDefect::EndsInsideLiteralOrComment);
    }
    let mut depth: isize = 0;
    for c in lines.iter().flat_map(|line| line.code.chars()) {
        depth += bracket_step(c);
        if depth < 0 {
            break;
        }
    }
    if depth != 0 {
        return Some(LexDefect::UnbalancedBrackets);
    }
    unsupported_test_cfg_line(lines)
        .map(|line| LexDefect::UnsupportedTestCfg { line })
        .or_else(|| {
            out_of_line_test_module_line(lines).map(|line| LexDefect::OutOfLineTestModule { line })
        })
}

/// The 1-based line of the first `mod name;` inside a `TEST_ITEMS` item. Its body
/// is another file, which every production row would read as production code.
fn out_of_line_test_module_line(lines: &[Line]) -> Option<usize> {
    let in_test_item = skipped_lines(lines, TEST_ITEMS);
    lines
        .iter()
        .zip(in_test_item)
        .position(|(line, inside)| inside && declares_out_of_line_module(&line.code))
        .map(|index| index + 1)
}

/// Whether `code` declares a module without a body: `mod name;`.
fn declares_out_of_line_module(code: &str) -> bool {
    let (_, item) = split_attributes(code);
    after_item_keyword(item, "mod").is_some_and(|after_keyword| {
        let after_name = &after_keyword[leading_identifier(after_keyword).len()..];
        after_name.trim_start().starts_with(';')
    })
}

/// The 1-based line of the first `cfg(..)` attribute naming `test` that is not
/// one of the `TEST_ITEMS` gates. The tracker cannot evaluate `all(test, ..)`,
/// `any(test, ..)` or `not(test)`, and an inner `#![cfg(test)]` gates a whole file.
fn unsupported_test_cfg_line(lines: &[Line]) -> Option<usize> {
    let joined = lines
        .iter()
        .map(|line| line.code.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let code = TEST_ITEMS
        .iter()
        .fold(joined, |code, gate| code.replace(gate, ""));
    code.match_indices("cfg")
        .filter(|&(at, _)| opens_cfg_attribute(&code, at))
        .find(|&(at, _)| arguments_name_test(&code[at + "cfg".len()..]))
        .map(|(at, _)| code[..at].matches('\n').count() + 1)
}

/// Whether the `cfg` at `at` is the whole name of an attribute: `#[cfg`, `#![cfg`.
fn opens_cfg_attribute(code: &str, at: usize) -> bool {
    let before = code[..at].trim_end();
    let name_continues = code[at + "cfg".len()..]
        .chars()
        .next()
        .is_some_and(is_ident_char);
    (before.ends_with("#[") || before.ends_with("#![")) && !name_continues
}

/// Whether the parenthesised arguments that open `after_name` contain the word `test`.
fn arguments_name_test(after_name: &str) -> bool {
    let Some(inside) = after_name.trim_start().strip_prefix('(') else {
        return false;
    };
    group_body(inside)
        .split(|c| !is_ident_char(c))
        .any(|word| word == "test")
}

/// The text up to the bracket that closes the group whose opener was just consumed.
fn group_body(after_opener: &str) -> &str {
    let mut depth = 1;
    for (offset, c) in after_opener.char_indices() {
        depth = nesting_after(depth, c);
        if depth == 0 {
            return &after_opener[..offset];
        }
    }
    after_opener
}

fn source_file(path: &str, text: &str) -> SourceFile {
    let (lines, end) = lex_to_end(text);
    let defect = lex_defect(&lines, end);
    SourceFile {
        path: path.to_string(),
        lines,
        defect,
    }
}

struct Hit {
    path: String,
    line: usize,
    text: String,
}

fn hit_lines(hits: &[Hit]) -> String {
    hits.iter()
        .map(|hit| format!("  {}:{}: {}", hit.path, hit.line, hit.text))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy)]
enum View {
    Code,
    CodeAndDocs,
}

/// A literal needle counted in the files under `include` (path prefixes; a
/// directory ends in `/`) minus `exclude`, outside the `skip_items` items.
#[derive(Clone, Copy)]
struct Pattern {
    needle: &'static str,
    include: &'static [&'static str],
    exclude: &'static [&'static str],
    skip_items: &'static [&'static str],
    view: View,
    /// One inline copy of the needle on a single quote-free line.
    seed: &'static str,
}

#[derive(Clone, Copy)]
enum Measure {
    Occurrences(Pattern),
    /// Helper fns defined in several files under `root`; a definition under
    /// `shared_dir` is the copy the other files duplicate.
    DuplicateTopLevelFns {
        root: &'static str,
        shared_dir: &'static str,
    },
}

struct Ratchet {
    name: &'static str,
    /// Where copies belong instead: the PRD decision and the leaf that lowers the row.
    home: &'static str,
    baseline: usize,
    measure: Measure,
}

const SRC: &[&str] = &["src/"];
const SRC_AND_TESTS: &[&str] = &["src/", "tests/"];
const NO_PATHS: &[&str] = &[];
const WHOLE_FILE: &[&str] = &[];

/// A pattern over production code: `#[cfg(test)]` items are skipped, so test
/// modules never count. No path is excluded and only the code view is read.
const fn production_pattern(
    needle: &'static str,
    include: &'static [&'static str],
    seed: &'static str,
) -> Pattern {
    Pattern {
        needle,
        include,
        exclude: NO_PATHS,
        skip_items: TEST_ITEMS,
        view: View::Code,
        seed,
    }
}

/// A pattern over whole files: no item is skipped, so `#[cfg(test)]` modules
/// count along with every other line under `include`.
const fn whole_file_pattern(
    needle: &'static str,
    include: &'static [&'static str],
    seed: &'static str,
) -> Pattern {
    Pattern {
        skip_items: WHOLE_FILE,
        ..production_pattern(needle, include, seed)
    }
}

const RATCHETS: &[Ratchet] = &[
    Ratchet {
        name: "shell-assembly-upward-imports",
        home: "elements/shell_frame.rs (solver-crates-layering D4, leaf β)",
        baseline: 0,
        measure: Measure::Occurrences(production_pattern(
            "shell_assembly::",
            &[
                "src/elements/",
                "src/geometric_stiffness/",
                "src/shell_kinematics.rs",
                "src/membrane_load.rs",
            ],
            "use crate::shell_assembly::plane_stress_d;",
        )),
    },
    Ratchet {
        name: "bubble-condensation-copies",
        home: "the one guarded bubble-condensation fn (solver-crates-layering D6, leaf γ)",
        baseline: 2,
        measure: Measure::Occurrences(production_pattern(
            "let det_bb =",
            SRC,
            "let det_bb = k00 * k11 - k01 * k10;",
        )),
    },
    Ratchet {
        name: "shear-correction-literal",
        home: "ShellSection::SHEAR_CORRECTION (solver-crates-layering D5, leaf γ)",
        baseline: 2,
        measure: Measure::Occurrences(production_pattern(
            "5.0 / 6.0",
            SRC,
            "const KAPPA: f64 = 5.0 / 6.0;",
        )),
    },
    Ratchet {
        name: "inline-shear-modulus",
        home: "IsotropicElastic::shear_modulus (solver-crates-layering D5, leaf γ)",
        baseline: 16,
        measure: Measure::Occurrences(whole_file_pattern(
            "/ (2.0 * (1.0 + nu))",
            SRC,
            "let g = e / (2.0 * (1.0 + nu));",
        )),
    },
    Ratchet {
        name: "thickness-scaled-d-blocks",
        home: "the shared scaled(d, s) helper (solver-crates-layering D6, leaf γ)",
        baseline: 5,
        measure: Measure::Occurrences(production_pattern(
            "td[i][j] = ",
            SRC,
            "td[i][j] = t * d[i][j];",
        )),
    },
    Ratchet {
        name: "assembly-element-literals",
        home: "AssemblyElement::new (solver-crates-layering D7, leaf δ)",
        baseline: 39,
        measure: Measure::Occurrences(Pattern {
            exclude: &["src/assembly/global.rs"],
            ..whole_file_pattern(
                "AssemblyElement {",
                SRC_AND_TESTS,
                "let el = AssemblyElement { id: 0, connectivity: &c, k_e: &k };",
            )
        }),
    },
    Ratchet {
        name: "element-stiffness-data-indexing",
        home: "the ElementStiffness accessors (solver-crates-layering D8, leaf ε)",
        baseline: 74,
        measure: Measure::Occurrences(Pattern {
            exclude: &["src/assembly/mod.rs", "src/resample.rs"],
            ..whole_file_pattern(".data[", SRC_AND_TESTS, "let v = k_e.data[0];")
        }),
    },
    Ratchet {
        name: "member-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(production_pattern(
            "fn member_force_delta(",
            SRC,
            "fn member_force_delta(x: f64) -> f64 { x }",
        )),
    },
    Ratchet {
        name: "bar-axial-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(production_pattern(
            "fn bar_axial_force_delta(",
            SRC,
            "fn bar_axial_force_delta(x: f64) -> f64 { x }",
        )),
    },
    Ratchet {
        name: "grounding-diagonal-definitions",
        home: "the membrane_load constant shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(production_pattern(
            "const GROUNDING_DIAGONAL:",
            SRC,
            "const GROUNDING_DIAGONAL: f64 = 1.0;",
        )),
    },
    Ratchet {
        name: "cable-sign-rule-arms",
        home: "MemberKind::admits (solver-crates-layering D1, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(production_pattern(
            "MemberKind::Cable =>",
            SRC,
            "MemberKind::Cable => qi > 0.0,",
        )),
    },
    Ratchet {
        name: "unchecked-anchor-partition",
        home: "ForceDensityProblem construction (solver-crates-layering D1, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(production_pattern(
            "is_anchor[a] = true",
            SRC,
            "is_anchor[a] = true;",
        )),
    },
    Ratchet {
        name: "member-force-loops",
        home: "member_axial_forces in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(production_pattern(
            "qi * len",
            SRC,
            "let n_i = qi * len;",
        )),
    },
    Ratchet {
        name: "surface-stencil-scatters",
        home: "scatter_surface_stencils in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(production_pattern(
            "(idx[a], idx[b])] +=",
            SRC,
            "d[(idx[a], idx[b])] += l[a][b];",
        )),
    },
    Ratchet {
        name: "line-rank1-scatters",
        home: "scatter_line_members in form_find.rs (solver-crates-layering D3, leaf η)",
        baseline: 1,
        measure: Measure::Occurrences(production_pattern(
            "[(j, k)] -= qi",
            SRC,
            "d[(j, k)] -= qi;",
        )),
    },
    Ratchet {
        name: "mpc-row-literals",
        home: "MpcRow::new (solver-crates-layering D10, leaf θ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            skip_items: &["impl MpcRow", "pub struct MpcRow"],
            view: View::CodeAndDocs,
            ..whole_file_pattern(
                "MpcRow {",
                SRC_AND_TESTS,
                "let r = MpcRow { dofs: vec![0], coeffs: vec![1.0], rhs: 0.0 };",
            )
        }),
    },
    Ratchet {
        name: "duplicated-test-helpers",
        home: "tests/common/fea_harness.rs (solver-crates-layering D11, leaf ι)",
        baseline: 84,
        measure: Measure::DuplicateTopLevelFns {
            root: "tests/",
            shared_dir: "tests/common/",
        },
    },
];

fn in_scope(path: &str, include: &[&str], exclude: &[&str]) -> bool {
    let under = |prefix: &&str| path.starts_with(prefix);
    include.iter().any(under) && !exclude.iter().any(under)
}

/// Non-overlapping occurrences of `needle` in `text`; a needle starting with an
/// identifier character must not continue a longer identifier on its left.
fn occurrences_in(text: &str, needle: &str) -> usize {
    let needs_left_boundary = needle.starts_with(is_ident_char);
    text.match_indices(needle)
        .filter(|&(at, _)| {
            let continues_identifier = text[..at].chars().next_back().is_some_and(is_ident_char);
            !(needs_left_boundary && continues_identifier)
        })
        .count()
}

fn counted_occurrences(pattern: &Pattern, line: &Line) -> usize {
    let in_code = occurrences_in(&line.code, pattern.needle);
    let in_docs = match pattern.view {
        View::Code => 0,
        View::CodeAndDocs => occurrences_in(&line.doc, pattern.needle),
    };
    in_code + in_docs
}

fn hits_in_file(pattern: &Pattern, file: &SourceFile) -> Vec<Hit> {
    let skipped = skipped_lines(&file.lines, pattern.skip_items);
    let mut hits = Vec::new();
    for (index, line) in file.lines.iter().enumerate() {
        if skipped[index] {
            continue;
        }
        for _ in 0..counted_occurrences(pattern, line) {
            hits.push(Hit {
                path: file.path.clone(),
                line: index + 1,
                text: line.raw.trim().to_string(),
            });
        }
    }
    hits
}

fn count_occurrences(pattern: &Pattern, files: &[SourceFile]) -> Vec<Hit> {
    files
        .iter()
        .filter(|file| in_scope(&file.path, pattern.include, pattern.exclude))
        .flat_map(|file| hits_in_file(pattern, file))
        .collect()
}

const TEST_ATTRIBUTE: &str = "#[test]";

/// Splits the leading `#[..]` attributes off `code`; an attribute that does not
/// close on this line takes the rest of it.
fn split_attributes(code: &str) -> (Vec<&str>, &str) {
    let mut attributes = Vec::new();
    let mut rest = code.trim_start();
    while rest.starts_with("#[") {
        let end = attribute_end(rest).unwrap_or(rest.len());
        attributes.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    (attributes, rest)
}

/// The byte offset just past the `]` closing the attribute that `text` opens.
fn attribute_end(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (offset, c) in text.char_indices().skip(1) {
        depth = nesting_after(depth, c);
        if depth == 0 {
            return Some(offset + c.len_utf8());
        }
    }
    None
}

/// The identifier at the start of `text`; empty when it starts with anything else.
fn leading_identifier(text: &str) -> &str {
    &text[..text.find(|c| !is_ident_char(c)).unwrap_or(text.len())]
}

/// The text after the `keyword` that `code` declares, past any `pub`, `pub(..)`,
/// `const`, `async`, `unsafe` and `extern ".."` modifiers (string contents are blanked).
fn after_item_keyword<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
    let mut rest = code.trim_start();
    loop {
        let word = leading_identifier(rest);
        rest = rest[word.len()..].trim_start();
        match word {
            _ if word == keyword => return Some(rest),
            "pub" => rest = skip_visibility_scope(rest),
            "extern" => rest = rest.strip_prefix("\"\"").unwrap_or(rest).trim_start(),
            "const" | "async" | "unsafe" => {}
            _ => return None,
        }
    }
}

/// The name of the fn that `code` declares.
fn fn_name(code: &str) -> Option<&str> {
    let name = leading_identifier(after_item_keyword(code, "fn")?);
    (!name.is_empty()).then_some(name)
}

/// `rest` without a leading `(crate)` / `(in path)` visibility scope.
fn skip_visibility_scope(rest: &str) -> &str {
    rest.strip_prefix('(')
        .and_then(|inner| inner.split_once(')'))
        .map_or(rest, |(_, after)| after.trim_start())
}

/// The helper fns defined at nesting depth zero as `(name, 1-based line)`: not
/// `#[test]` fns and not named `test_*`. Only lines that start at depth zero are
/// read, so multi-line attributes and signatures never look like definitions.
fn top_level_helpers(lines: &[Line]) -> Vec<(&str, usize)> {
    let mut helpers = Vec::new();
    let mut depth = 0;
    let mut test_attribute_pending = false;
    for (index, line) in lines.iter().enumerate() {
        let starts_at_top_level = depth == 0;
        depth = line.code.chars().fold(depth, nesting_after);
        if !starts_at_top_level || line.code.trim().is_empty() {
            continue;
        }
        let (attributes, rest) = split_attributes(&line.code);
        test_attribute_pending |= attributes.contains(&TEST_ATTRIBUTE);
        if rest.is_empty() {
            continue;
        }
        let helper =
            fn_name(rest).filter(|name| !test_attribute_pending && !name.starts_with("test_"));
        if let Some(name) = helper {
            helpers.push((name, index + 1));
        }
        test_attribute_pending = false;
    }
    helpers
}

/// One hit per helper copy beyond the kept one, over the files under `root`: a
/// name defined in several files is counted once per file. The kept copy is the
/// first under `shared_dir`, so a local redefinition of a shared helper is the
/// copy and never the shared one; with none, it is the first in path order.
/// Every hit names the kept copy.
fn duplicate_top_level_fns(root: &str, shared_dir: &str, files: &[SourceFile]) -> Vec<Hit> {
    let mut definitions: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for file in files.iter().filter(|file| file.path.starts_with(root)) {
        for (name, line) in top_level_helpers(&file.lines) {
            let defined_in = definitions.entry(name).or_default();
            defined_in.entry(file.path.as_str()).or_insert(line);
        }
    }
    let mut hits = Vec::new();
    for (name, defined_in) in &definitions {
        let mut copies: Vec<_> = defined_in.iter().collect();
        copies.sort_by_key(|(path, _)| Reverse(path.starts_with(shared_dir)));
        let Some((&(kept, _), extra_copies)) = copies.split_first() else {
            continue;
        };
        hits.extend(extra_copies.iter().map(|&(path, &line)| Hit {
            path: path.to_string(),
            line,
            text: format!("fn {name} (also in {kept})"),
        }));
    }
    hits
}

fn measure(how: &Measure, files: &[SourceFile]) -> Vec<Hit> {
    match how {
        Measure::Occurrences(pattern) => count_occurrences(pattern, files),
        Measure::DuplicateTopLevelFns { root, shared_dir } => {
            duplicate_top_level_fns(root, shared_dir, files)
        }
    }
}

/// Strict ratchet: the measured count must equal the row's `baseline`. Above it, a
/// copy was added; below it, a leaf migrated copies and must lower the row in the
/// same diff. A row's home does not exist until its leaf lands, so a genuinely new
/// copy that cannot use it yet raises the baseline instead.
fn check(ratchet: &Ratchet, files: &[SourceFile]) -> Result<(), String> {
    let hits = measure(&ratchet.measure, files);
    let (name, home, baseline) = (ratchet.name, ratchet.home, ratchet.baseline);
    let count = hits.len();
    match count.cmp(&baseline) {
        Ordering::Equal => Ok(()),
        Ordering::Greater => Err(format!(
            "{name}: {count} is above its baseline {baseline}: an inline copy was added; \
             route it through {home}\n\
             (if that home is not extracted yet, a genuinely new copy may raise the baseline \
             to {count} in {}, with the reason in the commit message)\n{}",
            file!(),
            hit_lines(&hits)
        )),
        Ordering::Less => Err(format!(
            "{name}: {count} is below its baseline {baseline}: \
             lower the baseline to {count} in {}",
            file!()
        )),
    }
}

// ── pattern-row self-tests ───────────────────────────────────────────────────

fn pattern_rows() -> Vec<(&'static Ratchet, &'static Pattern)> {
    RATCHETS
        .iter()
        .filter_map(|row| match &row.measure {
            Measure::Occurrences(pattern) => Some((row, pattern)),
            Measure::DuplicateTopLevelFns { .. } => None,
        })
        .collect()
}

/// Runs `verdict` on every row and fails once, naming every row that returned `Err`.
fn for_every_row(verdict: impl Fn(&Ratchet) -> Result<(), String>) {
    let failures: Vec<String> = RATCHETS
        .iter()
        .filter_map(|row| {
            verdict(row)
                .err()
                .map(|problem| format!("{}: {problem}", row.name))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

fn for_every_pattern_row(verdict: impl Fn(&Ratchet, &Pattern) -> Result<(), String>) {
    assert!(!pattern_rows().is_empty(), "no pattern rows to test");
    for_every_row(|row| match &row.measure {
        Measure::Occurrences(pattern) => verdict(row, pattern),
        Measure::DuplicateTopLevelFns { .. } => Ok(()),
    });
}

fn file_under(prefix_entry: &str) -> String {
    match prefix_entry.ends_with('/') {
        true => format!("{prefix_entry}seeded.rs"),
        false => prefix_entry.to_string(),
    }
}

fn seed_path(pattern: &Pattern) -> String {
    file_under(pattern.include[0])
}

fn out_of_scope_paths(pattern: &Pattern) -> Vec<String> {
    let excluded = pattern.exclude.iter().map(|entry| file_under(entry));
    let candidates = ["src/seeded.rs", "tests/seeded.rs", "benches/seeded.rs"]
        .map(String::from)
        .into_iter()
        .filter(|path| !in_scope(path, pattern.include, pattern.exclude));
    excluded.chain(candidates).collect()
}

fn skips_test_items(pattern: &Pattern) -> bool {
    TEST_ITEMS
        .iter()
        .all(|item| pattern.skip_items.contains(item))
}

fn hits_in(row: &Ratchet, path: &str, text: &str) -> Vec<Hit> {
    measure(&row.measure, &[source_file(path, text)])
}

fn expect_hit_count(hits: &[Hit], expected: usize) -> Result<(), String> {
    match hits.len() == expected {
        true => Ok(()),
        false => Err(format!(
            "expected {expected} hit(s), got {}:\n{}",
            hits.len(),
            hit_lines(hits)
        )),
    }
}

/// Hits for `pattern`'s seed in plain code (the control) followed by `tail`.
fn hits_after_control(row: &Ratchet, pattern: &Pattern, tail: &str) -> Vec<Hit> {
    let text = format!("{}\n{tail}", pattern.seed);
    hits_in(row, &seed_path(pattern), &text)
}

#[test]
fn every_pattern_row_seed_path_is_in_scope_and_seed_holds_one_needle() {
    for_every_pattern_row(|_, pattern| {
        let path = seed_path(pattern);
        if !in_scope(&path, pattern.include, pattern.exclude) {
            return Err(format!("seed path {path} is outside the row's scope"));
        }
        let needles = pattern.seed.matches(pattern.needle).count();
        if needles != 1 || pattern.seed.contains(['\n', '"']) {
            return Err(format!(
                "the seed must be one quote-free line holding the needle once, not {needles}"
            ));
        }
        Ok(())
    });
}

#[test]
fn every_pattern_row_flags_one_seeded_inline_copy() {
    for_every_pattern_row(|row, pattern| {
        let path = seed_path(pattern);
        let hits = hits_in(row, &path, pattern.seed);
        match hits.as_slice() {
            [hit] if hit.path == path && hit.line == 1 => Ok(()),
            _ => Err(format!(
                "expected one hit at {path}:1, got {}:\n{}",
                hits.len(),
                hit_lines(&hits)
            )),
        }
    });
}

#[test]
fn every_pattern_row_counts_each_occurrence_on_a_line() {
    for_every_pattern_row(|row, pattern| {
        let doubled = format!("{} {}", pattern.seed, pattern.seed);
        let hits = hits_in(row, &seed_path(pattern), &doubled);
        expect_hit_count(&hits, 2)
    });
}

#[test]
fn every_pattern_row_ignores_copies_in_non_counted_positions() {
    for_every_pattern_row(|row, pattern| {
        let seed = pattern.seed;
        let mut decoys = vec![
            format!("// {seed}"),
            format!("/* {seed} */"),
            format!("const _S: &str = \"\n{seed}\n\";"),
            format!("const _R: &str = r#\"{seed}\"#;"),
        ];
        for prefix in pattern
            .skip_items
            .iter()
            .filter(|p| !TEST_ITEMS.contains(p))
        {
            decoys.push(format!("{prefix} {{\n{seed}\n}}"));
        }
        for decoy in &decoys {
            expect_hit_count(&hits_after_control(row, pattern, decoy), 1)
                .map_err(|problem| format!("decoy {decoy:?}: {problem}"))?;
        }
        for path in out_of_scope_paths(pattern) {
            let files = [
                source_file(&path, seed),
                source_file(&seed_path(pattern), seed),
            ];
            expect_hit_count(&measure(&row.measure, &files), 1)
                .map_err(|problem| format!("out-of-scope {path}: {problem}"))?;
        }
        Ok(())
    });
}

#[test]
fn every_pattern_row_applies_its_region_to_cfg_test_modules() {
    for_every_pattern_row(|row, pattern| {
        let seed = pattern.seed;
        let gated_modules = [
            format!("#[cfg(test)]\nmod tests {{\n{seed}\n}}"),
            format!("#[allow(dead_code)] #[cfg(test)] mod tests {{\n{seed}\n}}"),
        ];
        let expected = if skips_test_items(pattern) { 1 } else { 2 };
        for gated in &gated_modules {
            expect_hit_count(&hits_after_control(row, pattern, gated), expected)
                .map_err(|problem| format!("gated module {gated:?}: {problem}"))?;
        }
        Ok(())
    });
}

#[test]
fn every_pattern_row_applies_its_view_to_doc_comments() {
    for_every_pattern_row(|row, pattern| {
        let seed = pattern.seed;
        let documented = [
            format!("/// {seed}"),
            format!("//! {seed}"),
            format!("/** {seed} */"),
            format!("/*! {seed} */"),
        ];
        let expected = match pattern.view {
            View::Code => 1,
            View::CodeAndDocs => 2,
        };
        for comment in &documented {
            expect_hit_count(&hits_after_control(row, pattern, comment), expected)
                .map_err(|problem| format!("doc comment {comment:?}: {problem}"))?;
        }
        Ok(())
    });
}

#[test]
fn needle_requires_a_left_identifier_boundary() {
    let boundary_rows = pattern_rows()
        .into_iter()
        .filter(|(_, pattern)| pattern.needle.starts_with(is_ident_char))
        .count();
    assert!(
        boundary_rows > 0,
        "no row starts its needle with an identifier character"
    );
    for_every_pattern_row(|row, pattern| {
        if !pattern.needle.starts_with(is_ident_char) {
            return Ok(());
        }
        let glued_needle = format!("x{}", pattern.needle);
        let glued = pattern.seed.replacen(pattern.needle, &glued_needle, 1);
        expect_hit_count(&hits_after_control(row, pattern, &glued), 1)
    });
}

// ── duplicate-helper census self-tests ───────────────────────────────────────

const CENSUS_ROOT: &str = "tests/";
const CENSUS_SHARED_DIR: &str = "tests/common/";
const SHARED_FILE: &str = "tests/common/mod.rs";

fn census_hits(files: &[(&str, &str)]) -> Vec<Hit> {
    let tree: Vec<SourceFile> = files
        .iter()
        .map(|&(path, text)| source_file(path, text))
        .collect();
    let census = Measure::DuplicateTopLevelFns {
        root: CENSUS_ROOT,
        shared_dir: CENSUS_SHARED_DIR,
    };
    measure(&census, &tree)
}

fn hit_texts(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.text.as_str()).collect()
}

#[test]
fn duplicate_census_flags_a_helper_defined_in_two_test_files() {
    let hits = census_hits(&[
        ("tests/a.rs", "fn seeded_helper() {}"),
        ("tests/b.rs", "fn seeded_helper() {}"),
    ]);
    assert_eq!(hit_texts(&hits), ["fn seeded_helper (also in tests/a.rs)"]);
    assert_eq!((hits[0].path.as_str(), hits[0].line), ("tests/b.rs", 1));
}

#[test]
fn duplicate_census_reports_one_hit_per_extra_copy_and_counts_files_not_definitions() {
    let three_files = census_hits(&[
        ("tests/a.rs", "fn h() {}"),
        ("tests/b.rs", "fn h() {}"),
        ("tests/c.rs", "fn h() {}"),
    ]);
    assert_eq!(hit_texts(&three_files), ["fn h (also in tests/a.rs)"; 2]);
    let one_file = census_hits(&[("tests/a.rs", "fn h() {}\nfn h() {}")]);
    assert_eq!(one_file.len(), 0);
}

#[test]
fn duplicate_census_reports_the_local_redefinition_not_the_shared_helper() {
    for local in ["tests/a.rs", "tests/z.rs"] {
        let hits = census_hits(&[
            (SHARED_FILE, "pub fn canonical_x() {}"),
            (local, "fn canonical_x() {}"),
        ]);
        assert_eq!(
            hit_texts(&hits),
            [format!("fn canonical_x (also in {SHARED_FILE})")],
            "local file: {local}"
        );
        assert_eq!((hits[0].path.as_str(), hits[0].line), (local, 1));
    }
}

#[test]
fn duplicate_census_keeps_the_shared_copy_and_reports_every_other_file() {
    let hits = census_hits(&[
        ("tests/z.rs", "fn canonical_x() {}"),
        (SHARED_FILE, "pub fn canonical_x() {}"),
        ("tests/a.rs", "fn canonical_x() {}"),
    ]);
    let reported: Vec<&str> = hits.iter().map(|hit| hit.path.as_str()).collect();
    assert_eq!(reported, ["tests/a.rs", "tests/z.rs"]);
    let names_shared_copy = format!("fn canonical_x (also in {SHARED_FILE})");
    assert_eq!(hit_texts(&hits), [names_shared_copy.as_str(); 2]);
}

#[test]
fn duplicate_census_keeps_the_first_of_several_shared_copies() {
    let hits = census_hits(&[
        (SHARED_FILE, "pub fn canonical_x() {}"),
        ("tests/common/extra.rs", "pub fn canonical_x() {}"),
        ("tests/a.rs", "fn canonical_x() {}"),
    ]);
    let mut reported: Vec<&str> = hits.iter().map(|hit| hit.path.as_str()).collect();
    reported.sort();
    assert_eq!(reported, ["tests/a.rs", SHARED_FILE]);
    assert_eq!(
        hit_texts(&hits),
        ["fn canonical_x (also in tests/common/extra.rs)"; 2]
    );
}

#[test]
fn duplicate_census_ignores_non_helper_definitions() {
    let decoys = [
        ("test attribute", "#[test]\nfn same_name() {}"),
        ("test attribute on one line", "#[test] fn same_name() {}"),
        ("test_ prefix", "fn test_same() {}"),
        (
            "attribute stack",
            "#[test]\n#[should_panic]\nfn same_name() {}",
        ),
        (
            "doc comment after the test attribute",
            "#[test]\n/// documented\nfn same_name() {}",
        ),
        (
            "multi-line cfg_attr after the test attribute",
            "#[test]\n#[cfg_attr(\n    debug_assertions,\n    ignore = \"slow\"\n)]\nfn same_name() {}",
        ),
        (
            "multi-line ignore string after the test attribute",
            "#[test]\n#[ignore = \"first \\\n    second\"]\nfn same_name() {}",
        ),
        ("nested in a mod", "mod m {\n    fn same_name() {}\n}"),
        (
            "method in an impl",
            "impl T {\n    fn same_name(&self) {}\n}",
        ),
        (
            "inside a string",
            "const S: &str = \"\nfn same_name() {}\n\";",
        ),
        (
            "inside comments",
            "// fn same_name() {}\n/* fn same_name() {} */",
        ),
    ];
    for (label, decoy) in decoys {
        let text = format!("fn control_helper() {{}}\n{decoy}");
        let hits = census_hits(&[("tests/a.rs", &text), ("tests/b.rs", &text)]);
        assert_eq!(
            hit_texts(&hits),
            ["fn control_helper (also in tests/a.rs)"],
            "decoy: {label}"
        );
    }
}

#[test]
fn duplicate_census_ignores_single_definitions_and_files_outside_its_root() {
    let hits = census_hits(&[
        (
            "tests/a.rs",
            "fn control_helper() {}\nfn solo() {}\nfn out_of_root() {}",
        ),
        ("tests/b.rs", "fn control_helper() {}"),
        ("src/c.rs", "fn out_of_root() {}"),
    ]);
    assert_eq!(hit_texts(&hits), ["fn control_helper (also in tests/a.rs)"]);
}

#[test]
fn duplicate_census_counts_every_fn_modifier_form() {
    let signatures = [
        "pub fn modded() {}",
        "pub(crate) fn modded() {}",
        "const fn modded() {}",
        "async fn modded() {}",
        "unsafe fn modded() {}",
        "pub(crate) const unsafe fn modded() {}",
        r#"extern "C" fn modded() {}"#,
        "pub fn modded(\n    a: u32,\n    callback: fn(u32) -> u32,\n) -> u32 {\n    a\n}",
    ];
    for signature in signatures {
        let hits = census_hits(&[("tests/a.rs", signature), ("tests/b.rs", signature)]);
        assert_eq!(
            hit_texts(&hits),
            ["fn modded (also in tests/a.rs)"],
            "signature: {signature}"
        );
    }
}

// ── BT14: every row is red for a re-added copy and for an unlowered baseline ─

fn seeded_tree(row: &Ratchet, copies: usize) -> Vec<SourceFile> {
    match &row.measure {
        Measure::Occurrences(pattern) => {
            let text = vec![pattern.seed; copies].join("\n");
            vec![source_file(&seed_path(pattern), &text)]
        }
        Measure::DuplicateTopLevelFns { root, .. } => (0..=copies)
            .map(|index| source_file(&format!("{root}seeded_{index}.rs"), "fn seeded_helper() {}"))
            .collect(),
    }
}

fn expect_red_containing(verdict: Result<(), String>, fragments: &[&str]) -> Result<(), String> {
    match verdict {
        Err(message) if fragments.iter().all(|fragment| message.contains(fragment)) => Ok(()),
        other => Err(format!(
            "expected a red verdict containing {fragments:?}, got {other:?}"
        )),
    }
}

#[test]
fn migrating_one_copy_without_lowering_the_baseline_is_red_for_every_row() {
    for_every_row(|row| {
        let at_two = Ratchet {
            baseline: 2,
            ..*row
        };
        check(&at_two, &seeded_tree(row, 2))
            .map_err(|red| format!("two copies at baseline 2 must hold, got: {red}"))?;
        let above = check(&at_two, &seeded_tree(row, 3));
        expect_red_containing(
            above,
            &[
                "above its baseline",
                row.name,
                row.home,
                "raise the baseline to 3",
            ],
        )?;
        let below = check(&at_two, &seeded_tree(row, 1));
        expect_red_containing(below, &["lower the baseline to 1"])
    });
}

// ── lexer health ─────────────────────────────────────────────────────────────

#[test]
fn a_file_the_lexer_cannot_follow_is_reported_as_defective() {
    let defect_of = |text: &str| source_file("src/probe.rs", text).defect;
    assert_eq!(
        defect_of("let s = \"never closed;\nfn f() {}"),
        Some(LexDefect::EndsInsideLiteralOrComment)
    );
    assert_eq!(
        defect_of("fn f() {}\n/* never closed"),
        Some(LexDefect::EndsInsideLiteralOrComment)
    );
    assert_eq!(
        defect_of("fn f() {\n    let x = (1;\n}"),
        Some(LexDefect::UnbalancedBrackets)
    );
    assert_eq!(
        defect_of("fn f() {}\n}"),
        Some(LexDefect::UnbalancedBrackets)
    );
    assert_eq!(
        defect_of("fn f() { let s = \"}\"; let c = '{'; /* ) */ }"),
        None
    );
}

#[test]
fn a_test_gate_the_item_tracker_does_not_model_is_reported_as_defective() {
    let defect_of = |text: &str| source_file("src/probe.rs", text).defect;
    let unsupported_gates = [
        "#[cfg(all(test, unix))]",
        "#[cfg(any(test, unix))]",
        "#[cfg(not(test))]",
        "#![cfg(test)]",
        "#[cfg( test )]",
        "#[cfg (test)]",
        "#[cfg(\n    test\n)]",
    ];
    for gate in unsupported_gates {
        let text = format!("fn before() {{}}\n{gate}\nmod gated {{}}");
        assert_eq!(
            defect_of(&text),
            Some(LexDefect::UnsupportedTestCfg { line: 2 }),
            "gate: {gate:?}"
        );
    }
    let supported_or_unrelated = [
        "#[cfg(test)]\nmod tests {}",
        "#[cfg(test)]\n#[allow(dead_code)]\nfn helper() {}",
        "#[cfg(feature = \"test\")]\nfn f() {}",
        "#[cfg(test_support)]\nfn f() {}",
        "#[cfg(unix)]\nfn f() {}",
        "#[cfg_attr(test, derive(Debug))]\nstruct S;",
        "fn f() -> bool { cfg!(test) }",
        "fn cfg(test: u32) {}",
        "// #[cfg(not(test))]\nfn f() {}",
        "const S: &str = \"#[cfg(not(test))]\";",
    ];
    for text in supported_or_unrelated {
        assert_eq!(defect_of(text), None, "text: {text:?}");
    }
}

#[test]
fn a_module_declared_out_of_line_inside_a_test_item_is_reported_as_defective() {
    let defect_of = |text: &str| source_file("src/probe.rs", text).defect;
    // Each module follows a one-line `fn before() {}`; the number is the line of its `mod`.
    let out_of_line_modules = [
        ("#[cfg(test)]\nmod tests;", 3),
        ("#[cfg(test)] mod tests;", 2),
        ("#[cfg(test)]\npub(crate) mod tests;", 3),
        ("#[cfg(test)]\n#[path = \"other.rs\"]\nmod tests;", 4),
        ("#[cfg(test)]\n/// documented\nmod tests;", 4),
        ("#[allow(dead_code)] #[cfg(test)] mod tests;", 2),
        ("mod outer {\n    #[cfg(test)]\n    mod tests;\n}", 4),
        ("#[cfg(test)]\nmod tests {\n    mod nested;\n}", 4),
    ];
    for (module, mod_line) in out_of_line_modules {
        let text = format!("fn before() {{}}\n{module}");
        assert_eq!(
            defect_of(&text),
            Some(LexDefect::OutOfLineTestModule { line: mod_line }),
            "module: {module:?}"
        );
    }
    let followable = [
        "#[cfg(test)]\nmod tests {}",
        "#[cfg(test)]\nmod tests {\n    mod inline {}\n}",
        "#[cfg(test)]\nuse crate::tests;",
        "#[cfg(test)]\nconst TESTS: u32 = 1;",
        "#[cfg(test)]\nfn tests() {}\nmod ungated;",
        "mod ungated;",
        "#[cfg(unix)]\nmod unix_only;",
        "// #[cfg(test)]\n// mod tests;",
        "const S: &str = \"#[cfg(test)] mod tests;\";",
    ];
    for text in followable {
        assert_eq!(defect_of(text), None, "text: {text:?}");
    }
}

// ── the real tree ────────────────────────────────────────────────────────────

fn collect_rs_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading directory {}: {error}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|error| panic!("listing {}: {error}", dir.display()))
            .path();
        if path.is_dir() {
            collect_rs_files(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            found.push(path);
        }
    }
}

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// This file's path under the crate root. The measured tree leaves it out, so
/// its seeds, docs and generic helper names can never count toward a row.
const HOST_PATH: &str = "tests/adoption_ratchets.rs";

/// `src/` and `tests/` of this crate except the host: crate-relative
/// `/`-separated paths, sorted; a file that cannot be read panics naming it.
fn read_crate_tree() -> Vec<SourceFile> {
    let root = crate_root();
    let mut paths = Vec::new();
    for dir in ["src", "tests"] {
        collect_rs_files(&root.join(dir), &mut paths);
    }
    let mut files: Vec<(String, PathBuf)> = paths
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(root).expect("found under the crate root");
            let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
            (parts.join("/"), path)
        })
        .filter(|(relative, _)| relative != HOST_PATH)
        .collect();
    files.sort();
    files
        .iter()
        .map(|(relative, path)| {
            let text = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            source_file(relative, &text)
        })
        .collect()
}

#[test]
fn the_measured_tree_leaves_out_the_host_file_itself() {
    assert!(
        file!().ends_with(HOST_PATH),
        "HOST_PATH {HOST_PATH:?} does not name this file ({}): update the constant",
        file!()
    );
    let on_disk = crate_root().join(HOST_PATH);
    assert!(on_disk.is_file(), "{} is not a file", on_disk.display());
    let tree = read_crate_tree();
    assert!(
        tree.iter().all(|file| file.path != HOST_PATH),
        "the host would count its own seeds, docs and helper names"
    );
}

/// The include / exclude entries (or census root and shared dir) of `row` that
/// prefix no file in `tree`.
fn dangling_scope_entries(row: &Ratchet, tree: &[SourceFile]) -> Vec<&'static str> {
    let entries: Vec<&'static str> = match &row.measure {
        Measure::Occurrences(pattern) => pattern
            .include
            .iter()
            .chain(pattern.exclude)
            .copied()
            .collect(),
        Measure::DuplicateTopLevelFns { root, shared_dir } => vec![*root, *shared_dir],
    };
    entries
        .into_iter()
        .filter(|entry| !tree.iter().any(|file| file.path.starts_with(entry)))
        .collect()
}

#[test]
fn a_census_shared_dir_that_matches_no_file_is_reported_as_dangling() {
    let tree = [source_file("tests/a.rs", "fn h() {}")];
    let census_with = |shared_dir| Ratchet {
        name: "probe",
        home: "nowhere",
        baseline: 0,
        measure: Measure::DuplicateTopLevelFns {
            root: "tests/",
            shared_dir,
        },
    };
    assert!(dangling_scope_entries(&census_with("tests/"), &tree).is_empty());
    assert_eq!(
        dangling_scope_entries(&census_with("tests/common/"), &tree),
        ["tests/common/"]
    );
}

#[test]
fn every_ratchet_row_holds_at_its_committed_baseline() {
    let tree = read_crate_tree();
    for dir in ["src/", "tests/"] {
        assert!(
            tree.iter().any(|file| file.path.starts_with(dir)),
            "the crate tree holds no file under {dir} ({} file(s) read)",
            tree.len()
        );
    }
    let mut failures = Vec::new();
    for file in &tree {
        if let Some(defect) = &file.defect {
            failures.push(format!(
                "{}: the matcher cannot follow this file ({defect:?}), so a row could miscount it",
                file.path
            ));
        }
    }
    for row in RATCHETS {
        let dangling = dangling_scope_entries(row, &tree);
        if !dangling.is_empty() {
            failures.push(format!(
                "{}: scope entries that match no file: {dangling:?}",
                row.name
            ));
        }
        failures.extend(check(row, &tree).err());
    }
    assert!(
        failures.is_empty(),
        "adoption ratchets out of step (docs/prds/v0_6/solver-crates-layering.md D20):\n{}",
        failures.join("\n")
    );
}
