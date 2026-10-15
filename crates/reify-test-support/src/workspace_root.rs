/// Re-exec harness for wiring tests. In a normal build, the tree this crate was
/// compiled in and the tree the test runs in are the same, so an in-process
/// test cannot tell a compile-time bake from a run-time resolution. A parent
/// test therefore re-runs this test binary with a runtime `CARGO_MANIFEST_DIR`
/// (or cwd) pointing into a hermetic fixture workspace. Its child asserts that
/// the production seam resolved the fixture.
///
/// Precedent: `reify-build-utils`'s `find_gmsh_honours_env_overrides`.
/// `std::env::set_var` is never used: it is unsafe in edition 2024 and races
/// libtest's threads.
#[cfg(test)]
pub(crate) mod reexec {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::ROOT_MARKER;

    /// Carries the fixture root to the child, and is the child's "act" guard:
    /// a child test run any other way is a no-op.
    const FIXTURE_ROOT_ENV: &str = "REIFY_TEST_SUPPORT_REEXEC_FIXTURE_ROOT";

    /// Printed by [`in_child`] only after its body returns, so the parent can
    /// tell a real pass from libtest's filter matching zero tests.
    const CHILD_REACHED: &str = "reify-test-support: re-exec child reached its assertions";

    /// The fixture's stand-in for the crate whose tests call into this one.
    pub(crate) const FIXTURE_CONSUMER: &str = "crates/consumer";

    /// Which runtime signal the child is given to find the fixture.
    pub(crate) enum Launch {
        /// `CARGO_MANIFEST_DIR` points at the fixture consumer; the cwd stays
        /// the parent's real crate dir, so a pass proves the env var wins.
        ManifestDirEnv,
        /// `CARGO_MANIFEST_DIR` is removed and the cwd is the fixture consumer.
        CwdOnly,
    }

    /// A minimal workspace: the root marker plus an empty
    /// [`FIXTURE_CONSUMER`] directory.
    pub(crate) fn fixture_workspace(prefix: &str) -> TempDir {
        let fixture = crate::temp_dirs::prefixed_tempdir(prefix);
        let marker = fixture.path().join(ROOT_MARKER);
        let marker_dir = marker.parent().expect("ROOT_MARKER has a parent directory");
        std::fs::create_dir_all(marker_dir)
            .unwrap_or_else(|e| panic!("create {}: {e}", marker_dir.display()));
        std::fs::write(&marker, "").unwrap_or_else(|e| panic!("write {}: {e}", marker.display()));
        let consumer = fixture.path().join(FIXTURE_CONSUMER);
        std::fs::create_dir_all(&consumer)
            .unwrap_or_else(|e| panic!("create {}: {e}", consumer.display()));
        fixture
    }

    /// The child half of a wiring pair: runs `body` with the fixture root only
    /// when re-exec'd by [`run_child`], then prints the reached sentinel.
    pub(crate) fn in_child(body: impl FnOnce(&Path)) {
        let Some(fixture_root) = std::env::var_os(FIXTURE_ROOT_ENV) else {
            return;
        };
        body(Path::new(&fixture_root));
        println!("{CHILD_REACHED}");
    }

