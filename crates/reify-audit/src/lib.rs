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
#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::AtomicUsize;

pub mod git_env;
pub mod p5_phantom_done;
pub mod p2_consumer_stub;
pub mod p1_producer_orphan;
pub mod pdead_dead_code;
pub mod puntested;
pub mod player;
pub mod ptodo;
pub mod pdssentinel;
pub mod pdiag;
pub mod pdoccover;
pub mod pdcheck;
/// Crate-internal: shared scaffolding for the lanes that read the task DB.
/// Not part of the detector API surface — the lanes are.
pub(crate) mod task_rows;
/// Crate-internal: shared text-scanning primitives for the structural
/// detectors. Not part of the detector API surface — the detectors are.
pub(crate) mod scan_util;
pub mod fused_memory_client;
pub mod jcodemunch_client;
pub mod jcodemunch_index;

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
    /// P5 — tests-assert-empty: a `done` task whose added test function BOTH
    /// (a) carries a placeholder/empty/not_yet/notyet/stub/todo/unimplemented
    /// marker in its fn name AND (b) asserts an empty/vacuous result
    /// (`is_empty()`, `vec![]`, `Vec::new()`, `assert_eq!(.., 0)`,
    /// `assert_eq!(.., [])`). The double-gate suppresses legitimately-empty
    /// capability tests while flagging placeholder tests that mask a missing
    /// implementation. See task 4140 / esc-4137-196 and
    /// `docs/architecture-audit/f-infra-design.md` §5 P5.
    P5TestsAssertEmpty,
    /// P5 — live-path-stranded: a `done` task whose changed capability symbol
    /// has no non-test workspace caller (stranded by a live-path relocation).
    /// Requires cross-crate scope (metadata.files span ≥2 distinct
    /// `crates/<name>/` roots) to scope to the documented cross-crate relocation
    /// pattern and avoid duplicating P1's single-crate orphan domain.
    /// Reuses P1's per-symbol suppression guards (stdlib scope-exclude,
    /// `#[allow(dead_code)]`, `#[cfg(test)]`, non-blank `// G-allow:`).
    /// See task 4140 / esc-4137-196 and
    /// `docs/architecture-audit/f-infra-design.md` §5 P5.
    P5LivePathStranded,
    /// PTODO — TODO-tracking-invariant: a TODO-family marker that is not backed
    /// by a *live* canonical `#NNNN` task citation. The §8.3 finding `kind` is
    /// carried as a stable summary prefix rather than a per-kind variant. Three
    /// lanes emit under this one variant, all Medium severity:
    /// - **Structural lane (task α)** — a marker with no canonical cite at all:
    ///   `untracked` / `malformed-cite` / `phantom-tracking` / `bare-ignore`.
    /// - **Liveness lane (task β)** — a canonical cite resolved against
    ///   `.taskmaster/tasks/tasks.db`: `orphaned` (cited status is terminal —
    ///   done / cancelled — summary carries the id + status) or `unknown-id`
    ///   (the cite parses but the id is absent from the DB).
    /// - **Inverse lane (task ζ)** — for each non-terminal task in the master
    ///   task DB, each `metadata.files` path absent from the tracked-file set
    ///   is checked for git history. If history exists, the last-touching
    ///   commit decides between two kinds: it RENAMED the path to a target
    ///   still tracked at HEAD → `task-cites-renamed-path` (summary carries
    ///   the task id, both paths, and the commit sha); otherwise the path was
    ///   deleted → `task-cites-deleted-path` (summary carries the task id,
    ///   path, and last commit sha). Paths that never existed (presumed
    ///   to-be-created) pass.
    ///
    /// The liveness and inverse lanes degrade fail-soft together (§6.7): when
    /// the task DB is missing or unreadable both are skipped with a single stderr
    /// breadcrumb and the structural lane still runs in full (exit class unchanged).
    /// See `docs/prds/reify-audit-ptodo-detector.md` §8 (grammar) / §6.3
    /// (inverse lane) / §6.7 (degradation).
    ///
    /// As of task ε (#4557) this pattern participates in the no-`--pattern`
    /// default sweep at Medium severity (exit-neutral: exit code = High count).
    PTodo,
    /// PDSSENTINEL — ds-sentinel reintroduction guard: a `dimensionless_scalar()`
    /// call that follows, within a bounded backward line window, a
    /// `diagnostics.push(Diagnostic::error(... UnresolvedType ...))` push in
    /// the scoped compiler source files, and is NOT marked with a
    /// `// ds-sentinel:allow <reason>` escape comment.
    ///
    /// Advisory / Medium severity — the same posture as PTODO/malformed-cite.
    /// Joins the no-`--pattern` default `/audit` sweep via `is_none_or`
    /// (mirroring `run_ptodo`). Structural: reads the working tree via
    /// `ls_files()` + `std::fs`, never contacts jcodemunch.
    ///
    /// Scope: `crates/reify-compiler/src/{entity,functions,traits,expr}.rs` and
    /// `crates/reify-compiler/src/conformance/*.rs` (PRD §8 scope).
    ///
    /// Reference: `docs/prds/dimensionless-scalar-sentinel-stampout.md` §8/§10.
    PDsSentinel,
    /// PDIAG — codes-mandatory ratchet (`INV-SF-6 diagnostics-carry-codes`):
    /// a `Diagnostic::error(...)` / `Diagnostic::warning(...)` construction
    /// site in scoped Rust source with no `.with_code(...)` attached within a
    /// bounded forward line window, and not marked with a `// pdiag:allow —
    /// reason` escape. The escape is forward-scoped and bounded by the next
    /// constructor as well as by the window, so one escape covers exactly one
    /// site and can never reach backwards over the site above it. Per-file
    /// counts ratchet against the committed
    /// `crates/reify-audit/pdiag-baseline.txt` manifest.
    ///
    /// **High** severity for a count that exceeds its baseline row (or a file
    /// with sites and no row) — unlike PTODO/PDSSENTINEL this pattern DOES
    /// move the process exit code, which is the hard gate PRD §8 boundary
    /// row 8 requires. Under-count and orphan-row advisories are Medium and
    /// exit-neutral, so an opportunistic fix never turns a diff RED. OPT-IN
    /// via `is_some_and` (mirroring `run_pdead`), NOT a member of the
    /// no-`--pattern` default sweep: because its verdicts move the exit code,
    /// joining that sweep would make every consumer which omits `--pattern`
    /// go RED the moment this ratchet drifted. Structural: reads the working
    /// tree via `ls_files()` + `std::fs`, never contacts jcodemunch or the
    /// task DB.
    ///
    /// Scope: `crates/<name>/src/**.rs` + `gui/src-tauri/src/**.rs`, minus the
    /// detector's own crate, `reify-test-support`, `tests/`-segment paths and
    /// `#[cfg(test)]` bodies.
    ///
    /// Reference: `docs/prds/v0_6/eradicate-silent-undef.md` §3 Leg C / §7;
    /// remediation: `docs/notes/diagnostic-severity-policy.md` §3.
    PDiag,
    /// PDOCCOVER — bidirectional registry↔chunk name drift between the
    /// compiler's builtin-name registries and the MCP language-reference
    /// chunks (`crates/reify-mcp/src/tools/chunks/*.md`). ONE detector, two
    /// directions, five finding categories carried as a stable summary prefix
    /// (PTODO's `kind`-as-prefix convention above), all at
    /// [`Severity::High`]:
    ///
    /// - **Omission lane** — a `*_NAMES` registry entry in
    ///   `crates/reify-compiler/src/units.rs` that is not documented in any
    ///   chunk, not marked `// pdoccover:allow — <reason>`, and not listed in
    ///   `crates/reify-audit/pdoccover-baseline.txt` → `undocumented-name:`.
    ///   Ratchet-honesty siblings: `stale-baseline-entry:` (a baselined name
    ///   that IS documented) and `stale-allow-entry:` (an allow-marked name
    ///   that IS documented).
    /// - **Fabrication lane** — a call-shaped name documented in a chunk that
    ///   exists nowhere in the compiler/stdlib sources → `fabricated-name:`.
    /// - Both lanes share `allow-missing-reason:` — a `pdoccover:allow` token
    ///   with a blank reason body confers NO exemption and is itself a finding.
    ///
    /// **Opt-in only** (`is_some_and`, mirroring PDEAD/PUNTESTED/PLAYER): the
    /// census is non-empty until #5480 seeds the baseline, and the CLI exit
    /// code is the High-severity count, so joining the no-`--pattern` default
    /// sweep would drown every other detector. Structural: reads the working
    /// tree via `ls_files()` + `std::fs`, never contacts jcodemunch.
    ///
    /// Reference: `docs/prds/v0_6/doc-chunk-truth-enforcement.md` §(b) / leaf γ.
    PDocCover,
    /// PDCHECK — `delivered_checks` dead-path lane: a non-terminal task's
    /// `kind: grep` capability-check row whose pathspec no longer resolves
    /// against the tracked-file set. TWO finding kinds, carried as a stable
    /// summary prefix (PTODO's `kind`-as-prefix convention above) and split on
    /// the row's `expect` polarity, because both readings of the runner's rc=1
    /// on an empty pathspec are silent but they are opposite defects:
    ///
    /// - `delivered-check-unsatisfiable-path` (**High**) — `expect: present`
    ///   and every path in the row absent. rc=1 reads as FAILED, so every
    ///   dependent blocks forever at `DEP_CAPABILITY_NOT_DELIVERED`.
    /// - `delivered-check-vacuous-absent-path` (**Medium**) — `expect: absent`
    ///   and every path absent. The identical rc=1 reads as PASSED, so the
    ///   check succeeds while asserting nothing.
    ///
    /// Quantified over the WHOLE row: a multi-`paths` row runs as ONE
    /// `git grep -E -e <pattern> <ref> -- <paths...>`, an ANY-match, so one
    /// dead path among live ones leaves the row satisfiable and yields no
    /// finding. Rename-vs-delete changes only the repair hint and is carried as
    /// [`EvidenceRef`], not as a third and fourth kind.
    ///
    /// **Opt-in only** (`is_some_and`, mirroring PDIAG/PDOCCOVER): the High
    /// kind moves the process exit code, which is the High-severity count.
    /// Reads `ls_files()` plus a read-only `.taskmaster/tasks/tasks.db`; never
    /// contacts jcodemunch.
    ///
    /// Reference: `docs/architecture-audit/f-infra-design.md` §5.
    PDeliveredCheckPath,
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
    /// One row of a task's `metadata.delivered_checks`, located by its `name`
    /// — the handle a fixer needs to find the row — plus the `paths` pathspec
    /// the row asserts over.
    ///
    /// Deliberately NOT [`EvidenceRef::MetadataFiles`], whose doc above pins
    /// its meaning to "entries from a task's `metadata.files`": a
    /// delivered_check row is a different thing with a different repair, and
    /// collapsing the two would make the fixer guess which they were handed.
    DeliveredCheck { check_name: String, paths: Vec<String> },
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

