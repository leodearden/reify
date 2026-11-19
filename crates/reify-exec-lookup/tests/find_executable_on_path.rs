#![cfg(unix)]

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use reify_exec_lookup::find_executable_on_path;

const CWD_FIXTURE_ENV: &str = "REIFY_EXEC_LOOKUP_CWD_FIXTURE";
const CHILD_ASSERTED: &str = "reify-exec-lookup: child asserted";

fn tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("reify-exec-lookup-test-")
        .tempdir()
        .expect("create temp dir")
}

fn fixture(dir: &Path, name: &str, mode: u32) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, "#!/bin/sh\n").expect("write fixture");
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("chmod fixture");
    path
}

fn path_var(dirs: &[&Path]) -> OsString {
    std::env::join_paths(dirs).expect("dirs hold no PATH separator")
}

#[test]
fn first_executable_match_in_path_order_wins() {
    let name = "tool";
    let without_name = tempdir();
    let not_executable = tempdir();
    fixture(not_executable.path(), name, 0o644);
    let directory_named_like_it = tempdir();
    fs::create_dir(directory_named_like_it.path().join(name)).expect("mkdir");
    let first_executable = tempdir();
    let expected = fixture(first_executable.path(), name, 0o755);
    let second_executable = tempdir();
    fixture(second_executable.path(), name, 0o755);

    let path = path_var(&[
        without_name.path(),
        not_executable.path(),
        directory_named_like_it.path(),
        first_executable.path(),
        second_executable.path(),
    ]);

    assert_eq!(
        find_executable_on_path(&[name], Some(&path)),
        Some(expected)
    );
}

#[test]
fn iteration_is_directory_major() {
    let dir1 = tempdir();
    let beta = fixture(dir1.path(), "beta", 0o755);
    let dir2 = tempdir();
    fixture(dir2.path(), "alpha", 0o755);

    let path = path_var(&[dir1.path(), dir2.path()]);

    assert_eq!(
        find_executable_on_path(&["alpha", "beta"], Some(&path)),
        Some(beta)
    );
}

#[test]
fn within_one_directory_the_earlier_name_wins() {
    let dir = tempdir();
    let alpha = fixture(dir.path(), "alpha", 0o755);
    fixture(dir.path(), "beta", 0o755);

    let path = path_var(&[dir.path()]);

    assert_eq!(
        find_executable_on_path(&["alpha", "beta"], Some(&path)),
        Some(alpha)
    );
}

#[test]
fn unset_or_empty_path_finds_nothing() {
    assert_eq!(find_executable_on_path(&["sh"], None), None);
    assert_eq!(find_executable_on_path(&["sh"], Some(OsStr::new(""))), None);

    let dir = tempdir();
    let expected = fixture(dir.path(), "tool", 0o755);
    let leading_empty_component = format!(":{}", dir.path().display());

    assert_eq!(
        find_executable_on_path(&["tool"], Some(OsStr::new(&leading_empty_component))),
        Some(expected)
    );
}

/// No-op unless re-exec'd by the parent below, which runs it with a cwd that
/// holds the executable named by `CWD_FIXTURE_ENV`.
#[test]
fn empty_component_child() {
    let Some(name) = std::env::var_os(CWD_FIXTURE_ENV) else {
        return;
    };
    let name = name.to_str().expect("fixture name is UTF-8");
    assert!(
        Path::new(name).is_file(),
        "the cwd must hold {name} for this check to bite"
    );

    for empty in ["", ":", "::"] {
        assert_eq!(
            find_executable_on_path(&[name], Some(OsStr::new(empty))),
            None,
            "PATH {empty:?} resolved {name} against the cwd"
        );
    }
    println!("{CHILD_ASSERTED}");
}

/// Re-execs this test binary in a cwd holding an executable, so the cwd
/// changes for the child only.
#[test]
fn empty_path_components_never_resolve_against_the_cwd() {
    let cwd = tempdir();
    let name = "reify-exec-lookup-cwd-fixture";
    fixture(cwd.path(), name, 0o755);

    let exe = std::env::current_exe().expect("path to this test binary");
    let out = Command::new(exe)
        .args(["--exact", "empty_component_child", "--nocapture"])
        .current_dir(cwd.path())
        .env(CWD_FIXTURE_ENV, name)
        .output()
        .expect("re-exec this test binary");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "child test failed ({}):\n{stdout}\n{stderr}",
        out.status
    );
    assert!(
        stdout.contains(CHILD_ASSERTED),
        "child never reached its assertions (filter matched no test?):\n{stdout}"
    );
}

#[test]
fn a_file_only_group_and_other_may_execute_is_not_found() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempdir();
    let name = "tool";
    let path = fixture(dir.path(), name, 0o611);
    if fs::metadata(&path).expect("stat fixture").uid() == 0 {
        eprintln!("skipping: root may execute any file with an execute bit");
        return;
    }

    assert_eq!(
        find_executable_on_path(&[name], Some(&path_var(&[dir.path()]))),
        None
    );
}
