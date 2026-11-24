//! Pin: `reify-gui --version` and `-V` print the version every shipped
//! binary reports, headless, before any GTK or tauri initialisation.

#![cfg(all(target_os = "linux", feature = "gui"))]

mod common;

#[test]
fn version_flags_print_the_reify_version_crate_version() {
    for flag in ["--version", "-V"] {
        let output = common::run_hermetic_reify_gui(|command| {
            command.arg(flag);
        });
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "reify-gui {flag} must exit 0; status {:?}, stdout:\n{stdout}\nstderr:\n{stderr}",
            output.status
        );
        assert_eq!(
            stdout,
            format!("reify-gui {}\n", reify_version::VERSION),
            "reify-gui {flag} stdout; status {:?}, stderr:\n{stderr}",
            output.status
        );
    }
}
