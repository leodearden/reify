//! Panic-safe temporary directories for tests, with forensically attributable
//! names.
//!
//! Test code that builds a directory under [`std::env::temp_dir`] and tears it
//! down with a trailing `fs::remove_dir_all(...)` leaks that directory whenever
//! an assertion fails and the test unwinds — i.e. precisely on RED CI runs.
//! This module supplies the RAII replacement.

use crate::ignore_hygiene::is_doc_comment_line;
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

/// The scanned needle, assembled at runtime.
///
/// DO NOT inline this as a literal: `temp_dirs.rs` legitimately calls the real
/// thing in its own attribution test, and an inlined literal would make this
/// module self-triggering the moment it were added to the scanned set. This is
/// the same defence `ignore_hygiene.rs` uses for its `#[ignore = "` marker.
///
/// One needle covers both spellings — `std::env::temp_dir()` (what every
/// migrating file uses today) and a bare `env::temp_dir()` under `use std::env`.
fn guarded_call_needle() -> String {
    ["env", "::temp_dir("].concat()
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
/// # Panics
///
/// Panics if the directory cannot be created — this is test-support code, and
/// an unwritable temp dir is not a condition any caller can recover from.
pub fn prefixed_tempdir(prefix: &str) -> TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .unwrap_or_else(|e| panic!("create temp dir with prefix {prefix:?}: {e}"))
}

/// Scan `source` (a Rust source file as a string) for hand-rolled temp-dir
/// construction — a call to `std::env::temp_dir()` that is not routed through
/// [`prefixed_tempdir`]. Returns one human-readable violation per offending
/// line, each carrying the 1-based line number and the trimmed line. An empty
/// `Vec` means clean.
///
/// Line-oriented, in the shape of the sibling
/// [`crate::ignore_hygiene::find_stale_plan_pointers_in_source`]:
///
/// - `///` and `//!` doc-comment lines are skipped, so prose that merely
///   mentions the call does not fire. Regular `//` comments are NOT skipped —
///   commented-out construction code is still a site worth reporting.
/// - A line carrying the `// temp-dir:allow — reason` escape is skipped.
/// - EVERY hit is collected, not just the first: `server.rs` holds nine sites
///   and `m5_integration.rs` two, and a first-hit-only scan would make those
///   look half-migrated.
pub fn find_unguarded_temp_dir_sites(source: &str) -> Vec<String> {
    let needle = guarded_call_needle();

    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !is_doc_comment_line(line))
        .filter(|(_, line)| !line.contains(ALLOW_ESCAPE))
        .filter(|(_, line)| line.contains(needle.as_str()))
        .map(|(idx, line)| {
            let preview: String = line.trim().chars().take(120).collect();
            format!("line {}: {preview:?}", idx + 1)
        })
        .collect()
}

/// Assert that the workspace file at `repo_relative_path` contains no
/// hand-rolled temp-dir construction, panicking with every violation and the
/// remediation if it does.
///
/// The repo root is resolved at compile time from `env!("CARGO_MANIFEST_DIR")`
/// evaluated inside **this** crate, which always sits at
/// `<repo>/crates/reify-test-support/`; two `.parent()` walks reach the root.
/// This is the same resolution [`crate::orphan_audit::run_orphan_audit`] uses,
/// so a guard here can scan a file owned by any other crate.
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

    let violations = find_unguarded_temp_dir_sites(&source);
    assert!(
        violations.is_empty(),
        "{} unguarded temp-dir site(s) in {}:\n  {}\n\n\
         Each builds a directory under {}) by hand and relies on a trailing \
         `fs::remove_dir_all(..)` that is SKIPPED when the test unwinds — so it \
         leaks precisely on RED runs. Replace with a named-local guard:\n\
         \x20   let guard = reify_test_support::prefixed_tempdir(\"<prefix>-\");\n\
         \x20   let dir = guard.path().to_path_buf();\n\
         A site that is legitimately hand-rolled annotates its line \
         `// {} — reason`.",
        violations.len(),
        absolute.display(),
        violations.join("\n  "),
        guarded_call_needle(),
        ALLOW_ESCAPE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::panic::AssertUnwindSafe;
    use std::path::PathBuf;
    use std::rc::Rc;

    /// THE property this module exists to provide: the directory is removed
    /// when the scope holding the guard unwinds, not merely when it returns.
    ///
    /// The path is captured in an outer cell so it outlives the closure the
    /// guard is dropped inside of.
    #[test]
    fn prefixed_tempdir_is_removed_on_panic_unwind() {
        let seen: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
        let seen_inner = Rc::clone(&seen);

        // Silence the default panic hook across the deliberate `panic!` below so
        // its backtrace does not drown the RED/GREEN signal.  NOTE: the hook is
        // process-global, so a *concurrent* test panicking inside this narrow
        // window would also be silenced — accepted, the window is two syscalls
        // wide and the concurrent test still fails with its own assertion text.
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let guard = prefixed_tempdir("reify-test-support-unwind-");
            *seen_inner.borrow_mut() = Some(guard.path().to_path_buf());
            assert!(
                guard.path().exists(),
                "the guard's directory must exist before we force the panic"
            );
            panic!("forced unwind");
        }));
        std::panic::set_hook(prev_hook);

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

    // ── find_unguarded_temp_dir_sites ────────────────────────────────────────
    //
    // Every synthetic source below assembles the guarded call at runtime via
    // `.concat()` — the `ignore_hygiene.rs` convention — so this file does not
    // contain the literal adjacent sequence and cannot self-trigger.  (It
    // legitimately calls the real thing in the attribution test above.)

    /// The needle, assembled so it never appears literally in this file.
    fn guarded_call() -> String {
        ["env", "::temp_dir()"].concat()
    }

    /// (1) The `std::`-qualified form — what all five migrating files use today.
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

    // ── assert_no_unguarded_temp_dir_sites ───────────────────────────────────

    /// An unreadable target must fail LOUDLY, naming the absolute path it
    /// tried.  Without this, a future file move would silently turn every
    /// ratchet guard into a vacuous pass.
    #[test]
    fn anuts_unreadable_file_panics_naming_the_absolute_path() {
        let rel = "crates/reify-test-support/definitely-not-a-real-file.rs";
        let err = std::panic::catch_unwind(|| assert_no_unguarded_temp_dir_sites(rel))
            .expect_err("scanning a nonexistent file must panic, not pass vacuously");
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("<non-string panic payload>")
            .to_string();
        assert!(
            msg.contains(&format!("/{rel}")),
            "the panic must name the ABSOLUTE path it attempted (repo root + \
             {rel:?}), so a moved file is obvious; got: {msg:?}"
        );
    }

    // ── ratchet: files migrated off hand-rolled temp dirs ────────────────────
    //
    // Each guard below is ratcheted on by task 5640 together with the migration
    // that makes it pass.  The list is deliberately EXPLICIT rather than a
    // repo-wide sweep: `crates/reify-build-utils/src/lib.rs` still holds a bare
    // call pending task 5639's merge, and a sweep would make this crate red on
    // an unrelated task's merge order.  Adding a file here is how you extend
    // the ratchet — but migrate it in the same or the very next commit.

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
}
