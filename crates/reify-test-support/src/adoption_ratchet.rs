//! The adoption-ratchet engine: counts the inline copies of a primitive in a
//! crate's `src/` and `tests/` and holds each count at a committed baseline.

mod census;
mod health;
mod items;
mod lexer;
mod source;

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
}
