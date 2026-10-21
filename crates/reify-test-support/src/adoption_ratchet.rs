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
use std::path::Path;

use census::duplicate_top_level_fns;
use items::skipped_lines;
use lexer::{Line, is_ident_char};
use source::{Hit, SourceFile, hit_lines, read_crate_tree, source_file};

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

/// The host of a crate's rows: the crate, and the host file itself, which the
/// measured tree leaves out so its seeds and docs never count toward a row.
#[derive(Clone, Copy)]
pub struct Host {
    /// The crate's root directory: `env!("CARGO_MANIFEST_DIR")`.
    pub crate_root: &'static str,
    /// The host file as `file!()` names it; red messages tell the reader to edit it.
    pub file: &'static str,
    /// The host file's `/`-separated path under `crate_root`.
    pub path_in_crate: &'static str,
}

impl Host {
    fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if !Path::new(self.file).ends_with(self.path_in_crate) {
            problems.push(format!(
                "path_in_crate {:?} does not name the host file ({}): update the constant",
                self.path_in_crate, self.file
            ));
        }
        let on_disk = Path::new(self.crate_root).join(self.path_in_crate);
        if !on_disk.is_file() {
            problems.push(format!("{} is not a file", on_disk.display()));
        }
        problems
    }
}

/// Panics listing every problem with `host` and every seeded self-test a row of
/// `rows` fails. A pattern row must flag one inline copy of its seed, count each
/// copy on a line, ignore copies in comments, strings, out-of-scope paths and
/// skipped items, and apply its region and view; every row must go red when a
/// copy is added, or migrated without lowering the baseline.
pub fn assert_seeded_self_tests(rows: &[Ratchet], host: &Host) {
    let mut failures = host.problems();
    failures.extend(seeded_self_test_failures(rows, host.file));
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// Panics unless every row of `rows` holds at exactly its baseline over the
/// `src/` and `tests/` of `host`'s crate, minus the host file, with every file
/// there one the lexer can follow and every scope entry matching some file.
pub fn assert_crate_tree_at_baselines(rows: &[Ratchet], host: &Host) {
    let problems = host.problems();
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    let tree = read_crate_tree(Path::new(host.crate_root), host.path_in_crate);
    let failures = crate_tree_failures(rows, &tree, host.file);
    assert!(
        failures.is_empty(),
        "adoption ratchets out of step (docs/prds/v0_6/solver-crates-layering.md D20):\n{}",
        failures.join("\n")
    );
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

fn crate_tree_failures(rows: &[Ratchet], tree: &[SourceFile], host_file: &str) -> Vec<String> {
    let mut failures = Vec::new();
    for dir in ["src/", "tests/"] {
        if !tree.iter().any(|file| file.path.starts_with(dir)) {
            failures.push(format!(
                "the crate tree holds no file under {dir} ({} file(s) read)",
                tree.len()
            ));
        }
    }
    for file in tree {
        if let Some(defect) = &file.defect {
            failures.push(format!(
                "{}: the matcher cannot follow this file ({defect:?}), so a row could miscount it",
                file.path
            ));
        }
    }
    for row in rows {
        let dangling = dangling_scope_entries(row, tree);
        if !dangling.is_empty() {
            failures.push(format!(
                "{}: scope entries that match no file: {dangling:?}",
                row.name
            ));
        }
        failures.extend(check(row, tree, host_file).err());
    }
    failures
}

// ── seeded self-tests ────────────────────────────────────────────────────────

/// One `"<property>: <row>: <problem>"` line per seeded self-test a row fails.
fn seeded_self_test_failures(rows: &[Ratchet], host_file: &str) -> Vec<String> {
    if rows.is_empty() {
        return vec!["the table holds no row, so no seeded self-test runs".to_string()];
    }
    rows.iter()
        .flat_map(|row| {
            seeded_verdicts(row, host_file)
                .into_iter()
                .filter_map(move |(property, verdict)| {
                    let problem = verdict.err()?;
                    Some(format!("{property}: {}: {problem}", row.name))
                })
        })
        .collect()
}

type PatternProperty = fn(&Ratchet, &Pattern) -> Result<(), String>;

const PATTERN_PROPERTIES: &[(&str, PatternProperty)] = &[
    (
        "seed_path_is_in_scope_and_seed_holds_one_needle",
        seed_path_is_in_scope_and_seed_holds_one_needle,
    ),
    ("flags_one_seeded_inline_copy", flags_one_seeded_inline_copy),
    (
        "counts_each_occurrence_on_a_line",
        counts_each_occurrence_on_a_line,
    ),
    (
        "ignores_copies_in_non_counted_positions",
        ignores_copies_in_non_counted_positions,
    ),
    (
        "applies_its_region_to_cfg_test_modules",
        applies_its_region_to_cfg_test_modules,
    ),
    (
        "applies_its_view_to_doc_comments",
        applies_its_view_to_doc_comments,
    ),
    (
        "needle_requires_a_left_identifier_boundary",
        needle_requires_a_left_identifier_boundary,
    ),
];

const MIGRATION_PROPERTY: &str = "migrating_one_copy_without_lowering_the_baseline_is_red";

/// Every seeded self-test of `row` by property: the pattern properties for a
/// pattern row, and for every row, that a re-added or migrated copy is red.
fn seeded_verdicts(row: &Ratchet, host_file: &str) -> Vec<(&'static str, Result<(), String>)> {
    let mut verdicts: Vec<_> = match &row.measure {
        Measure::Occurrences(pattern) => PATTERN_PROPERTIES
            .iter()
            .map(|&(property, verdict)| (property, verdict(row, pattern)))
            .collect(),
        Measure::DuplicateTopLevelFns { .. } => Vec::new(),
    };
    verdicts.push((
        MIGRATION_PROPERTY,
        migrating_one_copy_without_lowering_the_baseline_is_red(row, host_file),
    ));
    verdicts
}

fn file_under(prefix_entry: &str) -> String {
    match prefix_entry.ends_with('/') {
        true => format!("{prefix_entry}seeded.rs"),
        false => prefix_entry.to_string(),
    }
}

fn seed_path(pattern: &Pattern) -> String {
    file_under(pattern.include[0])
}

fn out_of_scope_paths(pattern: &Pattern) -> Vec<String> {
    let excluded = pattern.exclude.iter().map(|entry| file_under(entry));
    let candidates = ["src/seeded.rs", "tests/seeded.rs", "benches/seeded.rs"]
        .map(String::from)
        .into_iter()
        .filter(|path| !in_scope(path, pattern.include, pattern.exclude));
    excluded.chain(candidates).collect()
}

fn skips_test_items(pattern: &Pattern) -> bool {
    TEST_ITEMS
        .iter()
        .all(|item| pattern.skip_items.contains(item))
}

fn hits_in(row: &Ratchet, path: &str, text: &str) -> Vec<Hit> {
    measure(&row.measure, &[source_file(path, text)])
}

fn expect_hit_count(hits: &[Hit], expected: usize) -> Result<(), String> {
    match hits.len() == expected {
        true => Ok(()),
        false => Err(format!(
            "expected {expected} hit(s), got {}:\n{}",
            hits.len(),
            hit_lines(hits)
        )),
    }
}

/// Hits for `pattern`'s seed in plain code (the control) followed by `tail`.
fn hits_after_control(row: &Ratchet, pattern: &Pattern, tail: &str) -> Vec<Hit> {
    let text = format!("{}\n{tail}", pattern.seed);
    hits_in(row, &seed_path(pattern), &text)
}

fn seed_path_is_in_scope_and_seed_holds_one_needle(
    _: &Ratchet,
    pattern: &Pattern,
) -> Result<(), String> {
    let path = seed_path(pattern);
    if !in_scope(&path, pattern.include, pattern.exclude) {
        return Err(format!("seed path {path} is outside the row's scope"));
    }
    let needles = pattern.seed.matches(pattern.needle).count();
    if needles != 1 || pattern.seed.contains(['\n', '"']) {
        return Err(format!(
            "the seed must be one quote-free line holding the needle once, not {needles}"
        ));
    }
    Ok(())
}

fn flags_one_seeded_inline_copy(row: &Ratchet, pattern: &Pattern) -> Result<(), String> {
    let path = seed_path(pattern);
    let hits = hits_in(row, &path, pattern.seed);
    match hits.as_slice() {
        [hit] if hit.path == path && hit.line == 1 => Ok(()),
        _ => Err(format!(
            "expected one hit at {path}:1, got {}:\n{}",
            hits.len(),
            hit_lines(&hits)
        )),
    }
}

fn counts_each_occurrence_on_a_line(row: &Ratchet, pattern: &Pattern) -> Result<(), String> {
    let doubled = format!("{} {}", pattern.seed, pattern.seed);
    let hits = hits_in(row, &seed_path(pattern), &doubled);
    expect_hit_count(&hits, 2)
}

fn ignores_copies_in_non_counted_positions(row: &Ratchet, pattern: &Pattern) -> Result<(), String> {
    let seed = pattern.seed;
    let mut decoys = vec![
        format!("// {seed}"),
        format!("/* {seed} */"),
        format!("const _S: &str = \"\n{seed}\n\";"),
        format!("const _R: &str = r#\"{seed}\"#;"),
    ];
    for prefix in pattern
        .skip_items
        .iter()
        .filter(|p| !TEST_ITEMS.contains(p))
    {
        decoys.push(format!("{prefix} {{\n{seed}\n}}"));
    }
    for decoy in &decoys {
        expect_hit_count(&hits_after_control(row, pattern, decoy), 1)
            .map_err(|problem| format!("decoy {decoy:?}: {problem}"))?;
    }
    for path in out_of_scope_paths(pattern) {
        let files = [
            source_file(&path, seed),
            source_file(&seed_path(pattern), seed),
        ];
        expect_hit_count(&measure(&row.measure, &files), 1)
            .map_err(|problem| format!("out-of-scope {path}: {problem}"))?;
    }
    Ok(())
}

fn applies_its_region_to_cfg_test_modules(row: &Ratchet, pattern: &Pattern) -> Result<(), String> {
    let seed = pattern.seed;
    let gated_modules = [
        format!("#[cfg(test)]\nmod tests {{\n{seed}\n}}"),
        format!("#[allow(dead_code)] #[cfg(test)] mod tests {{\n{seed}\n}}"),
    ];
    let expected = if skips_test_items(pattern) { 1 } else { 2 };
    for gated in &gated_modules {
        expect_hit_count(&hits_after_control(row, pattern, gated), expected)
            .map_err(|problem| format!("gated module {gated:?}: {problem}"))?;
    }
    Ok(())
}

fn applies_its_view_to_doc_comments(row: &Ratchet, pattern: &Pattern) -> Result<(), String> {
    let seed = pattern.seed;
    let documented = [
        format!("/// {seed}"),
        format!("//! {seed}"),
        format!("/** {seed} */"),
        format!("/*! {seed} */"),
    ];
    let expected = match pattern.view {
        View::Code => 1,
        View::CodeAndDocs => 2,
    };
    for comment in &documented {
        expect_hit_count(&hits_after_control(row, pattern, comment), expected)
            .map_err(|problem| format!("doc comment {comment:?}: {problem}"))?;
    }
    Ok(())
}

fn needle_requires_a_left_identifier_boundary(
    row: &Ratchet,
    pattern: &Pattern,
) -> Result<(), String> {
    if !pattern.needle.starts_with(is_ident_char) {
        return Ok(());
    }
    let glued_needle = format!("x{}", pattern.needle);
    let glued = pattern.seed.replacen(pattern.needle, &glued_needle, 1);
    expect_hit_count(&hits_after_control(row, pattern, &glued), 1)
}

fn seeded_tree(row: &Ratchet, copies: usize) -> Vec<SourceFile> {
    match &row.measure {
        Measure::Occurrences(pattern) => {
            let text = vec![pattern.seed; copies].join("\n");
            vec![source_file(&seed_path(pattern), &text)]
        }
        Measure::DuplicateTopLevelFns { root, .. } => (0..=copies)
            .map(|index| source_file(&format!("{root}seeded_{index}.rs"), "fn seeded_helper() {}"))
            .collect(),
    }
}

fn expect_red_containing(verdict: Result<(), String>, fragments: &[&str]) -> Result<(), String> {
    match verdict {
        Err(message) if fragments.iter().all(|fragment| message.contains(fragment)) => Ok(()),
        other => Err(format!(
            "expected a red verdict containing {fragments:?}, got {other:?}"
        )),
    }
}

/// BT14: two seeded copies hold at baseline 2, a third is red naming the row and
/// its home, and migrating one without lowering the baseline is red.
fn migrating_one_copy_without_lowering_the_baseline_is_red(
    row: &Ratchet,
    host_file: &str,
) -> Result<(), String> {
    let at_two = Ratchet {
        baseline: 2,
        ..*row
    };
    check(&at_two, &seeded_tree(row, 2), host_file)
        .map_err(|red| format!("two copies at baseline 2 must hold, got: {red}"))?;
    let above = check(&at_two, &seeded_tree(row, 3), host_file);
    expect_red_containing(
        above,
        &[
            "above its baseline",
            row.name,
            row.home,
            "raise the baseline to 3",
        ],
    )?;
    let below = check(&at_two, &seeded_tree(row, 1), host_file);
    expect_red_containing(below, &["lower the baseline to 1"])
}

#[cfg(test)]
mod tests {
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
