//! The git seam: the [`GitOps`] trait every detector reads git through, its production [`RealGitOps`] impl, and the test-support `MockGitOps`.

use crate::git_history::{BranchMessages, FirstParentChanges};
use serde::{Deserialize, Serialize};
#[cfg(any(test, feature = "test-support"))]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::AtomicUsize;

/// A git commit row (subject is the first line of the commit message).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    pub sha: String,
    pub subject: String,
}

/// `git log --format=...` template used by [`RealGitOps::log_grep`] and
/// referenced from the [`GitOps::log_grep`] trait doc so a second
/// implementation (e.g. a future async / git2-based variant) follows the
/// same wire format the parser expects: SHA, tab (`%x09` = `\t`), subject.
pub const LOG_GREP_FORMAT: &str = "%H%x09%s";

/// All git operations the detectors need. Production: [`RealGitOps`] shells
/// out via [`std::process::Command`]. Tests: [`MockGitOps`] (gated behind
/// `feature = "test-support"`) holds canned answers.
///
/// Object-safe by design — `AuditContext` holds `&'a dyn GitOps` so the
/// production and mock impls coexist behind the same vtable.
pub trait GitOps {
    /// Equivalent of `git log <branch> --grep=<pattern> --format=<F>` where
    /// `F` is [`LOG_GREP_FORMAT`] (SHA, tab, subject). Returns one
    /// [`GitCommit`] per matching commit in `git log`'s default order
    /// (newest-first / reverse-chronological). The P5 detector unions all
    /// returned commits' diffs and does not depend on the order; future
    /// detectors that DO care about order must rely on this contract
    /// explicitly.
    ///
    /// Fail-safe: an empty vec on any git error, so "git found nothing" and
    /// "git failed" are indistinguishable. Callers for whom that collapse is
    /// unsafe must use [`GitOps::try_log_grep`] — see its note.
    fn log_grep(&self, branch: &str, pattern: &str) -> Vec<GitCommit> {
        self.try_log_grep(branch, pattern).unwrap_or_default()
    }

    /// Fallible variant of [`GitOps::log_grep`]: `Ok(hits)` when git ran (an
    /// empty vec meaning it genuinely matched nothing), `Err(description)`
    /// when it did not run or failed.
    ///
    /// Exists because the crate's blanket fail-safe direction INVERTS on the
    /// P5 pre-done gate. In the sweep an empty result converges on "no
    /// finding"; at the gate it empties the rescue candidate list and so
    /// converges on a BLOCKING refusal, making an unreadable pack or an fd
    /// exhaustion under orchestrator load indistinguishable from a genuine
    /// phantom-done. The gate feeds the `Err` into its advisory channel, which
    /// downgrades the refusal to a non-blocking `Low`.
    fn try_log_grep(&self, branch: &str, pattern: &str) -> Result<Vec<GitCommit>, String>;

    /// `git diff --name-only --no-renames <from>..<to>`. Returns the set of
    /// paths changed between the two refs. Renames are reported as
    /// delete + add (both sides), for the reason given on
    /// [`GitOps::changed_paths_in_commit`].
    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String>;

    /// Returns the paths changed by commit `commit` itself, i.e.
    /// `git diff --name-only <commit>^1..<commit>`. For a standard `--no-ff`
    /// merge commit M (first parent M^1 = pre-merge main tip, result = M) this
    /// is exactly the task's net delta. Deletions are reported like any other
    /// change, and renames are reported as delete + add — the diff runs
    /// `--no-renames`, so BOTH the old and the new path appear rather than
    /// git's default of collapsing a detected rename to the destination alone.
    /// That is load-bearing: a corroboration leg must be able to see the old
    /// path, or a task declaring its pre-rename deliverable is refused for work
    /// that did land. Fail-safe: returns an empty vec on any git error —
    /// unreachable or recycled SHA, a root commit with no `^1`, or a non-repo.
    ///
    /// The `--name-only` sibling of [`GitOps::diff_added_lines_in_commit`], and
    /// it exists for the same reason. `diff_changed_paths(main, X)` is
    /// DEGENERATE once `X` is an ancestor of main: `main..X` is a two-point
    /// TREE diff, so the paths the two trees agree on — which, post-merge, are
    /// exactly the paths `X` introduced — are excluded by construction, and
    /// what comes back is the reverse-delta of whatever landed after `X`. A
    /// leg built on it can therefore never corroborate a landed task. Post-merge
    /// the correct question is "what did this commit change".
    fn changed_paths_in_commit(&self, commit: &str) -> Vec<String>;

    /// `git check-ignore -- <path>` — true iff `path` is gitignored
    /// (or matches a negated rule that re-ignores).
    ///
    /// Fail-safe: `false` on any git error, so "not ignored" and "could not
    /// tell" are indistinguishable. Callers for whom that collapse is unsafe
    /// must use [`GitOps::try_is_gitignored`] — see its note.
    fn is_gitignored(&self, path: &str) -> bool {
        self.try_is_gitignored(path).unwrap_or(false)
    }

    /// Fallible variant of [`GitOps::is_gitignored`]: `Ok(false)` means git ran
    /// and the path is not ignored; `Err(description)` means git did not run or
    /// failed, and the question is UNANSWERED.
    ///
    /// Exists for the same inverted-fail-safe reason as
    /// [`GitOps::try_log_grep`]. The P5 pre-done gate builds its declared set by
    /// SUBTRACTING the gitignored subset from `metadata.files`, so a `false`
    /// from a failed `git check-ignore` keeps the entry in `declared` — the
    /// first half of a blocking refusal. The gate routes the `Err` into its
    /// advisory channel instead, downgrading any surviving refusal to a
    /// non-blocking `Low`.
    fn try_is_gitignored(&self, path: &str) -> Result<bool, String>;

    /// Returns `true` iff `path` resolves on `branch` to a tracked file OR a
    /// directory containing tracked files (git does not track empty dirs),
    /// equivalent to `git ls-tree <branch> -- <path>` returning non-empty.
    /// Used by P5's deliverable-presence rescue. Fail-safe: returns `false`
    /// on any git error (missing repo/ref, unknown path) — so "not tracked"
    /// and "could not tell" are indistinguishable. Callers for whom that
    /// collapse is unsafe must use [`GitOps::try_path_tracked_on`].
    fn path_tracked_on(&self, branch: &str, path: &str) -> bool {
        self.try_path_tracked_on(branch, path).unwrap_or(false)
    }