impl AuditContext<'_> {
    /// Contents of tracked file `path` (root-relative), or `None` when it
    /// cannot be read.
    ///
    /// The tracked-file read for PTODO, PDSSENTINEL and PDOCCOVER, which each
    /// inlined it separately before task #6036. Only ENUMERATION is a git
    /// seam — those detectors take path membership from `git.ls_files()` and
    /// then read the working tree directly through here, so a path that is
    /// tracked but absent, unreadable, a directory, or not valid UTF-8 is
    /// SKIPPED fail-safe: no finding, no panic. That matters because the
    /// callers are scanners run over the whole repo, where one unreadable
    /// file must not be able to take the detector — or the verify gate it
    /// runs in — down.
    ///
    /// Three of the crate's five such reads, not all five: `pdiag.rs` still
    /// hand-rolls this same `read_to_string(project_root.join(..))` twice, in
    /// its census sweep and in its baseline read. Both are behaviour-identical
    /// to this method and belong here; they sit outside the lock set of the
    /// hoist that created it, so converging them (and refreshing the two
    /// comments there that still cross-reference `ptodo.rs::check`'s
    /// since-removed `read_to_string` arm) is follow-up work rather than a
    /// second contract. A reader auditing fail-safe posture must look there
    /// too until then.
    ///
    /// `pub(crate)` because every caller is in-crate; a refactor is no reason
    /// to widen the crate's public API.
    pub(crate) fn read_relative(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.project_root.join(path)).ok()
    }
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
/// [`p5_phantom_done::check_pre_done`].
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

