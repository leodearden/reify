#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::super::health::LexDefect;
    use super::*;

    const FIXTURE_FILES: &[&str] = &[
        "src/lib.rs",
        "src/nested/deep.rs",
        "tests/host.rs",
        "tests/b.rs",
        "tests/notes.txt",
        "benches/x.rs",
    ];

    fn write_fixture_crate(root: &Path) {
        for relative in FIXTURE_FILES {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().expect("a fixture file sits in a directory"))
                .unwrap_or_else(|error| panic!("creating the parent of {relative}: {error}"));
            fs::write(&path, "fn f() {}\n")
                .unwrap_or_else(|error| panic!("writing {relative}: {error}"));
        }
    }

    #[test]
    fn source_file_keeps_its_path_and_records_the_lex_defect_of_its_text() {
        let clean = source_file("src/a.rs", "fn f() {}");
        assert_eq!(clean.path, "src/a.rs");
        assert_eq!(clean.defect, None);
        let unclosed = source_file("src/a.rs", "let s = \"never closed;");
        assert_eq!(unclosed.defect, Some(LexDefect::EndsInsideLiteralOrComment));
    }

    #[test]
    fn read_crate_tree_reads_the_rs_files_under_src_and_tests_except_the_left_out_one() {
        let dir = crate::temp_dirs::prefixed_tempdir("adoption-ratchet-tree-");
        write_fixture_crate(dir.path());
        let tree = read_crate_tree(dir.path(), "tests/host.rs");
        let paths: Vec<&str> = tree.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["src/lib.rs", "src/nested/deep.rs", "tests/b.rs"]);
    }

    #[test]
    #[should_panic(expected = "tests/missing.rs")]
    fn read_crate_tree_panics_naming_a_leave_out_path_that_matches_no_file() {
        let dir = crate::temp_dirs::prefixed_tempdir("adoption-ratchet-tree-");
        write_fixture_crate(dir.path());
        read_crate_tree(dir.path(), "tests/missing.rs");
    }
}
