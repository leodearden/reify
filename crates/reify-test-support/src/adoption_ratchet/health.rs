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
