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
//!    declaration-site disambiguation. Per `f-infra-design.md` §5 P1.
//!
//! All three seams let the integration tests in `tests/{p1,p2,p5}.rs` exercise
//! every code path (happy path + false-positive guards + `check_pre_done`
//! filtering) without a real git repo, a real runs.db, or a real jcodemunch
//! MCP server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
pub mod pdoccover_baseline;
pub mod ratchet_ledger;
pub mod pdcheck;
pub mod pcite;
pub mod pprdstatus;
pub mod plinecite;
pub mod plinecite_baseline;
pub mod pattern_flag;
/// Crate-internal: shared scaffolding for the lanes that read the task DB.
/// Not part of the detector API surface — the lanes are.
pub(crate) mod task_rows;
/// Crate-internal: shared text-scanning primitives for the structural
/// detectors. Not part of the detector API surface — the detectors are.
pub(crate) mod scan_util;
/// Crate-internal: PTODO lane δ-C's owner-position grammar for task cites.
pub(crate) mod ptodo_promise;
/// Not a detector: public only so the live-serve test harnesses decode MCP
/// response bodies with the same function both clients use.
pub mod mcp_wire;
/// Crate-internal: the one rule for what counts as a completed MCP
/// `initialize`, shared by both MCP clients.
pub(crate) mod mcp_handshake;
pub mod fused_memory_client;
pub mod jcodemunch_client;
pub mod jcodemunch_index;
pub mod jcodemunch_seam;
mod git_ops;

pub use git_ops::{GitCommit, GitOps, RealGitOps, LOG_GREP_FORMAT};
#[cfg(any(test, feature = "test-support"))]
pub use git_ops::MockGitOps;

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
/// - `P1TaskUnexamined` / `P5LivePathUnexamined` — P1 / P5 H2 examined
///   nothing where it should have. Per-task form: that task's
///   `get_changed_symbols` query failed. Run-level form, `task_id` empty: the
///   run had no usable jcodemunch seam from the start.
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
    /// P1 — task-unexamined: P1 examined nothing where it should have. Two
    /// forms, told apart by `task_id` alone:
    ///
    /// - **per-task** (`task_id` = the task): a `done` task P1 could not
    ///   examine because its `get_changed_symbols` query failed (see
    ///   [`JCodemunchCallFailed`]);
    /// - **run-level** (`task_id` empty, the convention the corpus-level
    ///   detectors use): the run had no usable jcodemunch seam from the start
    ///   — unreachable, handshake refused, or an unusable index — so P1
    ///   examined no task. At most one per run; the summary names the reason
    ///   ([`jcodemunch_seam::SeamUnavailable`]).
    ///
    /// Always Low, so it never moves the exit code. Its presence is what
    /// stops such a sweep from reading as clean: without it, zero
    /// producer-orphan findings would be indistinguishable from a range that
    /// introduced nothing.
    P1TaskUnexamined,
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
    /// P5 — live-path-unexamined: H2 examined nothing where it should have.
    /// H2's counterpart of [`Pattern::P1TaskUnexamined`], with the same two
    /// forms told apart by `task_id`:
    ///
    /// - **per-task**: a cross-crate `done` task whose `get_changed_symbols`
    ///   query failed;
    /// - **run-level** (`task_id` empty): the run had no usable jcodemunch
    ///   seam from the start, and it sweeps at least one task H2 would have
    ///   queried for. At most one per run.
    ///
    /// Always Low, so it never moves the exit code; its presence stops zero
    /// `P5LivePathStranded` findings from reading as clean.
    P5LivePathUnexamined,
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
    /// directions, its finding categories carried as a stable summary prefix
    /// (PTODO's `kind`-as-prefix convention above), all at
    /// [`Severity::High`]:
    ///
    /// - **Omission lane** — a `*_NAMES` registry entry in
    ///   `crates/reify-compiler/src/units.rs` that is not documented in any
    ///   chunk, not marked `// pdoccover:allow — <reason>`, and not ledgered
    ///   as a bare-name row → `undocumented-name:`. Ratchet-honesty sibling:
    ///   `stale-allow-entry:` (an allow-marked name that IS documented).
    /// - **Fabrication lane** — a call-shaped name documented in a chunk that
    ///   exists nowhere in the compiler/stdlib sources and is not ledgered as
    ///   a `<chunk path>:<name>` row → `fabricated-name:`.
    /// - Both lanes share `allow-missing-reason:` — a `pdoccover:allow` token
    ///   with a blank reason body confers NO exemption and is itself a finding.
    /// - `stale-baseline-entry:` — a row of the committed ledger
    ///   `crates/reify-audit/pdoccover-baseline.txt` that settles no live debt.
    /// - `census-empty:` / `no-chunks:` — no registry name, or no readable
    ///   chunk (a failed `git ls-files` is both): the run's ONLY finding, so a
    ///   tree PDOCCOVER never read cannot report clean.
    ///
    /// **Opt-in only**, like PDIAG: the exit code is the High-severity count,
    /// so a drifting ledger in the no-`--pattern` default sweep would move
    /// every bare `reify-audit` exit code. The hard gate over the committed
    /// ledger is `tests/infra/test_reify_audit_pdoccover.sh`. Structural:
    /// reads the working tree via `ls_files()` + `std::fs`, never contacts
    /// jcodemunch.
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
    /// PCITE — capability-manifest cite lane: a symbol a
    /// `docs/prds/**/*.capability-manifest.md` row cites as `grep:` evidence
    /// that no tracked source outside `docs/` and markdown contains. Two kinds,
    /// carried as a stable summary prefix (PTODO's `kind`-as-prefix convention
    /// above): `fabricated-cite:` (one per manifest and name) and
    /// `allow-missing-reason:` (a `pcite:allow` marker with no reason body,
    /// which exempts nothing). Each carries one [`EvidenceRef::FileLine`]:
    /// the manifest line, and the cited symbol for `fabricated-cite:`.
    ///
    /// **Medium only** — report-only and exit-neutral, so the lane cannot gate
    /// even when selected. Opt-in, like PDIAG/PDOCCOVER/PDCHECK. Structural:
    /// reads the working tree via `ls_files()` + `std::fs`, never contacts
    /// jcodemunch. Grammar, oracle and their measured basis: `pcite.rs`.
    PManifestCite,
    /// PPRDSTATUS — PRD status-prose drift. The [`pprdstatus`] module doc is
    /// the canonical definition of its lanes, inputs and opt-in rationale.
    PPrdStatus,
    /// PLINECITE — stale hard line cites. The [`plinecite`] module doc is the
    /// canonical definition.
    PLineCite,
}

