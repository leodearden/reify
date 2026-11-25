//! Pin: reify-gui answers `--help`, unknown options and `cli` before any GTK or
//! tauri initialisation, and `cli` runs the reify CLI beside the executable,
//! from the directory an AppImage was run from.
//!
//! The CLI is a stub written beside a hard link to the freshly built reify-gui,
//! so these tests pin reify-gui's forwarding (arguments, stdout, exit status,
//! cwd), never a prebuilt `reify`.

#![cfg(all(target_os = "linux", feature = "gui"))]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use reify_gui::bundled_cli::CLI_EXECUTABLE;
use reify_gui::command_line::USAGE;
use reify_test_support::test_package::bin_exe;

/// A hard link to the freshly built reify-gui in a tempdir on the same
/// filesystem, with room beside it for a stub CLI and launch-dir fixtures.
struct LinkedGui {
    dir: tempfile::TempDir,
    exe: PathBuf,
}

fn linked_gui() -> LinkedGui {
    let built = bin_exe("reify-gui");
    let built_dir = built.parent().expect("reify-gui has a parent directory");
    let dir = tempfile::tempdir_in(built_dir)
        .unwrap_or_else(|e| panic!("tempdir in {}: {e}", built_dir.display()));
    let exe = dir.path().join("reify-gui");
    fs::hard_link(&built, &exe)
        .unwrap_or_else(|e| panic!("hard-link {} to {}: {e}", built.display(), exe.display()));
    LinkedGui { dir, exe }
}

impl LinkedGui {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn mkdir(&self, name: &str) -> PathBuf {
        let dir = self.path(name);
        fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir {}: {e}", dir.display()));
        dir
    }

    fn run(&self, configure: impl FnOnce(&mut std::process::Command)) -> Output {
        common::run_hermetic(&self.exe, configure)
    }
}

/// Writes an executable `#!/bin/sh` script running `body` where reify-gui
/// looks for its bundled CLI.
fn write_stub_cli(dir: &Path, body: &str) {
    let stub = dir.join(CLI_EXECUTABLE);
    fs::write(&stub, format!("#!/bin/sh\n{body}\n"))
        .unwrap_or_else(|e| panic!("write {}: {e}", stub.display()));
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| panic!("chmod {}: {e}", stub.display()));
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|e| panic!("canonicalize {}: {e}", path.display()))
}

fn describe(output: &Output) -> String {
    format!(
        "status {:?}, stdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const PRINT_PWD: &str = "pwd -P";

#[test]
fn help_flags_print_usage_and_exit_0() {
    for flag in ["--help", "-h"] {
        let output = common::run_hermetic_reify_gui(|command| {
            command.arg(flag);
        });
        assert!(
            output.status.success(),
            "reify-gui {flag} must exit 0; {}",
            describe(&output)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            USAGE,
            "reify-gui {flag} stdout; {}",
            describe(&output)
        );
    }
}

#[test]
fn unknown_option_is_a_usage_error() {
    let output = common::run_hermetic_reify_gui(|command| {
        command.arg("--no-such-option");
    });
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{}", describe(&output));
    assert!(output.stdout.is_empty(), "{}", describe(&output));
    assert!(
        stderr.contains("--no-such-option") && stderr.contains("--help"),
        "stderr must name the option and point at --help; {}",
        describe(&output)
    );
}

#[test]
fn cli_forwards_arguments_stdout_and_exit_status() {
    let gui = linked_gui();
    write_stub_cli(
        gui.dir.path(),
        "for arg in \"$@\"; do printf '%s\\n' \"$arg\"; done\nexit 7",
    );
    let output = gui.run(|command| {
        command.args(["cli", "check", "a b.ri"]);
    });
    assert_eq!(output.status.code(), Some(7), "{}", describe(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "check\na b.ri\n",
        "{}",
        describe(&output)
    );
}

#[test]
fn cli_without_a_bundled_cli_exits_127_naming_the_path() {
    let gui = linked_gui();
    let output = gui.run(|command| {
        command.args(["cli", "--version"]);
    });
    let looked_for = canonical(gui.dir.path()).join(CLI_EXECUTABLE);
    assert_eq!(output.status.code(), Some(127), "{}", describe(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(&looked_for.display().to_string()),
        "stderr must name {}; {}",
        looked_for.display(),
        describe(&output)
    );
}

#[test]
fn cli_runs_from_the_appimage_launch_dir() {
    let gui = linked_gui();
    write_stub_cli(gui.dir.path(), PRINT_PWD);
    let appdir = gui.path("appdir");
    let appdir_usr = gui.mkdir("appdir/usr");
    let launch = gui.mkdir("launch");
    let output = gui.run(|command| {
        command
            .arg("cli")
            .current_dir(&appdir_usr)
            .env("APPDIR", &appdir)
            .env("PWD", &launch);
    });
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", canonical(&launch).display()),
        "{}",
        describe(&output)
    );
}

#[test]
fn cli_keeps_its_cwd_outside_an_appimage() {
    let gui = linked_gui();
    write_stub_cli(gui.dir.path(), PRINT_PWD);
    let launch = gui.mkdir("launch");
    let other = gui.mkdir("other");
    let output = gui.run(|command| {
        command.arg("cli").current_dir(&other).env("PWD", &launch);
    });
    assert!(output.status.success(), "{}", describe(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{}\n", canonical(&other).display()),
        "{}",
        describe(&output)
    );
}
