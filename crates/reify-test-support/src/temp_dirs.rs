//! Panic-safe temporary directories for tests, with forensically attributable
//! names.
//!
//! Test code that builds a directory under [`std::env::temp_dir`] and tears it
//! down with a trailing `fs::remove_dir_all(...)` leaks that directory whenever
//! an assertion fails and the test unwinds — i.e. precisely on RED CI runs.
//! This module supplies the RAII replacement.

use crate::ignore_hygiene::is_doc_comment_line;
use std::ffi::OsStr;
use std::path::Path;

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
    #[test]
    fn prefixed_tempdir_is_removed_on_panic_unwind() {
        let seen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let seen_inner = Rc::clone(&seen);

        let result = with_silent_panic_hook(|| {
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                let guard = prefixed_tempdir("reify-test-support-unwind-");
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
    #[test]
    fn prefixed_tempdir_is_removed_on_normal_scope_exit() {
        let path: PathBuf = {
            let guard = prefixed_tempdir("reify-test-support-scope-");
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
            Some(std::env::temp_dir().as_path()),
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
            "the bare `env::temp_dir()` form must be flagged too"
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

    // ── ratchet: files migrated off hand-rolled temp dirs ────────────────────
    //
    // Each guard below is ratcheted on by #5640 together with the migration that
    // makes it pass.  The list is deliberately EXPLICIT rather than a repo-wide
    // sweep: `crates/reify-build-utils/src/lib.rs` still holds a bare call
    // pending #5639's merge, and a sweep would make this crate red on an
    // unrelated task's merge order.  Adding a file here is how you extend the
    // ratchet — but migrate it in the same or the very next commit.
    //
    // The end state is a workspace sweep with an explicit, comment-justified
    // exception list, reusing `ignore_hygiene::walk_test_rs_files` (which needs a
    // path filter first — `server.rs` and `reify-build-utils/src/lib.rs` are
    // `src/` files it currently excludes).  That change edits `ignore_hygiene.rs`,
    // outside this task's locks, and is filed as follow-up work.

    #[test]
    fn import_resolve_tests_have_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites("crates/reify-compiler/tests/import_resolve_tests.rs");
    }

    #[test]
    fn user_defined_unit_tests_have_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites(
            "crates/reify-compiler/tests/user_defined_unit_tests.rs",
        );
    }

    #[test]
    fn m5_integration_has_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites("crates/reify-eval/tests/m5_integration.rs");
    }

    #[test]
    fn cli_doc_has_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites("crates/reify-cli/tests/harness_cli/cli_doc.rs");
    }

    #[test]
    fn lsp_server_has_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites("crates/reify-lsp/src/server.rs");
    }

    #[test]
    fn rpath_smoke_has_no_unguarded_temp_dirs() {
        assert_no_unguarded_temp_dir_sites("crates/reify-kernel-gmsh/tests/rpath_smoke.rs");
    }
}
