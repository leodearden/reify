//! Panic-safe temporary directories for tests, with forensically attributable
//! names.
//!
//! Test code that builds a directory under [`std::env::temp_dir`] and tears it
//! down with a trailing `fs::remove_dir_all(...)` leaks that directory whenever
//! an assertion fails and the test unwinds — i.e. precisely on RED CI runs.
//! This module supplies the RAII replacement.
//!
//! # The workspace-wide invariant
//!
//! [`collect_workspace_unguarded_temp_dirs`] sweeps every `.rs` file in the
//! workspace — `src/`, `tests/`, and `build.rs` alike — for sites that defeat
//! the guard, and this module's `workspace_has_no_unguarded_temp_dirs` test
//! enforces it BY DEFAULT, workspace-wide. The invariant is narrowed only two
//! ways:
//!
//! - a per-line `// temp-dir:allow — reason` escape at the offending site, or
//! - a comment-justified entry in that test's `SWEEP_EXCEPTIONS` table, for a
//!   file that is migrating but is not clean yet.
//!
//! A `SWEEP_EXCEPTIONS` entry must be DELETED the moment its file goes clean
//! — the table is asserted against staleness in both directions (see
//! `partition_enforced_and_stale`), so a stale entry fails loudly rather than
//! rotting silently the way this module's earlier six-entry allowlist did.
//!
//! [`assert_no_unguarded_temp_dir_sites`] remains available as a targeted
//! single-file guard for callers who want to pin one specific file rather
//! than rely on the workspace sweep.

use crate::ignore_hygiene::{is_doc_comment_line, walk_rs_files};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Re-exported so call sites can NAME the guard type (in a helper's return
/// signature, say) through `reify_test_support` without their own crate
/// gaining a `tempfile` dependency.
pub use tempfile::TempDir;

/// The per-line escape hatch, mirroring the house `// ptodo:allow — reason`
/// convention documented in `CLAUDE.md`.  A site that legitimately builds its
/// own directory (this module's own implementation, say) annotates the line
/// `// temp-dir:allow — reason` and the scanner passes over it.
const ALLOW_ESCAPE: &str = "temp-dir:allow";

/// Post-mortem opt-out: when this variable is set to a non-empty value, EVERY
/// guard built by [`prefixed_tempdir`] retains its directory instead of removing
/// it on drop, and prints the retained path to stderr.
///
/// This restores — globally, for one run, without editing any call site — the
/// post-mortem affordance that per-file "we deliberately don't clean up" helpers
/// used to provide. It matters most for tests whose artifact under test IS the
/// directory contents (`reify doc --out <dir>` writing `index.html`, say): on a
/// RED run the guard would otherwise delete exactly the output an engineer needs
/// to read.
///
/// Retained directories are NOT cleaned up by anything. That is the point, and
/// it is why this is opt-in per run rather than a default.
pub const KEEP_TEMP_DIRS_ENV: &str = "REIFY_KEEP_TEMP_DIRS";

/// The hand-rolled-construction needle, assembled at runtime.
///
/// DO NOT inline this as a literal: `temp_dirs.rs` legitimately calls the real
/// thing in its own attribution test, and an inlined literal would make this
/// module self-triggering the moment it were added to the scanned set. This is
/// the same defence `ignore_hygiene.rs` uses for its `#[ignore = "` marker.
///
/// One needle covers both spellings — `std::env::temp_dir()` (what every
/// migrating file used before the guard) and a bare `env::temp_dir()` under
/// `use std::env`.
fn guarded_call_needle() -> String {
    ["env", "::temp_dir("].concat()
}

/// The unbound-guard needles, assembled at runtime for the same reason.
///
/// Returns `(call, immediate_path)`. A line carrying BOTH is the immediate-drop
/// one-liner this module warns about four separate times:
/// `prefixed_tempdir("x-").path().to_path_buf()` compiles, drops the `TempDir`
/// at the end of that statement, and deletes the directory before the test uses
/// it — surfacing as a confusing downstream `ENOENT` rather than a clear
/// failure. Requiring both needles on one line is what keeps the ordinary
/// two-line form (`let guard = ...;` then `guard.path()`) clean.
fn unbound_guard_needles() -> (String, String) {
    (["prefixed_", "tempdir("].concat(), [")", ".path("].concat())
}

/// Whether the [`KEEP_TEMP_DIRS_ENV`] opt-out is engaged, given the raw variable.
///
/// Takes the already-read value rather than reading the process environment so
/// both branches are unit-testable without mutating process-global state (which
/// libtest's concurrency makes unsound to do from a test).
fn keep_requested(raw: Option<&OsStr>) -> bool {
    matches!(raw, Some(value) if !value.is_empty())
}

/// Create a temporary directory under [`std::env::temp_dir`] whose name starts
/// with `prefix`, guarded by [`TempDir`]'s RAII teardown.
///
/// # The directory is removed on every unwinding exit path
///
/// `Drop` runs on normal scope exit, on early return, AND while unwinding out
/// of a panicking assertion. That last path is the whole point: a trailing
/// `fs::remove_dir_all(...)` at the end of a test body is skipped exactly when
/// the test fails, so hand-rolled teardown leaks on RED runs and only on RED
/// runs.
///
/// # The returned guard MUST be bound to a named local
///
/// It has to outlive every use of the directory — including, in `async` tests,
/// every `.await`. Writing
///
/// ```ignore
/// let dir = prefixed_tempdir("x-").path().to_path_buf(); // WRONG
/// ```
///
/// compiles, but the temporary `TempDir` is dropped at the end of that
/// statement and the directory is deleted before the test ever uses it. Write
/// two lines instead:
///
/// ```ignore
/// let guard = prefixed_tempdir("x-");
/// let dir = guard.path().to_path_buf();
/// ```
///
/// [`find_unguarded_temp_dir_sites`] flags the wrong form.
///
/// # Names stay attributable
///
/// `Drop` cannot run on SIGKILL, OOM-kill or power loss, so residual debris
/// stays possible and must remain traceable to the test that made it:
/// `find /tmp -maxdepth 1 -name '<prefix>*'` is the triage tool. That is why
/// this wraps [`tempfile::Builder::prefix`] rather than plain
/// `tempfile::tempdir()`, which would name every directory an anonymous
/// `.tmpXXXXXX` and leave the next operator worse off than manual teardown.
///
/// Callers do not need a `{pid}` component: `Builder` creates the directory
/// with `O_EXCL`, so uniqueness is already guaranteed.
///
/// # Keeping the directory for a post-mortem
///
/// Setting [`KEEP_TEMP_DIRS_ENV`] (`REIFY_KEEP_TEMP_DIRS=1`) retains every
/// directory this function hands out and echoes each path to stderr, so a RED
/// run can be diagnosed from the artifacts it produced:
///
/// ```text
/// REIFY_KEEP_TEMP_DIRS=1 cargo test -p reify-cli --test harness_cli
/// ```
///
/// Nothing removes the retained directories afterwards — delete them by hand.
///
/// # Panics
///
/// Panics if the directory cannot be created — this is test-support code, and
/// an unwritable temp dir is not a condition any caller can recover from.
pub fn prefixed_tempdir(prefix: &str) -> TempDir {
    let raw = std::env::var_os(KEEP_TEMP_DIRS_ENV);
    prefixed_tempdir_with_retention(prefix, keep_requested(raw.as_deref()))
}

/// The retention-explicit core of [`prefixed_tempdir`], split out so both
/// branches of the [`KEEP_TEMP_DIRS_ENV`] opt-out are unit-testable without
/// mutating the process environment.
fn prefixed_tempdir_with_retention(prefix: &str, retain: bool) -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .disable_cleanup(retain)
        .tempdir()
        .unwrap_or_else(|e| panic!("create temp dir with prefix {prefix:?}: {e}"));

    if retain {
        eprintln!(
            "[{KEEP_TEMP_DIRS_ENV}] retaining {} — nothing will remove it; delete it by hand",
            dir.path().display()
        );
    }
    dir
}