    /// Fallible variant of [`GitOps::path_tracked_on`]: `Ok(false)` means git
    /// ran and the path does not resolve on `branch`; `Err(description)` means
    /// git did not run or failed, and the question is UNANSWERED.
    ///
    /// Exists for the same inverted-fail-safe reason as
    /// [`GitOps::try_log_grep`]. At the P5 pre-done gate a `false` from a
    /// failed `git ls-tree` is read as "the declared deliverable is absent
    /// from main", which is the first half of a blocking refusal — so a
    /// transient (unreadable pack, fd exhaustion, index contention) would
    /// refuse a legitimate done-flip. The gate routes the `Err` into its
    /// advisory channel instead, downgrading the refusal to a non-blocking
    /// `Low`.
    fn try_path_tracked_on(&self, branch: &str, path: &str) -> Result<bool, String>;

    /// Returns the added lines in `git diff <from>..<to> -- <path>` as
    /// `(new_side_line_no, content)` pairs — one entry per `+` line in the
    /// unified diff, with the leading `+` stripped. Line numbers track the
    /// new-file side (the `+c` field of each `@@ -a,b +c,d @@` hunk header).
    /// Returns an empty vec when the branch does not exist or the path has no
    /// added lines.
    fn diff_added_lines(&self, from: &str, to: &str, path: &str) -> Vec<(usize, String)>;

    /// Returns the added lines introduced by commit `commit` into `path`, i.e.
    /// `git diff <commit>^1..<commit> -- <path>`. For a standard `--no-ff` merge
    /// commit M (first parent M^1 = pre-merge main tip, result = M), this yields
    /// exactly the task's net delta on the given path. Fail-safe: returns an empty
    /// vec on any git error, including an unreachable or recycled commit SHA.
    ///
    /// Used by P2's reaped-branch recall path: when `done_provenance.commit` is
    /// set and reachable from `main`, the task's `task/N` branch has typically
    /// been reaped by the orchestrator — but the merge commit M survives on
    /// `main`, so `git diff M^1..M` recovers the exact task delta.
    fn diff_added_lines_in_commit(&self, commit: &str, path: &str) -> Vec<(usize, String)>;

    /// Returns all lines of `path` at `reference` (e.g. `"main"`, `"HEAD"`) as
    /// `(1-based_line_no, content)` pairs, equivalent to `git show
    /// <reference>:<path>` split on `\n`. Fail-safe: returns an empty vec when
    /// the path is missing on that ref or any git error occurs. Trailing
    /// newlines produce no spurious empty entry (a file ending with `\n` returns
    /// the same line count as its logical line count).
    ///
    /// Used by P2's recycled-commit fallback: when `done_provenance.commit` is
    /// set but NOT reachable from `main` (gc'd / recycled SHA), the full-file
    /// content scan on `main` serves as a last-resort recall path.
    fn file_lines_on(&self, reference: &str, path: &str) -> Vec<(usize, String)>;

    /// Returns `true` iff `commit` is a valid ancestor of `branch` (reachable
    /// from it), equivalent to `git merge-base --is-ancestor <commit> <branch>`
    /// (exit 0 = ancestor, exit 1 = not). Used by P5's scope-extension to
    /// corroborate a merged task whose runs.db task_completed event is missing.
    /// Fail-safe: returns `false` on any git error or spawn failure (exit 128
    /// from an unknown commit correctly maps to "not an ancestor").
    fn is_ancestor(&self, commit: &str, branch: &str) -> bool;

    /// Equivalent of `git -C <root> ls-files`: every tracked file path,
    /// root-relative, one per line. Used by the PTODO structural lane to
    /// enumerate the working-tree files it scans (content is then read
    /// directly via `std::fs`, not through this seam). Fail-safe: returns an
    /// empty vec on any git error (missing repo, spawn failure) — the
    /// structural lane treats "no tracked files" and "git failed" identically
    /// (it simply finds nothing to scan).
    fn ls_files(&self) -> Vec<String>;

    /// Equivalent of `git log -1 --format=<LOG_GREP_FORMAT> -- <path>`:
    /// returns `Some(GitCommit)` if the path has any git history (including
    /// paths that were deleted — the `--` ensures the path is treated as a
    /// pathspec even after deletion), or `None` when the path never existed in
    /// the repository OR when any git error occurs.
    ///
    /// Fail-safe semantics: a git invocation failure (spawn error, non-zero
    /// exit, non-UTF-8 output) returns `None` rather than propagating an
    /// error, so the caller (ζ inverse lane) can treat "no history" and
    /// "git unavailable" identically — a git failure can never manufacture a
    /// false-positive `task-cites-deleted-path` / `task-cites-renamed-path`
    /// finding.
    ///
    /// Implementation note: uses [`LOG_GREP_FORMAT`] (`%H%x09%s`) and the
    /// same `splitn(2, '\t')` parse as [`log_grep`], keeping the two seam
    /// methods consistent.
    fn last_commit_for_path(&self, path: &str) -> Option<GitCommit>;

    /// Equivalent of `git show -M --name-status --format= <sha>`: given a
    /// commit that touched `path`, returns `Some(new_path)` iff that commit
    /// RENAMED `path`, i.e. its name-status output carries an `R` line whose
    /// old side is exactly `path`. Used by the ζ inverse lane to tell a
    /// renamed-not-deleted `metadata.files` citation from a genuine deletion,
    /// on the commit [`last_commit_for_path`](GitOps::last_commit_for_path)
    /// already resolved.
    ///
    /// Fail-safe semantics — every one of these returns `None`, and `None`
    /// means the caller keeps the unchanged `task-cites-deleted-path`
    /// classification:
    ///   1. No `R` line whose old side equals `path` (a genuine delete prints
    ///      only `D\t<path>`; a modification prints `M\t<path>`).
    ///   2. A **merge** commit: `git show` defaults to `--cc`, which prints no
    ///      diff for a merge at all (measured), so a rename landed directly in
    ///      a merge degrades to the deleted kind rather than being mislabelled.
    ///   3. Any git error — spawn failure, non-zero exit (e.g. `fatal: bad
    ///      object` from an unreachable/recycled sha).
    ///   4. Non-UTF-8 output.
    ///
    /// So a git failure can never manufacture a false `task-cites-renamed-path`
    /// finding; it can only ever cause a MISSED reclassification.
    ///
    /// Implementation note: `-M` is passed explicitly rather than relying on
    /// git's `diff.renames` default, because a user or global
    /// `diff.renames=false` would otherwise silently disable detection. Copies
    /// (`C` status) are deliberately not resolved — only `-M` is passed.
    fn rename_target_for_path(&self, path: &str, sha: &str) -> Option<String>;
}

