//! Shared by the integration tests that launch the real `reify-gui` binary.
//! Each consuming test binary declares `mod common;`.

use std::process::{Command, Output, Stdio};

/// Runs `reify-gui` hermetic and headless, then returns its output.
///
/// No display reaches the child (`DISPLAY` and `WAYLAND_DISPLAY` removed,
/// `GDK_BACKEND=x11` so a `wayland-0` socket cannot be found either), so a
/// regressed binary dies at GTK init instead of opening a window. It gets a
/// private `XDG_CACHE_HOME`, no inherited `REIFY_*` knob, and a null stdin.
/// `configure` adds the test's own arguments and environment on top.
pub fn run_hermetic_reify_gui(configure: impl FnOnce(&mut Command)) -> Output {
    let cache_home = tempfile::tempdir().expect("tempdir for XDG_CACHE_HOME");

    let mut command = Command::new(env!("CARGO_BIN_EXE_reify-gui"));
    command
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env("GDK_BACKEND", "x11")
        .env_remove("REIFY_VITE_PORT")
        .env_remove("REIFY_CACHE_DIR")
        .env_remove("REIFY_DEBUG")
        .env("XDG_CACHE_HOME", cache_home.path())
        .stdin(Stdio::null());
    configure(&mut command);

    command.output().expect("spawn reify-gui")
}
