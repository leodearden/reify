//! Reify architecture audit forensics.
//!
//! This crate implements the F-infra detector core described in
//! `docs/architecture-audit/f-infra-design.md`. The crate currently ships
//! three detectors: P5 (phantom-done), P2 (consumer-stub), and P1
//! (producer-orphan). The integration suites in
//! `tests/{p1,p2,p5}.rs` exercise every code path through hermetic mocks.
//!
//! ## Design seams
//!
//! Per `f-infra-design.md` §3 ("pure logic; no scheduler, no MCP server")
//! and §10 (T-1 single-crate, narrow-lock-friendly), all side effects are
//! abstracted behind three seams:
//!
//! 1. **`&rusqlite::Connection`** — production opens
//!    `data/orchestrator/runs.db`; tests use [`rusqlite::Connection::open_in_memory`]
//!    seeded with the schema embedded in `tests/p5.rs`.
//! 2. **[`GitOps`] trait** — production uses [`RealGitOps`] which shells out
//!    to `git`; tests use [`MockGitOps`] (gated behind the `test-support`
//!    feature) with HashMap-backed canned answers.
//! 3. **[`JCodemunchOps`] trait** — production uses a jcodemunch-MCP-backed
//!    impl supplied by the T-4 CLI (#3672); tests use [`MockJCodemunchOps`]
//!    (gated behind `feature = "test-support"`) with HashMap-backed canned
//!    answers keyed on `(since_sha, until_sha)` for changed-symbol queries
//!    and `(file, name)` for reference queries, enabling per-commit and
//!    file-level disambiguation. Per `f-infra-design.md` §5 P1.
//!
//! All three seams let the integration tests in `tests/{p1,p2,p5}.rs` exercise
//! every code path (happy path + false-positive guards + `check_pre_done`
//! filtering) without a real git repo, a real runs.db, or a real jcodemunch
//! MCP server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod p5_phantom_done;
pub mod p2_consumer_stub;
pub mod p1_producer_orphan;
pub mod fused_memory_client;
pub mod jcodemunch_client;

// -----------------------------------------------------------------------
// Public surface — finding shape
// -----------------------------------------------------------------------

/// Severity ladder for findings emitted by any detector.
///
/// Per task description ("verified phantom-done → high"; the documented
/// false-positive guards "downgrade to low"). `Medium` is reserved for
/// metadata-cleanliness findings such as gitignored entries in
/// `metadata.files` (see
/// `~/.claude/projects/-home-leo-src-reify/memory/project_steward_metadata_files_gitignore_falsepositive.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
}

/// Detector pattern identifier. Each variant identifies one detector pattern;
/// downstream consumers (T-4 CLI report renderer) dispatch on this field alone
/// for severity routing.
///
/// - `P5PhantomDone` — phantom-done: commit provenance cannot be corroborated.
/// - `P2ConsumerStub` — consumer task with stub markers in changed lines.
/// - `P1ProducerOrphan` — producer with no non-test workspace callers.
/// - `P5MetadataFilesGitignored` — metadata-hygiene: gitignored paths in
///   `metadata.files` that should be stripped. Complement to `P5PhantomDone`
///   (medium-severity cleanliness signal, not a phantom-done).
///   See `project_steward_metadata_files_gitignore_falsepositive.md`.
///
/// ## Naming convention
///
/// All variants carry a `P<N>` prefix mapping to the corresponding
/// `f-infra-design.md` §5 invariant. New detector variants must follow the
/// same `P<N><Name>` shape so downstream dispatch (T-4 CLI report renderer)
/// can route on prefix without an out-of-band mapping table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pattern {
    /// P5 — phantom-done: a task marked `status=done` whose claimed
    /// provenance commit cannot be corroborated against runs.db /
    /// `git log main`.
    P5PhantomDone,
    /// P2 — consumer-with-stub: added lines in `metadata.files` contain
    /// canonical stub markers (TODO(pending), unimplemented!, etc.).
    /// See `docs/architecture-audit/f-infra-design.md` §5 P2.
    P2ConsumerStub,
    /// P1 — producer-orphan: a `done` task introduced a public symbol that
    /// has no non-test caller in the workspace and no pending/in-progress
    /// consumer task; flagged Medium past the 14-day grace window, Low
    /// within it. See `docs/architecture-audit/f-infra-design.md` §5 P1.
    P1ProducerOrphan,
    /// Metadata-hygiene: one or more entries in `metadata.files` are
    /// gitignored paths that should be stripped. Distinct from `P5PhantomDone`
    /// (medium-severity cleanliness signal, not a phantom-done).
    P5MetadataFilesGitignored,
    /// P-dead-code — public symbol with no callers above a minimum confidence
    /// threshold, as reported by `mcp__jcodemunch__get_dead_code_v2`.
    /// See `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
    PDeadCode,
    /// P-untested — symbol not reached by any test above a minimum confidence
    /// threshold, as reported by `mcp__jcodemunch__get_untested_symbols`.
    /// See `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
    PUntested,
    /// P-layer-violation — an import that violates the project's layer rules,
    /// as reported by `mcp__jcodemunch__get_layer_violations`.
    /// See `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
    PLayerViolation,
}

