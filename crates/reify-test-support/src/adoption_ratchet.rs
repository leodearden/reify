//! The adoption-ratchet engine: each row counts the inline copies of a primitive
//! that a later leaf extracts, in a crate's `src/` and `tests/`, and the count
//! must equal the row's committed baseline. Above it is a re-added copy; below it
//! means the migrating leaf has to lower the baseline in the same diff.
//!
//! These rows are not `tests/infra/adoption-ratchet.manifest` rows: that engine
//! reds only above a ceiling, so a migrated copy would go unrecorded, and it
//! matches line by line, so it cannot skip `#[cfg(test)]` items or string contents.
//!
//! A host is one integration-test file per crate (`tests/adoption_ratchets.rs`).
//! It holds a [`Host`] constant naming itself, a `RATCHETS: &[Ratchet]` table, and
//! two `#[test]` fns: one calls [`assert_seeded_self_tests`] and the other
//! [`assert_crate_tree_at_baselines`]. The measured tree leaves the host out, so
//! its seeds and docs never count toward a row.

mod census;
mod health;
mod items;
mod lexer;
mod source;

use std::cmp::Ordering;

use census::duplicate_top_level_fns;
use items::skipped_lines;
use lexer::{Line, is_ident_char};
use source::{Hit, SourceFile, hit_lines};

pub use items::TEST_ITEMS;

/// The text of a line that a pattern reads.
#[derive(Clone, Copy)]
pub enum View {
    /// Code with comments removed and string and char literal contents blanked.
    Code,
    /// The code view plus the text of the doc comments on the line.
    CodeAndDocs,
}

/// A literal needle counted in the files under `include` (path prefixes; a
/// directory ends in `/`) minus `exclude`, outside the `skip_items` items.
#[derive(Clone, Copy)]
pub struct Pattern {
    pub needle: &'static str,
    pub include: &'static [&'static str],
    pub exclude: &'static [&'static str],
    pub skip_items: &'static [&'static str],
    pub view: View,
    /// One inline copy of the needle on a single quote-free line.
    pub seed: &'static str,
}

impl Pattern {
    /// A pattern over production code: `#[cfg(test)]` items are skipped, so test
    /// modules never count. No path is excluded and only the code view is read.
    pub const fn production(
        needle: &'static str,
        include: &'static [&'static str],
        seed: &'static str,
    ) -> Pattern {
        Pattern {
            needle,
            include,
            exclude: NO_PATHS,
            skip_items: TEST_ITEMS,
            view: View::Code,
            seed,
        }
    }

    /// A pattern over whole files: no item is skipped, so `#[cfg(test)]` modules
    /// count along with every other line under `include`.
    pub const fn whole_file(
        needle: &'static str,
        include: &'static [&'static str],
        seed: &'static str,
    ) -> Pattern {
        Pattern {
            skip_items: WHOLE_FILE,
            ..Pattern::production(needle, include, seed)
        }
    }
}

const NO_PATHS: &[&str] = &[];
const WHOLE_FILE: &[&str] = &[];

/// What a row counts.
#[derive(Clone, Copy)]
pub enum Measure {
    /// Each occurrence of the pattern's needle.
    Occurrences(Pattern),
    /// Helper fns defined in several files under `root`; a definition under
    /// `shared_dir` is the copy the other files duplicate.
    DuplicateTopLevelFns {
        root: &'static str,
        shared_dir: &'static str,
    },
}

/// One row: a measure held at a committed baseline.
#[derive(Clone, Copy)]
pub struct Ratchet {
    pub name: &'static str,
    /// Where copies belong instead: the PRD decision and the leaf that lowers the row.
    pub home: &'static str,
    pub baseline: usize,
    pub measure: Measure,
}

fn in_scope(path: &str, include: &[&str], exclude: &[&str]) -> bool {
    let under = |prefix: &&str| path.starts_with(prefix);
    include.iter().any(under) && !exclude.iter().any(under)
}