    /// The parent half of a wiring pair: re-runs this test binary on exactly
    /// `child_test` under `launch`, and asserts the child passed after
    /// reaching its assertions.
    pub(crate) fn run_child(child_test: &str, fixture_root: &Path, launch: Launch) {
        let exe = std::env::current_exe().expect("path to this test binary");
        let consumer = fixture_root.join(FIXTURE_CONSUMER);
        let mut cmd = Command::new(exe);
        cmd.args(["--exact", child_test, "--nocapture", "--test-threads=1"])
            .env(FIXTURE_ROOT_ENV, fixture_root);
        match launch {
            Launch::ManifestDirEnv => {
                cmd.env("CARGO_MANIFEST_DIR", &consumer);
            }
            Launch::CwdOnly => {
                cmd.env_remove("CARGO_MANIFEST_DIR").current_dir(&consumer);
            }
        }
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("re-exec this test binary for {child_test}: {e}"));

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "child test {child_test} failed ({}):\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
            out.status
        );
        assert!(
            stdout.contains(CHILD_REACHED),
            "child test {child_test} never reached its assertions (filter matched no test?):\n\
             --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::reexec::{FIXTURE_CONSUMER, Launch, fixture_workspace, in_child, run_child};
    use super::*;
    use crate::temp_dirs::prefixed_tempdir;

    /// A fresh directory that no ancestor-marker walk can resolve, or `None`
    /// (with the reason on stderr) when the temp dir itself sits inside a
    /// reify tree, e.g. a `TMPDIR` inside a checkout. The premise is checked on
    /// the filesystem directly, not through the function under test.
    fn dir_outside_any_tree(prefix: &str) -> Option<tempfile::TempDir> {
        let dir = prefixed_tempdir(prefix);
        if let Some(holder) = dir.path().ancestors().find(|d| d.join(ROOT_MARKER).is_file()) {
            eprintln!(
                "skipping: temp dir {} sits inside a tree whose root {} holds {ROOT_MARKER}",
                dir.path().display(),
                holder.display()
            );
            return None;
        }
        Some(dir)
    }

    #[test]
    fn find_workspace_root_returns_the_nearest_marker_ancestor() {
        let fixture = fixture_workspace("workspace-root-nearest-");
        let root = fixture.path();
        let deep = root.join(FIXTURE_CONSUMER).join("src/deep");
        std::fs::create_dir_all(&deep).unwrap_or_else(|e| panic!("create {}: {e}", deep.display()));

        assert_eq!(find_workspace_root(&deep).as_deref(), Some(root));
        assert_eq!(
            find_workspace_root(root).as_deref(),
            Some(root),
            "the walk is inclusive of its start"
        );
    }

    #[test]
    fn find_workspace_root_prefers_the_innermost_of_nested_trees() {
        let outer = fixture_workspace("workspace-root-nested-");
        let inner = outer.path().join("vendor/inner");
        let inner_marker = inner.join(ROOT_MARKER);
        std::fs::create_dir_all(inner_marker.parent().expect("ROOT_MARKER has a parent"))
            .unwrap_or_else(|e| panic!("create parent of {}: {e}", inner_marker.display()));
        std::fs::write(&inner_marker, "")
            .unwrap_or_else(|e| panic!("write {}: {e}", inner_marker.display()));
        let start = inner.join(FIXTURE_CONSUMER);
        std::fs::create_dir_all(&start).unwrap_or_else(|e| panic!("create {}: {e}", start.display()));

        assert_eq!(find_workspace_root(&start), Some(inner));
    }

    #[test]
    fn find_workspace_root_is_none_outside_any_tree() {
        let Some(outside) = dir_outside_any_tree("workspace-root-outside-") else {
            return;
        };
        assert_eq!(find_workspace_root(outside.path()), None);
    }

    #[test]
    fn resolve_workspace_root_prefers_manifest_dir_then_falls_back_to_cwd() {
        let a = fixture_workspace("workspace-root-policy-a-");
        let b = fixture_workspace("workspace-root-policy-b-");
        let a_consumer = a.path().join(FIXTURE_CONSUMER);
        let b_consumer = b.path().join(FIXTURE_CONSUMER);

        assert_eq!(
            resolve_workspace_root(Some(&a_consumer), Some(&b_consumer)).ok().as_deref(),
            Some(a.path()),
            "a resolvable CARGO_MANIFEST_DIR wins over the cwd"
        );
        assert_eq!(
            resolve_workspace_root(None, Some(&b_consumer)).ok().as_deref(),
            Some(b.path()),
            "an absent CARGO_MANIFEST_DIR falls back to the cwd"
        );

        let Some(no_marker) = dir_outside_any_tree("workspace-root-policy-none-") else {
            return;
        };
        assert_eq!(
            resolve_workspace_root(Some(no_marker.path()), Some(&b_consumer)).ok().as_deref(),
            Some(b.path()),
            "an unresolvable CARGO_MANIFEST_DIR falls back to the cwd"
        );
    }

    #[test]
    fn unresolved_workspace_root_names_the_marker_and_both_candidates() {
        let (Some(manifest_dir), Some(cwd)) = (
            dir_outside_any_tree("workspace-root-unresolved-manifest-"),
            dir_outside_any_tree("workspace-root-unresolved-cwd-"),
        ) else {
            return;
        };

        let err = resolve_workspace_root(Some(manifest_dir.path()), Some(cwd.path()))
            .expect_err("neither candidate has a marker ancestor");
        let message = err.to_string();

        for needle in [
            ROOT_MARKER.to_string(),
            format!("{:?}", manifest_dir.path()),
            format!("{:?}", cwd.path()),
        ] {
            assert!(message.contains(&needle), "message lacks {needle}: {message}");
        }
    }

    #[test]
    fn workspace_root_holds_the_marker_and_contains_the_runtime_manifest_dir() {
        let root = workspace_root();
        assert!(
            root.join(ROOT_MARKER).is_file(),
            "workspace_root() {} does not hold {ROOT_MARKER}",
            root.display()
        );
        if let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") {
            assert!(
                Path::new(&manifest_dir).starts_with(root),
                "runtime CARGO_MANIFEST_DIR {manifest_dir:?} is not under workspace_root() {}",
                root.display()
            );
        }
    }

    #[test]
    fn workspace_root_follows_the_runtime_manifest_dir_child() {
        in_child(|root| assert_eq!(workspace_root(), root));
    }

    #[test]
    fn workspace_root_follows_the_runtime_manifest_dir() {
        let fixture = fixture_workspace("workspace-root-manifest-dir-");
        run_child(
            "workspace_root::tests::workspace_root_follows_the_runtime_manifest_dir_child",
            fixture.path(),
            Launch::ManifestDirEnv,
        );
    }

    #[test]
    fn workspace_root_falls_back_to_the_cwd_child() {
        in_child(|root| assert_eq!(workspace_root(), root));
    }

    #[test]
    fn workspace_root_falls_back_to_the_cwd() {
        let fixture = fixture_workspace("workspace-root-cwd-");
        run_child(
            "workspace_root::tests::workspace_root_falls_back_to_the_cwd_child",
            fixture.path(),
            Launch::CwdOnly,
        );
    }
}
