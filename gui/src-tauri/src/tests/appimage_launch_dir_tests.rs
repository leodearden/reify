//! Tests for returning reify-gui to the directory the AppImage was run from
//! (`crate::appimage_launch_dir::launch_dir`). Driven only through the lookup
//! closure and an explicit current directory, never the real process
//! environment or cwd.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::appimage_launch_dir::launch_dir;

/// Real directories laid out the way an AppImage launch sees them.
struct Fixture {
    _root: tempfile::TempDir,
    appdir: PathBuf,
    appdir_usr: PathBuf,
    launch: PathBuf,
    other: PathBuf,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("create tempdir");
    let appdir = root.path().join("appdir");
    let appdir_usr = appdir.join("usr");
    let launch = root.path().join("launch");
    let other = root.path().join("other");
    for dir in [&appdir_usr, &launch, &other] {
        fs::create_dir_all(dir).expect("create fixture dir");
    }
    Fixture {
        _root: root,
        appdir,
        appdir_usr,
        launch,
        other,
    }
}

fn lookup_with(vars: &[(&'static str, OsString)]) -> impl Fn(&str) -> Option<OsString> {
    let vars: HashMap<&'static str, OsString> = vars.iter().cloned().collect();
    move |asked| vars.get(asked).cloned()
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn assert_launch_dir(found: Option<PathBuf>, expected: &Path) {
    let found = found.unwrap_or_else(|| panic!("expected Some({})", expected.display()));
    assert_eq!(canonical(&found), canonical(expected));
}

#[test]
fn unset_appdir_restores_nothing() {
    let f = fixture();
    let var = lookup_with(&[
        ("OWD", f.launch.clone().into()),
        ("PWD", f.launch.clone().into()),
    ]);
    assert_eq!(launch_dir(var, &f.appdir_usr), None);
}

#[test]
fn empty_appdir_restores_nothing() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", OsString::new()),
        ("OWD", f.launch.clone().into()),
    ]);
    assert_eq!(launch_dir(var, &f.appdir_usr), None);
}

#[test]
fn fuse_mode_restores_owd() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", f.launch.clone().into()),
    ]);
    assert_launch_dir(launch_dir(var, &f.appdir_usr), &f.launch);
}

#[test]
fn extract_and_run_mode_restores_pwd() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("PWD", f.launch.clone().into()),
    ]);
    assert_launch_dir(launch_dir(var, &f.appdir_usr), &f.launch);
}

#[test]
fn owd_wins_over_pwd() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", f.launch.clone().into()),
        ("PWD", f.other.clone().into()),
    ]);
    assert_launch_dir(launch_dir(var, &f.appdir_usr), &f.launch);
}

#[test]
fn missing_owd_falls_back_to_pwd() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", f.launch.join("missing").into()),
        ("PWD", f.launch.clone().into()),
    ]);
    assert_launch_dir(launch_dir(var, &f.appdir_usr), &f.launch);
}

#[test]
fn missing_owd_and_pwd_restore_nothing() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", f.launch.join("missing").into()),
        ("PWD", f.other.join("missing").into()),
    ]);
    assert_eq!(launch_dir(var, &f.appdir_usr), None);
}

#[test]
fn relative_candidates_are_ignored() {
    let f = fixture();
    let owd_only = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", OsString::from("launch")),
    ]);
    assert_eq!(launch_dir(owd_only, &f.appdir_usr), None);
    let pwd_only = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("PWD", OsString::from("launch")),
    ]);
    assert_eq!(launch_dir(pwd_only, &f.appdir_usr), None);
}

#[test]
fn cwd_outside_appdir_usr_restores_nothing() {
    let f = fixture();
    let var = lookup_with(&[
        ("APPDIR", f.appdir.clone().into()),
        ("OWD", f.launch.clone().into()),
        ("PWD", f.launch.clone().into()),
    ]);
    assert_eq!(launch_dir(var, &f.other), None);
}
