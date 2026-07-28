// Shared helpers for CLI integration tests.

use std::path::{Path, PathBuf};
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
    run_args(None, &[subcommand, path])
}

/// The one `Command`/`Stdio` spawn every helper in this module shares.
///
/// `cwd == None` inherits the test binary's working directory — exactly what
/// the unpinned helpers did before this was factored out, so their behaviour is
/// unchanged (no `current_dir` call is made at all).
fn run_args(cwd: Option<&Path>, args: &[&str]) -> (ExitStatus, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_reify"));
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd.output().expect("failed to execute reify binary");

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
    run_args(None, args)
}

/// Run `reify <args...>` with the child process's working directory pinned to
/// `cwd`, returning `(status, stdout, stderr)`.
///
/// The cwd-pinned twin of [`run_with_args`]. Every test module here compiles
/// into the SAME `harness_cli` binary, so this is the module-shared place for a
/// cwd-pinned spawn — a per-module copy would be duplication rather than
/// isolation.
///
/// It is NOT (yet) the only such spawn in the harness, and this docstring does
/// not claim to have consolidated one: two hand-rolled cwd-pinned copies remain
/// outside this file — `cli_build_outputs.rs`'s private `run_in` (which returns
/// `bool` rather than `ExitStatus`, so it is a signature change, not a
/// substitution) and `cli_imported_field_eval.rs`'s inline `current_dir` call.
/// Both live in test modules outside task #6170's lock set and were left
/// untouched deliberately; migrating them is a mechanical follow-up for whoever
/// next holds those files.
///
/// Pinning matters whenever a test asserts on DESIGN-FILE-relative artifact
/// paths (io-export B7): it keeps every artifact inside the caller's tempdir and
/// guarantees a stray write cannot land in `tests/fixtures/` or the crate root.
#[allow(dead_code)]
pub fn run_with_args_in(cwd: &Path, args: &[&str]) -> (ExitStatus, String, String) {
    run_args(Some(cwd), args)
}

// ── Relocated reify-eval e2e helpers (task #5718) ─────────────────────────────
//
// The CLI-spawning e2e tests that used to live in `reify-eval` were relocated
// here so cargo's own build graph guarantees a freshly built `reify` — see
// `harness_cli/cli_*_golden.rs` and `tests/infra/test_no_prebuilt_reify_bin_spawn.sh`.
//
// Their fixtures and golden corpus deliberately STAY in `crates/reify-eval/tests/`
// (goldens are governed by #5618's regeneration ruling and are not moved), so the
// helpers below resolve those paths from the workspace root. The spawn helper
// preserves `.current_dir(<workspace root>)` exactly as every original site set
// it: four of the relocated tests assert byte-identical golden stdout, so a cwd
// change would shift the goldens as a side effect of the move — indistinguishable
// from the real drift these tests exist to detect.

/// The workspace root — `<CARGO_MANIFEST_DIR>/../..`, canonicalized.
///
/// `reify-cli` lives at `<root>/crates/reify-cli`, so the root is two levels up.
/// Falls back to the un-canonicalized path if canonicalization fails, keeping
/// assertion messages readable either way.
#[allow(dead_code)]
pub fn workspace_root() -> PathBuf {
    let raw = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(&raw).unwrap_or(raw)
}

/// Resolve a fixture under `crates/reify-eval/tests/fixtures/`.
///
/// `rel` is the path relative to that directory, e.g.
/// `"backcompat/bt1_single_scope.ri"`.
#[allow(dead_code)]
pub fn eval_fixture_path(rel: &str) -> PathBuf {
    workspace_root()
        .join("crates/reify-eval/tests/fixtures")
        .join(rel)
}

/// Resolve a golden under `crates/reify-eval/tests/golden/<stem>.txt`.
///
/// The golden corpus keeps its existing single location; only the tests that
/// read it moved.
#[allow(dead_code)]
pub fn eval_golden_path(stem: &str) -> PathBuf {
    workspace_root()
        .join("crates/reify-eval/tests/golden")
        .join(format!("{stem}.txt"))
}

