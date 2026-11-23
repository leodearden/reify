//! The package whose test or bench is RUNNING: its manifest directory and the
//! paths of its `[[bin]]` targets.
//!
//! Both are read from the run-time environment, never through `env!()`: a
//! compile-time bake in a test source makes a warm-lane seed relink that
//! source, rebuilding its target. See the "env!()-baked-path test/bench
//! relink" block in `scripts/seed-warm-lane.sh`.
//!
//! For a path relative to the repo root, use
//! [`crate::workspace_root::workspace_root`] instead.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The running package's manifest directory, from the run-time
/// `CARGO_MANIFEST_DIR`, resolved once.
///
/// # Panics
///
/// Panics, naming the variable, when it is absent or empty.
pub fn manifest_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        non_empty_var("CARGO_MANIFEST_DIR").unwrap_or_else(|| {
            panic!(
                "CARGO_MANIFEST_DIR is unset or empty at run time; cargo test and cargo \
                 nextest set it to the package directory. For a repo-relative path use \
                 reify_test_support::workspace_root::workspace_root()"
            )
        })
    })
}

/// The path of the running package's `[[bin]] name`, from the run-time
/// `CARGO_BIN_EXE_<name>`.
///
/// # Panics
///
/// Panics, naming the variable, when it is absent or empty.
pub fn bin_exe(name: &str) -> PathBuf {
    let var = format!("CARGO_BIN_EXE_{name}");
    non_empty_var(&var).unwrap_or_else(|| {
        panic!(
            "{var} is unset or empty at run time; cargo sets it only for the integration \
             tests and benches of the package that owns [[bin]] {name:?}"
        )
    })
}

fn non_empty_var(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panic_capture::panic_message_of;
    use crate::workspace_root::reexec::{
        FIXTURE_CONSUMER, Launch, fixture_workspace, in_child, run_child,
    };

    /// Not the reify binary's name: `tests/infra/test_no_prebuilt_reify_bin_spawn.sh`
    /// forbids resolving that binary by path outside crates/reify-cli.
    const FIXTURE_BIN: &str = "fixture-tool";

    #[test]
    fn manifest_dir_follows_the_runtime_manifest_dir_child() {
        in_child(|root| assert_eq!(manifest_dir(), root.join(FIXTURE_CONSUMER)));
    }

    #[test]
    fn manifest_dir_follows_the_runtime_manifest_dir() {
        let fixture = fixture_workspace("test-package-manifest-dir-");
        run_child(
            "test_package::tests::manifest_dir_follows_the_runtime_manifest_dir_child",
            fixture.path(),
            Launch::ManifestDirEnv,
        );
    }

    #[test]
    fn manifest_dir_without_cargo_names_the_variable_child() {
        in_child(|_| {
            let message = panic_message_of(manifest_dir);
            assert!(
                message.contains("CARGO_MANIFEST_DIR"),
                "panic does not name the variable: {message}"
            );
        });
    }

    #[test]
    fn manifest_dir_without_cargo_names_the_variable() {
        let fixture = fixture_workspace("test-package-manifest-dir-unset-");
        run_child(
            "test_package::tests::manifest_dir_without_cargo_names_the_variable_child",
            fixture.path(),
            Launch::CwdOnly,
        );
    }

    #[test]
    fn bin_exe_follows_the_runtime_variable_child() {
        in_child(|root| {
            assert_eq!(
                bin_exe(FIXTURE_BIN),
                root.join(FIXTURE_CONSUMER).join(FIXTURE_BIN)
            );
        });
    }

    #[test]
    fn bin_exe_follows_the_runtime_variable() {
        let fixture = fixture_workspace("test-package-bin-exe-");
        run_child(
            "test_package::tests::bin_exe_follows_the_runtime_variable_child",
            fixture.path(),
            Launch::BinExeEnv(FIXTURE_BIN),
        );
    }

    #[test]
    fn bin_exe_unset_names_the_variable() {
        let message = panic_message_of(|| bin_exe("no-such-bin-for-test-package"));
        assert!(
            message.contains("CARGO_BIN_EXE_no-such-bin-for-test-package"),
            "panic does not name the variable: {message}"
        );
    }
}
