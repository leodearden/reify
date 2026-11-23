#[cfg(test)]
mod tests {
    use super::*;
    use crate::panic_capture::panic_message_of;
    use crate::workspace_root::reexec::{
        FIXTURE_CONSUMER, Launch, fixture_workspace, in_child, run_child,
    };

    /// Not "reify": `.join("reify")` outside crates/reify-cli trips
    /// `tests/infra/test_no_prebuilt_reify_bin_spawn.sh`'s Check 2.
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
