// Shared helpers for CLI integration tests.

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use tempfile::TempDir;

/// Resolve a fixture file path relative to the crate's test fixtures directory.
pub fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/{}", manifest_dir, name)
}

/// Captures the output of a `reify build` invocation.
#[allow(dead_code)]
pub struct BuildOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
    pub output_path: PathBuf,
    /// Keeps the temp directory alive so `output_path` remains valid.
    #[allow(dead_code)]
    _dir: TempDir,
}

/// Run `reify build <fixture> -o <tempdir>/out.step` and return the captured output.
///
/// `fixture` is the fixture filename (e.g. `"bracket.ri"`), resolved via [`fixture_path`].
#[allow(dead_code)]
pub fn run_build(fixture: &str) -> BuildOutput {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &fixture_path(fixture),
            "-o",
            output_path.to_str().expect("temp path is not valid UTF-8"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    BuildOutput {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        output_path,
        _dir: dir,
    }
}
