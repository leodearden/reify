//! Tests for `crate::sidecar_runtime::require_sidecar_interpreter`: the host
//! finds the interpreter the sidecar's `#!/usr/bin/env node` line names on the
//! PATH it hands the sidecar, the way `env` does, or refuses with a coded error.

#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::sidecar_runtime::{
    E_GUI_SIDECAR_NODE_MISSING, SIDECAR_INTERPRETER, require_sidecar_interpreter,
};

fn interpreter_file(dir: &Path, mode: u32) -> PathBuf {
    let interpreter = dir.join(SIDECAR_INTERPRETER);
    fs::write(&interpreter, "#!/bin/sh\n").expect("write interpreter fixture");
    fs::set_permissions(&interpreter, fs::Permissions::from_mode(mode))
        .expect("chmod interpreter fixture");
    interpreter
}

fn path_var(dirs: &[&TempDir]) -> OsString {
    std::env::join_paths(dirs.iter().map(|dir| dir.path())).expect("join PATH")
}

fn temp_dir() -> TempDir {
    tempfile::tempdir().expect("create temp dir")
}

#[test]
fn resolves_the_first_executable_node_in_path_order() {
    let without_node = temp_dir();
    let not_executable = temp_dir();
    interpreter_file(not_executable.path(), 0o644);
    let first_executable = temp_dir();
    let expected = interpreter_file(first_executable.path(), 0o755);
    let second_executable = temp_dir();
    interpreter_file(second_executable.path(), 0o755);
    let path = path_var(&[
        &without_node,
        &not_executable,
        &first_executable,
        &second_executable,
    ]);

    assert_eq!(
        require_sidecar_interpreter(Some(path.as_os_str())),
        Ok(expected)
    );
}

#[test]
fn a_directory_named_node_is_not_an_interpreter() {
    let dir = temp_dir();
    fs::create_dir(dir.path().join(SIDECAR_INTERPRETER)).expect("create node dir");
    let path = path_var(&[&dir]);

    assert!(require_sidecar_interpreter(Some(path.as_os_str())).is_err());
}

#[test]
fn no_executable_node_on_path_is_refused_naming_the_searched_path() {
    let without_node = temp_dir();
    let not_executable = temp_dir();
    interpreter_file(not_executable.path(), 0o644);
    let path = path_var(&[&without_node, &not_executable]);

    let refusal =
        require_sidecar_interpreter(Some(path.as_os_str())).expect_err("no executable node");

    assert!(
        refusal.starts_with(E_GUI_SIDECAR_NODE_MISSING),
        "refusal must start with its code: {refusal}"
    );
    let searched = path.to_str().expect("temp PATH is UTF-8");
    assert!(
        refusal.contains(searched),
        "refusal must name the searched PATH {searched}: {refusal}"
    );
}

#[test]
fn an_unset_path_is_refused() {
    let refusal = require_sidecar_interpreter(None).expect_err("no PATH");

    assert!(
        refusal.starts_with(E_GUI_SIDECAR_NODE_MISSING),
        "refusal must start with its code: {refusal}"
    );
}