/// Production [`GitOps`] impl that shells out to `git`. Untested by the
/// slice-1 integration suite (see `MockGitOps` for the test seam) — kept
/// minimal so the eventual T-4 CLI can construct one and call
/// [`p5_phantom_done::check_pre_done`](crate::p5_phantom_done::check_pre_done).
///
/// # Invariants
///
/// **Construct exactly once per `project_root`.** The private
/// `gitignore_unavailable` field is a per-instance `AtomicBool` that
/// short-circuits all subsequent
/// [`try_is_gitignored`](GitOps::try_is_gitignored) calls after the first
/// unrecoverable `git check-ignore` exit, so a task with N files against
/// a broken git repo emits at most one
/// `reify-audit: git check-ignore exited …` breadcrumb rather than N
/// copies of the same line. It is a BREADCRUMB budget, not a cached
/// answer: a short-circuited call returns `Err` like the one that latched
/// it, silently.
///
/// This dedup is silently defeated by constructing a fresh [`RealGitOps`]
/// per task, per file, or per worker: each new instance starts with a
/// cleared flag and re-emits the breadcrumb on its first failing call.
/// The CLI binary (`bin/reify-audit.rs`) constructs exactly one
/// [`RealGitOps`] per invocation and threads it through [`AuditContext`](crate::AuditContext)
/// for every detector; future callers MUST preserve this single-instance
/// discipline.
///
/// The multi-file regression test
/// `cli::git_check_ignore_breadcrumb_dedups_across_files`
/// (`tests/cli.rs`) pins the user-visible signal: with N≥2 files in a
/// non-git directory, exactly one breadcrumb appears in stderr.
///
/// # History snapshot (opt-in, for sweeps)
///
/// [`RealGitOps::with_history_snapshot`] lets one instance answer three
/// questions about a branch from memory instead of forking per question:
/// [`is_ancestor`](GitOps::is_ancestor) and
/// [`try_log_grep`](GitOps::try_log_grep) against that branch, and
/// [`changed_paths_in_commit`](GitOps::changed_paths_in_commit) for a commit
/// reachable from it. A whole-corpus sweep asks them thousands of times, and
/// each fork walks the branch's history again.
///
/// Each answer is exactly what the per-call command would give, and anything
/// the snapshot cannot answer exactly — an abbreviated SHA, an unreachable
/// commit, a non-literal pattern, a root commit — still forks. The two dumps
/// it reads are loaded lazily, each on the first question that needs it, so a
/// run that never asks pays nothing; a dump that fails to load or parse costs
/// one breadcrumb and leaves every question forking as before.
///
/// The snapshot is point-in-time: the branch is read once, and commits landing
/// on it later are not seen. That gives a sweep one consistent view of
/// history. A short single-task run should not opt in, since one dump costs
/// more than the handful of forks it would replace.
pub struct RealGitOps {
    /// Working directory passed as `git -C <dir>` to every invocation.
    pub project_root: PathBuf,
    /// Set to `true` the first time `try_is_gitignored` encounters a genuine
    /// non-0/1 exit status from `git check-ignore` (exit code other than 0 or
    /// 1). Subsequent calls short-circuit to `Err` silently, so a task with N
    /// files against a broken git repo emits at most one breadcrumb rather than
    /// N copies of the same line. `Err`, not `Ok(false)`: the flag budgets the
    /// breadcrumb and makes no claim that the answer is known.
    ///
    /// A spawn-level `Err` (EAGAIN/ENOMEM transient) does **not** latch this
    /// flag — a transient OS failure is not evidence that `git check-ignore` is
    /// permanently broken for this repo.
    ///
    /// Invariant: per-instance — see [`RealGitOps`] doc for the
    /// single-instance construction requirement that makes this budget
    /// meaningful in production.
    gitignore_unavailable: AtomicBool,
    /// The opt-in history snapshot; see the type doc.
    history: Option<HistorySnapshot>,
    /// Number of `spawn_once` invocations to fail with a synthetic
    /// `Err(WouldBlock)` before delegating to a real `git` subprocess.
    /// Mirrors the `gitignore_unavailable` interior-mutability pattern.
    /// Compiled out of production builds entirely.
    #[cfg(any(test, feature = "test-support"))]
    inject_spawn_failures: AtomicUsize,
}

/// Parse the `+` lines from a unified diff (`git diff` stdout) into
/// `(new_side_line_no, content)` pairs, with the leading `+` stripped.
/// Line numbers track the new-file side (the `+c` field of each
/// `@@ -a,b +c,d @@` hunk header). Shared by [`RealGitOps::diff_added_lines`]
/// and [`RealGitOps::diff_added_lines_in_commit`] to avoid duplicating ~20 lines.
fn parse_added_lines(stdout: &str) -> Vec<(usize, String)> {
    let mut result = Vec::new();
    let mut new_line: usize = 0;
    let mut in_hunk = false;
    for line in stdout.lines() {
        if line.starts_with("@@ ") {
            in_hunk = true;
            // Parse "@@ -a,b +c,d @@" to extract c (new-file start line).
            if let Some(plus_pos) = line.find(" +") {
                let rest = &line[plus_pos + 2..];
                let delim = rest.find([',', ' ']).unwrap_or(rest.len());
                if let Ok(c) = rest[..delim].parse::<usize>() {
                    // Set counter so first context/+ line yields c.
                    new_line = c.saturating_sub(1);
                }
            }
        } else if !in_hunk {
            // Pre-hunk header lines (diff/index/---/+++ headers): skip.
        } else if let Some(stripped) = line.strip_prefix('+') {
            new_line += 1;
            result.push((new_line, stripped.to_string()));
        } else if line.starts_with('-') {
            // Removed line: new-side counter does not advance.
        } else if line.starts_with('\\') {
            // "\ No newline at end of file" — ignore.
        } else {
            // Context line (starts with ' '): both sides advance.
            new_line += 1;
        }
    }
    result
}