/// A pointer to forensic evidence supporting a [`Finding`]. Renders verbatim
/// in the eventual `/audit` report; consumers may follow it back to the
/// underlying source (file, commit, metadata blob, runs.db row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceRef {
    /// Filesystem path relative to `project_root`.
    File { path: String },
    /// A git commit by SHA + first-line subject.
    Commit { sha: String, subject: String },
    /// One or more entries from a task's `metadata.files`.
    MetadataFiles { entries: Vec<String> },
    /// A row in `data/orchestrator/runs.db`. `key` is a free-form locator
    /// (e.g. `"task_id=3242"`) — humans, not parsers, consume this.
    RunsDb { table: String, key: String },
}

/// A single forensic finding emitted by a detector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub pattern: Pattern,
    pub severity: Severity,
    pub task_id: String,
    pub summary: String,
    pub evidence: Vec<EvidenceRef>,
}

// -----------------------------------------------------------------------
// Public surface — input shape
// -----------------------------------------------------------------------

/// Subset of Taskmaster's `tasks.json` schema needed by P5.
///
/// Caller pre-loads this from fused-memory / Taskmaster (T-4 CLI will be the
/// loader). Keeping the library decoupled from fused-memory's wire format
/// makes the API stable and mocking trivial — see
/// `f-infra-design.md` §3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub task_id: String,
    pub status: String,
    pub files: Vec<String>,
    pub done_provenance: Option<DoneProvenance>,
    /// The task's title from Taskmaster `tasks.json`. Used by P2 to downgrade
    /// to `Severity::Low` when the title itself signals a stub/placeholder.
    /// Populated by the T-4 CLI loader; defaulted to descriptive strings in tests.
    pub title: String,
    /// PRD path this task was decomposed from (`/prd`-decomposed tasks carry
    /// it; pre-`/prd` legacy tasks have `None`). P1 correlates a producer's
    /// `prd` against other tasks' `consumer_ref` to suppress orphan findings
    /// when a downstream consumer is queued. Per `f-infra-design.md` §5 P1.
    pub prd: Option<String>,
    /// The producing PRD this task consumes (set on `/prd`-decomposed
    /// consumer tasks). P1's "downstream consumer task exists" guard matches
    /// a pending/in-progress task whose `consumer_ref` equals a producer's
    /// `prd`. `None` for legacy tasks. Per `f-infra-design.md` §5 P1.
    pub consumer_ref: Option<String>,
    /// `true` when the task is a foundation/scaffold task whose symbols are
    /// intentionally not yet consumed (`audit_foundation=true` metadata or a
    /// `## Phase N (foundation)` PRD header). P1 suppresses orphan findings
    /// for such tasks. Per `f-infra-design.md` §5 P1 false-positive guards.
    pub audit_foundation: Option<bool>,
    /// Epoch-seconds timestamp of the task's done-flip. P1's grace-window
    /// math compares `ctx.now - done_at` against the 14-day window. `None`
    /// for non-`done` tasks (P1 skips them). The T-4 CLI converts the ISO
    /// timestamp once at the boundary. Per `f-infra-design.md` §5 P1.
    pub done_at: Option<i64>,
}

