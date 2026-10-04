//! `reify` writing into a stdout pipe whose reader has already gone must end the way a Unix
//! filter does — terminated by SIGPIPE — not with a Rust panic (exit 101, "failed printing
//! to stdout: Broken pipe"); task #7906.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output, Stdio};

fn run_with_closed_stdout(args: &[&str]) -> Output {
    let cache_dir = tempfile::tempdir().expect("create hermetic REIFY_CACHE_DIR");
    let (reader, writer) = std::io::pipe().expect("create stdout pipe");
    // Closed before spawn, so the child's first stdout write can never land in a pipe buffer.
    drop(reader);
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .expect("failed to execute reify binary")
}

fn assert_terminated_by_sigpipe_without_panic(output: &Output, invocation: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "`{invocation}` into a closed stdout panicked (status {}); the pre-fix signature is \
         exit 101 with 'failed printing to stdout: Broken pipe'. stderr:\n{stderr}",
        output.status
    );
    assert_eq!(
        output.status.signal(),
        Some(libc::SIGPIPE),
        "`{invocation}` into a closed stdout must be terminated by SIGPIPE, got status {}. \
         stderr:\n{stderr}",
        output.status
    );
}

#[test]
fn version_into_closed_stdout_is_terminated_by_sigpipe_without_panic() {
    let output = run_with_closed_stdout(&["--version"]);
    assert_terminated_by_sigpipe_without_panic(&output, "reify --version");
}

/// `check` writes its report only after parse, compile, eval and the kernel build, and it
/// discards write errors, so this pins the disposition well past the startup path.
#[test]
fn check_into_closed_stdout_is_terminated_by_sigpipe_without_panic() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");
    let output = run_with_closed_stdout(&["check", fixture]);
    assert_terminated_by_sigpipe_without_panic(&output, "reify check bracket.ri");
}