/// Extract the rename TARGET of `old_path` from `git show -M --name-status
/// --format= <sha>` stdout: the third TAB-separated field of the `R` line
/// whose second field is exactly `old_path`.
///
/// Pure (no I/O) so the parse can be unit-tested against the measured real
/// shapes; [`RealGitOps::rename_target_for_path`] supplies the stdout.
///
/// The only shape that yields `Some` is a status field of `R` followed by
/// zero or more ASCII digits (git's similarity score, e.g. `R100`) whose OLD
/// side matches. That exactness is the safety property: a bare
/// `starts_with('R')` would let a future status letter false-match, and
/// requiring the old side to match keeps the relation from being inverted
/// (querying a rename's TARGET must not resolve).
fn parse_rename_target(stdout: &str, old_path: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let mut fields = line.split('\t');
        // Status: `R` + similarity score (`R100`, `R087`); reject `D`, `M`,
        // `A`, `C…`, and any hypothetical future `R`-prefixed letter.
        let score = fields.next()?.strip_prefix('R')?;
        if !score.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        // Old side must be the cited path; new side is the answer.
        if fields.next()? != old_path {
            return None;
        }
        let new_path = fields.next()?;
        if new_path.is_empty() {
            return None;
        }
        Some(new_path.to_string())
    })
}

/// One branch's history, each part loaded on first need. `None` inside a
/// loaded cell records a load that failed, so it is not retried.
struct HistorySnapshot {
    branch: String,
    messages: OnceLock<Option<BranchMessages>>,
    changes: OnceLock<Option<FirstParentChanges>>,
}

impl RealGitOps {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            gitignore_unavailable: AtomicBool::new(false),
            history: None,
            #[cfg(any(test, feature = "test-support"))]
            inject_spawn_failures: AtomicUsize::new(0),
        }
    }

    /// Answer ancestry, message grep and per-commit changed paths for
    /// `branch` from a point-in-time snapshot of its history. See the type
    /// doc's "History snapshot" section.
    pub fn with_history_snapshot(mut self, branch: &str) -> Self {
        self.history = Some(HistorySnapshot {
            branch: branch.to_string(),
            messages: OnceLock::new(),
            changes: OnceLock::new(),
        });
        self
    }

    /// The snapshot's messages of `branch`, loading them on first need, or
    /// `None` when there is no snapshot of `branch` or it failed to load.
    fn snapshot_messages(&self, branch: &str) -> Option<&BranchMessages> {
        let history = self.history.as_ref().filter(|h| h.branch == branch)?;
        history
            .messages
            .get_or_init(|| {
                let argv = BranchMessages::argv(&history.branch);
                self.load_history_part(&history.branch, "messages", &argv, |raw| {
                    BranchMessages::parse(&raw)
                })
            })
            .as_ref()
    }

    /// The snapshot's first-parent changed paths, loading them on first need,
    /// or `None` when there is no snapshot or it failed to load.
    fn snapshot_changes(&self) -> Option<&FirstParentChanges> {
        let history = self.history.as_ref()?;
        history
            .changes
            .get_or_init(|| {
                let argv = FirstParentChanges::argv(&history.branch);
                self.load_history_part(&history.branch, "changed paths", &argv, |raw| {
                    let dump = String::from_utf8(raw)
                        .map_err(|_| "git output not valid UTF-8".to_string())?;
                    FirstParentChanges::parse(&dump)
                })
            })
            .as_ref()
    }

    /// Run one history dump and parse it, or emit ONE breadcrumb and return
    /// `None` so every question it would have answered forks instead.
    fn load_history_part<T>(
        &self,
        branch: &str,
        part: &str,
        argv: &[String],
        parse: impl FnOnce(Vec<u8>) -> Result<T, String>,
    ) -> Option<T> {
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.run_raw(&argv).and_then(parse).map_or_else(
            |e| {
                eprintln!(
                    "reify-audit: git history snapshot of {branch} ({part}) unavailable in {}: \
                     {e}; falling back to one git call per question",
                    self.project_root.display()
                );
                None
            },
            Some,
        )
    }

    /// Inject `n` transient spawn failures into the next `n` `spawn_once`
    /// calls.  Each call that fires returns
    /// `Err(io::ErrorKind::WouldBlock, "injected transient spawn failure (EAGAIN)")`.
    ///
    /// Mirrors the `gitignore_unavailable` interior-mutability pattern with
    /// `Ordering::Relaxed` — safe because a single-threaded integration test
    /// drives the injection.
    ///
    /// Compiled out of production builds entirely
    /// (`#[cfg(any(test, feature = "test-support"))]`).
    #[cfg(any(test, feature = "test-support"))]
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_next_spawns(&self, n: usize) {
        self.inject_spawn_failures.store(n, Ordering::Relaxed);
    }

    /// Spawn a single `git -C <root> <args…>` invocation and return its
    /// `Output`.
    ///
    /// Under `#[cfg(any(test, feature = "test-support"))]`, if
    /// `inject_spawn_failures > 0`, decrements the counter and returns a
    /// synthetic `Err(WouldBlock)` simulating an EAGAIN transient OS failure,
    /// without touching a real subprocess.  Production builds delegate
    /// unconditionally to `Command::output()`.
    fn spawn_once(&self, args: &[&str]) -> std::io::Result<std::process::Output> {
        #[cfg(any(test, feature = "test-support"))]
        {
            let remaining = self.inject_spawn_failures.load(Ordering::Relaxed);
            if remaining > 0 {
                self.inject_spawn_failures.store(remaining - 1, Ordering::Relaxed);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "injected transient spawn failure (EAGAIN)",
                ));
            }
        }
        crate::git_env::command(&self.project_root)
            .args(args)
            .output()
    }

    /// Spawn a `git` invocation with bounded retry on transient OS-level spawn
    /// failures (`Command::output()` returns `Err`).
    ///
    /// Retries up to `MAX_ATTEMPTS - 1` times with a linearly-increasing
    /// short backoff (50 ms, 100 ms; ~150 ms total) when `spawn_once` returns `Err`.
    /// Returns `Ok` immediately on the first successful spawn, regardless of the
    /// git exit code (a non-zero exit is an `Ok` result whose status is checked
    /// by `run()`).  After `MAX_ATTEMPTS` exhaustion returns the last `Err`.
    ///
    /// Why retry on ANY `Err` (not just `WouldBlock`/`OutOfMemory`):
    ///   `Command::output()` returns `Err` ONLY when the process could not be
    ///   started or its output collected — the EAGAIN/ENOMEM class.  Once git
    ///   actually runs, `output()` is `Ok` regardless of exit code.  Filtering
    ///   by `ErrorKind` risks under-matching host-specific transient errnos.
    ///   The only cost of the broader match is bounded extra latency on a truly
    ///   permanent error (e.g. `git` not on PATH), which already fails degraded.
    fn spawn_with_retry(&self, args: &[&str]) -> std::io::Result<std::process::Output> {
        const MAX_ATTEMPTS: u32 = 3;
        let mut last_err = None;
        for attempt in 0..MAX_ATTEMPTS {
            match self.spawn_once(args) {
                Ok(out) => return Ok(out),
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < MAX_ATTEMPTS {
                        std::thread::sleep(std::time::Duration::from_millis(
                            50 * u64::from(attempt + 1),
                        ));
                    }
                }
            }
        }
        // Unreachable without exhausting the loop, but satisfies the type-checker.
        Err(last_err.expect("at least one attempt was made"))
    }

    /// Run a git command and return its stdout as `Ok(String)`, or an error
    /// description as `Err(String)`. Three failure modes:
    ///   1. `Command::output()` failed (spawn error, all retries exhausted) →
    ///      Err("git invocation failed: …")
    ///   2. Non-zero exit status → Err("git exited N: <stderr>")
    ///   3. Non-UTF-8 stdout → Err("git output not valid UTF-8")
    ///
    /// Transient OS-level spawn failures (EAGAIN / ENOMEM) are retried
    /// transparently by `spawn_with_retry`.  The happy path (first `Ok`)
    /// pays zero added latency.  After retry exhaustion the error propagates
    /// through `run_or_warn` → `None` → callers return `vec![]`, degrading
    /// exactly as before.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        String::from_utf8(self.run_raw(args)?).map_err(|_| "git output not valid UTF-8".to_string())
    }

    /// [`RealGitOps::run`] without the UTF-8 decode: failure modes 1 and 2
    /// only, and stdout as raw bytes.
    fn run_raw(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = self.spawn_with_retry(args)
            .map_err(|e| format!("git invocation failed: {}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "git exited {:?}: {}",
                out.status.code(),
                stderr.trim()
            ));
        }
        Ok(out.stdout)
    }

    /// `git rev-parse HEAD` — the working tree's current commit sha.
    ///
    /// Routed through `run` (hence `spawn_with_retry`) rather than spawning
    /// `git` directly, so a transient OS-level spawn failure — the EAGAIN /
    /// ENOMEM class that was the root cause of the #4800 flake, and that this
    /// project's CPU-load management makes a live possibility — is retried
    /// instead of surfacing to the caller as a hard failure. The caller (the
    /// jcodemunch §4.3 freshness gate) turns an `Err` here into a refusal of
    /// the whole run, so an unretried fork failure would abort an audit with a
    /// message blaming index freshness — a misleading diagnosis for a
    /// transient the retry absorbs.
    ///
    /// `run` also inherits `git_env::command`'s sanitization, which is
    /// load-bearing here: an inherited `GIT_DIR` / `GIT_WORK_TREE` makes
    /// `git -C <root>` report a DIFFERENT repository, and a HEAD read from the
    /// wrong repo would make the freshness comparison silently meaningless.
    ///
    /// Errors on a failed rev-parse (not a repo, unborn HEAD) and on an empty
    /// sha, which no healthy invocation produces.
    pub fn head_sha(&self) -> Result<String, String> {
        let sha = self.run(&["rev-parse", "HEAD"])?.trim().to_string();
        if sha.is_empty() {
            return Err(format!(
                "`git rev-parse HEAD` produced no sha in {}",
                self.project_root.display()
            ));
        }
        Ok(sha)
    }

    /// Run a git command, emitting a `reify-audit:` breadcrumb on failure and
    /// PRESERVING the error description for callers that must distinguish
    /// "git answered no" from "git failed".
    ///
    /// `label` is the human-readable git subcommand used in the breadcrumb
    /// (e.g. `"log --grep"`, `"diff --name-only"`, `"diff"`).
    fn run_warned(&self, label: &str, args: &[&str]) -> Result<String, String> {
        self.run(args).map_err(|e| {
            eprintln!(
                "reify-audit: git {} failed in {}: {}",
                label,
                self.project_root.display(),
                e
            );
            e
        })
    }

    /// [`RealGitOps::run_warned`] with the error discarded, so callers can
    /// `else { return vec![]; }` in one line. The breadcrumb is identical —
    /// only the recoverable error string is dropped.
    fn run_or_warn(&self, label: &str, args: &[&str]) -> Option<String> {
        self.run_warned(label, args).ok()
    }
}