/// `metadata.done_provenance` payload as written by reify-orchestrator's
/// resolution path. `kind` is one of `"merged"`, `"found_on_main"`,
/// `"manual"`, etc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoneProvenance {
    pub kind: Option<String>,
    pub commit: Option<String>,
    pub note: Option<String>,
}

/// Optional time window for narrowing detector scope (e.g. "audit only the
/// last N hours"). Reserved for the periodic `/audit` sweep; the D-1
/// pre-done hook path leaves this `None` and lets `target_task_id` do the
/// scoping. Per `f-infra-design.md` §10.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    /// ISO-8601 `since` bound (inclusive). `None` = unbounded.
    pub since: Option<String>,
    /// ISO-8601 `until` bound (exclusive). `None` = unbounded.
    pub until: Option<String>,
}

/// Read-only execution context threaded into each detector's `check(...)`.
///
/// Borrowed from caller (D-1 hook path or periodic `/audit` sweep) so the
/// crate never owns a connection or spawns processes itself.
pub struct AuditContext<'a> {
    pub project_root: PathBuf,
    pub conn: &'a rusqlite::Connection,
    pub git: &'a dyn GitOps,
    /// Source-introspection seam for P1 (changed-symbol / reference queries).
    /// Required and object-safe, mirroring [`git`](Self::git): production
    /// supplies a real jcodemunch-MCP-backed impl; tests use
    /// [`MockJCodemunchOps`]. Per `f-infra-design.md` §3 (pure-logic) and §5
    /// P1 (source-introspection behind a mockable seam).
    pub jcodemunch: &'a dyn JCodemunchOps,
    pub task_metadata: HashMap<String, TaskMetadata>,
    /// When `Some`, the periodic-sweep [`p5_phantom_done::check`] entry point
    /// restricts its work to that single task. Honored by periodic-sweep
    /// callers; intentionally ignored by [`p5_phantom_done::check_pre_done`],
    /// which takes `task_id` as an explicit argument for O(1) HashMap lookup
    /// on the D-1 hot path (setting both would be confusing and the explicit
    /// parameter is unambiguous).
    pub target_task_id: Option<String>,
    /// Reserved for periodic-sweep scoping. None of the slice-1 detector
    /// paths consume this yet — see [`TimeWindow`].
    pub window: Option<TimeWindow>,
    /// Synthetic clock (epoch-seconds) for P1's grace-window math. `None`
    /// falls back to `SystemTime::now()`; tests pass `Some(e)` so grace-window
    /// boundaries are deterministic. Epoch-seconds keeps the crate's dep-set
    /// minimal (no chrono/time) per `f-infra-design.md` §12.
    pub now: Option<i64>,
    /// Reserved for future sweep CLI use (T-4 #3672). P1 no longer reads this
    /// field — it now resolves symbols via `done_provenance.commit` (commit-range
    /// `{commit}^1..{commit}`) rather than a branch+timestamp query. Kept pub
    /// because ~50 construction sites set it to `None`; removing it is outside
    /// the scope of L-TRAIT and deferred to a later cleanup pass.
    pub producer_branch: Option<String>,
}

// -----------------------------------------------------------------------
// GitOps seam
// -----------------------------------------------------------------------

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
    fn log_grep(&self, branch: &str, pattern: &str) -> Vec<GitCommit>;

    /// `git diff --name-only <from>..<to>`. Returns the set of paths
    /// changed between the two refs.
    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String>;

    /// `git check-ignore <path>` — true iff `path` is gitignored
    /// (or matches a negated rule that re-ignores).
    fn is_gitignored(&self, path: &str) -> bool;

    /// Returns `true` iff `path` resolves on `branch` to a tracked file OR a
    /// directory containing tracked files (git does not track empty dirs),
    /// equivalent to `git ls-tree <branch> -- <path>` returning non-empty.
    /// Used by P5's deliverable-presence rescue. Fail-safe: returns `false`
    /// on any git error (missing repo/ref, unknown path).
    fn path_tracked_on(&self, branch: &str, path: &str) -> bool;

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
}

