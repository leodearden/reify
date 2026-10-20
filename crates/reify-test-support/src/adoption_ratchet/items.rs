//! Reads item structure from lexed code: the lines a named item spans and the
//! parts of an item header.

use super::lexer::{Line, is_ident_char, nesting_after};

/// The item prefixes that gate test-only code; the bare `#[cfg(test)]` is the
/// one gate the tracker models.
pub const TEST_ITEMS: &[&str] = &["#[cfg(test)]"];

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
pub(super) fn skipped_lines(lines: &[Line], item_prefixes: &[&str]) -> Vec<bool> {
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

/// Splits the leading `#[..]` attributes off `code`; an attribute that does not
/// close on this line takes the rest of it.
pub(super) fn split_attributes(code: &str) -> (Vec<&str>, &str) {
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
pub(super) fn leading_identifier(text: &str) -> &str {
    &text[..text.find(|c| !is_ident_char(c)).unwrap_or(text.len())]
}

/// The text after the `keyword` that `code` declares, past any `pub`, `pub(..)`,
/// `const`, `async`, `unsafe` and `extern ".."` modifiers (string contents are blanked).
pub(super) fn after_item_keyword<'a>(code: &'a str, keyword: &str) -> Option<&'a str> {
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
pub(super) fn fn_name(code: &str) -> Option<&str> {
    let name = leading_identifier(after_item_keyword(code, "fn")?);
    (!name.is_empty()).then_some(name)
}

/// `rest` without a leading `(crate)` / `(in path)` visibility scope.
fn skip_visibility_scope(rest: &str) -> &str {
    rest.strip_prefix('(')
        .and_then(|inner| inner.split_once(')'))
        .map_or(rest, |(_, after)| after.trim_start())
}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    fn skipped_indices(source: &[&str], item_prefixes: &[&str]) -> Vec<usize> {
        let lines = lex(&source.join("\n")).lines;
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
}
