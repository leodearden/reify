//! Tests for which executable reify-gui runs as the reify CLI it ships
//! (`crate::bundled_cli::cli_executable_beside`). Where a shipped executable
//! lives is `shipped_executables_tests`'s subject; the CLI's file name is
//! pinned end to end by `tests/infra/test_bundle_clean_install.py`.

use std::path::Path;

use crate::bundled_cli::{CLI_EXECUTABLE, cli_executable_beside};

#[test]
fn cli_is_the_cli_executable_beside_the_gui() {
    assert_eq!(
        cli_executable_beside(Path::new("/usr/bin/reify-gui")),
        Ok(Path::new("/usr/bin").join(CLI_EXECUTABLE)),
    );
}