/// Production [`GitOps`] impl that shells out to `git`. Untested by the
/// slice-1 integration suite (see `MockGitOps` for the test seam) — kept
/// minimal so the eventual T-4 CLI can construct one and call
/// [`p5_phantom_done::check_pre_done`].
///
/// # Invariants
///
/// **Construct exactly once per `project_root`.** The private
/// `gitignore_unavailable` field is a per-instance `AtomicBool` that
/// short-circuits all subsequent
/// [`is_gitignored`](GitOps::is_gitignored) calls after the first
/// unrecoverable `git check-ignore` exit, so a task with N files against
/// a broken git repo emits at most one
/// `reify-audit: git check-ignore exited …` breadcrumb rather than N
/// copies of the same line.
///
/// This dedup is silently defeated by constructing a fresh [`RealGitOps`]
/// per task, per file, or per worker: each new instance starts with a
/// cleared flag and re-emits the breadcrumb on its first failing call.
/// The CLI binary (`bin/reify-audit.rs`) constructs exactly one
/// [`RealGitOps`] per invocation and threads it through [`AuditContext`]
/// for every detector; future callers MUST preserve this single-instance
/// discipline.
///
/// The multi-file regression test
/// `cli::git_check_ignore_breadcrumb_dedups_across_files`
/// (`tests/cli.rs`) pins the user-visible signal: with N≥2 files in a
/// non-git directory, exactly one breadcrumb appears in stderr.
pub struct RealGitOps {
    /// Working directory passed as `git -C <dir>` to every invocation.
    pub project_root: PathBuf,
    /// Set to `true` the first time `is_gitignored` encounters an unrecoverable
    /// exit code (anything other than 0 or 1). Subsequent calls short-circuit
    /// and return `false` silently, so a task with N files against a broken git
    /// repo emits at most one breadcrumb rather than N copies of the same line.
    ///
    /// Invariant: per-instance — see [`RealGitOps`] doc for the
    /// single-instance construction requirement that makes this budget
    /// meaningful in production.
    gitignore_unavailable: AtomicBool,
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

impl RealGitOps {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self { project_root: project_root.into(), gitignore_unavailable: AtomicBool::new(false) }
    }

    /// Run a git command and return its stdout as `Ok(String)`, or an error
    /// description as `Err(String)`. Three failure modes:
    ///   1. `Command::output()` failed (spawn error) → Err("git invocation failed: …")
    ///   2. Non-zero exit status → Err("git exited N: <stderr>")
    ///   3. Non-UTF-8 stdout → Err("git output not valid UTF-8")
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.project_root)
            .args(args)
            .output()
            .map_err(|e| format!("git invocation failed: {}", e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!(
                "git exited {:?}: {}",
                out.status.code(),
                stderr.trim()
            ));
        }
        String::from_utf8(out.stdout).map_err(|_| "git output not valid UTF-8".to_string())
    }

    /// Run a git command, emitting a `reify-audit:` breadcrumb on failure and
    /// returning `None` so callers can `else { return vec![]; }` in one line.
    /// `label` is the human-readable git subcommand used in the breadcrumb
    /// (e.g. `"log --grep"`, `"diff --name-only"`, `"diff"`).
    fn run_or_warn(&self, label: &str, args: &[&str]) -> Option<String> {
        match self.run(args) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!(
                    "reify-audit: git {} failed in {}: {}",
                    label,
                    self.project_root.display(),
                    e
                );
                None
            }
        }
    }
}

