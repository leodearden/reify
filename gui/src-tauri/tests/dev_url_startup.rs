//! Pin: the `reify-gui` binary reads `REIFY_VITE_PORT` at startup and refuses
//! a malformed value before it boots the engine or opens a window, rather than
//! silently loading the devUrl baked from `tauri.conf.json`.

#![cfg(all(target_os = "linux", feature = "gui"))]

use std::process::{Command, Stdio};

#[test]
fn reify_gui_refuses_an_invalid_vite_port_before_opening_a_window() {
    let cache_home = tempfile::tempdir().expect("tempdir for XDG_CACHE_HOME");

    let output = Command::new(env!("CARGO_BIN_EXE_reify-gui"))
        .env("REIFY_VITE_PORT", "not-a-port")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("XDG_CACHE_HOME", cache_home.path())
        .stdin(Stdio::null())
        .output()
        .expect("spawn reify-gui");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "reify-gui must exit 2 on a malformed REIFY_VITE_PORT; status {:?}, stderr:\n{stderr}",
        output.status
    );
    assert!(
        stderr.contains("REIFY_VITE_PORT"),
        "stderr must name the variable; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("not-a-port"),
        "stderr must quote the rejected value; stderr:\n{stderr}"
    );
}