/// Non-overlapping occurrences of `needle` in `text`; a needle starting with an
/// identifier character must not continue a longer identifier on its left.
fn occurrences_in(text: &str, needle: &str) -> usize {
    let needs_left_boundary = needle.starts_with(is_ident_char);
    text.match_indices(needle)
        .filter(|&(at, _)| {
            let continues_identifier = text[..at].chars().next_back().is_some_and(is_ident_char);
            !(needs_left_boundary && continues_identifier)
        })
        .count()
}

fn counted_occurrences(pattern: &Pattern, line: &Line) -> usize {
    let in_code = occurrences_in(&line.code, pattern.needle);
    let in_docs = match pattern.view {
        View::Code => 0,
        View::CodeAndDocs => occurrences_in(&line.doc, pattern.needle),
    };
    in_code + in_docs
}

fn hits_in_file(pattern: &Pattern, file: &SourceFile) -> Vec<Hit> {
    let skipped = skipped_lines(&file.lines, pattern.skip_items);
    let mut hits = Vec::new();
    for (index, line) in file.lines.iter().enumerate() {
        if skipped[index] {
            continue;
        }
        for _ in 0..counted_occurrences(pattern, line) {
            hits.push(Hit {
                path: file.path.clone(),
                line: index + 1,
                text: line.raw.trim().to_string(),
            });
        }
    }
    hits
}

fn count_occurrences(pattern: &Pattern, files: &[SourceFile]) -> Vec<Hit> {
    files
        .iter()
        .filter(|file| in_scope(&file.path, pattern.include, pattern.exclude))
        .flat_map(|file| hits_in_file(pattern, file))
        .collect()
}

fn measure(how: &Measure, files: &[SourceFile]) -> Vec<Hit> {
    match how {
        Measure::Occurrences(pattern) => count_occurrences(pattern, files),
        Measure::DuplicateTopLevelFns { root, shared_dir } => {
            duplicate_top_level_fns(root, shared_dir, files)
        }
    }
}

/// Strict ratchet: the measured count must equal the row's `baseline`. Above it, a
/// copy was added; below it, a leaf migrated copies and must lower the row in the
/// same diff. A row's home does not exist until its leaf lands, so a genuinely new
/// copy that cannot use it yet raises the baseline instead.
fn check(ratchet: &Ratchet, files: &[SourceFile], host_file: &str) -> Result<(), String> {
    let hits = measure(&ratchet.measure, files);
    let (name, home, baseline) = (ratchet.name, ratchet.home, ratchet.baseline);
    let count = hits.len();
    match count.cmp(&baseline) {
        Ordering::Equal => Ok(()),
        Ordering::Greater => Err(format!(
            "{name}: {count} is above its baseline {baseline}: an inline copy was added; \
             route it through {home}\n\
             (if that home is not extracted yet, a genuinely new copy may raise the baseline \
             to {count} in {}, with the reason in the commit message)\n{}",
            host_file,
            hit_lines(&hits)
        )),
        Ordering::Less => Err(format!(
            "{name}: {count} is below its baseline {baseline}: \
             lower the baseline to {count} in {}",
            host_file
        )),
    }
}

