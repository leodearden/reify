//! Finds the helper fns defined at top level in several files under a root.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::items::{fn_name, split_attributes};
use super::lexer::{Line, nesting_after};
use super::source::{Hit, SourceFile};

const TEST_ATTRIBUTE: &str = "#[test]";

/// The helper fns defined at nesting depth zero as `(name, 1-based line)`: not
/// `#[test]` fns and not named `test_*`. Only lines that start at depth zero are
/// read, so multi-line attributes and signatures never look like definitions.
fn top_level_helpers(lines: &[Line]) -> Vec<(&str, usize)> {
    let mut helpers = Vec::new();
    let mut depth = 0;
    let mut test_attribute_pending = false;
    for (index, line) in lines.iter().enumerate() {
        let starts_at_top_level = depth == 0;
        depth = line.code.chars().fold(depth, nesting_after);
        if !starts_at_top_level || line.code.trim().is_empty() {
            continue;
        }
        let (attributes, rest) = split_attributes(&line.code);
        test_attribute_pending |= attributes.contains(&TEST_ATTRIBUTE);
        if rest.is_empty() {
            continue;
        }
        let helper =
            fn_name(rest).filter(|name| !test_attribute_pending && !name.starts_with("test_"));
        if let Some(name) = helper {
            helpers.push((name, index + 1));
        }
        test_attribute_pending = false;
    }
    helpers
}

/// One hit per helper copy beyond the kept one, over the files under `root`: a
/// name defined in several files is counted once per file. The kept copy is the
/// first under `shared_dir`, so a local redefinition of a shared helper is the
/// copy and never the shared one; with none, it is the first in path order.
/// Every hit names the kept copy.
pub(super) fn duplicate_top_level_fns(
    root: &str,
    shared_dir: &str,
    files: &[SourceFile],
) -> Vec<Hit> {
    let mut definitions: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for file in files.iter().filter(|file| file.path.starts_with(root)) {
        for (name, line) in top_level_helpers(&file.lines) {
            let defined_in = definitions.entry(name).or_default();
            defined_in.entry(file.path.as_str()).or_insert(line);
        }
    }
    let mut hits = Vec::new();
    for (name, defined_in) in &definitions {
        let mut copies: Vec<_> = defined_in.iter().collect();
        copies.sort_by_key(|(path, _)| Reverse(path.starts_with(shared_dir)));
        let Some((&(kept, _), extra_copies)) = copies.split_first() else {
            continue;
        };
        hits.extend(extra_copies.iter().map(|&(path, &line)| Hit {
            path: path.to_string(),
            line,
            text: format!("fn {name} (also in {kept})"),
        }));
    }
    hits
}

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