impl RealGitOps {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            gitignore_unavailable: AtomicBool::new(false),
            #[cfg(any(test, feature = "test-support"))]
            inject_spawn_failures: AtomicUsize::new(0),
        }
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
        String::from_utf8(out.stdout).map_err(|_| "git output not valid UTF-8".to_string())
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

// -----------------------------------------------------------------------
// JCodemunchOps seam (P1)
// -----------------------------------------------------------------------

/// The opt-outs a declaration carries, as READ by the suppression-enrichment
/// pass (`jcodemunch_client`'s `extract_suppression`). Reaches a detector only
/// inside [`ChangedSymbol::suppression`], whose `Option` carries the separate
/// question of whether the declaration was located at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclSuppression {
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

impl DeclSuppression {
    /// `true` when the declaration carries ANY of the opt-outs above — the
    /// single home of the rule P1 and P5 H2 both apply, so the two cannot
    /// drift. Per `f-infra-design.md` §5 P1/P5.
    // G-allow: single home of the opt-out rule; callers are intra-crate (is_symbol_suppressed) — orphan-audit script counts only inter-crate call sites
    pub fn opts_out(&self) -> bool {
        self.has_allow_dead_code
            || self.has_cfg_test
            || self
                .g_allow_marker
                .as_deref()
                .is_some_and(|r| !r.trim().is_empty())
    }
}

