//! The git seam: the [`GitOps`] trait every detector reads git through, its production [`RealGitOps`] impl, and the test-support `MockGitOps`.

#[cfg(test)]
mod tests {
    use super::parse_rename_target;

    /// The measured single-rename shape: `git show -M --name-status --format=`
    /// prints `R<score>\t<old>\t<new>`.
    #[test]
    fn parse_rename_target_matches_measured_rename_line() {
        let stdout = "R100\told.rs\tsub/new.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "old.rs"),
            Some("sub/new.rs".to_string()),
            "an R line whose old side matches must yield the new side",
        );
    }

    /// Modelled on 60be72d922, whose measured shape has the rename line NOT
    /// first (one unrelated `A` line precedes it), so the scan must not stop at
    /// the first non-`R` line. The preceding lines here are synthetic — the
    /// property under test is the position of the `R` line, not the exact
    /// neighbours that commit happens to carry.
    #[test]
    fn parse_rename_target_finds_rename_after_other_status_lines() {
        let stdout = "A\tcrates/reify-compiler/tests/harness_doc_chunks/mod.rs\n\
                      M\tcrates/reify-compiler/src/lib.rs\n\
                      R100\tcrates/reify-compiler/tests/geometry_chunk_smoke.rs\tcrates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "crates/reify-compiler/tests/geometry_chunk_smoke.rs"),
            Some(
                "crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs"
                    .to_string()
            ),
            "a rename line preceded by A/M lines must still be found",
        );
    }

    /// A genuine delete prints only `D\t<path>` lines — the fail-safe path that
    /// keeps `task-cites-deleted-path` unchanged.
    #[test]
    fn parse_rename_target_delete_only_output_is_none() {
        let stdout = "D\tdoomed.rs\nD\tcrates/other.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "doomed.rs"),
            None,
            "a delete-only commit must not resolve a rename target",
        );
    }

    /// An `R` line for a DIFFERENT path must not match: the old side is the
    /// key, so the relation can never be inverted or cross-wired.
    #[test]
    fn parse_rename_target_other_path_rename_is_none() {
        let stdout = "R100\tunrelated.rs\tsub/unrelated.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "old.rs"),
            None,
            "an R line whose old side is a different path must not match",
        );
        assert_eq!(
            parse_rename_target(stdout, "sub/unrelated.rs"),
            None,
            "querying the rename TARGET as if it were the source must not match",
        );
    }

    /// Empty stdout — the measured MERGE-commit shape (`git show` defaults to
    /// `--cc`, which prints no diff for a merge) and the empty-output case in
    /// general.
    #[test]
    fn parse_rename_target_empty_stdout_is_none() {
        assert_eq!(
            parse_rename_target("", "old.rs"),
            None,
            "empty stdout (e.g. a merge commit) must not resolve a rename target",
        );
    }
}
