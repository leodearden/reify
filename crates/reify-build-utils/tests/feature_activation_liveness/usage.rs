//! Does a consumer package's Rust source name any of a set of identifiers?

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fixture::Fixture;

    const MANIFEST: &str = "[package]\nname = \"cons\"\nversion = \"0.1.0\"\n";

    fn package(files: &[(&str, &str)]) -> Fixture {
        let fx = Fixture::new();
        fx.write("Cargo.toml", MANIFEST);
        for (rel, contents) in files {
            fx.write(rel, contents);
        }
        fx
    }

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn find(fx: &Fixture, exclude: Option<&Path>) -> Option<Use> {
        first_use(
            fx.root(),
            exclude,
            &names(&["cache_store", "set_panic_on_eval"]),
        )
        .expect("fixture package lexes")
    }

    #[test]
    fn a_method_call_is_a_use_with_its_file_and_line() {
        let fx = package(&[(
            "tests/a.rs",
            "fn t(engine: E) {\n    engine.cache_store();\n}\n",
        )]);
        assert_eq!(
            find(&fx, None),
            Some(Use {
                file: fx.root().join("tests/a.rs"),
                line: 2,
                name: "cache_store".to_string(),
            })
        );
    }

    #[test]
    fn comments_doc_comments_and_string_literals_are_not_uses() {
        let fx = package(&[(
            "tests/a.rs",
            "// cache_store\n/* cache_store */\n/// cache_store\nfn f() {\n    let _ = \"cache_store\";\n}\n",
        )]);
        assert_eq!(find(&fx, None), None);
    }

    #[test]
    fn a_name_inside_a_macro_body_is_a_use() {
        let fx = package(&[(
            "tests/a.rs",
            "fn t(e: E) {\n    assert_eq!(\n        e.cache_store().len(),\n        0\n    );\n}\n",
        )]);
        let found = find(&fx, None).expect("macro body is lexed");
        assert_eq!((found.line, found.name.as_str()), (3, "cache_store"));
    }

    #[test]
    fn a_raw_identifier_is_a_use() {
        let fx = package(&[("tests/a.rs", "fn t(e: E) {\n    e.r#cache_store();\n}\n")]);
        let found = find(&fx, None).expect("raw identifier is matched");
        assert_eq!(found.name, "cache_store");
    }

    #[test]
    fn a_longer_identifier_containing_the_name_is_not_a_use() {
        let fx = package(&[(
            "tests/a.rs",
            "fn t(e: E) {\n    e.cache_store_extra();\n}\n",
        )]);
        assert_eq!(find(&fx, None), None);
    }

    #[test]
    fn build_output_hidden_dirs_and_nested_packages_are_not_scanned() {
        let body = "fn t(e: E) {\n    e.cache_store();\n}\n";
        for rel in ["target/debug/build/out.rs", ".git/hooks/x.rs", ".hidden.rs"] {
            let fx = package(&[(rel, body)]);
            assert_eq!(find(&fx, None), None, "{rel} must not be scanned");
        }
        let fx = package(&[("nested/Cargo.toml", MANIFEST), ("nested/src/lib.rs", body)]);
        assert_eq!(
            find(&fx, None),
            None,
            "a nested package is its own consumer"
        );
    }

    #[test]
    fn the_excluded_directory_is_not_scanned() {
        let fx = package(&[("src/lib.rs", "pub fn f(e: E) {\n    e.cache_store();\n}\n")]);
        let src = fx.root().join("src");
        assert_eq!(find(&fx, Some(&src)), None);
        assert!(find(&fx, None).is_some(), "the exclusion is what hides it");
    }

    #[test]
    fn non_rust_files_are_ignored() {
        let fx = package(&[
            ("README.md", "call engine.cache_store()\n"),
            ("tests/fixture.ri", "cache_store\n"),
        ]);
        assert_eq!(find(&fx, None), None);
    }

    #[test]
    fn an_unlexable_file_is_an_error_naming_it() {
        let fx = package(&[("tests/bad.rs", "fn f() {\n")]);
        let err = first_use(fx.root(), None, &names(&["cache_store"])).unwrap_err();
        let bad = fx.root().join("tests/bad.rs");
        assert!(
            matches!(&err, UsageError::Lex { file } if *file == bad),
            "{err:?}"
        );
    }

    #[test]
    fn the_first_hit_of_a_sorted_depth_first_walk_wins() {
        let fx = package(&[
            ("tests/a.rs", "fn t(e: E) {\n    e.cache_store();\n}\n"),
            ("src/zeta.rs", "fn t(e: E) {\n    e.cache_store();\n}\n"),
            (
                "src/alpha/mod.rs",
                "\nfn t(e: E) {\n    e.set_panic_on_eval();\n}\n",
            ),
        ]);
        assert_eq!(
            find(&fx, None),
            Some(Use {
                file: fx.root().join("src/alpha/mod.rs"),
                line: 3,
                name: "set_panic_on_eval".to_string(),
            })
        );
    }
}