/// A public symbol introduced (or changed) by a `done` task, as reported by
/// `mcp__jcodemunch__get_changed_symbols`. Carries pre-extracted suppression
/// metadata so the detector stays pure-logic (it never reads source files —
/// symmetric with how [`GitOps::diff_added_lines`] pre-extracts strings).
/// Per `f-infra-design.md` §3 and §5 P1.
///
/// Suppression is THREE-state, and [`decl_located`](Self::decl_located) is the
/// state a consumer branches on FIRST: `None` means the declaration was never
/// located, so no opt-out judgement was possible; a default
/// [`DeclSuppression`] means it was read and carries none; one with a flag set
/// means it was read and opted out. All three causes of `None` — the wire
/// reported no `line`, the line is past the declaring file's current end
/// (a stale index), or the declaring file could not be read — are carried
/// here per symbol, not merely summarised to the operator on stderr by the
/// enrichment pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedSymbol {
    /// The symbol's name, used as the key for [`JCodemunchOps::find_references`].
    pub name: String,
    /// Workspace-relative path of the file declaring the symbol.
    pub file: String,
    /// 1-based line of the declaration (forensic evidence locator) WHEN
    /// the wire reports one. `0` is the sentinel for "not reported",
    /// mirroring [`SymbolReference::line`]: a `get_changed_symbols` payload
    /// that omits the `line` column still yields the symbol, located at
    /// `0`, rather than dropping it. The sentinel is for forensic display
    /// only — it covers just one of the three ways a declaration goes
    /// unlocatable, so a consumer asks
    /// [`decl_located`](Self::decl_located), never `line == 0`.
    pub line: usize,
    /// The opt-outs this symbol's declaration carries, or `None` when the
    /// declaration was never located — see the three-state note on the struct.
    pub suppression: Option<DeclSuppression>,
}

impl ChangedSymbol {
    /// `true` when the suppression-enrichment pass actually read this symbol's
    /// declaration, so [`suppression`](Self::suppression) is a judgement about
    /// the source rather than an absence of one.
    ///
    /// `false` reports a degradation of the jcodemunch substrate, never a
    /// statement about the code: the wire reported no `line` (the grammar
    /// drift release 1.108.54 already shipped for `find_references`), the
    /// reported line is past the declaring file's current end (a stale index),
    /// or the declaring file could not be read. A consumer that skips this
    /// question and reads `None` as "the author declined every opt-out" turns
    /// any of the three into a false positive over every intentionally
    /// suppressed symbol.
    // G-allow: accessor on the public ChangedSymbol API; every caller is intra-crate or a test — orphan-audit script counts only inter-crate call sites
    pub fn decl_located(&self) -> bool {
        self.suppression.is_some()
    }

