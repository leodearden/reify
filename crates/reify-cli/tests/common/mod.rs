// Shared helpers for CLI integration tests.

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use tempfile::TempDir;

/// Resolve a fixture file path relative to the crate's test fixtures directory.
pub fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/tests/fixtures/{}", manifest_dir, name)
}

/// Resolve an example file path relative to the workspace root's `examples/` directory.
///
/// The crate lives at `<root>/crates/reify-cli`, so the examples directory is
/// two levels up: `<CARGO_MANIFEST_DIR>/../../examples/<name>`.
///
/// When the file exists on disk, the path is canonicalized (resolving `..`
/// segments) so that assertion failure messages are readable.  When the file
/// does not yet exist (e.g. it belongs to a sibling task not yet merged), the
/// raw path is returned — callers can still call `.exists()` on it.
#[allow(dead_code)]
pub fn example_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let raw = PathBuf::from(manifest_dir)
        .join("../../examples")
        .join(name);
    std::fs::canonicalize(&raw)
        .unwrap_or(raw)
        .to_string_lossy()
        .into_owned()
}

/// Run `reify <subcommand> <path>` and return `(status, stdout, stderr)`.
///
/// This generic helper avoids duplicating the `Command`-building boilerplate
/// across test files that only differ in the subcommand name (`"check"`, `"test"`,
/// etc.).
#[allow(dead_code)]
pub fn run_subcommand(subcommand: &str, path: &str) -> (ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([subcommand, path])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
}

/// Run `reify <args...>` with arbitrary args and return `(status, stdout, stderr)`.
///
/// Unlike [`run_subcommand`], which forwards exactly a `(subcommand, path)` pair,
/// this helper forwards the full arg list verbatim so tests can pass flags such
/// as `--purpose <value>` (including repeated occurrences). The same
/// `Command`/`Stdio` boilerplate is shared with `run_subcommand`.
#[allow(dead_code)]
pub fn run_with_args(args: &[&str]) -> (ExitStatus, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, stdout, stderr)
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

/// Run `reify build <path> -o <tempdir>/out.step` and return the captured output.
///
/// Unlike [`run_build`], this variant accepts an absolute path directly, making it
/// suitable for example files outside the `tests/fixtures/` directory.
#[allow(dead_code)]
pub fn run_build_at(path: &str) -> BuildOutput {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            path,
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