impl GitOps for RealGitOps {
    fn log_grep(&self, branch: &str, pattern: &str) -> Vec<GitCommit> {
        let Some(stdout) = self.run_or_warn("log --grep", &[
            "log",
            branch,
            &format!("--grep={}", pattern),
            &format!("--format={}", LOG_GREP_FORMAT),
        ]) else {
            return vec![];
        };
        stdout
            .lines()
            .filter_map(|l| {
                let mut parts = l.splitn(2, '\t');
                let sha = parts.next()?.to_string();
                let subject = parts.next().unwrap_or("").to_string();
                Some(GitCommit { sha, subject })
            })
            .collect()
    }

    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        let Some(stdout) = self.run_or_warn(
            "diff --name-only",
            &["diff", "--name-only", &format!("{}..{}", from, to)],
        ) else {
            return vec![];
        };
        stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    fn is_gitignored(&self, path: &str) -> bool {
        // `git check-ignore` exit code 0 = ignored, 1 = not ignored.
        // Any other outcome (spawn error, exit code other than 0/1) is a git
        // failure — log a breadcrumb and default to false.
        //
        // Use `.output()` (not `.status()`) to capture git's own stderr so
        // that "fatal: not a git repository" and similar diagnostics do not
        // leak to *our* process's stderr and corrupt the machine-readable
        // JSON output written there by the CLI dispatcher.
        //
        // Once an unrecoverable exit code is observed, `gitignore_unavailable`
        // is set so that subsequent calls for this project_root short-circuit
        // without forking git again — a task with N files in a broken repo
        // emits at most one breadcrumb rather than N identical lines.
        if self.gitignore_unavailable.load(Ordering::Relaxed) {
            return false;
        }
        match std::process::Command::new("git")
            .arg("-C")
            .arg(&self.project_root)
            .args(["check-ignore", "--quiet", path])
            .output()
        {
            Ok(out) if out.status.code() == Some(0) => true,
            Ok(out) if out.status.code() == Some(1) => false,
            Ok(out) => {
                self.gitignore_unavailable.store(true, Ordering::Relaxed);
                eprintln!(
                    "reify-audit: git check-ignore exited {:?} in {}",
                    out.status.code(),
                    self.project_root.display()
                );
                false
            }
            Err(e) => {
                self.gitignore_unavailable.store(true, Ordering::Relaxed);
                eprintln!(
                    "reify-audit: git check-ignore failed in {}: {}",
                    self.project_root.display(),
                    e
                );
                false
            }
        }
    }

    fn path_tracked_on(&self, branch: &str, path: &str) -> bool {
        match self.run_or_warn("ls-tree", &["ls-tree", branch, "--", path]) {
            Some(stdout) => !stdout.trim().is_empty(),
            None => false,
        }
    }

