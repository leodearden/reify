//! `reify` writing into a stdout pipe whose reader has already gone must end the way a Unix
//! filter does — terminated by SIGPIPE — not with a Rust panic (exit 101, "failed printing
//! to stdout: Broken pipe"); task #7906.

use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Output, Stdio};

/// Points stdout at a pipe nobody can read. The pipe is built in the forked child rather than in
/// the multi-threaded test process, where a parallel test's fork could copy the read end and
/// keep a reader alive across the fork-to-exec window (#10035).
///
/// # Safety
///
/// Call only in a forked child before exec: it replaces the caller's stdout.
unsafe fn point_stdout_at_a_pipe_without_reader() -> io::Result<()> {
    let mut pipe_fds: [libc::c_int; 2] = [-1; 2];
    // SAFETY: plain fd syscalls on a buffer and descriptors that only this child holds.
    unsafe {
        if libc::pipe(pipe_fds.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error());
        }
        let [read_end, write_end] = pipe_fds;
        libc::close(read_end);
        if libc::dup2(write_end, libc::STDOUT_FILENO) == -1 {
            return Err(io::Error::last_os_error());
        }
        libc::close(write_end);
    }
    Ok(())
}

fn run_with_closed_stdout(args: &[&str]) -> Output {
    let cache_dir = tempfile::tempdir().expect("create hermetic REIFY_CACHE_DIR");
    let mut command = Command::new(reify_test_support::test_package::bin_exe("reify"));
    command
        .args(args)
        .env("REIFY_CACHE_DIR", cache_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null()) // the hook below replaces fd 1 in the child
        .stderr(Stdio::piped());
    // SAFETY: runs in the forked child before exec and calls only async-signal-safe libc.
    unsafe { command.pre_exec(|| point_stdout_at_a_pipe_without_reader()) };
    command.output().expect("failed to execute reify binary")
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
    let fixture =
        reify_test_support::test_package::manifest_dir().join("tests/fixtures/bracket.ri");
    let output = run_with_closed_stdout(&["check", fixture.to_str().expect("UTF-8 fixture path")]);
    assert_terminated_by_sigpipe_without_panic(&output, "reify check bracket.ri");
}
