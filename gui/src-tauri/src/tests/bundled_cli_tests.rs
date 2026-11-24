//! Tests for where reify-gui looks for the reify CLI it ships
//! (`crate::bundled_cli::{CLI_EXECUTABLE, cli_executable_beside}`): beside the
//! GUI executable, in every layout Tauri installs it to.

use std::path::Path;

use crate::bundled_cli::{CLI_EXECUTABLE, cli_executable_beside};

#[test]
fn cli_is_beside_the_gui_in_every_tauri_layout() {
    let layouts = [
        (
            "cargo target dir",
            "/work/reify/target/release/reify-gui",
            "/work/reify/target/release/reify",
        ),
        ("deb", "/usr/bin/reify-gui", "/usr/bin/reify"),
        (
            "AppImage",
            "/tmp/.mount_ReifyX1/usr/bin/reify-gui",
            "/tmp/.mount_ReifyX1/usr/bin/reify",
        ),
    ];
    for (layout, gui_exe, cli_exe) in layouts {
        assert_eq!(
            cli_executable_beside(Path::new(gui_exe)),
            Ok(Path::new(cli_exe).to_path_buf()),
            "{layout} layout",
        );
    }
}

#[test]
fn relative_gui_exe_is_refused_naming_it() {
    let err = cli_executable_beside(Path::new("reify-gui")).unwrap_err();
    assert!(
        err.contains("reify-gui"),
        "error does not name the path: {err}"
    );
}

#[test]
fn constant_is_the_cli_the_deb_ships() {
    assert_eq!(CLI_EXECUTABLE, "reify");
}