    /// `true` when this symbol's declaration was located AND carries one of
    /// [`DeclSuppression`]'s opt-outs — the `Option`-lifting of
    /// [`DeclSuppression::opts_out`] onto a symbol, so no detector rewrites
    /// it inline.
    ///
    /// An unlocatable declaration answers `false`: it made no opt-out claim
    /// either way. That is NOT permission to report the symbol — ask
    /// [`decl_located`](Self::decl_located) first; this answers only "did
    /// the author opt out".
    // G-allow: accessor on the public ChangedSymbol API; every caller is intra-crate or a test — orphan-audit script counts only inter-crate call sites
    pub fn opts_out(&self) -> bool {
        self.suppression
            .as_ref()
            .is_some_and(DeclSuppression::opts_out)
    }
}

/// A non-declaration reference (caller site) of a symbol, as reported by
/// `mcp__jcodemunch__find_references`. P1 filters these to non-test paths to
/// decide whether a workspace consumer exists. Per `f-infra-design.md` §5 P1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolReference {
    /// Workspace-relative path of the referencing file.
    pub file: String,
    /// 1-based line of the reference WHEN the wire reports one. `0` is the
    /// sentinel for "not reported": jcodemunch's `find_references` records
    /// carry only `file`/`specifier`/`match_type`, no line number.
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
    /// 1-based line of the declaration WHEN the wire reports one. `0` is the
    /// sentinel for "not reported", mirroring [`ChangedSymbol::line`] and
    /// [`SymbolReference::line`]: a `get_dead_code_v2` payload that omits the
    /// `line` column still yields the symbol, located at `0`, rather than
    /// dropping it from the PDEAD sweep.
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