/// A pointer to forensic evidence supporting a [`Finding`]. Renders verbatim
/// in the eventual `/audit` report; consumers may follow it back to the
/// underlying source (file, commit, metadata blob, runs.db row).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceRef {
    /// Filesystem path relative to `project_root`.
    File { path: String },
    /// One line of a file relative to `project_root`, and the `symbol` on it
    /// the finding is about — `None` when the finding is about the line
    /// itself. A detector that reports several findings per file carries
    /// each one's handle here, so a consumer (the `/audit` dedupe key, a
    /// follow-up title) reads it rather than parsing it out of the summary.
    FileLine {
        path: String,
        line: usize,
        symbol: Option<String>,
    },
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
    /// The tracked-file read for PTODO, PDSSENTINEL and PDOCCOVER. Only
    /// ENUMERATION is a git seam — those detectors take path membership from
    /// `git.ls_files()` and then read the working tree directly through here,
    /// so a path that is tracked but absent, unreadable, a directory, or not
    /// valid UTF-8 is SKIPPED fail-safe: no finding, no panic. That matters
    /// because the callers are scanners run over the whole repo, where one
    /// unreadable file must not be able to take the detector — or the verify
    /// gate it runs in — down.
    ///
    /// Not every such read in the crate: `pdiag.rs` hand-rolls two more, and
    /// converging them is #7740.
    pub(crate) fn read_relative(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.project_root.join(path)).ok()
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
    /// whitespace does NOT (mirrors `scripts/audit-orphan-producers.sh`'s
    /// `G_ALLOW_RE` pattern, `//\s*G-allow:\s*(.+)`, where `(.+)` requires
    /// content).
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

/// A [`JCodemunchOps`] query that failed outright, as distinct from one that
/// answered with nothing. Carried by
/// [`JCodemunchOps::get_changed_symbols`]'s `Err` so a caller can report the
/// range it could not examine instead of reading the failure as empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JCodemunchCallFailed {
    /// The jcodemunch tool whose call failed, e.g. `"get_changed_symbols"`.
    pub tool: &'static str,
    /// The client's description of the failure (transport, timeout, or an
    /// error envelope).
    pub detail: String,
}

