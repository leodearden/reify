//! The Rust runtime ignores SIGPIPE before `main`, so a write to a stdout pipe whose reader
//! has gone (`reify … | head`) returns EPIPE and `println!` panics with exit 101, burying the
//! pipeline's real outcome under a panic trace. Restoring SIG_DFL makes reify end like any
//! other Unix filter: terminated by SIGPIPE (shell status 141), silently.
//!
//! Invariant: the disposition is process-wide, so after this call ANY write to a pipe or
//! socket with no reader terminates reify. Today stdout/stderr are its only such writers (no
//! child-stdin pipe, no socket); a future writer of that kind must account for it.
//! Behavioural pin: `crates/reify-cli/tests/harness_cli_surface/cli_broken_pipe.rs`.

/// Call first in `main`, before any output and before any thread is spawned.
#[cfg(unix)]
pub fn restore_default() {
    // SAFETY: SIG_DFL installs no handler, and this runs before any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
pub fn restore_default() {}
