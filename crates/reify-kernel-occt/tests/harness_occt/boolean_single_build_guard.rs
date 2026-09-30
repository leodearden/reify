//! Pins that the OCCT wrapper never constructs a boolean algorithm with its
//! operands (task 7437).
//!
//! In OCCT 7.8.1 the operand-bearing constructors of `BRepAlgoAPI_Fuse`,
//! `BRepAlgoAPI_Cut`, `BRepAlgoAPI_Common` and `BRepAlgoAPI_Section` already
//! call `Build()`, and `BRepAlgoAPI_BooleanOperation::Build` starts with
//! `NotDone(); Clear();`, so an explicit `Build()` after one discards the
//! whole pass and runs it again. The sanctioned idiom is default construction,
//! then `SetArguments`, `SetTools` and exactly one `Build()`, all through
//! `build_boolean_pass` in `cpp/occt_wrapper.cpp`.
//!
//! This is a source scan because the discarded pass leaves no observable
//! difference in the result, and wall time is not a test signal.

#![cfg(has_occt)]

use std::path::{Path, PathBuf};

const EAGER_BOOLEAN_CLASSES: [&str; 4] = [
    "BRepAlgoAPI_Fuse",
    "BRepAlgoAPI_Cut",
    "BRepAlgoAPI_Common",
    "BRepAlgoAPI_Section",
];

const CPP_SOURCE_EXTENSIONS: [&str; 5] = ["cpp", "h", "hpp", "cxx", "hxx"];

#[derive(Clone, Copy)]
enum Lexeme {
    Code,
    LineComment,
    BlockComment,
    Literal(char),
}

/// One lexer transition: `width` chars are consumed, then kept verbatim (code
/// and literal delimiters) or blanked, and lexing continues in `next`.
struct Transition {
    next: Lexeme,
    width: usize,
    keep: bool,
}

fn transition(state: Lexeme, c: char, lookahead: Option<char>) -> Transition {
    let (next, width, keep) = match (state, c, lookahead) {
        (Lexeme::Code, '/', Some('/')) => (Lexeme::LineComment, 2, false),
        (Lexeme::Code, '/', Some('*')) => (Lexeme::BlockComment, 2, false),
        (Lexeme::Code, '"' | '\'', _) => (Lexeme::Literal(c), 1, true),
        (Lexeme::Code, _, _) => (Lexeme::Code, 1, true),
        (Lexeme::LineComment, '\n', _) => (Lexeme::Code, 1, false),
        (Lexeme::BlockComment, '*', Some('/')) => (Lexeme::Code, 2, false),
        (Lexeme::Literal(_), '\\', Some(_)) => (state, 2, false),
        (Lexeme::Literal(quote), _, _) if c == quote => (Lexeme::Code, 1, true),
        (Lexeme::Literal(_), '\n', _) => (Lexeme::Code, 1, false),
        _ => (state, 1, false),
    };
    Transition { next, width, keep }
}

/// Replaces every comment and the body of every string/char literal with
/// spaces, keeping each newline so line numbers survive. A literal left open at
/// a newline ends there, so a stray apostrophe cannot swallow the file.
fn blank_comments_and_literals(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut state = Lexeme::Code;
    let mut i = 0;
    while i < chars.len() {
        let step = transition(state, chars[i], chars.get(i + 1).copied());
        for &c in &chars[i..i + step.width] {
            out.push(if step.keep || c == '\n' { c } else { ' ' });
        }
        state = step.next;
        i += step.width;
    }
    out
}

fn is_identifier_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn skip_whitespace(code: &[char], mut i: usize) -> usize {
    while code.get(i).is_some_and(|c| c.is_whitespace()) {
        i += 1;
    }
    i
}

