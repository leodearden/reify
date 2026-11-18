//! Tests for where the host looks for the Claude sidecar executable
//! (`crate::claude_bridge::{SIDECAR_EXECUTABLE, sidecar_executable_beside}`):
//! beside the GUI executable, in every layout Tauri installs it to.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use crate::claude_bridge::{SIDECAR_EXECUTABLE, sidecar_executable_beside};

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().expect("fixture path has a parent")).unwrap();
    fs::write(path, b"").unwrap();
}

#[test]
fn deb_layout_finds_the_installed_sidecar_in_usr_bin() {
    let root = tempfile::TempDir::new().unwrap();
    let gui = root.path().join("usr/bin/reify-gui");
    let sidecar = root.path().join("usr/bin/reify-sidecar");
    touch(&gui);
    touch(&sidecar);
    touch(&root.path().join("usr/lib/Reify/sandbox/landlock_exec.py"));

    let resolved = sidecar_executable_beside(&gui).unwrap();

    assert_eq!(resolved, sidecar);
    assert!(resolved.is_file(), "{} is not a file", resolved.display());
}

#[test]
fn appimage_layout_finds_the_sidecar_beside_the_gui() {
    let app_dir = Path::new("/tmp/.mount_ReifyX1/");
    assert_eq!(
        sidecar_executable_beside(&app_dir.join("usr/bin/reify-gui")).unwrap(),
        app_dir.join("usr/bin/reify-sidecar"),
    );
}

#[test]
fn relative_gui_exe_is_refused_naming_it() {
    let err = sidecar_executable_beside(Path::new("reify-gui")).unwrap_err();
    assert!(
        err.contains("reify-gui"),
        "error does not name the path: {err}"
    );
}

#[test]
fn parentless_gui_exe_is_refused_naming_it() {
    let err = sidecar_executable_beside(Path::new("/")).unwrap_err();
    assert!(err.contains("\"/\""), "error does not name the path: {err}");
}

#[test]
fn constant_matches_the_bundled_external_bin() {
    let conf_path = Path::new(&super::gui_crate_manifest_dir()).join("tauri.conf.json");
    let conf: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&conf_path).unwrap()).unwrap();
    let external_bins: Vec<&str> = conf["bundle"]["externalBin"]
        .as_array()
        .unwrap_or_else(|| {
            panic!(
                "{}: bundle.externalBin is not an array",
                conf_path.display()
            )
        })
        .iter()
        .filter_map(|entry| entry.as_str())
        .collect();

    let matching: Vec<&str> = external_bins
        .iter()
        .copied()
        .filter(|entry| Path::new(entry).file_name() == Some(OsStr::new(SIDECAR_EXECUTABLE)))
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "bundle.externalBin {external_bins:?} must have exactly one entry named {SIDECAR_EXECUTABLE}",
    );
}
