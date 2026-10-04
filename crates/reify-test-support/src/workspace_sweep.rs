//! Anti-vacuity guard for workspace-wide source-sweep ratchets.
//!
//! A sweep that reports the workspace clean means something only if it
//! actually read the workspace. A walker that silently stops descending, a
//! workspace root resolved to the wrong directory, or source files that
//! silently fail to read all turn "zero violations" into a vacuous pass.
//! [`assert_workspace_rs_walk_is_healthy`] turns each of those into a loud,
//! specific panic. A ratchet calls it before trusting its own clean result.

use crate::ignore_hygiene::walk_rs_files;
use crate::temp_dirs::count_readable_rs_files;
use std::path::Path;

/// Floor on the `.rs` files a whole-workspace walk must return. The first
/// ratchet to carry this guard measured 1758 files. 500 sits far enough
/// below the real count that ordinary file churn never trips it, and still an
/// order of magnitude above what a partial-subtree breakage returns.
const MIN_WORKSPACE_RS_FILES: usize = 500;

/// How many walked files may fail to read before the sweep is declared blind.
/// Not zero: a concurrent build can legitimately delete a file mid-walk, and
/// the sweep collectors skip per-file I/O errors for the same reason.
const MAX_UNREADABLE_RS_FILES: usize = 5;

/// Walk every `.rs` file under `workspace_root` with [`walk_rs_files`] and
/// assert that the walk covers the workspace, so that a clean sweep over the
/// same tree is meaningful.
///
/// Each guard catches one distinct way a sweep can scan nothing and still
/// pass:
/// 1. **Count**: more than [`MIN_WORKSPACE_RS_FILES`] files, which catches a
///    broken walker or a mis-resolved `workspace_root`.
/// 2. **Sentinel**: `sentinel`, a path relative to `workspace_root`, was
///    walked. The caller picks a file in the subtree its sweep most needs to
///    reach, so losing that subtree gets its own diagnostic instead of
///    disappearing under the count floor.
/// 3. **Read health**: at most [`MAX_UNREADABLE_RS_FILES`] walked files fail
///    to read as UTF-8. Guards 1 and 2 count files before any read, so a
///    systematic read failure (a permissions change, non-UTF-8 source) would
///    otherwise empty the sweep's real coverage and leave the walked count
///    untouched.
///
/// # Panics
///
/// When any guard fails; the message names the guard.
pub fn assert_workspace_rs_walk_is_healthy(workspace_root: &Path, sentinel: &str) {
    let walked = walk_rs_files(workspace_root, |_| true);
    assert!(
        walked.len() > MIN_WORKSPACE_RS_FILES,
        "walker found only {} .rs file(s) under {workspace_root:?}, expected \
         more than {MIN_WORKSPACE_RS_FILES}; the walker may be broken, or the \
         workspace root resolved to the wrong directory",
        walked.len()
    );

    let sentinel_path = workspace_root.join(sentinel);
    assert!(
        walked.contains(&sentinel_path),
        "sentinel file {sentinel_path:?} not found in walker output; the walker \
         may be broken, or no longer reaches that subtree"
    );

    let readable = count_readable_rs_files(&walked);
    let unread = walked.len().saturating_sub(readable);
    assert!(
        unread <= MAX_UNREADABLE_RS_FILES,
        "the walker found {} .rs file(s) but only {readable} could be read; \
         {unread} file(s) silently failed (non-UTF-8 source, permission denied, \
         or similar), so the sweep may be blind to real violations in those \
         files. Investigate before trusting a clean result.",
        walked.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL: &str = "crates/sentinel/src/lib.rs";

    /// A fixture workspace of `file_count` readable `.rs` files, optionally
    /// including [`SENTINEL`] among them.
    fn fixture_workspace(file_count: usize, with_sentinel: bool) -> crate::temp_dirs::TempDir {
        let guard = crate::temp_dirs::prefixed_tempdir("workspace-sweep-");
        let src = guard.path().join("crates/filler/src");
        std::fs::create_dir_all(&src).expect("create fixture dir");
        let filler_count = if with_sentinel {
            let sentinel = guard.path().join(SENTINEL);
            std::fs::create_dir_all(sentinel.parent().expect("sentinel has a parent"))
                .expect("create sentinel dir");
            std::fs::write(&sentinel, "fn sentinel() {}\n").expect("write sentinel");
            file_count - 1
        } else {
            file_count
        };
        for i in 0..filler_count {
            std::fs::write(src.join(format!("f{i}.rs")), "fn f() {}\n").expect("write filler");
        }
        guard
    }

    #[test]
    fn accepts_a_walk_that_clears_every_guard() {
        let workspace = fixture_workspace(MIN_WORKSPACE_RS_FILES + 1, true);

        assert_workspace_rs_walk_is_healthy(workspace.path(), SENTINEL);
    }

    #[test]
    #[should_panic(expected = "walker found only")]
    fn rejects_a_walk_at_or_below_the_count_floor() {
        let workspace = fixture_workspace(MIN_WORKSPACE_RS_FILES, true);

        assert_workspace_rs_walk_is_healthy(workspace.path(), SENTINEL);
    }

    #[test]
    #[should_panic(expected = "not found in walker output")]
    fn rejects_a_walk_that_misses_the_sentinel() {
        let workspace = fixture_workspace(MIN_WORKSPACE_RS_FILES + 1, false);

        assert_workspace_rs_walk_is_healthy(workspace.path(), SENTINEL);
    }

    #[test]
    #[should_panic(expected = "silently failed")]
    fn rejects_a_walk_whose_files_do_not_read() {
        let workspace = fixture_workspace(MIN_WORKSPACE_RS_FILES + 1, true);
        for i in 0..=MAX_UNREADABLE_RS_FILES {
            std::fs::write(
                workspace.path().join(format!("crates/filler/src/f{i}.rs")),
                [0xff, 0xfe],
            )
            .expect("overwrite filler with non-UTF-8 bytes");
        }

        assert_workspace_rs_walk_is_healthy(workspace.path(), SENTINEL);
    }
}