/// Given the index just past a class name, returns the index just past a
/// following non-empty `(…)` / `{…}` operand list, i.e. the end of an
/// operand-bearing construction. Accepts an optional `>` (template argument,
/// as in `make_unique<…>(…)`) and an optional variable name in between.
fn operand_list_end(code: &[char], after_name: usize) -> Option<usize> {
    let mut i = skip_whitespace(code, after_name);
    if code.get(i) == Some(&'>') {
        i = skip_whitespace(code, i + 1);
    }
    while code.get(i).copied().is_some_and(is_identifier_char) {
        i += 1;
    }
    let open_at = skip_whitespace(code, i);
    let open = *code.get(open_at)?;
    let close = match open {
        '(' => ')',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0usize;
    for (j, &c) in code.iter().enumerate().skip(open_at) {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                let has_operands = code[open_at + 1..j].iter().any(|c| !c.is_whitespace());
                return has_operands.then_some(j + 1);
            }
        }
    }
    None
}

/// Every operand-bearing construction of an eager boolean class in `src`, as
/// (1-based line, whitespace-collapsed snippet), sorted by line.
fn operand_constructions(src: &str) -> Vec<(usize, String)> {
    let code: Vec<char> = blank_comments_and_literals(src).chars().collect();
    let mut findings = Vec::new();
    for class in EAGER_BOOLEAN_CLASSES {
        let name: Vec<char> = class.chars().collect();
        for start in 0..code.len() {
            let end_of_name = start + name.len();
            let whole_word = code[start..].starts_with(&name)
                && (start == 0 || !is_identifier_char(code[start - 1]))
                && !code
                    .get(end_of_name)
                    .copied()
                    .is_some_and(is_identifier_char);
            if !whole_word {
                continue;
            }
            if let Some(end) = operand_list_end(&code, end_of_name) {
                let line = 1 + code[..start].iter().filter(|&&c| c == '\n').count();
                let text: String = code[start..end].iter().collect();
                findings.push((line, text.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
    }
    findings.sort();
    findings
}

fn cpp_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable cpp/ entry").path();
        if path.is_dir() {
            sources.extend(cpp_sources(&path));
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| CPP_SOURCE_EXTENSIONS.contains(&ext))
        {
            sources.push(path);
        }
    }
    sources.sort();
    sources
}

#[test]
fn wrapper_sources_construct_no_boolean_with_operands() {
    let cpp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cpp");
    let sources = cpp_sources(&cpp_dir);
    assert!(
        sources
            .iter()
            .any(|path| path.ends_with("occt_wrapper.cpp")),
        "the scan must cover cpp/occt_wrapper.cpp; scanned {sources:?}"
    );

    let mut findings = Vec::new();
    for path in &sources {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let file = path
            .strip_prefix(&cpp_dir)
            .unwrap_or(path)
            .display()
            .to_string();
        for (line, snippet) in operand_constructions(&src) {
            findings.push(format!("  cpp/{file}:{line}: {snippet}"));
        }
    }
    assert!(
        findings.is_empty(),
        "{} BRepAlgoAPI boolean(s) constructed with operands. In OCCT 7.8.1 that \
         constructor already runs the whole pass, which a later Build() discards and reruns. \
         Default-construct and go through build_boolean_pass (SetArguments, SetTools, one \
         Build()):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

#[test]
fn scanner_flags_every_operand_construction_form() {
    let fixtures = [
        "BRepAlgoAPI_Cut cut(left.shape, right.shape);",
        "BRepAlgoAPI_Fuse fuse(\n    a,\n    b);",
        "BRepAlgoAPI_Common c{a, b};",
        "auto s = BRepAlgoAPI_Common(a, b).Shape();",
        "auto op = std::make_unique<BRepAlgoAPI_Section>(a, b);",
    ];
    for fixture in fixtures {
        let findings = operand_constructions(fixture);
        assert_eq!(
            findings.len(),
            1,
            "expected one finding in {fixture:?}, got {findings:?}"
        );
    }
}

#[test]
fn scanner_ignores_default_construction_comments_and_literals() {
    let fixture = r#"
void f(BRepAlgoAPI_BooleanOperation& op) {
    BRepAlgoAPI_Fuse fuse;
    BRepAlgoAPI_Cut::Build();
    // BRepAlgoAPI_Cut cut(a, b) already builds
    /* BRepAlgoAPI_Common c(a, b); */
    throw std::runtime_error("fuse_shape_list: BRepAlgoAPI_Fuse failed (IsDone=false)");
}
"#;
    assert_eq!(
        operand_constructions(fixture),
        Vec::<(usize, String)>::new()
    );
}
