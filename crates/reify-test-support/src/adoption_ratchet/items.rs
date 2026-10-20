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