/// Scan `source` (a Rust source file as a string) for temp-dir handling that
/// defeats the guard. Returns one human-readable violation per offending line,
/// each carrying the 1-based line number, the violation kind, and the trimmed
/// line. An empty `Vec` means clean.
///
/// Two kinds are reported:
///
/// - **hand-rolled construction** — a call to `std::env::temp_dir()` that is not
///   routed through [`prefixed_tempdir`], torn down (if at all) by a trailing
///   `fs::remove_dir_all(..)` that a panicking assertion skips.
/// - **unbound guard** — [`prefixed_tempdir`] called and `.path()` taken in the
///   same statement, so the `TempDir` drops immediately and the directory is
///   gone before the test uses it. This one is worse than the idiom it replaced:
///   the hand-rolled form at least worked on GREEN runs.
///
/// Line-oriented, in the shape of the sibling
/// [`crate::ignore_hygiene::find_stale_plan_pointers_in_source`]:
///
/// - `///` and `//!` doc-comment lines are skipped, so prose that merely
///   mentions the call does not fire. Regular `//` comments are NOT skipped —
///   commented-out construction code is still a site worth reporting, and the
///   remediation message tells a prose-mention author what to do instead.
/// - A line carrying the `// temp-dir:allow — reason` escape is skipped.
/// - EVERY hit is collected, not just the first: `server.rs` holds nine sites
///   and `m5_integration.rs` two, and a first-hit-only scan would make those
///   look half-migrated.
pub fn find_unguarded_temp_dir_sites(source: &str) -> Vec<String> {
    let construction = guarded_call_needle();
    let (guard_call, immediate_path) = unbound_guard_needles();

    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_doc_comment_line(line))
        .filter(|(_, line)| !line.contains(ALLOW_ESCAPE))
        .filter_map(|(idx, line)| {
            let kind = if line.contains(construction.as_str()) {
                "hand-rolled construction"
            } else if line.contains(guard_call.as_str()) && line.contains(immediate_path.as_str()) {
                "unbound guard"
            } else {
                return None;
            };
            let preview: String = line.trim().chars().take(120).collect();
            Some(format!("line {}: {kind}: {preview:?}", idx + 1))
        })
        .collect()
}

/// Check one already-read source for temp-dir hygiene, returning the full
/// remediation message on failure.
///
/// Split out of [`assert_no_unguarded_temp_dir_sites`] so the message — the
/// entire user-facing value of the wrapper over the raw
/// [`find_unguarded_temp_dir_sites`] — is directly testable, mirroring the
/// sibling [`crate::ignore_hygiene::check_ignore_reasons`] signature.
/// `display_path` appears verbatim in the message and is only ever formatted,
/// never opened.
pub fn check_temp_dir_hygiene(source: &str, display_path: &str) -> Result<(), String> {
    let violations = find_unguarded_temp_dir_sites(source);
    if violations.is_empty() {
        return Ok(());
    }

    Err(format!(
        "{} temp-dir hygiene violation(s) in {display_path}:\n  {}\n\n\
         `hand-rolled construction` — the line builds a directory under {}) by \
         hand and relies on a trailing `fs::remove_dir_all(..)` that is SKIPPED \
         when the test unwinds, so it leaks precisely on RED runs.\n\
         `unbound guard` — the line takes `.path()` off the guard in the same \
         statement that creates it, so the directory is deleted BEFORE the test \
         uses it and the failure surfaces as a confusing downstream ENOENT.\n\n\
         Both are fixed the same way — a NAMED LOCAL outliving the test body \
         (and, in async tests, every `.await`):\n\
         \x20   let guard = reify_test_support::prefixed_tempdir(\"<prefix>-\");\n\
         \x20   let dir = guard.path().to_path_buf();\n\n\
         Only `///` and `//!` lines are skipped, so a regular `//` comment that \
         merely MENTIONS the call in prose is reported too: move the mention into \
         a `///` doc comment, or annotate the line `// {} — prose mention`. A \
         site that is legitimately hand-rolled takes the same escape with its own \
         reason.",
        violations.len(),
        violations.join("\n  "),
        guarded_call_needle(),
        ALLOW_ESCAPE,
    ))
}

/// Assert that the workspace file at `repo_relative_path` contains no temp-dir
/// hygiene violation, panicking with every violation and the remediation if it
/// does.
///
/// The repo root is resolved at compile time from `env!("CARGO_MANIFEST_DIR")`
/// evaluated inside **this** crate, which always sits at
/// `<repo>/crates/reify-test-support/`; two `.parent()` walks reach the root.
/// This is the same resolution [`crate::orphan_audit::run_orphan_audit`] uses,
/// so a guard here can scan a file owned by any other crate.
///
/// Passing an ABSOLUTE path is also supported — [`Path::join`] discards the base
/// when its argument is absolute — which is how this function's own tests point
/// it at synthetic fixtures. Production guards should pass repo-relative paths
/// so a moved file is obvious in the panic text.
///
/// # Panics
///
/// Panics if the file cannot be read, naming the absolute path it attempted —
/// a moved or renamed file must fail loudly rather than turn this guard into a
/// vacuous pass. Panics with the collected violations if any are found.
pub fn assert_no_unguarded_temp_dir_sites(repo_relative_path: &str) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/reify-test-support has a parent (crates/)")
        .parent()
        .expect("crates/ has a parent (the repo root)");
    let absolute = repo_root.join(repo_relative_path);

    let source = std::fs::read_to_string(&absolute).unwrap_or_else(|e| {
        panic!(
            "temp-dir hygiene scan could not read {}: {e}\n\
             If this file moved, update the guard's path — do not delete the guard.",
            absolute.display()
        )
    });

    if let Err(message) = check_temp_dir_hygiene(&source, &absolute.display().to_string()) {
        panic!("{message}");
    }
}

