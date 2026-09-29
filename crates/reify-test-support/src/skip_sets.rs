//! Support for SKIP_SET dead-key guards: the test-suite checks that every
//! skip-list entry still names a file on disk, so a renamed or deleted file
//! cannot silently disable coverage.

#[cfg(test)]
mod tests {
    // ─── missing_paths_under contract ─────────────────────────────────────

    /// Prefix for every temp dir these `missing_paths_under` tests create, so
    /// SIGKILL debris under `/tmp` stays attributable to this suite (see
    /// `temp_dirs::prefixed_tempdir`'s "Names stay attributable" section).
    const MISSING_PATHS_TEMPDIR_PREFIX: &str = "reify-missing-paths-under-";

    /// Materialise a "present" fixture at `dir.join(rel)`, creating any parent
    /// directories the forward-slash-separated `rel` implies.
    fn touch_under(dir: &std::path::Path, rel: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|e| panic!("create parent dirs for fixture {rel:?}: {e}"));
        }
        std::fs::write(&path, "// present fixture\n")
            .unwrap_or_else(|e| panic!("write fixture {rel:?}: {e}"));
    }

    /// missing_paths_under: over a mixed skip list, exactly the entries with no
    /// file on disk come back — in input order.
    ///
    /// One case carries the whole contract on purpose. Entries are ordered
    /// missing, present, missing, present, so neither a short-circuit on the
    /// first miss nor an off-by-one can pass; the comparison is made WITHOUT
    /// sorting, so an order-scrambling implementation cannot pass either; and
    /// the keys are nested, forward-slash-separated paths — the real SKIP_SET
    /// key shape — so `dir.join(rel)` resolution is exercised for a present
    /// nested file, a missing sibling, and a path under an entirely absent
    /// subdirectory.
    #[test]
    fn test_missing_paths_under_flags_only_missing_entries_in_input_order() {
        let guard = crate::temp_dirs::prefixed_tempdir(MISSING_PATHS_TEMPDIR_PREFIX);
        let dir = guard.path();
        touch_under(dir, "topology_selectors/fillet_top_edges.ri");
        touch_under(dir, "present.ri");

        let missing = super::missing_paths_under(
            dir,
            [
                "topology_selectors/deleted_by_a_rename.ri",
                "topology_selectors/fillet_top_edges.ri",
                "auto/never_existed.ri",
                "present.ri",
            ],
        );

        assert_eq!(
            missing,
            vec![
                "topology_selectors/deleted_by_a_rename.ri",
                "auto/never_existed.ri"
            ],
            "expected exactly the two entries with no file on disk, in input order and \
             compared without sorting: an implementation that short-circuited on the first \
             miss would drop 'auto/never_existed.ri', and neither materialised fixture \
             (nested or top-level) may be flagged; got {missing:?}"
        );
    }

    /// missing_paths_under: an empty input iterator yields an empty `Vec`
    /// rather than panicking, even when `dir` names a path that does not
    /// exist. (Whether the call touches the filesystem at all is not something
    /// this test can observe, so it does not claim it.)
    #[test]
    fn test_missing_paths_under_empty_input_yields_empty_vec() {
        let empty: [&str; 0] = [];
        let missing =
            super::missing_paths_under(std::path::Path::new("/definitely/not/a/real/dir"), empty);

        assert!(
            missing.is_empty(),
            "expected an empty input iterator to yield an empty Vec, even for a `dir` that \
             does not exist; got {missing:?}"
        );
    }

    /// missing_paths_under: a dangling symlink reports as *missing*, pinning the
    /// documented `Path::exists` semantics — it follows symlinks, so a link whose
    /// target is gone is indistinguishable from an absent path.
    ///
    /// This is the one documented filesystem behaviour with a real failure mode
    /// behind it: an `examples/` entry that decays into a dangling link trips a
    /// SKIP_SET guard exactly as a deleted file would.
    #[cfg(unix)]
    #[test]
    fn test_missing_paths_under_reports_dangling_symlink_as_missing() {
        let guard = crate::temp_dirs::prefixed_tempdir(MISSING_PATHS_TEMPDIR_PREFIX);
        let dir = guard.path();
        touch_under(dir, "live_target.ri");
        std::os::unix::fs::symlink(dir.join("live_target.ri"), dir.join("live_link.ri"))
            .expect("create resolvable symlink fixture");
        std::os::unix::fs::symlink(dir.join("deleted_target.ri"), dir.join("dangling_link.ri"))
            .expect("create dangling symlink fixture");

        let missing = super::missing_paths_under(dir, ["live_link.ri", "dangling_link.ri"]);

        assert_eq!(
            missing,
            vec!["dangling_link.ri"],
            "expected the symlink whose target is gone to report as missing and the one \
             pointing at a live file not to — Path::exists resolves through the link; \
             got {missing:?}"
        );
    }
}