    fn is_ancestor(&self, commit: &str, branch: &str) -> bool {
        // Use .output() (not .status()) so git's stderr ("fatal: not a git
        // repository", "fatal: Not a valid commit name", etc.) is captured and
        // does not leak to our process's stderr / corrupt JSON output.
        // exit 0 = ancestor; exit 1 = not an ancestor; exit 128 = bad object
        // or not-a-repo — all non-zero cases correctly map to false (fail-safe).
        match std::process::Command::new("git")
            .arg("-C")
            .arg(&self.project_root)
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
    is_gitignored: HashMap<String, bool>,
    diff_added_lines: HashMap<(String, String, String), Vec<(usize, String)>>,
    diff_added_lines_in_commit: HashMap<(String, String), Vec<(usize, String)>>,
    file_lines_on: HashMap<(String, String), Vec<(usize, String)>>,
    path_tracked_on: HashMap<(String, String), bool>,
    is_ancestor: HashMap<(String, String), bool>,
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
    pub fn set_file_lines_on(
        &mut self,
        reference: &str,
        path: &str,
        lines: Vec<(usize, String)>,
    ) {
        self.file_lines_on
            .insert((reference.to_string(), path.to_string()), lines);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl GitOps for MockGitOps {
    fn log_grep(&self, branch: &str, pattern: &str) -> Vec<GitCommit> {
        self.log_grep
            .get(&(branch.to_string(), pattern.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn diff_changed_paths(&self, from: &str, to: &str) -> Vec<String> {
        self.diff_changed_paths
            .get(&(from.to_string(), to.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn is_gitignored(&self, path: &str) -> bool {
        self.is_gitignored.get(path).copied().unwrap_or(false)
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

    fn path_tracked_on(&self, branch: &str, path: &str) -> bool {
        self.path_tracked_on
            .get(&(branch.to_string(), path.to_string()))
            .copied()
            .unwrap_or(false)
    }

    fn is_ancestor(&self, commit: &str, branch: &str) -> bool {
        self.is_ancestor
            .get(&(commit.to_string(), branch.to_string()))
            .copied()
            .unwrap_or(false)
    }
}

// -----------------------------------------------------------------------
// JCodemunchOps seam (P1)
// -----------------------------------------------------------------------

/// A public symbol introduced (or changed) by a `done` task, as reported by
/// `mcp__jcodemunch__get_changed_symbols`. Carries pre-extracted suppression
/// metadata so the detector stays pure-logic (it never reads source files —
/// symmetric with how [`GitOps::diff_added_lines`] pre-extracts strings).
/// Per `f-infra-design.md` §3 and §5 P1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSymbol {
    /// The symbol's name, used as the key for [`JCodemunchOps::find_references`].
    pub name: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// 1-based line of the declaration (forensic evidence locator).
    pub line: usize,
    /// `true` when the declaration carries `#[allow(dead_code)]` — an
    /// intentional-orphan opt-out (suppresses the finding). Per
    /// `f-infra-design.md` §5 P1.
    pub has_allow_dead_code: bool,
    /// `true` when the declaration is `#[cfg(test)]`-gated (test-only symbol;
    /// suppresses the finding). Per `f-infra-design.md` §5 P1.
    pub has_cfg_test: bool,
    /// The reason text of a `// G-allow:` marker on the declaration, if any.
    /// A `Some` with non-blank content suppresses the finding; `Some("")` /
    /// whitespace does NOT (mirrors `scripts/audit-orphan-producers.sh:150`
    /// `G_ALLOW_RE = //\s*G-allow:\s*(.+)` where `(.+)` requires content).
    pub g_allow_marker: Option<String>,
}

/// A non-declaration reference (caller site) of a symbol, as reported by
/// `mcp__jcodemunch__find_references`. P1 filters these to non-test paths to
/// decide whether a workspace consumer exists. Per `f-infra-design.md` §5 P1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolReference {
    /// Workspace-relative path of the referencing file.
    pub file: String,
    /// 1-based line of the reference.
    pub line: usize,
}

/// A symbol with no callers above a given confidence, as reported by
/// `mcp__jcodemunch__get_dead_code_v2`. Mirrors the jcodemunch tool's response
/// shape for use by the L-PDEAD detector leaf.
///
/// Note: `confidence` is `f64` (IEEE 754), so `Eq` and `Hash` are intentionally
/// NOT derived — floating-point equality semantics are unsuitable for collection
/// key use. This deviates from [`ChangedSymbol`]'s derive set by design.
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeadSymbol {
    /// Unique identifier for the symbol as assigned by jcodemunch.
    pub id: String,
    /// The symbol's declared name.
    pub name: String,
    /// The kind of symbol (e.g. `"function"`, `"struct"`, `"const"`).
    pub kind: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// 1-based line of the declaration.
    pub line: usize,
    /// Jcodemunch's confidence score that the symbol is truly unreachable
    /// (0.0 = uncertain; 1.0 = certain). Filtered by `min_confidence` in
    /// [`JCodemunchOps::get_dead_code`].
    pub confidence: f64,
    /// Diagnostic signals contributing to the confidence score
    /// (e.g. `"no_callers"`, `"private_module"`).
    pub signals: Vec<String>,
}

/// A symbol not reached by any test, as reported by
/// `mcp__jcodemunch__get_untested_symbols`. Mirrors the jcodemunch tool's
/// response shape for use by the L-PUNTESTED detector leaf.
///
/// Note: `confidence` is `f64` — `Eq`/`Hash` not derived. See [`DeadSymbol`].
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UntestedSymbol {
    /// Unique identifier for the symbol as assigned by jcodemunch.
    pub symbol_id: String,
    /// The symbol's declared name.
    pub name: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// `false` when no test path reaches the symbol.
    pub reached: bool,
    /// Confidence score (0.0–1.0) that the symbol is genuinely untested.
    /// Filtered by `min_confidence` in [`JCodemunchOps::get_untested_symbols`].
    pub confidence: f64,
}

/// A layer-violation: an import that is forbidden by the project's layering
/// rules, as reported by `mcp__jcodemunch__get_layer_violations`. Mirrors the
/// jcodemunch tool's response shape for use by the L-PLAYER detector leaf.
///
/// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerViolation {
    /// Workspace-relative path of the file containing the violating import.
    pub from_file: String,
    /// Workspace-relative path of the file (or crate root) being imported.
    pub to_file: String,
    /// Human-readable name of the layer rule that was violated
    /// (e.g. `"gui-must-not-depend-on-kernel"`).
    pub rule: String,
}

/// Source-introspection operations the P1 detector needs. Production: a
/// jcodemunch-MCP-backed impl supplied by the T-4 CLI. Tests:
/// [`MockJCodemunchOps`] (gated behind `feature = "test-support"`) holds
/// canned answers keyed on `(since_sha, until_sha)` for changed-symbol
/// queries and `(file, name)` for reference queries, enabling per-commit and
/// file-level disambiguation. Per `f-infra-design.md` §3 and §5 P1.
///
/// Object-safe by design — `AuditContext` holds `&'a dyn JCodemunchOps` so
/// the production and mock impls coexist behind the same vtable (mirrors
/// [`GitOps`]).
pub trait JCodemunchOps {
    /// Equivalent of `mcp__jcodemunch__get_changed_symbols(since_sha, until_sha)`
    /// (jcodemunch v1.108.27+): the public symbols introduced/changed in the
    /// commit range `since_sha..until_sha`. Typically `since_sha = "{commit}^1"`
    /// and `until_sha = "{commit}"` for a single merged commit (mirrors the
    /// `^1..commit` convention from `RealGitOps::diff_added_lines_in_commit`).
    /// Returns an empty vec when the range is empty or the commits are not found.
    fn get_changed_symbols(&self, since_sha: &str, until_sha: &str) -> Vec<ChangedSymbol>;

    /// Equivalent of `mcp__jcodemunch__find_references(symbol)`: every
    /// non-declaration reference of the symbol across the workspace, scoped
    /// to the symbol's declaring file so that two same-named symbols in
    /// different files are not conflated. Production impls MUST scope the
    /// lookup to `symbol.file` (e.g. pass the file path to jcodemunch-MCP
    /// for module-level disambiguation); tests key on `(file, name)`.
    /// Returns an empty vec when the symbol has no callers (an orphan
    /// candidate). Per `f-infra-design.md` §5 P1.
    fn find_references(&self, symbol: &ChangedSymbol) -> Vec<SymbolReference>;

    /// Equivalent of `mcp__jcodemunch__get_dead_code_v2(min_confidence)`:
    /// public symbols with no callers whose confidence score meets or exceeds
    /// `min_confidence` (0.0–1.0). Returns an empty vec when none found.
    /// Per `docs/prds/reify-audit-p1-jcodemunch-substrate.md` §4-b.
    fn get_dead_code(&self, min_confidence: f64) -> Vec<DeadSymbol>;

    /// Equivalent of `mcp__jcodemunch__get_untested_symbols(min_confidence)`:
    /// symbols not reached by any test whose confidence score meets or exceeds
    /// `min_confidence`. Returns an empty vec when none found. Per PRD §4-b.
    fn get_untested_symbols(&self, min_confidence: f64) -> Vec<UntestedSymbol>;

    /// Equivalent of `mcp__jcodemunch__get_layer_violations()`: all detected
    /// imports that violate the project's layering rules. Returns an empty vec
    /// when none found. Per PRD §4-b.
    fn get_layer_violations(&self) -> Vec<LayerViolation>;
}

/// HashMap-backed [`JCodemunchOps`] for tests. Gated behind
/// `feature = "test-support"` so it never pollutes the production public API
/// (mirrors [`MockGitOps`]). The crate self-pulls this feature in its own
/// `[dev-dependencies]` so integration tests in `tests/p1.rs` see it.
///
/// Changed-symbol queries are keyed on `(since_sha, until_sha)` (the
/// commit-range surface per jcodemunch v1.108.27+); reference queries are
/// keyed on `(file, name)` for file-level disambiguation.
/// Dead-code / untested-symbol data is stored as a flat Vec and filtered by
/// `min_confidence` at query time (mirrors the real tool's semantics).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct MockJCodemunchOps {
    get_changed_symbols: HashMap<(String, String), Vec<ChangedSymbol>>,
    find_references: HashMap<(String, String), Vec<SymbolReference>>,
    dead_code: Vec<DeadSymbol>,
    untested: Vec<UntestedSymbol>,
    layer_violations: Vec<LayerViolation>,
}

#[cfg(any(test, feature = "test-support"))]
impl MockJCodemunchOps {
    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn new() -> Self {
        Self::default()
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_changed_symbols(
        &mut self,
        since_sha: &str,
        until_sha: &str,
        symbols: Vec<ChangedSymbol>,
    ) {
        self.get_changed_symbols
            .insert((since_sha.to_string(), until_sha.to_string()), symbols);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_find_references(&mut self, file: &str, name: &str, refs: Vec<SymbolReference>) {
        self.find_references.insert((file.to_string(), name.to_string()), refs);
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_dead_code(&mut self, symbols: Vec<DeadSymbol>) {
        self.dead_code = symbols;
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_untested_symbols(&mut self, symbols: Vec<UntestedSymbol>) {
        self.untested = symbols;
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn set_layer_violations(&mut self, violations: Vec<LayerViolation>) {
        self.layer_violations = violations;
    }
}

#[cfg(any(test, feature = "test-support"))]
impl JCodemunchOps for MockJCodemunchOps {
    fn get_changed_symbols(&self, since_sha: &str, until_sha: &str) -> Vec<ChangedSymbol> {
        self.get_changed_symbols
            .get(&(since_sha.to_string(), until_sha.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    fn find_references(&self, symbol: &ChangedSymbol) -> Vec<SymbolReference> {
        self.find_references
            .get(&(symbol.file.clone(), symbol.name.clone()))
            .cloned()
            .unwrap_or_default()
    }

    fn get_dead_code(&self, min_confidence: f64) -> Vec<DeadSymbol> {
        self.dead_code
            .iter()
            .filter(|s| s.confidence >= min_confidence)
            .cloned()
            .collect()
    }

    fn get_untested_symbols(&self, min_confidence: f64) -> Vec<UntestedSymbol> {
        self.untested
            .iter()
            .filter(|s| s.confidence >= min_confidence)
            .cloned()
            .collect()
    }

    fn get_layer_violations(&self) -> Vec<LayerViolation> {
        self.layer_violations.clone()
    }
}

// -----------------------------------------------------------------------
// Shared path predicate
// -----------------------------------------------------------------------

/// Returns `true` when the path looks like a test file.
///
/// The crate's *single* canonical test-path predicate. A non-test caller of
/// a `done`-task symbol proves the symbol is genuinely consumed (P1), and
/// test-shaped paths are skipped when scanning for stub markers (P2).
/// Defining it once here makes every detector's test-path semantics
/// compiler-guaranteed identical instead of relying on a hand-synced copy
/// (the prior P1/P2 duplication could silently diverge under a one-sided
/// edit). Private to the crate root, so all detector submodules reach it via
/// `crate::is_test_path`.
fn is_test_path(p: &str) -> bool {
    // `tests/` with and without a leading slash covers both repo-root paths
    // (e.g. `tests/foo.rs`) and nested paths (e.g. `crates/x/tests/foo.rs`).
    p.starts_with("tests/")
        || p.contains("/tests/")
        || p.ends_with("_test.rs")
        || p.contains("__tests__/")
        || p.contains(".test.")  // JS/TS: foo.test.ts
        || p.contains(".spec.")  // JS/TS: foo.spec.ts
}
