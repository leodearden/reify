//! Holds the lexed source files of a crate and the lines a measure counts in
//! them.

use std::fs;
use std::path::{Path, PathBuf};

use super::health::{LexDefect, lex_defect};
use super::lexer::{Line, lex};

pub(super) struct SourceFile {
    pub(super) path: String,
    pub(super) lines: Vec<Line>,
    pub(super) defect: Option<LexDefect>,
}

pub(super) fn source_file(path: &str, text: &str) -> SourceFile {
    let lexed = lex(text);
    let defect = lex_defect(&lexed);
    SourceFile {
        path: path.to_string(),
        lines: lexed.lines,
        defect,
    }
}

pub(super) struct Hit {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) text: String,
}

pub(super) fn hit_lines(hits: &[Hit]) -> String {
    hits.iter()
        .map(|hit| format!("  {}:{}: {}", hit.path, hit.line, hit.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading directory {}: {error}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|error| panic!("listing {}: {error}", dir.display()))
            .path();
        if path.is_dir() {
            collect_rs_files(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            found.push(path);
        }
    }
}

/// `src/` and `tests/` of the crate at `crate_root` except `leave_out`:
/// crate-relative `/`-separated paths, sorted. A missing `src/` or `tests/`
/// contributes no file; a file that cannot be read panics naming it, and so does
/// a `leave_out` that names no file.
pub(super) fn read_crate_tree(crate_root: &Path, leave_out: &str) -> Vec<SourceFile> {
    let mut paths = Vec::new();
    for dir in ["src", "tests"].map(|dir| crate_root.join(dir)) {
        if dir.is_dir() {
            collect_rs_files(&dir, &mut paths);
        }
    }
    let (left_out, mut files): (Vec<_>, Vec<_>) = paths
        .into_iter()
        .map(|path| {
            let relative = path
                .strip_prefix(crate_root)
                .expect("found under the crate root");
            let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
            (parts.join("/"), path)
        })
        .partition(|(relative, _)| relative == leave_out);
    assert!(
        !left_out.is_empty(),
        "the left-out path {leave_out:?} names no .rs file under src/ or tests/ of {}; \
         the measured tree would otherwise count the host's own seeds",
        crate_root.display()
    );
    files.sort();
    files
        .iter()
        .map(|(relative, path)| {
            let text = fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
            source_file(relative, &text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
        write_fixture_files(root, FIXTURE_FILES);
    }

    fn write_fixture_files(root: &Path, files: &[&str]) {
        for relative in files {
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
    fn read_crate_tree_reads_a_crate_with_no_src_dir_as_holding_no_src_file() {
        let dir = crate::temp_dirs::prefixed_tempdir("adoption-ratchet-tree-");
        write_fixture_files(dir.path(), &["tests/host.rs", "tests/b.rs"]);
        let tree = read_crate_tree(dir.path(), "tests/host.rs");
        let paths: Vec<&str> = tree.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["tests/b.rs"]);
    }

    #[test]
    #[should_panic(expected = "tests/missing.rs")]
    fn read_crate_tree_panics_naming_a_leave_out_path_that_matches_no_file() {
        let dir = crate::temp_dirs::prefixed_tempdir("adoption-ratchet-tree-");
        write_fixture_crate(dir.path());
        read_crate_tree(dir.path(), "tests/missing.rs");
    }
}