/// Walk the entire workspace collecting every temp-dir hygiene violation
/// found in ANY `.rs` file — `src/`, `tests/`, and `build.rs` alike, since a
/// leaking temp dir in a build script is just as real as one in a test. This
/// is what closes the gap [`assert_no_unguarded_temp_dir_sites`]'s six
/// hand-picked call sites left open: those never saw a `src/` file.
///
/// The structural twin of
/// [`crate::ignore_hygiene::collect_workspace_stale_pointers`]: walk with
/// [`walk_rs_files`] under an accept-everything predicate, read, scan with
/// [`find_unguarded_temp_dir_sites`], and format each hit as
/// `"<relative/path>: <violation>"`. I/O errors on individual files are
/// silently skipped — a file deleted mid-walk by a concurrent build must not
/// become a spurious violation.
///
/// This is a PURE COLLECTOR: it has no knowledge of any exception list. The
/// denylist is policy, not mechanism, and lives beside its justification in
/// the ratchet test that calls this function — keeping the collector
/// reusable by any future caller. [`assert_no_unguarded_temp_dir_sites`]
/// remains the right tool for a targeted single-file guard; this function
/// does not replace it.
pub fn collect_workspace_unguarded_temp_dirs(workspace_root: &Path) -> Vec<String> {
    walk_rs_files(workspace_root, |_| true)
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok().map(|s| (path, s)))
        .flat_map(|(path, source)| {
            let rel = path
                .strip_prefix(workspace_root)
                .unwrap_or(path)
                .display()
                .to_string();
            find_unguarded_temp_dir_sites(&source)
                .into_iter()
                .map(move |v| format!("{rel}: {v}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Companion to [`collect_workspace_unguarded_temp_dirs`]: counts how many
/// of `rs_files` (as produced by [`walk_rs_files`]) can be successfully read
/// as UTF-8 text, as opposed to silently skipped by a per-file I/O error —
/// non-UTF-8 source, permission denied, or a file deleted mid-walk (see
/// [`collect_workspace_unguarded_temp_dirs`]'s doc comment for why skipping
/// is the deliberate policy for that last case).
///
/// # Why this exists alongside the walker-health guard
///
/// `workspace_has_no_unguarded_temp_dirs`'s walker-health guard counts files
/// the WALKER returned — before any read is ever attempted — so it cannot
/// see a SYSTEMATIC read failure (a permissions change under `crates/`, a
/// source file that somehow isn't valid UTF-8) that would silently empty
/// the sweep's real coverage while leaving the walked count untouched. That
/// is precisely the vacuous-pass failure mode the health guard exists to
/// catch, just one layer deeper than the walk itself: comparing this
/// function's result against `rs_files.len()` closes the gap.
pub fn count_readable_rs_files(rs_files: &[PathBuf]) -> usize {
    rs_files
        .iter()
        .filter(|path| std::fs::read_to_string(path).is_ok())
        .count()
}

/// Partition a raw sweep result (as produced by
/// [`collect_workspace_unguarded_temp_dirs`]) against an exception table of
/// `(repo_relative_path, expected_count, justification)` triples, returning
/// `(enforced, stale)`:
///
/// - `enforced` — violations not covered by any exception. Non-empty means
///   the sweep must fail: bind a named guard local, or annotate the line
///   `// temp-dir:allow — reason`.
/// - `stale` — exception paths (the first tuple element) that no longer
///   match ANY violation. Non-empty ALSO means the sweep must fail, but with
///   the opposite remedy: the file was migrated off hand-rolled temp dirs,
///   so its entry must be DELETED from the exception table.
///
/// # Why the stale half exists
///
/// The six-entry allowlist this task retires decayed into a no-op ratchet
/// because nothing forced it to grow. A denylist decays the mirror-image
/// way: entries outlive the condition that justified them and silently
/// re-open the hole the exception was meant to narrowly carve out. Asserting
/// `stale` empty is what makes the exception list self-cleaning — when a
/// listed file is migrated, this half goes RED and mechanically forces the
/// entry's deletion in the same change.
///
/// # The count must match exactly, or nothing is exempted
///
/// A path-prefix match alone would exempt EVERY violation in the named
/// file, present and future — so a second hand-rolled site added later to
/// an already-excepted file would be silently swallowed instead of
/// surfacing as a regression. `expected_count` closes that hole: an
/// exception only exempts its file's violations when the actual count of
/// matches equals `expected_count` exactly. A mismatched-but-nonzero count
/// exempts nothing at all — every one of that file's violations falls
/// through to `enforced`, exactly as if the file had never been excepted —
/// so a new site is caught as loudly as a new site anywhere else. A count of
/// zero is unchanged: the existing `stale` signal, since the file has been
/// fully migrated off hand-rolled temp dirs.
///
/// # Matching is anchored, not substring-based
///
/// An exception matches a violation only via the `"{path}: "` prefix the
/// collector emits — never a bare substring search — so an exception for
/// `foo.rs` can never accidentally suppress a violation in `notfoo.rs`.
/// Path separators are normalised (`\` → `/`) before comparing, so the
/// match is robust regardless of how either side constructed its path.
///
/// `pub` (not test-only): its only current caller is this module's
/// `workspace_has_no_unguarded_temp_dirs`, but that does not make it dead
/// code — `reify-test-support` is a library crate whose entire purpose is
/// to be depended on by other crates' tests, so any crate wanting the same
/// ratchet-with-self-cleaning-exceptions shape can reuse this classification
/// rather than reimplementing it.
pub fn partition_enforced_and_stale(
    violations: &[String],
    exceptions: &[(&str, usize, &str)],
) -> (Vec<String>, Vec<String>) {
    let normalize = |s: &str| s.replace('\\', "/");
    let prefix_for = |path: &str| format!("{}: ", normalize(path));

    let mut exempted = vec![false; violations.len()];
    let mut stale: Vec<String> = Vec::new();

    for &(path, expected_count, _justification) in exceptions {
        let prefix = prefix_for(path);
        let match_indices: Vec<usize> = violations
            .iter()
            .enumerate()
            .filter(|(_, v)| normalize(v).starts_with(prefix.as_str()))
            .map(|(i, _)| i)
            .collect();

        if match_indices.is_empty() {
            // No violation left in this file at all — the exception has
            // gone stale and must be deleted.
            stale.push(path.to_string());
        } else if match_indices.len() == expected_count {
            // The count matches exactly — exempt precisely these matches.
            for i in match_indices {
                exempted[i] = true;
            }
        }
        // else: nonzero but != expected_count. Deliberately exempt NOTHING
        // here — every matched violation falls through to `enforced` below,
        // the same as a violation in a file with no exception at all. This
        // is what stops a whole-file prefix match from silently widening to
        // cover a brand-new site that was never accounted for.
    }

    let enforced: Vec<String> = violations
        .iter()
        .enumerate()
        .filter(|(i, _)| !exempted[*i])
        .map(|(_, v)| v.clone())
        .collect();

    (enforced, stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// Run `f` with the default panic hook suppressed, restoring it afterwards.
    ///
    /// Several tests below panic ON PURPOSE inside `catch_unwind`. Without this,
    /// every GREEN run of this crate prints a full panic message (and a
    /// backtrace under `RUST_BACKTRACE`) that reads exactly like a real failure.
    ///
    /// The hook is process-GLOBAL and libtest runs tests concurrently, so a
    /// sibling test that panics for real inside this window loses its panic
    /// message and is reported as a bare failure. The window is kept as narrow
    /// as the deliberate panic allows; when triaging an unrelated failure in
    /// this crate, re-run with `--test-threads=1` so no suppression overlaps it.
    fn with_silent_panic_hook<R>(f: impl FnOnce() -> R) -> R {
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = f();
        std::panic::set_hook(prev_hook);
        result
    }

    /// Recover a panic payload as a `String`, whichever of the two standard
    /// payload types it was raised with.
    fn panic_message(err: Box<dyn std::any::Any + Send>) -> String {
        err.downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>")
            .to_string()
    }

    /// THE property this module exists to provide: the directory is removed
    /// when the scope holding the guard unwinds, not merely when it returns.
    ///
    /// The path is captured in an outer cell so it outlives the closure the
    /// guard is dropped inside of.
    ///
    /// Constructs with retention explicitly OFF rather than through
    /// [`prefixed_tempdir`], so that running the suite under
    /// `REIFY_KEEP_TEMP_DIRS=1` to triage some other failure does not turn this
    /// invariant red.  The env→retention wiring is pinned separately by
    /// `keep_requested_reads_set_and_non_empty`.
    #[test]
    fn prefixed_tempdir_is_removed_on_panic_unwind() {
        let seen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let seen_inner = Rc::clone(&seen);

        let result = with_silent_panic_hook(|| {
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                let guard = prefixed_tempdir_with_retention("reify-test-support-unwind-", false);
                *seen_inner.borrow_mut() = Some(guard.path().to_path_buf());
                assert!(
                    guard.path().exists(),
                    "the guard's directory must exist before we force the panic"
                );
                panic!("forced unwind");
            }))
        });

        assert!(
            result.is_err(),
            "the closure must have unwound — otherwise this test proves nothing"
        );
        let path = seen
            .borrow()
            .clone()
            .expect("the closure recorded the path before panicking");
        assert!(
            !path.exists(),
            "unwinding out of the guard's scope must remove {path:?}; \
             a surviving directory is the leak this module exists to prevent"
        );
    }

    /// The ordinary path: the directory is removed when the guard's scope ends
    /// normally.  Pins that the RAII teardown is not somehow panic-only.
    ///
    /// Retention explicitly OFF for the same reason as the unwind test above.
    #[test]
    fn prefixed_tempdir_is_removed_on_normal_scope_exit() {
        let path: PathBuf = {
            let guard = prefixed_tempdir_with_retention("reify-test-support-scope-", false);
            let path = guard.path().to_path_buf();
            assert!(path.exists(), "the guard's directory must exist in scope");
            path
        };
        assert!(
            !path.exists(),
            "leaving the guard's scope must remove {path:?}"
        );
    }

    /// Forensic attribution.  `Drop` cannot run on SIGKILL, OOM-kill or power
    /// loss, so residual debris stays possible and must stay ATTRIBUTABLE to the
    /// test that produced it: `find /tmp -maxdepth 1 -name '<prefix>*'` is the
    /// triage tool, and it only works if (a) the directory is a direct child of
    /// [`std::env::temp_dir`] and (b) its name carries the caller's prefix.
    ///
    /// Anonymous `.tmpXXXXXX` names — what plain `tempfile::tempdir()` produces
    /// — would leave the next operator strictly worse off than the manual
    /// teardown this module replaces.
    #[test]
    fn prefixed_tempdir_name_is_attributable_to_its_producer() {
        let prefix = "reify-test-support-attrib-";
        let guard = prefixed_tempdir(prefix);
        let path = guard.path();

        assert_eq!(
            path.parent(),
            Some(std::env::temp_dir().as_path()), // temp-dir:allow — attribution test must compare against the REAL temp dir
            "the directory must be a DIRECT child of the temp dir so the \
             `find /tmp -maxdepth 1` triage glob reaches it; got {path:?}"
        );

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("temp dir name is valid UTF-8");
        assert!(
            name.starts_with(prefix),
            "the directory name {name:?} must start with the caller's prefix \
             {prefix:?} so surviving debris names the test that made it"
        );
    }

    // ── REIFY_KEEP_TEMP_DIRS post-mortem opt-out ─────────────────────────────

    /// The knob's parsing contract.  Reads a passed-in value rather than the
    /// process environment: libtest runs tests concurrently, so mutating the
    /// real variable would race every other test in this binary.
    #[test]
    fn keep_requested_reads_set_and_non_empty() {
        assert!(!keep_requested(None), "unset must not retain");
        assert!(
            !keep_requested(Some(OsStr::new(""))),
            "an empty value must not retain — `REIFY_KEEP_TEMP_DIRS=` reads as off"
        );
        assert!(keep_requested(Some(OsStr::new("1"))));
        assert!(
            keep_requested(Some(OsStr::new("0"))),
            "any non-empty value retains; there is no truthiness parsing to get \
             wrong at 2am"
        );
    }

    /// The retain branch keeps the directory alive past the guard's drop, so a
    /// RED run's artifacts survive for a post-mortem.  This test cleans up
    /// explicitly — nothing else will, which is exactly the documented contract.
    #[test]
    fn prefixed_tempdir_retains_the_directory_when_opted_in() {
        let path = {
            let guard = prefixed_tempdir_with_retention("reify-test-support-keep-", true);
            guard.path().to_path_buf()
        };
        assert!(
            path.exists(),
            "with retention on, {path:?} must survive the guard's drop so the \
             artifacts under test can be read after a failure"
        );
        std::fs::remove_dir_all(&path).expect("this test owns the retained directory");
    }

    /// ...and the default branch is unchanged: retention off still removes.
    #[test]
    fn prefixed_tempdir_removes_the_directory_when_not_opted_in() {
        let path = {
            let guard = prefixed_tempdir_with_retention("reify-test-support-nokeep-", false);
            guard.path().to_path_buf()
        };
        assert!(
            !path.exists(),
            "retention off must behave exactly as before: {path:?} removed on drop"
        );
    }

    // ── find_unguarded_temp_dir_sites ────────────────────────────────────────
    //
    // Every synthetic source below assembles the scanned needles at runtime via
    // `.concat()` — the `ignore_hygiene.rs` convention — so this file never
    // contains a line carrying an adjacent match and cannot self-trigger.  (It
    // legitimately calls the real thing in the attribution test above.)

    /// The construction needle, assembled so it never appears literally here.
    fn guarded_call() -> String {
        ["env", "::temp_dir()"].concat()
    }

    /// The guard's own name, assembled for the same reason.
    fn guard_fn() -> String {
        ["prefixed_", "tempdir"].concat()
    }

    /// (1) The `std::`-qualified form — what all five migrating files used
    /// before the guard.
    #[test]
    fn fnuts_flags_std_qualified_call() {
        let call = guarded_call();
        let src = format!("    let dir = std::{call}.join(\"reify_test\");\n");
        let violations = find_unguarded_temp_dir_sites(&src);
        assert_eq!(
            violations.len(),
            1,
            "expected exactly one violation, got: {violations:?}"
        );
        assert!(
            violations[0].contains("line 1"),
            "violation should cite the line number: {:?}",
            violations[0]
        );
        assert!(
            violations[0].contains(&call),
            "violation should quote the offending line: {:?}",
            violations[0]
        );
        assert!(
            violations[0].contains("hand-rolled construction"),
            "violation should name its kind so the remediation is unambiguous: {:?}",
            violations[0]
        );
    }

    /// (2) The bare form.  No file uses it today, but a future `use std::env;`
    /// shortening must not silently escape the guard.
    #[test]
    fn fnuts_flags_bare_call() {
        let call = guarded_call();
        let src = format!("    let dir = {call};\n");
        assert_eq!(
            find_unguarded_temp_dir_sites(&src).len(),
            1,
            "the bare `env::temp_dir()` form must be flagged too" // temp-dir:allow — prose mention inside an assertion message
        );
    }

    /// (3) Doc-comment lines that merely mention the call in prose are skipped,
    /// mirroring `is_doc_comment_line` in `ignore_hygiene.rs`.  Both the `///`
    /// and `//!` arms are pinned so neither can be dropped silently.
    #[test]
    fn fnuts_skips_doc_comment_prose() {
        let call = guarded_call();
        let src = format!(
            "/// Historically this built a dir under {call} by hand.\n\
             //! See {call} for the pre-guard idiom.\n\
             \x20   /// indented mention of {call}\n"
        );
        let violations = find_unguarded_temp_dir_sites(&src);
        assert!(
            violations.is_empty(),
            "doc-comment prose must not fire, got: {violations:?}"
        );
    }

    /// (4) The `// temp-dir:allow — reason` escape, mirroring the house
    /// `// ptodo:allow — reason` convention in CLAUDE.md.
    #[test]
    fn fnuts_honours_the_allow_escape() {
        let call = guarded_call();
        let src = format!(
            "    let base = {call}; // temp-dir:allow — this IS the guard's own impl\n"
        );
        let violations = find_unguarded_temp_dir_sites(&src);
        assert!(
            violations.is_empty(),
            "an explicit `temp-dir:allow` escape must suppress the violation, got: {violations:?}"
        );
    }

    /// (5) Clean source → empty Vec.  Pins the no-match contract.
    #[test]
    fn fnuts_clean_source_returns_empty_vec() {
        assert!(find_unguarded_temp_dir_sites("").is_empty());
        assert!(
            find_unguarded_temp_dir_sites("fn main() {}\nlet x = 1;\n").is_empty(),
            "source with no temp-dir call must be clean"
        );
    }

    /// (6) Line numbers are 1-based, and EVERY hit is collected — the scanner
    /// must not stop at the first.  `m5_integration.rs` has two sites and
    /// `server.rs` has nine; a first-hit-only scanner would under-report both
    /// and make those ratchet steps look half-done.
    #[test]
    fn fnuts_reports_every_hit_with_one_based_line_numbers() {
        let call = guarded_call();
        let src = format!(
            "fn a() {{}}\n\
             fn b() {{}}\n\
             \x20   let first = std::{call};\n\
             fn c() {{}}\n\
             \x20   let second = std::{call};\n"
        );
        let violations = find_unguarded_temp_dir_sites(&src);
        assert_eq!(
            violations.len(),
            2,
            "both sites must be reported, not just the first: {violations:?}"
        );
        assert!(
            violations[0].contains("line 3"),
            "first hit is on line 3 (1-based): {:?}",
            violations[0]
        );
        assert!(
            violations[1].contains("line 5"),
            "second hit is on line 5 (1-based): {:?}",
            violations[1]
        );
    }

    /// (7) The immediate-drop one-liner.  This is the failure mode the migration
    /// INTRODUCES — every new call site is correct only by hand-discipline — and
    /// it is worse than the idiom it replaced, because the hand-rolled form at
    /// least worked on GREEN runs.
    #[test]
    fn fnuts_flags_the_unbound_guard_one_liner() {
        let guard_fn = guard_fn();
        let src = format!("    let dir = {guard_fn}(\"x-\").path().to_path_buf();\n");
        let violations = find_unguarded_temp_dir_sites(&src);
        assert_eq!(
            violations.len(),
            1,
            "the guard must be bound to a named local; taking `.path()` in the \
             same statement drops it immediately: {violations:?}"
        );
        assert!(
            violations[0].contains("unbound guard"),
            "violation should name its kind: {:?}",
            violations[0]
        );
    }

    /// (8) ...and the correct two-line form stays clean, so the new needle
    /// cannot turn every migrated call site red.
    #[test]
    fn fnuts_accepts_the_bound_two_line_form() {
        let guard_fn = guard_fn();
        let src = format!(
            "    let guard = {guard_fn}(\"x-\");\n\
             \x20   let dir = guard.path().to_path_buf();\n"
        );
        let violations = find_unguarded_temp_dir_sites(&src);
        assert!(
            violations.is_empty(),
            "the documented two-line form is exactly what the guard asks for, \
             got: {violations:?}"
        );
    }

    // ── check_temp_dir_hygiene ───────────────────────────────────────────────

    /// Clean source → `Ok(())`.  The happy path of the message-building layer.
    #[test]
    fn cth_clean_source_is_ok() {
        assert_eq!(
            check_temp_dir_hygiene("fn main() {}\n", "some/file.rs"),
            Ok(())
        );
    }

    /// The remediation message is the entire user-facing value of this layer
    /// over the raw scanner, so every part a reader depends on is pinned: the
    /// count, the path, the offending line number, and the copy-pasteable fix.
    #[test]
    fn cth_violation_message_carries_count_path_line_and_remedy() {
        let call = guarded_call();
        let guard_fn = guard_fn();
        let src = format!(
            "fn a() {{}}\n\
             \x20   let dir = std::{call};\n\
             \x20   let other = {guard_fn}(\"x-\").path().to_path_buf();\n"
        );
        let err = check_temp_dir_hygiene(&src, "crates/some-crate/tests/thing.rs")
            .expect_err("a source with two violations must not report clean");

        assert!(
            err.contains("2 temp-dir hygiene violation(s)"),
            "message must count the violations: {err}"
        );
        assert!(
            err.contains("crates/some-crate/tests/thing.rs"),
            "message must name the file being scanned: {err}"
        );
        assert!(
            err.contains("line 2") && err.contains("line 3"),
            "message must cite every offending line: {err}"
        );
        assert!(
            err.contains(&format!("{guard_fn}(\"<prefix>-\")")),
            "message must show the copy-pasteable remediation: {err}"
        );
        assert!(
            err.contains(ALLOW_ESCAPE),
            "message must point at the escape hatch, which is otherwise \
             undiscoverable from the edit site: {err}"
        );
        assert!(
            err.contains("prose mention"),
            "regular `//` comments are NOT skipped, so the message must tell a \
             prose-mention author what to do instead: {err}"
        );
    }

    // ── assert_no_unguarded_temp_dir_sites ───────────────────────────────────

    /// An unreadable target must fail LOUDLY, naming the absolute path it
    /// tried.  Without this, a future file move would silently turn every
    /// ratchet guard into a vacuous pass.
    #[test]
    fn anuts_unreadable_file_panics_naming_the_absolute_path() {
        let rel = "crates/reify-test-support/definitely-not-a-real-file.rs";
        let err = with_silent_panic_hook(|| {
            std::panic::catch_unwind(|| assert_no_unguarded_temp_dir_sites(rel))
                .expect_err("scanning a nonexistent file must panic, not pass vacuously")
        });
        let msg = panic_message(err);
        assert!(
            msg.contains(&format!("/{rel}")),
            "the panic must name the ABSOLUTE path it attempted (repo root + \
             {rel:?}), so a moved file is obvious; got: {msg:?}"
        );
    }

    /// The wrapper's violation path, end to end over a real file on disk: a
    /// synthetic violating source must panic carrying the scanner's findings.
    #[test]
    fn anuts_violating_file_panics_with_the_remediation() {
        let call = guarded_call();
        let guard = prefixed_tempdir("reify-test-support-anuts-bad-");
        let file = guard.path().join("violating.rs");
        std::fs::write(&file, format!("fn t() {{\n    let d = std::{call};\n}}\n"))
            .expect("write the synthetic fixture");

        // An absolute path survives `repo_root.join(..)` unchanged, which is how
        // this test reaches a fixture that is not a tracked workspace file.
        let path = file.display().to_string();
        let err = with_silent_panic_hook(|| {
            std::panic::catch_unwind(|| assert_no_unguarded_temp_dir_sites(&path))
                .expect_err("a violating file must panic, not pass")
        });
        let msg = panic_message(err);

        assert!(
            msg.contains("line 2"),
            "the panic must cite the offending line: {msg}"
        );
        assert!(
            msg.contains(&file.display().to_string()),
            "the panic must name the file it scanned: {msg}"
        );
        assert!(
            msg.contains(&format!("{}(\"<prefix>-\")", guard_fn())),
            "the panic must carry the copy-pasteable remediation: {msg}"
        );
    }

    /// ...and the wrapper's happy path returns normally rather than, say,
    /// panicking on any file it can read.
    #[test]
    fn anuts_clean_file_does_not_panic() {
        let guard = prefixed_tempdir("reify-test-support-anuts-ok-");
        let file = guard.path().join("clean.rs");
        std::fs::write(&file, "fn t() {\n    let d = 1;\n}\n")
            .expect("write the synthetic fixture");

        assert_no_unguarded_temp_dir_sites(&file.display().to_string());
    }

    // ── collect_workspace_unguarded_temp_dirs ────────────────────────────────
    //
    // Mirrors ignore_hygiene's `cwsp_*` tests: a synthetic workspace exercised
    // end to end through the walk-read-scan-format collector. Needles reuse
    // the `guarded_call`/`guard_fn` helpers above, so this file never contains
    // an adjacent literal match and cannot self-trigger the very sweep it is
    // testing here.

    /// Fixture shape: (a) `crates/alpha/src/lib.rs` — a `src/` file with a
    /// hand-rolled construction site, unreachable by the old tests-only
    /// walker and thus the whole point of the workspace-wide sweep; (b)
    /// `crates/beta/tests/thing.rs` — a `tests/` file with an unbound-guard
    /// one-liner, pinning that BOTH violation kinds survive the workspace
    /// layer; (c) `crates/gamma/src/ok.rs` — a `src/` file whose only site
    /// carries the per-line `// temp-dir:allow` escape, pinning that the
    /// escape propagates through the collector (the exact mechanism pre-2
    /// relies on to exempt this module's own two sites); (d)
    /// `crates/delta/src/clean.rs` — clean, contributes nothing; (e)
    /// `target/skip.rs` — a violation under a pruned directory, must not
    /// surface.
    #[test]
    fn cwutd_synthetic_workspace_reports_src_and_tests_violations_only() {
        // Uses this module's own prefixed_tempdir (like the anuts_* fixtures
        // above) rather than raw tempfile::tempdir(), so any debris surviving
        // a SIGKILL of this test is attributable to it — an anonymous
        // `.tmpXXXXXX` name would leave an operator strictly worse off, per
        // prefixed_tempdir's own doc comment.
        let guard = prefixed_tempdir("reify-test-support-cwutd-");
        let root = guard.path();

        let call = guarded_call();
        let guard_fn = guard_fn();

        let alpha = root.join("crates/alpha/src/lib.rs");
        let alpha_src = format!("fn f() {{\n    let d = std::{call}.join(\"x\");\n}}\n");

        let beta = root.join("crates/beta/tests/thing.rs");
        let beta_src =
            format!("fn g() {{\n    let d = {guard_fn}(\"x-\").path().to_path_buf();\n}}\n");

        let gamma = root.join("crates/gamma/src/ok.rs");
        let gamma_src =
            format!("fn h() {{\n    let d = {call}; // temp-dir:allow — test fixture\n}}\n");

        let delta = root.join("crates/delta/src/clean.rs");
        let delta_src = "fn clean() {}\n".to_string();

        let pruned = root.join("target/skip.rs");
        let pruned_src = format!("fn p() {{\n    let d = std::{call};\n}}\n");

        for (path, content) in [
            (&alpha, alpha_src.as_str()),
            (&beta, beta_src.as_str()),
            (&gamma, gamma_src.as_str()),
            (&delta, delta_src.as_str()),
            (&pruned, pruned_src.as_str()),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).expect("create parent dirs");
            std::fs::write(path, content.as_bytes()).expect("write file");
        }

        let violations = collect_workspace_unguarded_temp_dirs(root);

        assert_eq!(
            violations.len(),
            2,
            "expected exactly 2 violations — alpha (src/) and beta (tests/) — with gamma \
             escaped, delta clean, and target/ pruned: {violations:?}"
        );
        let combined = violations.join("\n");
        assert!(
            combined.contains("crates/alpha/src/lib.rs"),
            "expected a violation naming the alpha src/ file — the whole point of the \
             workspace-wide sweep, unreachable by the old tests-only walker: {violations:?}"
        );
        assert!(
            combined.contains("crates/beta/tests/thing.rs"),
            "expected a violation naming the beta tests/ file: {violations:?}"
        );
    }

    // ── count_readable_rs_files ──────────────────────────────────────────────

    /// A mix of one valid-UTF-8 file and one file containing invalid UTF-8
    /// bytes → count is 1, not 2. Pins that the function actually attempts a
    /// read rather than trusting the file list handed to it — the whole
    /// point, since the walker-health guard it complements counts files
    /// BEFORE any read is attempted and so cannot see this kind of silent
    /// drop on its own.
    #[test]
    fn crf_counts_only_files_that_read_successfully_as_utf8() {
        let guard = prefixed_tempdir("reify-test-support-crf-");
        let root = guard.path();

        let good = root.join("good.rs");
        std::fs::write(&good, b"fn main() {}\n").expect("write good fixture");

        let bad = root.join("bad.rs");
        // 0x80 is a bare continuation byte: it can never start a valid UTF-8
        // sequence, so `read_to_string` must fail with InvalidData.
        std::fs::write(&bad, [0x66, 0x6e, 0x20, 0x80, 0x28, 0x29]).expect("write bad fixture");

        let files = vec![good, bad];
        assert_eq!(
            count_readable_rs_files(&files),
            1,
            "expected only the valid-UTF-8 file to count as readable"
        );
    }

    /// Empty input → 0, pinning the no-files contract.
    #[test]
    fn crf_empty_input_returns_zero() {
        assert_eq!(count_readable_rs_files(&[]), 0);
    }

    // ── partition_enforced_and_stale ─────────────────────────────────────────
    //
    // Tests exercise the pure classification over synthetic (violations,
    // exceptions) inputs — no live repo state — so the stale-entry branch
    // (whose trigger condition, #5639 landing, does not exist in the repo
    // today) is directly testable. The function itself lives in the outer
    // module, directly beneath `collect_workspace_unguarded_temp_dirs` — see
    // its doc comment there for the full contract.

    /// (1) A violation for path A, exempted by an exception naming A with
    /// the matching expected count (1) → both halves empty: the exception
    /// is exercised and current.
    #[test]
    fn pes_exact_match_leaves_both_halves_empty() {
        let violations =
            vec!["crates/a/src/lib.rs: line 3: hand-rolled construction: \"...\"".to_string()];
        let exceptions: &[(&str, usize, &str)] = &[("crates/a/src/lib.rs", 1, "justification")];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert!(
            enforced.is_empty(),
            "expected no enforced violations: {enforced:?}"
        );
        assert!(stale.is_empty(), "expected no stale exceptions: {stale:?}");
    }

    /// (2) A violation for A only, but the exception table ALSO lists B →
    /// B is STALE. This is the exact case that fires when #5639 lands and
    /// migrates `reify-build-utils`: its entry must be deleted, and this
    /// assertion is what forces that deletion.
    #[test]
    fn pes_exception_with_no_matching_violation_is_stale() {
        let violations =
            vec!["crates/a/src/lib.rs: line 3: hand-rolled construction: \"...\"".to_string()];
        let exceptions: &[(&str, usize, &str)] = &[
            ("crates/a/src/lib.rs", 1, "justification a"),
            ("crates/b/src/lib.rs", 1, "justification b"),
        ];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert!(
            enforced.is_empty(),
            "expected no enforced violations: {enforced:?}"
        );
        assert_eq!(
            stale,
            vec!["crates/b/src/lib.rs".to_string()],
            "expected b's exception to be reported stale: {stale:?}"
        );
    }

    /// (3) Violations for A and C, exception list covers only A → C is
    /// ENFORCED (an un-excepted violation — the sweep must fail on it).
    #[test]
    fn pes_unexempted_violation_is_enforced() {
        let violations = vec![
            "crates/a/src/lib.rs: line 3: hand-rolled construction: \"...\"".to_string(),
            "crates/c/src/lib.rs: line 7: unbound guard: \"...\"".to_string(),
        ];
        let exceptions: &[(&str, usize, &str)] = &[("crates/a/src/lib.rs", 1, "justification a")];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert_eq!(
            enforced,
            vec!["crates/c/src/lib.rs: line 7: unbound guard: \"...\"".to_string()],
            "expected c's violation to be enforced: {enforced:?}"
        );
        assert!(stale.is_empty(), "expected no stale exceptions: {stale:?}");
    }

    /// (4) No violations at all, but the exception table is non-empty →
    /// EVERY exception is stale.
    #[test]
    fn pes_empty_violations_makes_every_exception_stale() {
        let violations: Vec<String> = Vec::new();
        let exceptions: &[(&str, usize, &str)] = &[
            ("crates/a/src/lib.rs", 1, "justification a"),
            ("crates/b/src/lib.rs", 1, "justification b"),
        ];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert!(
            enforced.is_empty(),
            "expected no enforced violations: {enforced:?}"
        );
        assert_eq!(
            stale.len(),
            2,
            "expected both exceptions to be stale against zero violations: {stale:?}"
        );
    }

    /// (5) Anchoring: an exception path that is merely a SUBSTRING of a
    /// violation's path (not a true `"{path}: "` prefix) must NOT suppress
    /// that violation, and the exception itself must be reported stale — a
    /// substring collision must never silently exempt the wrong file.
    #[test]
    fn pes_exception_matching_is_anchored_on_the_path_prefix_not_a_substring() {
        let violations =
            vec!["crates/b/notfoo.rs: line 1: hand-rolled construction: \"...\"".to_string()];
        // "foo.rs" is a substring of "notfoo.rs" but not a `"{path}: "` prefix
        // of the violation string above.
        let exceptions: &[(&str, usize, &str)] = &[("foo.rs", 1, "should not match notfoo.rs")];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert_eq!(
            enforced,
            vec!["crates/b/notfoo.rs: line 1: hand-rolled construction: \"...\"".to_string()],
            "a substring match must not suppress the real violation: {enforced:?}"
        );
        assert_eq!(
            stale,
            vec!["foo.rs".to_string()],
            "the non-matching exception must be reported stale: {stale:?}"
        );
    }

    /// (6) An exception pinned at `expected_count == 1` but the file now has
    /// TWO matching violations (a second hand-rolled site was added after
    /// the exception was written) → NEITHER is exempted, both surface via
    /// `enforced` exactly like a violation in a never-excepted file would,
    /// and the exception is NOT reported stale (the file still has
    /// violations — `stale` is reserved for a zero-match count). This is
    /// the exact regression a bare path-prefix match would silently swallow.
    #[test]
    fn pes_exception_with_wrong_violation_count_exempts_nothing() {
        let violations = vec![
            "crates/a/src/lib.rs: line 3: hand-rolled construction: \"...\"".to_string(),
            "crates/a/src/lib.rs: line 9: hand-rolled construction: \"...\"".to_string(),
        ];
        // Written when the file had exactly one site.
        let exceptions: &[(&str, usize, &str)] = &[("crates/a/src/lib.rs", 1, "justification")];

        let (enforced, stale) = partition_enforced_and_stale(&violations, exceptions);

        assert_eq!(
            enforced, violations,
            "a violation count that no longer matches expected_count must \
             exempt nothing at all — both sites must surface: {enforced:?}"
        );
        assert!(
            stale.is_empty(),
            "the file still has violations, so its exception is not STALE \
             (that signal is reserved for a zero-match count): {stale:?}"
        );
    }

    // ── workspace_has_no_unguarded_temp_dirs (the live ratchet) ──────────────
    //
    // Replaces the six hand-picked `*_have_no_unguarded_temp_dirs` guards below
    // with a whole-workspace sweep, narrowed only by the comment-justified
    // `SWEEP_EXCEPTIONS` table. `partition_enforced_and_stale` above classifies
    // the sweep's raw output against that table in both directions, and BOTH
    // halves are asserted empty here — see that function's doc comment for why
    // the stale half exists.

    /// The sweep's exception table: `(repo-relative path, expected violation
    /// count, justification)` triples. A bare path can never be added
    /// without a written reason — the tuple's third element enforces that
    /// structurally.
    ///
    /// The count pins exactly how many violations in that file the
    /// exception may cover. `partition_enforced_and_stale` only exempts a
    /// file when its ACTUAL violation count equals this number — so if a
    /// SECOND hand-rolled site is added to an already-excepted file, the
    /// count stops matching and NEITHER site is exempted; both surface as
    /// ordinary enforced violations instead of being silently swallowed by
    /// a whole-file prefix match. This closes a hole a plain
    /// `(path, justification)` pair would leave open.
    ///
    /// Every entry here must CURRENTLY violate, with its count exactly
    /// right — `workspace_has_no_unguarded_temp_dirs` asserts that too (the
    /// `stale` half of `partition_enforced_and_stale`), so an entry whose
    /// file gets migrated off hand-rolled temp dirs fails LOUDLY until its
    /// entry is deleted, rather than silently rotting the way this module's
    /// earlier six-entry allowlist did.
    ///
    /// Re-measured (not merely trusted) at debug-fix time for this task:
    /// `crates/reify-build-utils/src/lib.rs` was the sole exception when this
    /// ratchet was authored, because it predated #5639 (which migrates its
    /// `fn tempdir()` test helper off the hand-rolled std-library accessor)
    /// landing on `main`. #5639 has since landed and this branch
    /// picked it up, so that file no longer violates — exactly the
    /// self-cleaning failure this table's staleness check exists to force:
    /// the ratchet went RED with a stale entry, and the entry was deleted
    /// here rather than left to rot. `temp_dirs.rs`'s own two self-trigger
    /// sites are exempted per-line instead of listed here (see pre-2).
    ///
    /// The PAST TENSE above is load-bearing, not stylistic. This paragraph
    /// narrates a deferral that has since been RESOLVED, on lines that also
    /// carry a `#NNNN` cite to the terminal task that resolved it — the shape
    /// PTODO lane δ-B claims (comment line + canonical cite + deferral prose
    /// => High `orphaned`, hard gate). Re-phrasing it as an outstanding
    /// deferral therefore reds the §6.6 ratchet. If natural phrasing is ever
    /// worth it, take the sanctioned §6.8 escape (`ptodo:allow`) rather than
    /// re-wording around the detector; class and rationale: PRD
    /// `reify-audit-ptodo-detector.md` §16 Row 2, "narrated non-deferral".
    const SWEEP_EXCEPTIONS: &[(&str, usize, &str)] = &[];

    /// The live ratchet. Every `.rs` file in the workspace — `src/`, `tests/`,
    /// and `build.rs` alike — must be free of temp-dir hygiene violations,
    /// except for the paths explicitly listed (with justification) in
    /// `SWEEP_EXCEPTIONS` below. An exception that no longer matches any
    /// violation has rotted stale and must be deleted — see
    /// `partition_enforced_and_stale`'s doc comment for why that direction is
    /// asserted too.
    #[test]
    fn workspace_has_no_unguarded_temp_dirs() {
        // Same resolution as `assert_no_unguarded_temp_dir_sites`: this crate's
        // CARGO_MANIFEST_DIR is always `<repo>/crates/reify-test-support`, so
        // two `.parent()` walks reach the repo root.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/reify-test-support has a parent (crates/)")
            .parent()
            .expect("crates/ has a parent (the repo root)");

        // Walker-health guard FIRST, mirroring the two-part check in
        // `tests/ignore_reason_hygiene.rs`: a walk that silently breaks (a
        // renamed directory component, say) must fail loudly and
        // specifically, not report the workspace clean because it scanned
        // nothing.
        //   1. Minimum count (>500) — measured 1758 `.rs` files at the time
        //      this ratchet was written; 500 is far below the real count
        //      (stable against ordinary file churn) yet an order of magnitude
        //      above what a partial-subtree breakage would return.
        //   2. Sentinel path — a `src/` file, so a regression in the walker's
        //      `src/`-inclusion (the entire reason this sweep replaces the
        //      six hand-picked guards below) is caught with a specific
        //      diagnostic rather than folded into the count check.
        let all_rs_files = walk_rs_files(repo_root, |_| true);
        assert!(
            all_rs_files.len() > 500,
            "walker found only {} .rs file(s) — expected >500; the walker may \
             be broken, or repo_root resolved to the wrong directory",
            all_rs_files.len()
        );
        let sentinel = repo_root.join("crates/reify-build-utils/src/lib.rs");
        assert!(
            all_rs_files.contains(&sentinel),
            "sentinel file {sentinel:?} not found in walker output — the \
             walker may be broken, or may have regressed to excluding src/ \
             files again"
        );

        //   3. Read-health guard — closes a blind spot the two checks above
        //      cannot see: they count files the WALKER returned, before any
        //      read is attempted, so a SYSTEMATIC read failure (a
        //      permissions change under `crates/`, say) would silently
        //      empty the sweep's real coverage below while leaving the
        //      walked count untouched — the same vacuous-pass failure mode,
        //      one layer deeper than the walk itself. A small delta (not
        //      zero) tolerates a file legitimately deleted mid-walk by a
        //      concurrent build, the same tradeoff
        //      `collect_workspace_unguarded_temp_dirs` accepts for its own
        //      per-file I/O errors.
        let readable = count_readable_rs_files(&all_rs_files);
        let unread = all_rs_files.len().saturating_sub(readable);
        assert!(
            unread <= 5,
            "the walker found {} .rs file(s) but only {readable} could be \
             read — {unread} file(s) silently failed (non-UTF-8 source, \
             permission denied, or similar); the sweep may be blind to real \
             violations in those files. Investigate before trusting a clean \
             result.",
            all_rs_files.len(),
        );

        // `collect_workspace_unguarded_temp_dirs` walks again internally.
        // Intentional test-only overhead, the same tradeoff
        // `tests/ignore_reason_hygiene.rs` accepts: the walk is cheap next to
        // the file I/O that follows, and keeping the health guard and the
        // sweep as independent calls is cleaner than threading a shared
        // pre-computation through both.
        let violations = collect_workspace_unguarded_temp_dirs(repo_root);
        let (enforced, stale) = partition_enforced_and_stale(&violations, SWEEP_EXCEPTIONS);

        assert!(
            enforced.is_empty(),
            "Found {} un-excepted temp-dir hygiene violation(s):\n  {}\n\n\
             Fix each by binding a named guard local:\n\
             \x20   let guard = reify_test_support::prefixed_tempdir(\"<prefix>-\");\n\
             \x20   let dir = guard.path().to_path_buf();\n\n\
             ...or, if the site is legitimately hand-rolled, annotate the \
             line `// {} — reason` and it will be skipped.",
            enforced.len(),
            enforced.join("\n  "),
            ALLOW_ESCAPE,
        );
        assert!(
            stale.is_empty(),
            "Found stale SWEEP_EXCEPTIONS entry/entries that no longer match \
             any violation:\n  {}\n\n\
             The file was migrated off hand-rolled temp dirs — DELETE its \
             entry from SWEEP_EXCEPTIONS. A stale exception silently re-opens \
             the hole it was written to narrowly carve out.",
            stale.join("\n  ")
        );
    }
}