impl GitOps for RealGitOps {
    fn try_log_grep(&self, branch: &str, pattern: &str) -> Result<Vec<GitCommit>, String> {
        if let Some(hits) = self
            .snapshot_messages(branch)
            .and_then(|messages| messages.grep_literal(pattern))
        {
            return Ok(hits
                .into_iter()
                .map(|(sha, subject)| GitCommit {
                    sha: sha.to_string(),
                    subject: subject.to_string(),
                })
                .collect());
        }
        let stdout = self.run_warned("log --grep", &[
            "log",
            branch,
            &format!("--grep={}", pattern),
            &format!("--format={}", LOG_GREP_FORMAT),
        ])?;
        Ok(stdout
            .lines()
            .filter_map(|l| {
                let mut parts = l.splitn(2, '\t');
                let sha = parts.next()?.to_string();
                let subject = parts.next().unwrap_or("").to_string();
                Some(GitCommit { sha, subject })
            })
            .collect())
    }

    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        // `--no-renames`: see `changed_paths_in_commit` below for the full
        // rationale. This seam has the identical exposure — it is the arm
        // `changed_paths_for_claim` takes for the un-landed branch-tip case.
        let Some(stdout) = self.run_or_warn(
            "diff --name-only",
            &[
                "diff",
                "--name-only",
                "--no-renames",
                &format!("{}..{}", from, to),
            ],
        ) else {
            return vec![];
        };
        stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn changed_paths_in_commit(&self, commit: &str) -> Vec<String> {
        if let Some(paths) = self
            .snapshot_changes()
            .and_then(|changes| changes.changed_paths(commit))
        {
            return paths.to_vec();
        }
        // Goes through run_or_warn (not Command::output directly) so the
        // single-RealGitOps-instance breadcrumb dedup stays intact.
        //
        // `--no-renames` is load-bearing, not cosmetic. `diff.renames` has
        // defaulted to true since git 2.9 and this repo sets no override, so a
        // detected rename collapses to the DESTINATION path alone and the
        // source vanishes from the listing. The consumers of this seam only
        // ever SUBTRACT from the pre-done gate's "absent from main" set, so a
        // task declaring its pre-rename path would find that path neither
        // tracked on main (renamed away) nor in its landing commit's delta
        // (detection hid it) — a refused flip for work that did land. Widening
        // a rename back to both paths cannot manufacture a refusal, and it
        // cannot over-accept either: the rename really did touch both.
        //
        // Scope boundary: `diff_added_lines_in_commit` deliberately does NOT
        // take this flag — see its own comment.
        let Some(stdout) = self.run_or_warn(
            "diff --name-only",
            &[
                "diff",
                "--name-only",
                "--no-renames",
                &format!("{}^1..{}", commit, commit),
            ],
        ) else {
            return vec![];
        };
        stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn try_is_gitignored(&self, path: &str) -> Result<bool, String> {
        // `git check-ignore` exit code 0 = ignored, 1 = not ignored.
        // Any other outcome (spawn error, exit code other than 0/1) is a git
        // failure — log a breadcrumb and report the question as unanswered.
        //
        // `--` is load-bearing: `metadata.files` is hand-authored and
        // unescaped, so an entry beginning with `-` would otherwise be parsed
        // as an option and exit 129.
        //
        // Use `.output()` (not `.status()`) to capture git's own stderr so
        // that "fatal: not a git repository" and similar diagnostics do not
        // leak to *our* process's stderr and corrupt the machine-readable
        // JSON output written there by the CLI dispatcher.
        //
        // Once a genuine non-0/1 exit status is observed (Ok arm with bad
        // code), `gitignore_unavailable` is latched so subsequent calls
        // short-circuit without forking git again — a task with N files
        // against a broken repo emits at most one breadcrumb rather than N
        // identical lines.  The latch budgets the BREADCRUMB only: a
        // short-circuited call is still an unanswered question and returns
        // `Err`, never `Ok(false)`.  A spawn-level Err (EAGAIN/ENOMEM
        // transient) does NOT latch the flag: a transient OS failure is not
        // evidence that git check-ignore is permanently broken for this repo.
        if self.gitignore_unavailable.load(Ordering::Relaxed) {
            return Err("git check-ignore previously unavailable in this repository".to_string());
        }
        // Intentionally calls Command::output() directly rather than going
        // through spawn_with_retry.  This seam has its own per-session
        // AtomicBool dedup latch (gitignore_unavailable) that a retry loop
        // would complicate; a spawn-level transient EAGAIN here already does
        // NOT set the latch (see Err branch below), so recovery is possible
        // on the next call.  The shell-layer run_audit retry in the PTODO infra
        // test provides defense-in-depth against persistent spawn pressure.
        match crate::git_env::command(&self.project_root)
            .args(["check-ignore", "--quiet", "--", path])
            .output()
        {
            Ok(out) if out.status.code() == Some(0) => Ok(true),
            Ok(out) if out.status.code() == Some(1) => Ok(false),
            Ok(out) => {
                self.gitignore_unavailable.store(true, Ordering::Relaxed);
                eprintln!(
                    "reify-audit: git check-ignore exited {:?} in {}",
                    out.status.code(),
                    self.project_root.display()
                );
                Err(format!("git check-ignore exited {:?}", out.status.code()))
            }
            Err(e) => {
                // Spawn failure (EAGAIN/ENOMEM under load) — do NOT latch
                // `gitignore_unavailable`.  A transient spawn error is not
                // evidence that git check-ignore is permanently unavailable;
                // latching here would silently disable ignore-filtering for
                // the entire session after a single resource blip.  Only a
                // genuine non-0/1 exit status (above) warrants the dedup latch.
                eprintln!(
                    "reify-audit: git check-ignore failed in {}: {}",
                    self.project_root.display(),
                    e
                );
                Err(format!("git check-ignore failed: {e}"))
            }
        }
    }

    fn try_path_tracked_on(&self, branch: &str, path: &str) -> Result<bool, String> {
        self.run_warned("ls-tree", &["ls-tree", branch, "--", path])
            .map(|stdout| !stdout.trim().is_empty())
    }

    fn is_ancestor(&self, commit: &str, branch: &str) -> bool {
        if self
            .snapshot_messages(branch)
            .is_some_and(|messages| messages.contains(commit))
        {
            return true;
        }
        // Use .output() (not .status()) so git's stderr ("fatal: not a git
        // repository", "fatal: Not a valid commit name", etc.) is captured and
        // does not leak to our process's stderr / corrupt JSON output.
        // exit 0 = ancestor; exit 1 = not an ancestor; exit 128 = bad object
        // or not-a-repo — all non-zero cases correctly map to false (fail-safe).
        //
        // Intentionally calls Command::output() directly rather than going
        // through spawn_with_retry: is_ancestor() already fails-safe (returns
        // false) on any error.  Residual transient risk: a spawn failure
        // returns false (not-ancestor) when the commit IS actually an ancestor,
        // which may affect orphan-detection in the over-conservative direction
        // (exit 0→1 via a spurious finding) — caught by re-running.
        // The shell-layer run_audit retry in the PTODO infra test provides
        // defense-in-depth.
        match crate::git_env::command(&self.project_root)
            .args(["merge-base", "--is-ancestor", commit, branch])
            .output()
        {
            Ok(out) => out.status.code() == Some(0),
            Err(_) => false,
        }
    }

    fn diff_added_lines(&self, from: &str, to: &str, path: &str) -> Vec<(usize, String)> {
        let Some(stdout) = self.run_or_warn(
            "diff",
            &["diff", &format!("{}..{}", from, to), "--", path],
        ) else {
            return vec![];
        };
        parse_added_lines(&stdout)
    }

    fn diff_added_lines_in_commit(&self, commit: &str, path: &str) -> Vec<(usize, String)> {
        // `<commit>^1..<commit>` is the first-parent diff of the merge commit:
        //   - M^1 = pre-merge main tip
        //   - M   = merged result
        // This yields exactly the task's net delta on `path`.
        //
        // Deliberately NOT `--no-renames`, unlike the two `--name-only` path
        // listing seams above. This is a pathspec-scoped CONTENT diff on P2's
        // provenance path, not on the pre-done gate path, and `--no-renames`
        // here would re-render a pure move as a whole-file add — a behaviour
        // change with no defect behind it. Do not "finish the job".
        let range = format!("{}^1..{}", commit, commit);
        let Some(stdout) = self.run_or_warn(
            "diff (commit)",
            &["diff", &range, "--", path],
        ) else {
            return vec![];
        };
        parse_added_lines(&stdout)
    }

    fn file_lines_on(&self, reference: &str, path: &str) -> Vec<(usize, String)> {
        // `git show <reference>:<path>` prints the file at that ref.
        let spec = format!("{}:{}", reference, path);
        let Some(stdout) = self.run_or_warn("show", &["show", &spec]) else {
            return vec![];
        };
        stdout
            .lines()
            .enumerate()
            .map(|(i, l)| (i + 1, l.to_string()))
            .collect()
    }

    fn ls_files(&self) -> Vec<String> {
        let Some(stdout) = self.run_or_warn("ls-files", &["ls-files"]) else {
            return vec![];
        };
        stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn last_commit_for_path(&self, path: &str) -> Option<GitCommit> {
        // `git log -1 --format=<F> -- <path>` returns the most recent commit
        // touching `path` (including deletions). The `--` separator ensures
        // the path is treated as a pathspec even for files no longer present.
        // run_or_warn returns None on any git failure → fail-safe (no false positive).
        let stdout = self.run_or_warn(
            "log -1 (path)",
            &["log", "-1", &format!("--format={}", LOG_GREP_FORMAT), "--", path],
        )?;
        let line = stdout.trim();
        if line.is_empty() {
            // Path never existed (or `git log` returned nothing) → not deleted.
            return None;
        }
        let mut parts = line.splitn(2, '\t');
        let sha = parts.next()?.to_string();
        let subject = parts.next().unwrap_or("").to_string();
        Some(GitCommit { sha, subject })
    }

    fn rename_target_for_path(&self, path: &str, sha: &str) -> Option<String> {
        // `git show -M --name-status --format= <sha>` prints one status line
        // per path the commit touched, with rename detection ON. `--format=`
        // suppresses the commit header so only status lines remain, and `-M`
        // is explicit so a user/global `diff.renames=false` cannot silently
        // disable detection. run_or_warn returns None on any git failure →
        // fail-safe (the caller keeps `task-cites-deleted-path`).
        let stdout = self.run_or_warn(
            "show -M --name-status",
            &["show", "-M", "--name-status", "--format=", sha],
        )?;
        parse_rename_target(&stdout, path)
    }
}

// -----------------------------------------------------------------------
// Test-support seam
// -----------------------------------------------------------------------

/// HashMap-backed [`GitOps`] for tests. Gated behind `feature = "test-support"`
/// so it never pollutes the production public API. The crate self-pulls this
/// feature in its own `[dev-dependencies]` so integration tests in
/// `tests/p5.rs` see it; downstream crates wanting to construct one for
/// their own tests should depend on `reify-audit` with the feature enabled.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct MockGitOps {
    log_grep: HashMap<(String, String), Vec<GitCommit>>,
    diff_changed_paths: HashMap<(String, String), Vec<String>>,
    changed_paths_in_commit: HashMap<String, Vec<String>>,
    is_gitignored: HashMap<String, bool>,
    /// Simulated `git check-ignore` FAILURES, keyed like `is_gitignored`. An
    /// entry here makes `try_is_gitignored` return `Err`, which is a different
    /// observation from `Ok(false)` — see [`GitOps::try_is_gitignored`].
    is_gitignored_errors: HashMap<String, String>,
    diff_added_lines: HashMap<(String, String, String), Vec<(usize, String)>>,
    diff_added_lines_in_commit: HashMap<(String, String), Vec<(usize, String)>>,
    file_lines_on: HashMap<(String, String), Vec<(usize, String)>>,
    path_tracked_on: HashMap<(String, String), bool>,
    /// Simulated `git ls-tree` FAILURES, keyed like `path_tracked_on`. An
    /// entry here makes `try_path_tracked_on` return `Err`, which is a
    /// different observation from `Ok(false)` — see
    /// [`GitOps::try_path_tracked_on`].
    path_tracked_on_errors: HashMap<(String, String), String>,
    /// Simulated `git log --grep` FAILURES, keyed like `log_grep`.
    log_grep_errors: HashMap<(String, String), String>,
    is_ancestor: HashMap<(String, String), bool>,
    ls_files: Vec<String>,
    last_commit_for_path: HashMap<String, GitCommit>,
    rename_target_for_path: HashMap<(String, String), String>,
}

#[cfg(any(test, feature = "test-support"))]
impl MockGitOps {
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn new() -> Self {
        Self::default()
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_log_grep(&mut self, branch: &str, pattern: &str, commits: Vec<GitCommit>) {
        self.log_grep
            .insert((branch.to_string(), pattern.to_string()), commits);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_diff_changed_paths(&mut self, from: &str, to: &str, paths: Vec<String>) {
        self.diff_changed_paths
            .insert((from.to_string(), to.to_string()), paths);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_is_gitignored(&mut self, path: &str, ignored: bool) {
        self.is_gitignored.insert(path.to_string(), ignored);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_diff_added_lines(
        &mut self,
        from: &str,
        to: &str,
        path: &str,
        added: Vec<(usize, String)>,
    ) {
        self.diff_added_lines
            .insert((from.to_string(), to.to_string(), path.to_string()), added);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_path_tracked_on(&mut self, branch: &str, path: &str, present: bool) {
        self.path_tracked_on
            .insert((branch.to_string(), path.to_string()), present);
    }

    /// Make `git ls-tree <branch> -- <path>` FAIL rather than answer.
    ///
    /// Distinct from `set_path_tracked_on(.., false)`: that is git answering
    /// "not tracked", this is git not answering at all. The P5 pre-done gate
    /// must not build a blocking refusal on the latter.
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_path_tracked_on_error(&mut self, branch: &str, path: &str, err: &str) {
        self.path_tracked_on_errors
            .insert((branch.to_string(), path.to_string()), err.to_string());
    }

    /// Make `git check-ignore -- <path>` FAIL rather than answer.
    ///
    /// Distinct from `set_is_gitignored(.., false)`: that is git answering
    /// "not ignored", this is git not answering at all. The P5 pre-done gate
    /// subtracts the ignored set from the declared set, so it must not read the
    /// latter as the former.
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_is_gitignored_error(&mut self, path: &str, err: &str) {
        self.is_gitignored_errors
            .insert(path.to_string(), err.to_string());
    }

    /// Make `git log <branch> --grep=<pattern>` FAIL rather than answer.
    ///
    /// Distinct from `set_log_grep(.., vec![])`: that is git answering "no
    /// matching commits", this is git not answering at all.
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_log_grep_error(&mut self, branch: &str, pattern: &str, err: &str) {
        self.log_grep_errors
            .insert((branch.to_string(), pattern.to_string()), err.to_string());
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_is_ancestor(&mut self, commit: &str, branch: &str, ancestor: bool) {
        self.is_ancestor
            .insert((commit.to_string(), branch.to_string()), ancestor);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_diff_added_lines_in_commit(
        &mut self,
        commit: &str,
        path: &str,
        added: Vec<(usize, String)>,
    ) {
        self.diff_added_lines_in_commit
            .insert((commit.to_string(), path.to_string()), added);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_changed_paths_in_commit(&mut self, commit: &str, paths: Vec<String>) {
        self.changed_paths_in_commit.insert(commit.to_string(), paths);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_file_lines_on(
        &mut self,
        reference: &str,
        path: &str,
        lines: Vec<(usize, String)>,
    ) {
        self.file_lines_on
            .insert((reference.to_string(), path.to_string()), lines);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_ls_files(&mut self, files: Vec<String>) {
        self.ls_files = files;
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_last_commit_for_path(&mut self, path: &str, commit: GitCommit) {
        self.last_commit_for_path.insert(path.to_string(), commit);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_rename_target_for_path(&mut self, path: &str, sha: &str, target: &str) {
        self.rename_target_for_path
            .insert((path.to_string(), sha.to_string()), target.to_string());
    }
}

#[cfg(any(test, feature = "test-support"))]
impl GitOps for MockGitOps {
    fn try_log_grep(&self, branch: &str, pattern: &str) -> Result<Vec<GitCommit>, String> {
        let key = (branch.to_string(), pattern.to_string());
        if let Some(err) = self.log_grep_errors.get(&key) {
            return Err(err.clone());
        }
        Ok(self.log_grep.get(&key).cloned().unwrap_or_default())
    }

    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        self.diff_changed_paths
            .get(&(from.to_string(), to.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn changed_paths_in_commit(&self, commit: &str) -> Vec<String> {
        self.changed_paths_in_commit
            .get(commit)
            .cloned()
            .unwrap_or_default()
    }

    fn try_is_gitignored(&self, path: &str) -> Result<bool, String> {
        if let Some(err) = self.is_gitignored_errors.get(path) {
            return Err(err.clone());
        }
        Ok(self.is_gitignored.get(path).copied().unwrap_or(false))
    }

    fn diff_added_lines(&self, from: &str, to: &str, path: &str) -> Vec<(usize, String)> {
        self.diff_added_lines
            .get(&(from.to_string(), to.to_string(), path.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn diff_added_lines_in_commit(&self, commit: &str, path: &str) -> Vec<(usize, String)> {
        self.diff_added_lines_in_commit
            .get(&(commit.to_string(), path.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn file_lines_on(&self, reference: &str, path: &str) -> Vec<(usize, String)> {
        self.file_lines_on
            .get(&(reference.to_string(), path.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn try_path_tracked_on(&self, branch: &str, path: &str) -> Result<bool, String> {
        let key = (branch.to_string(), path.to_string());
        if let Some(err) = self.path_tracked_on_errors.get(&key) {
            return Err(err.clone());
        }
        Ok(self.path_tracked_on.get(&key).copied().unwrap_or(false))
    }

    fn is_ancestor(&self, commit: &str, branch: &str) -> bool {
        self.is_ancestor
            .get(&(commit.to_string(), branch.to_string()))
            .copied()
            .unwrap_or(false)
    }

    fn ls_files(&self) -> Vec<String> {
        self.ls_files.clone()
    }

    fn last_commit_for_path(&self, path: &str) -> Option<GitCommit> {
        self.last_commit_for_path.get(path).cloned()
    }

    fn rename_target_for_path(&self, path: &str, sha: &str) -> Option<String> {
        self.rename_target_for_path
            .get(&(path.to_string(), sha.to_string()))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::{RealGitOps, parse_rename_target};

    /// A history snapshot must not cost `RealGitOps` its `Sync`: the CLI's
    /// one instance is shared by every detector.
    #[test]
    fn real_git_ops_with_a_history_snapshot_is_sync() {
        fn assert_sync<T: Sync>(_: &T) {}
        assert_sync(&RealGitOps::new("/nonexistent").with_history_snapshot("main"));
    }

    /// The measured single-rename shape: `git show -M --name-status --format=`
    /// prints `R<score>\t<old>\t<new>`.
    #[test]
    fn parse_rename_target_matches_measured_rename_line() {
        let stdout = "R100\told.rs\tsub/new.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "old.rs"),
            Some("sub/new.rs".to_string()),
            "an R line whose old side matches must yield the new side",
        );
    }

    /// Modelled on 60be72d922, whose measured shape has the rename line NOT
    /// first (one unrelated `A` line precedes it), so the scan must not stop at
    /// the first non-`R` line. The preceding lines here are synthetic — the
    /// property under test is the position of the `R` line, not the exact
    /// neighbours that commit happens to carry.
    #[test]
    fn parse_rename_target_finds_rename_after_other_status_lines() {
        let stdout = "A\tcrates/reify-compiler/tests/harness_doc_chunks/mod.rs\n\
                      M\tcrates/reify-compiler/src/lib.rs\n\
                      R100\tcrates/reify-compiler/tests/geometry_chunk_smoke.rs\tcrates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "crates/reify-compiler/tests/geometry_chunk_smoke.rs"),
            Some(
                "crates/reify-compiler/tests/harness_doc_chunks/geometry_chunk_smoke.rs"
                    .to_string()
            ),
            "a rename line preceded by A/M lines must still be found",
        );
    }

    /// A genuine delete prints only `D\t<path>` lines — the fail-safe path that
    /// keeps `task-cites-deleted-path` unchanged.
    #[test]
    fn parse_rename_target_delete_only_output_is_none() {
        let stdout = "D\tdoomed.rs\nD\tcrates/other.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "doomed.rs"),
            None,
            "a delete-only commit must not resolve a rename target",
        );
    }

    /// An `R` line for a DIFFERENT path must not match: the old side is the
    /// key, so the relation can never be inverted or cross-wired.
    #[test]
    fn parse_rename_target_other_path_rename_is_none() {
        let stdout = "R100\tunrelated.rs\tsub/unrelated.rs\n";
        assert_eq!(
            parse_rename_target(stdout, "old.rs"),
            None,
            "an R line whose old side is a different path must not match",
        );
        assert_eq!(
            parse_rename_target(stdout, "sub/unrelated.rs"),
            None,
            "querying the rename TARGET as if it were the source must not match",
        );
    }

    /// Empty stdout — the measured MERGE-commit shape (`git show` defaults to
    /// `--cc`, which prints no diff for a merge) and the empty-output case in
    /// general.
    #[test]
    fn parse_rename_target_empty_stdout_is_none() {
        assert_eq!(
            parse_rename_target("", "old.rs"),
            None,
            "empty stdout (e.g. a merge commit) must not resolve a rename target",
        );
    }
}
