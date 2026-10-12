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

// ── ratchet rows ─────────────────────────────────────────────────────────────

struct SourceFile {
    path: String,
    lines: Vec<Line>,
}

fn source_file(path: &str, text: &str) -> SourceFile {
    SourceFile {
        path: path.to_string(),
        lines: lex(text),
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

enum View {
    Code,
    CodeAndDocs,
}

/// A literal needle counted in the files under `include` (path prefixes; a
/// directory ends in `/`) minus `exclude`, outside the `skip_items` items.
struct Pattern {
    needle: &'static str,
    include: &'static [&'static str],
    exclude: &'static [&'static str],
    skip_items: &'static [&'static str],
    view: View,
    /// One inline copy of the needle on a single quote-free line.
    seed: &'static str,
}

enum Measure {
    Occurrences(Pattern),
    DuplicateTopLevelFns { root: &'static str },
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

const RATCHETS: &[Ratchet] = &[
    Ratchet {
        name: "shell-assembly-upward-imports",
        home: "elements/shell_frame.rs (solver-crates-layering D4, leaf β)",
        baseline: 5,
        measure: Measure::Occurrences(Pattern {
            needle: "shell_assembly::",
            include: &[
                "src/elements/",
                "src/geometric_stiffness/",
                "src/shell_kinematics.rs",
                "src/membrane_load.rs",
            ],
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "use crate::shell_assembly::plane_stress_d;",
        }),
    },
    Ratchet {
        name: "bubble-condensation-copies",
        home: "the one guarded bubble-condensation fn (solver-crates-layering D6, leaf γ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "let det_bb =",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "let det_bb = k00 * k11 - k01 * k10;",
        }),
    },
    Ratchet {
        name: "shear-correction-literal",
        home: "ShellSection::SHEAR_CORRECTION (solver-crates-layering D5, leaf γ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "5.0 / 6.0",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "const KAPPA: f64 = 5.0 / 6.0;",
        }),
    },
    Ratchet {
        name: "inline-shear-modulus",
        home: "IsotropicElastic::shear_modulus (solver-crates-layering D5, leaf γ)",
        baseline: 16,
        measure: Measure::Occurrences(Pattern {
            needle: "/ (2.0 * (1.0 + nu))",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: WHOLE_FILE,
            view: View::Code,
            seed: "let g = e / (2.0 * (1.0 + nu));",
        }),
    },
    Ratchet {
        name: "thickness-scaled-d-blocks",
        home: "the shared scaled(d, s) helper (solver-crates-layering D6, leaf γ)",
        baseline: 5,
        measure: Measure::Occurrences(Pattern {
            needle: "td[i][j] = ",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "td[i][j] = t * d[i][j];",
        }),
    },
    Ratchet {
        name: "assembly-element-literals",
        home: "AssemblyElement::new (solver-crates-layering D7, leaf δ)",
        baseline: 38,
        measure: Measure::Occurrences(Pattern {
            needle: "AssemblyElement {",
            include: SRC_AND_TESTS,
            exclude: &["src/assembly/global.rs"],
            skip_items: WHOLE_FILE,
            view: View::Code,
            seed: "let el = AssemblyElement { id: 0, connectivity: &c, k_e: &k };",
        }),
    },
    Ratchet {
        name: "element-stiffness-data-indexing",
        home: "the ElementStiffness accessors (solver-crates-layering D8, leaf ε)",
        baseline: 74,
        measure: Measure::Occurrences(Pattern {
            needle: ".data[",
            include: SRC_AND_TESTS,
            exclude: &["src/assembly/mod.rs", "src/resample.rs"],
            skip_items: WHOLE_FILE,
            view: View::Code,
            seed: "let v = k_e.data[0];",
        }),
    },
    Ratchet {
        name: "member-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "fn member_force_delta(",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "fn member_force_delta(x: f64) -> f64 { x }",
        }),
    },
    Ratchet {
        name: "bar-axial-force-delta-definitions",
        home: "the membrane_load helper shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "fn bar_axial_force_delta(",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "fn bar_axial_force_delta(x: f64) -> f64 { x }",
        }),
    },
    Ratchet {
        name: "grounding-diagonal-definitions",
        home: "the membrane_load constant shared with tensegrity_load (solver-crates-layering D9, leaf ζ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "const GROUNDING_DIAGONAL:",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "const GROUNDING_DIAGONAL: f64 = 1.0;",
        }),
    },
    Ratchet {
        name: "cable-sign-rule-arms",
        home: "MemberKind::admits (solver-crates-layering D1, leaf η)",
        baseline: 6,
        measure: Measure::Occurrences(Pattern {
            needle: "MemberKind::Cable =>",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "MemberKind::Cable => qi > 0.0,",
        }),
    },
    Ratchet {
        name: "unchecked-anchor-partition",
        home: "ForceDensityProblem construction (solver-crates-layering D1, leaf η)",
        baseline: 3,
        measure: Measure::Occurrences(Pattern {
            needle: "is_anchor[a] = true",
            include: SRC,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed: "is_anchor[a] = true;",
        }),
    },
    Ratchet {
        name: "mpc-row-literals",
        home: "MpcRow::new (solver-crates-layering D10, leaf θ)",
        baseline: 2,
        measure: Measure::Occurrences(Pattern {
            needle: "MpcRow {",
            include: SRC_AND_TESTS,
            exclude: NO_PATHS,
            skip_items: &["impl MpcRow", "pub struct MpcRow"],
            view: View::CodeAndDocs,
            seed: "let r = MpcRow { dofs: vec![0], coeffs: vec![1.0], rhs: 0.0 };",
        }),
    },
    Ratchet {
        name: "duplicated-test-helpers",
        home: "tests/common/fea_harness.rs (solver-crates-layering D11, leaf ι)",
        baseline: 76,
        measure: Measure::DuplicateTopLevelFns { root: "tests/" },
    },
];

