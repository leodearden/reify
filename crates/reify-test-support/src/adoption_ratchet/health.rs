//! Reports the ways a lexed file falls outside what the lexer and the item
//! tracker can follow.

use super::items::{
    TEST_ITEMS, after_item_keyword, leading_identifier, skipped_lines, split_attributes,
};
use super::lexer::{Lexed, Line, bracket_step, is_ident_char, nesting_after};

/// A way a file can be shown to be one the matcher cannot follow: compilable
/// Rust ends outside every literal and comment with ()[]{} nested to zero, the
/// item tracker models one test gate only, the bare `#[cfg(test)]`, and a test
/// item's body is in the same file.
#[derive(Debug, PartialEq)]
pub(super) enum LexDefect {
    EndsInsideLiteralOrComment,
    UnbalancedBrackets,
    UnsupportedTestCfg { line: usize },
    OutOfLineTestModule { line: usize },
}

pub(super) fn lex_defect(lexed: &Lexed) -> Option<LexDefect> {
    if !lexed.ends_in_code {
        return Some(LexDefect::EndsInsideLiteralOrComment);
    }
    let lines = &lexed.lines;
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

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    #[test]
    fn a_file_the_lexer_cannot_follow_is_reported_as_defective() {
        let defect_of = |text: &str| lex_defect(&lex(text));
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
        let defect_of = |text: &str| lex_defect(&lex(text));
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
        let defect_of = |text: &str| lex_defect(&lex(text));
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
}