/// Inert [`JCodemunchOps`] — every query answers "nothing".
///
/// Unlike [`MockJCodemunchOps`] this is NOT test-support: it is the production
/// binding whenever a run does not need the jcodemunch seam at all, and it is
/// ungated for exactly that reason. Three call sites, all of them real:
///
/// 1. `--no-jcodemunch` — the explicit offline escape hatch: P1 runs and
///    produces zero findings without opening a socket.
/// 2. Detector runs that never touch the seam (`needs_jcodemunch() == false`):
///    P5/pre-done, P2-only, and the purely structural lanes (PTODO, PDIAG).
/// 3. `pdiag-baseline-gen`, a structural census that still has to populate
///    [`AuditContext`]'s field.
///
/// Lives here rather than in each bin because it was copy-pasted into three of
/// them, so every future change to the trait had to be replayed by hand in
/// three places — a silent drift hazard with no compiler backstop until one
/// copy stopped building. Two of the three now bind this one.
///
/// The third, `src/bin/ptodo-baseline-gen.rs`, still carries a private copy
/// that re-opens that hazard in the one bin that still has it — a residual
/// defect, not a design choice, tracked as #7132.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(&self, _since_sha: &str, _until_sha: &str) -> Vec<ChangedSymbol> {
        vec![]
    }
    fn find_references(&self, _symbol: &ChangedSymbol) -> Vec<SymbolReference> {
        vec![]
    }
    fn get_dead_code(&self, _min_confidence: f64) -> Vec<DeadSymbol> {
        vec![]
    }
    fn get_untested_symbols(&self, _min_confidence: f64) -> Vec<UntestedSymbol> {
        vec![]
    }
    fn get_layer_violations(&self) -> Vec<LayerViolation> {
        vec![]
    }
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
    // Records the min_confidence last passed to get_dead_code() so tests can
    // assert the detector passes the intended threshold to the seam.
    last_dead_code_min_confidence: std::cell::Cell<Option<f64>>,
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
    pub fn last_dead_code_min_confidence(&self) -> Option<f64> {
        self.last_dead_code_min_confidence.get()
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
        self.last_dead_code_min_confidence.set(Some(min_confidence));
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

/// Combined OPT-OUT predicate for P5 H2 (`check_live_path_stranded`): the
/// author's own opt-out, plus the `crates/reify-stdlib/` scope-exclude (every
/// `.ri` structure def is technically orphan until something calls it).
///
/// Answers "did the author opt out", NOT "should this be reported". A symbol
/// whose declaration was never located answers `false`, because no opt-out
/// was ever observed; whether it is reportable is the SEPARATE question
/// [`ChangedSymbol::decl_located`] asks, and folding the two into one
/// predicate is what made an unlocatable declaration indistinguishable from a
/// clean one.
///
/// P1 (`p1_producer_orphan`) calls [`ChangedSymbol::opts_out`] on its own, so
/// the stdlib clause below is the whole of the difference between the two
/// detectors. Per `f-infra-design.md` §5 P1/P5.
// G-allow: shared suppression predicate; callers are intra-crate (p5_phantom_done::check_live_path_stranded) — orphan-audit script counts only inter-crate call sites
pub(crate) fn is_symbol_suppressed(symbol: &ChangedSymbol) -> bool {
    symbol.file.starts_with("crates/reify-stdlib/") || symbol.opts_out()
}

/// `Some(n)` when `symbols` carries `n >= 1` entries and NOT ONE of their
/// declarations could be located, so a detector's per-symbol pass examined
/// nothing it was handed; `None` when at least one was examinable.
///
/// Such a sweep reports zero findings for the same reason an empty one does —
/// it looked at nothing — but it is invisible to an `is_empty()` check, so
/// the two states need separate detection. The cause is a degraded jcodemunch
/// substrate (see [`ChangedSymbol::decl_located`]); before unlocatable symbols
/// were skipped, that degradation announced itself as a false-positive storm,
/// and without this it would announce itself as nothing at all.
///
/// An EMPTY sweep is deliberately not folded in: "received nothing" and
/// "examined nothing of what was received" have different remedies, so each
/// detector words them as separate clauses.
// G-allow: shared vacuity rule; callers are intra-crate (p1_producer_orphan, p5_phantom_done) — orphan-audit script counts only inter-crate call sites
pub(crate) fn wholly_unlocatable_count(symbols: &[ChangedSymbol]) -> Option<usize> {
    (!symbols.is_empty() && symbols.iter().all(|s| !s.decl_located())).then_some(symbols.len())
}

// -----------------------------------------------------------------------
// Unit tests — pure parse helpers
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::parse_rename_target;

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

    /// Every arm of the tracked-file read that PTODO, PDSSENTINEL and
    /// PDOCCOVER each used to inline separately and none of them tested
    /// directly: the path resolves against `project_root` rather than the
    /// process cwd, and anything unreadable — absent, a directory, or not
    /// valid UTF-8 — yields `None` rather than a finding or a panic.
    ///
    /// The encoding arm is the one a real corpus reaches: `read_to_string`
    /// fails on invalid UTF-8, so any tracked file with a swept extension and
    /// binary-ish content exercises it, and it is what the detectors'
    /// `files_scanned` accounting means when it excludes paths whose read
    /// yielded `None`.
    #[test]
    fn read_relative_resolves_against_project_root_and_skips_unreadable() {
        use super::{AuditContext, MockGitOps, MockJCodemunchOps};
        use rusqlite::Connection;
        use std::collections::HashMap;

        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("crates/x/src")).expect("mkdir src");
        std::fs::write(root.path().join("crates/x/src/a.rs"), "fn a() {}\n").expect("write");
        std::fs::create_dir_all(root.path().join("crates/x/tests")).expect("mkdir tests");

        let conn = Connection::open_in_memory().expect("in-memory db");
        let git = MockGitOps::new();
        let jc = MockJCodemunchOps::new();
        let ctx = AuditContext {
            project_root: root.path().to_path_buf(),
            conn: &conn,
            git: &git,
            jcodemunch: &jc,
            task_metadata: HashMap::new(),
            target_task_id: None,
            window: None,
            now: None,
            producer_branch: None,
        };

        assert_eq!(
            ctx.read_relative("crates/x/src/a.rs").as_deref(),
            Some("fn a() {}\n"),
            "the path is resolved against project_root, not the process cwd",
        );
        assert_eq!(
            ctx.read_relative("crates/x/src/absent.rs"),
            None,
            "an absent file is skipped fail-safe",
        );
        assert_eq!(
            ctx.read_relative("crates/x/tests"),
            None,
            "a directory is unreadable — `None`, never a panic",
        );

        std::fs::write(root.path().join("crates/x/src/bad.rs"), [0xFF, 0xFE, 0x00])
            .expect("write non-UTF-8");
        assert_eq!(
            ctx.read_relative("crates/x/src/bad.rs"),
            None,
            "a non-UTF-8 file is skipped fail-safe, not a panic",
        );
    }
}
