//! Pin: the `reify-gui` binary puts its WebKit environment defaults into its
//! own process environment at the top of `main`, on every launch path.

#![cfg(all(target_os = "linux", feature = "gui"))]

use std::process::{Command, Output, Stdio};

const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

fn print_webkit_env(dmabuf: Option<&str>) -> Output {
    let cache_home = tempfile::tempdir().expect("tempdir for XDG_CACHE_HOME");

    let mut command = Command::new(env!("CARGO_BIN_EXE_reify-gui"));
    command
        .arg("--print-webkit-env")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("GDK_BACKEND", "x11")
        .env_remove("REIFY_VITE_PORT")
        .env_remove("REIFY_CACHE_DIR")
        .env_remove("REIFY_DEBUG")
        .env("XDG_CACHE_HOME", cache_home.path())
        .stdin(Stdio::null());
    match dmabuf {
        Some(value) => command.env(DMABUF, value),
        None => command.env_remove(DMABUF),
    };

    let output = command.output().expect("spawn reify-gui");
    drop(cache_home);
    output
}

fn assert_reports(output: &Output, expected_line: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "reify-gui --print-webkit-env must exit 0; status {:?}, stdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        stdout.lines().any(|line| line == expected_line),
        "stdout must carry the line `{expected_line}`; status {:?}, stdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
}

#[test]
fn reify_gui_disables_the_dmabuf_renderer_by_default() {
    assert_reports(&print_webkit_env(None), "WEBKIT_DISABLE_DMABUF_RENDERER=1");
}

#[test]
fn reify_gui_keeps_a_callers_explicit_dmabuf_opt_out() {
    assert_reports(
        &print_webkit_env(Some("0")),
        "WEBKIT_DISABLE_DMABUF_RENDERER=0",
    );
}
