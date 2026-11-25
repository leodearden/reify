//! Tests for where reify-gui finds the executables its package ships
//! (`crate::shipped_executables::beside_gui`): beside the GUI executable, in
//! every layout it runs from, and never by a path `Command::new` would search
//! `PATH` for.

use std::path::Path;

use crate::shipped_executables::beside_gui;

#[test]
fn executable_is_beside_the_gui_in_every_layout() {
    let layouts = [
        ("cargo target dir", "/work/reify/target/release"),
        ("deb", "/usr/bin"),
        ("AppImage", "/tmp/.mount_ReifyX1/usr/bin"),
    ];
    for (layout, dir) in layouts {
        let dir = Path::new(dir);
        assert_eq!(
            beside_gui(&dir.join("reify-gui"), "shipped-tool"),
            Ok(dir.join("shipped-tool")),
            "{layout} layout",
        );
    }
}

#[test]
fn relative_gui_exe_is_refused_naming_it() {
    let err = beside_gui(Path::new("reify-gui"), "shipped-tool").unwrap_err();
    assert!(
        err.contains("\"reify-gui\""),
        "error does not name the path: {err}"
    );
}

#[test]
fn parentless_gui_exe_is_refused_naming_it() {
    let err = beside_gui(Path::new("/"), "shipped-tool").unwrap_err();
    assert!(err.contains("\"/\""), "error does not name the path: {err}");
}

#[test]
fn refusal_names_the_executable_sought() {
    let err = beside_gui(Path::new("reify-gui"), "shipped-tool").unwrap_err();
    assert!(
        err.contains("shipped-tool"),
        "error does not name the executable: {err}"
    );
}
