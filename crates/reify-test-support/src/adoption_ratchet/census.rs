#[cfg(test)]
mod tests {
    use super::super::source::source_file;
    use super::*;

    const CENSUS_ROOT: &str = "tests/";
    const CENSUS_SHARED_DIR: &str = "tests/common/";
    const SHARED_FILE: &str = "tests/common/mod.rs";

    fn census_hits(files: &[(&str, &str)]) -> Vec<Hit> {
        let tree: Vec<SourceFile> = files
            .iter()
            .map(|&(path, text)| source_file(path, text))
            .collect();
        duplicate_top_level_fns(CENSUS_ROOT, CENSUS_SHARED_DIR, &tree)
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
}