fn in_scope(path: &str, include: &[&str], exclude: &[&str]) -> bool {
    false
}

fn measure(how: &Measure, files: &[SourceFile]) -> Vec<Hit> {
    Vec::new()
}

fn check(ratchet: &Ratchet, files: &[SourceFile], baseline: usize) -> Result<(), String> {
    Ok(())
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

/// Runs `verdict` on every pattern row and fails once, naming every row that returned `Err`.
fn for_every_pattern_row(verdict: impl Fn(&Ratchet, &Pattern) -> Result<(), String>) {
    let rows = pattern_rows();
    assert!(!rows.is_empty(), "no pattern rows to test");
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|&(row, pattern)| {
            verdict(row, pattern)
                .err()
                .map(|problem| format!("{}: {problem}", row.name))
        })
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
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
        for prefix in pattern.skip_items.iter().filter(|p| !TEST_ITEMS.contains(p)) {
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
        let gated = format!("#[cfg(test)]\nmod tests {{\n{}\n}}", pattern.seed);
        let expected = if skips_test_items(pattern) { 1 } else { 2 };
        expect_hit_count(&hits_after_control(row, pattern, &gated), expected)
    });
}

#[test]
fn every_pattern_row_applies_its_view_to_doc_comments() {
    for_every_pattern_row(|row, pattern| {
        let documented = format!("/// {}", pattern.seed);
        let expected = match pattern.view {
            View::Code => 1,
            View::CodeAndDocs => 2,
        };
        expect_hit_count(&hits_after_control(row, pattern, &documented), expected)
    });
}

#[test]
fn needle_requires_a_left_identifier_boundary() {
    let boundary_rows = pattern_rows()
        .into_iter()
        .filter(|(_, pattern)| pattern.needle.starts_with(is_ident_char))
        .count();
    assert!(boundary_rows > 0, "no row starts its needle with an identifier character");
    for_every_pattern_row(|row, pattern| {
        if !pattern.needle.starts_with(is_ident_char) {
            return Ok(());
        }
        let glued_needle = format!("x{}", pattern.needle);
        let glued = pattern.seed.replacen(pattern.needle, &glued_needle, 1);
        expect_hit_count(&hits_after_control(row, pattern, &glued), 1)
    });
}