impl std::fmt::Display for JCodemunchCallFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "jcodemunch {} call failed: {}", self.tool, self.detail)
    }
}

impl std::error::Error for JCodemunchCallFailed {}

/// Source-introspection operations the P1 detector needs. Production: a
/// jcodemunch-MCP-backed impl supplied by the T-4 CLI. Tests:
/// [`MockJCodemunchOps`] (gated behind `feature = "test-support"`) holds
/// canned answers keyed on `(since_sha, until_sha)` for changed-symbol
/// queries and `(file, name)` for reference queries, enabling per-commit and
/// declaration-site disambiguation. Per `f-infra-design.md` §3 and §5 P1.
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
    ///
    /// `Ok` with an empty vec means the range introduced nothing (or its
    /// commits are not in the index). `Err` means the query itself failed —
    /// unreachable mid-sweep, timed out, an error envelope — and must never
    /// be read as empty: the range went unexamined.
    fn get_changed_symbols(
        &self,
        since_sha: &str,
        until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed>;

    /// Equivalent of `mcp__jcodemunch__find_references(identifier = symbol.name)`:
    /// the workspace references to `symbol`, where each
    /// [`SymbolReference::file`] is the REFERENCING site.
    ///
    /// Impls MUST NOT narrow the result by the referencing file. jcodemunch's
    /// lookup is import-based, so the declaring file is never in it, and a
    /// cross-file caller is exactly the evidence P1 and P5 H2 read as "any
    /// non-test reference" (#6504).
    ///
    /// Impls MAY drop references to same-named symbols declared elsewhere
    /// when their substrate can tell them apart (`MockJCodemunchOps` keys on
    /// `(file, name)`). The jcodemunch-backed impl cannot: its lookup is
    /// name-based with no file parameter, so it over-approximates, counting
    /// a same-named symbol's importer as a caller. That is a false-negative
    /// bias, accepted for a low-false-positive detector.
    ///
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
/// Its `Ok(vec![])` from `get_changed_symbols` is not a statement about any
/// range: no query was made. Unlike [`MockJCodemunchOps`] this is NOT
/// test-support: it is the production binding whenever a run has no live
/// seam, and it is ungated for exactly that reason. Its production call sites:
///
/// 1. `--no-jcodemunch` — the explicit offline escape hatch: P1 runs and
///    produces zero findings without opening a socket.
/// 2. Detector runs that never touch the seam (`needs_jcodemunch() == false`).
/// 3. The fallback for a needed seam that is unreachable, refused the
///    handshake, or sits over an unusable index
///    ([`jcodemunch_seam::Seam::Unavailable`]). The bin records the affected
///    detectors as unexamined in that case, so their silence is not read as
///    a clean result.
/// 4. The `*-baseline-gen` structural censuses, which still have to populate
///    [`AuditContext`]'s field.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopJCodemunchOps;

impl JCodemunchOps for NoopJCodemunchOps {
    fn get_changed_symbols(
        &self,
        _since_sha: &str,
        _until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed> {
        Ok(vec![])
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
/// keyed on `(file, name)` for declaration-site disambiguation, which the
/// [`JCodemunchOps::find_references`] contract permits but the
/// jcodemunch-backed impl cannot provide.
/// Dead-code / untested-symbol data is stored as a flat Vec and filtered by
/// `min_confidence` at query time (mirrors the real tool's semantics).
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Default)]
pub struct MockJCodemunchOps {
    get_changed_symbols:
        HashMap<(String, String), Result<Vec<ChangedSymbol>, JCodemunchCallFailed>>,
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
            .insert((since_sha.to_string(), until_sha.to_string()), Ok(symbols));
    }

    // G-allow: test-support fixture (feature = "test-support"); not consumed in production builds
    pub fn fail_changed_symbols(&mut self, since_sha: &str, until_sha: &str, detail: &str) {
        let failure = JCodemunchCallFailed {
            tool: "get_changed_symbols",
            detail: detail.to_string(),
        };
        self.get_changed_symbols
            .insert((since_sha.to_string(), until_sha.to_string()), Err(failure));
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
    fn get_changed_symbols(
        &self,
        since_sha: &str,
        until_sha: &str,
    ) -> Result<Vec<ChangedSymbol>, JCodemunchCallFailed> {
        self.get_changed_symbols
            .get(&(since_sha.to_string(), until_sha.to_string()))
            .cloned()
            .unwrap_or_else(|| Ok(vec![]))
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