/// Run `reify <subcommand> <path>` from the workspace root; return
/// `(success, stdout, stderr)`.
///
/// The binary is `env!("CARGO_BIN_EXE_reify")`, which cargo builds before this
/// crate's integration tests run — the whole point of the #5718 relocation.
#[allow(dead_code)]
pub fn run_cli_from_workspace_root(subcommand: &str, path: &Path) -> (bool, String, String) {
    let bin = env!("CARGO_BIN_EXE_reify");
    let out = Command::new(bin)
        .current_dir(workspace_root())
        .arg(subcommand)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn the cargo-built reify binary at {bin} for \
                 `{subcommand} {}`: {e}",
                path.display()
            )
        });
    let stdout = String::from_utf8(out.stdout).expect("stdout must be valid UTF-8");
    let stderr = String::from_utf8(out.stderr).expect("stderr must be valid UTF-8");
    (out.status.success(), stdout, stderr)
}

/// Run `reify eval <path>` from the workspace root; return
/// `(success, stdout, stderr)`.
#[allow(dead_code)]
pub fn run_eval_from_workspace_root(path: &Path) -> (bool, String, String) {
    run_cli_from_workspace_root("eval", path)
}

/// Assert `stdout` matches the on-disk golden, regenerating when
/// `REIFY_REGENERATE_GOLDEN` is set.
///
/// Returns `true` when regeneration happened, so the caller can return early
/// without running its remaining assertions (the BT2 order-independence test
/// relies on this).
#[allow(dead_code)]
pub fn assert_or_regenerate_golden(stdout: &str, golden: &Path) -> bool {
    let label = golden
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| golden.display().to_string());

    if std::env::var("REIFY_REGENERATE_GOLDEN").is_ok() {
        std::fs::create_dir_all(
            golden
                .parent()
                .expect("golden must be inside a directory"),
        )
        .expect("create golden parent dir");
        std::fs::write(golden, stdout)
            .unwrap_or_else(|e| panic!("failed to write golden {label}: {e}"));
        return true;
    }

    let expected = std::fs::read_to_string(golden).unwrap_or_else(|_| {
        panic!(
            "golden {label} missing at {}; \
             run once with REIFY_REGENERATE_GOLDEN=1 to generate",
            golden.display()
        )
    });
    assert_eq!(
        stdout, expected,
        "`reify eval` stdout drifted from golden {label}; \
         re-run with REIFY_REGENERATE_GOLDEN=1 to update",
    );
    false
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
/// Unlike [`run_build`], this variant takes the design path VERBATIM rather than
/// resolving it through [`fixture_path`], making it suitable for example files
/// outside the `tests/fixtures/` directory. [`run_build`] is this function plus
/// that resolution, so the tempdir + `-o` + spawn shape is written once.
///
/// The spawn itself is [`run_args`] — the same one every helper above uses — so
/// stdio wiring cannot drift between the `(status, stdout, stderr)` helpers and
/// the [`BuildOutput`] ones.
#[allow(dead_code)]
pub fn run_build_at(path: &str) -> BuildOutput {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.step");
    let (status, stdout, stderr) = run_args(
        None,
        &[
            "build",
            path,
            "-o",
            output_path.to_str().expect("temp path is not valid UTF-8"),
        ],
    );

    BuildOutput {
        status,
        stdout,
        stderr,
        output_path,
        _dir: dir,
    }
}

/// Run `reify build <fixture> -o <tempdir>/out.step` and return the captured output.
///
/// `fixture` is the fixture filename (e.g. `"bracket.ri"`), resolved via
/// [`fixture_path`]; everything after that resolution is [`run_build_at`].
#[allow(dead_code)]
pub fn run_build(fixture: &str) -> BuildOutput {
    run_build_at(&fixture_path(fixture))
}