/// The include / exclude entries (or census root and shared dir) of `row` that
/// prefix no file in `tree`.
fn dangling_scope_entries(row: &Ratchet, tree: &[SourceFile]) -> Vec<&'static str> {
    let entries: Vec<&'static str> = match &row.measure {
        Measure::Occurrences(pattern) => pattern
            .include
            .iter()
            .chain(pattern.exclude)
            .copied()
            .collect(),
        Measure::DuplicateTopLevelFns { root, shared_dir } => vec![*root, *shared_dir],
    };
    entries
        .into_iter()
        .filter(|entry| !tree.iter().any(|file| file.path.starts_with(entry)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::source::source_file;
    use super::*;

    const PROBE_HOST_FILE: &str = "crates/probe/tests/adoption_ratchets.rs";

    const PROBE_SEED: &str = "let a = probe_copy(1);";

    fn probe_row(baseline: usize) -> Ratchet {
        Ratchet {
            name: "probe-row",
            home: "the probe home (probe PRD D1, leaf alpha)",
            baseline,
            measure: Measure::Occurrences(Pattern::production(
                "probe_copy(",
                &["src/"],
                PROBE_SEED,
            )),
        }
    }

    fn seeded_file(copies: usize) -> [SourceFile; 1] {
        [source_file(
            "src/seeded.rs",
            &vec![PROBE_SEED; copies].join("\n"),
        )]
    }

    #[test]
    fn check_is_strict_equality_and_names_the_host_file() {
        let row = probe_row(2);
        assert_eq!(check(&row, &seeded_file(2), PROBE_HOST_FILE), Ok(()));
        let above = check(&row, &seeded_file(3), PROBE_HOST_FILE).unwrap_err();
        for fragment in [
            "probe-row: 3 is above its baseline 2",
            row.home,
            "raise the baseline to 3 in crates/probe/tests/adoption_ratchets.rs",
            "  src/seeded.rs:3: let a = probe_copy(1);",
        ] {
            assert!(
                above.contains(fragment),
                "{fragment:?} missing from:\n{above}"
            );
        }
        assert_eq!(
            check(&row, &seeded_file(1), PROBE_HOST_FILE),
            Err("probe-row: 1 is below its baseline 2: \
                 lower the baseline to 1 in crates/probe/tests/adoption_ratchets.rs"
                .to_string())
        );
    }

    #[test]
    fn a_production_pattern_skips_cfg_test_items_and_a_whole_file_pattern_counts_them() {
        let text = format!("{PROBE_SEED}\n#[cfg(test)]\nmod tests {{\n{PROBE_SEED}\n}}");
        let tree = [source_file("src/seeded.rs", &text)];
        let production =
            Measure::Occurrences(Pattern::production("probe_copy(", &["src/"], PROBE_SEED));
        let whole_file =
            Measure::Occurrences(Pattern::whole_file("probe_copy(", &["src/"], PROBE_SEED));
        assert_eq!(measure(&production, &tree).len(), 1);
        assert_eq!(measure(&whole_file, &tree).len(), 2);
    }

    #[test]
    fn an_identifier_needle_needs_a_left_boundary_and_a_punctuation_needle_does_not() {
        let count = |needle: &'static str, text: &str| {
            let pattern = Measure::Occurrences(Pattern::production(needle, &["src/"], ""));
            measure(&pattern, &[source_file("src/probe.rs", text)]).len()
        };
        assert_eq!(count("probe_copy(", "let a = xprobe_copy(1);"), 0);
        assert_eq!(count(".data[", "let v = k_e.data[0];"), 1);
    }

    #[test]
    fn a_census_shared_dir_that_matches_no_file_is_reported_as_dangling() {
        let tree = [source_file("tests/a.rs", "fn h() {}")];
        let census_with = |shared_dir| Ratchet {
            name: "probe",
            home: "nowhere",
            baseline: 0,
            measure: Measure::DuplicateTopLevelFns {
                root: "tests/",
                shared_dir,
            },
        };
        assert!(dangling_scope_entries(&census_with("tests/"), &tree).is_empty());
        assert_eq!(
            dangling_scope_entries(&census_with("tests/common/"), &tree),
            ["tests/common/"]
        );
    }

    const PROBE_DEFINITION_AND_TEST_ITEMS: &[&str] = &[TEST_ITEMS[0], "fn probe_definition"];

    const PROBE_ROWS: &[Ratchet] = &[
        Ratchet {
            name: "probe-production",
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(Pattern::production(
                "probe_copy(",
                &["src/"],
                "let a = probe_copy(1);",
            )),
        },
        Ratchet {
            name: "probe-whole-file",
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(Pattern {
                exclude: &["src/excluded.rs"],
                ..Pattern::whole_file(".data[", &["src/", "tests/"], "let v = k_e.data[0];")
            }),
        },
        Ratchet {
            name: "probe-code-and-docs",
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(Pattern {
                skip_items: &["impl Probe", "pub struct Probe"],
                view: View::CodeAndDocs,
                ..Pattern::whole_file("Probe {", &["src/", "tests/"], "let p = Probe { x: 1 };")
            }),
        },
        Ratchet {
            name: "probe-skips-its-definition",
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(Pattern {
                skip_items: PROBE_DEFINITION_AND_TEST_ITEMS,
                ..Pattern::production(
                    "probe_definition",
                    &["src/"],
                    "let d = probe_definition(1);",
                )
            }),
        },
        Ratchet {
            name: "probe-census",
            home: "the probe home",
            baseline: 0,
            measure: Measure::DuplicateTopLevelFns {
                root: "tests/",
                shared_dir: "tests/common/",
            },
        },
    ];

    #[test]
    fn seeded_self_tests_pass_for_a_well_formed_probe_table() {
        let failures = seeded_self_test_failures(PROBE_ROWS, PROBE_HOST_FILE);
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    fn malformed_seed_row(name: &'static str, pattern: Pattern) -> Ratchet {
        Ratchet {
            name,
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(pattern),
        }
    }

    #[test]
    fn seeded_self_tests_name_the_row_and_property_of_each_malformed_seed() {
        let rows = [
            malformed_seed_row(
                "seed-lacks-its-needle",
                Pattern::production("probe_copy(", &["src/"], "let a = other(1);"),
            ),
            malformed_seed_row(
                "seed-holds-it-twice",
                Pattern::production(
                    "probe_copy(",
                    &["src/"],
                    "let a = probe_copy(probe_copy(1));",
                ),
            ),
            malformed_seed_row(
                "seed-has-a-quote",
                Pattern::production("probe_copy(", &["src/"], "let a = probe_copy(\"x\");"),
            ),
            malformed_seed_row(
                "seed-path-out-of-scope",
                Pattern {
                    exclude: &["src/"],
                    ..Pattern::production("probe_copy(", &["src/"], "let a = probe_copy(1);")
                },
            ),
        ];
        let failures = seeded_self_test_failures(&rows, PROBE_HOST_FILE);
        for row in &rows {
            assert!(
                failures.iter().any(|line| line.contains(row.name)
                    && line.contains("seed_path_is_in_scope_and_seed_holds_one_needle")),
                "no seed failure names {}:\n{}",
                row.name,
                failures.join("\n")
            );
        }
    }

    #[test]
    fn seeded_self_tests_reject_an_empty_table() {
        assert_eq!(seeded_self_test_failures(&[], PROBE_HOST_FILE).len(), 1);
    }

    #[test]
    fn crate_tree_failures_report_defects_dangling_entries_and_off_baseline_rows() {
        let tree = [
            source_file("src/a.rs", "let s = \"never closed;"),
            source_file("src/b.rs", PROBE_SEED),
        ];
        let dangling = Ratchet {
            name: "probe-dangling",
            home: "the probe home",
            baseline: 0,
            measure: Measure::Occurrences(Pattern::production(
                "probe_copy(",
                &["src/missing/"],
                PROBE_SEED,
            )),
        };
        let failures = crate_tree_failures(&[dangling, probe_row(2)], &tree, PROBE_HOST_FILE);
        for expected in [
            "the crate tree holds no file under tests/",
            "src/a.rs: the matcher cannot follow this file (EndsInsideLiteralOrComment)",
            "probe-dangling: scope entries that match no file: [\"src/missing/\"]",
            "probe-row: 1 is below its baseline 2",
        ] {
            assert!(
                failures.iter().any(|line| line.contains(expected)),
                "{expected:?} missing from:\n{}",
                failures.join("\n")
            );
        }
    }

    #[test]
    fn host_problems_accept_this_file_and_name_a_path_in_crate_that_is_not_the_host_file() {
        let this_file = Host {
            crate_root: env!("CARGO_MANIFEST_DIR"),
            file: file!(),
            path_in_crate: "src/adoption_ratchet.rs",
        };
        assert_eq!(this_file.problems(), Vec::<String>::new());
        let other_file = Host {
            path_in_crate: "src/lib.rs",
            ..this_file
        };
        let problems = other_file.problems().join("\n");
        assert!(
            problems.contains("\"src/lib.rs\"") && problems.contains(file!()),
            "{problems}"
        );
        let missing = Host {
            file: "x/src/missing.rs",
            path_in_crate: "src/missing.rs",
            ..this_file
        };
        let problems = missing.problems().join("\n");
        assert!(
            problems.contains("src/missing.rs is not a file"),
            "{problems}"
        );
    }
}
