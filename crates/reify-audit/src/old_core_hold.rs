#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
holder = 10510
files = [
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_edit.rs",
    "crates/reify-eval/src/engine_edit/",
    "crates/reify-eval/src/engine_build.rs",
    "crates/reify-eval/src/engine_build/",
    "crates/reify-eval/src/lib.rs",
]
registration_only = ["crates/reify-eval/src/lib.rs"]
"#;

    fn valid() -> HoldList {
        HoldList::parse(VALID).expect("the valid fixture parses")
    }

    fn holds(list: &HoldList, path: &str) -> bool {
        !list.held_files_of(&[path.to_string()]).is_empty()
    }

    #[test]
    fn a_valid_list_parses_and_reads_back_its_holder() {
        assert_eq!(valid().holder_id(), "10510");
    }

    #[test]
    fn a_file_entry_holds_by_equality_and_a_dir_entry_holds_itself_and_below() {
        let list = valid();
        for held in [
            "crates/reify-eval/src/engine_eval.rs",
            "crates/reify-eval/src/engine_build/stage.rs",
            "crates/reify-eval/src/engine_build/",
            "./crates/reify-eval/src/engine_eval.rs",
        ] {
            assert!(holds(&list, held), "{held} is held");
        }
    }

    #[test]
    fn no_prefix_of_a_crate_module_or_directory_name_is_held() {
        let list = valid();
        for not_held in [
            "crates/reify-evalcore/src/engine_eval.rs",
            "crates/reify-eval/src/engine_editor.rs",
            "crates/reify-eval/src/engine_build_util.rs",
            "crates/reify-eval/src",
        ] {
            assert!(!holds(&list, not_held), "{not_held} is not held");
        }
    }

    #[test]
    fn held_files_of_returns_the_normalised_held_non_test_paths_in_order() {
        let paths = [
            "./crates/reify-eval/src/engine_eval.rs",
            "crates/reify-gui/src/x.rs",
            "crates/reify-eval/src/engine_build/tests/stage.rs",
            "crates/reify-eval/src/engine_build/",
        ]
        .map(String::from);
        assert_eq!(
            valid().held_files_of(&paths),
            [
                "crates/reify-eval/src/engine_eval.rs",
                "crates/reify-eval/src/engine_build",
            ],
        );
    }

    #[test]
    fn only_a_registration_only_entry_reads_as_registration_only() {
        let list = valid();
        assert!(list.is_registration_only("crates/reify-eval/src/lib.rs"));
        assert!(!list.is_registration_only("crates/reify-eval/src/engine_eval.rs"));
    }

    /// A list over `files` with no registration-only entries.
    fn with_files(files: &str) -> String {
        format!("holder = 10510\nfiles = [{files}]\n")
    }

    #[test]
    fn every_invariant_violation_names_the_invariant_and_the_offending_value() {
        let cases = [
            ("malformed TOML", "holder = \n".to_string(), "TOML", "line 1"),
            (
                "an unknown key",
                format!("{}extra = 1\n", with_files(r#""a.rs""#)),
                "unknown field",
                "extra",
            ),
            (
                "holder 0",
                "holder = 0\nfiles = [\"a.rs\"]\n".to_string(),
                "positive",
                "0",
            ),
            ("empty files", with_files(""), "empty", "files"),
            (
                "an absolute entry",
                with_files(r#""/crates/x.rs""#),
                "absolute",
                "/crates/x.rs",
            ),
            (
                "a ./ prefix",
                with_files(r#""./crates/x.rs""#),
                "`.` segment",
                "./crates/x.rs",
            ),
            (
                "a .. segment",
                with_files(r#""crates/../x.rs""#),
                "`..` segment",
                "crates/../x.rs",
            ),
            (
                "an empty segment",
                with_files(r#""crates//x.rs""#),
                "empty segment",
                "crates//x.rs",
            ),
            (
                "a glob",
                with_files(r#""crates/reify-eval/src/*.rs""#),
                "glob",
                "crates/reify-eval/src/*.rs",
            ),
            (
                "a duplicate",
                with_files(r#""crates/x.rs", "crates/x.rs""#),
                "listed twice",
                "crates/x.rs",
            ),
            (
                "a registration_only entry absent from files",
                format!(
                    "{}registration_only = [\"crates/y.rs\"]\n",
                    with_files(r#""crates/x.rs""#)
                ),
                "not in files",
                "crates/y.rs",
            ),
            (
                "a registration_only directory",
                format!(
                    "{}registration_only = [\"crates/d/\"]\n",
                    with_files(r#""crates/d/""#)
                ),
                "directory",
                "crates/d/",
            ),
        ];
        for (label, text, invariant, offending) in cases {
            let message = match HoldList::parse(&text) {
                Ok(_) => panic!("{label}: parsed, but must be refused:\n{text}"),
                Err(e) => e.to_string(),
            };
            assert!(
                message.contains(invariant) && message.contains(offending),
                "{label}: `{message}` must name the invariant ({invariant}) and the offending value ({offending})",
            );
        }
    }
}
