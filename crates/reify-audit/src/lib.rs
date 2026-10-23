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
mod jcodemunch_ops;

pub use git_ops::{GitCommit, GitOps, RealGitOps, LOG_GREP_FORMAT};
#[cfg(any(test, feature = "test-support"))]
pub use git_ops::MockGitOps;
pub use jcodemunch_ops::{
    ChangedSymbol, DeadSymbol, DeclSuppression, JCodemunchCallFailed, JCodemunchOps,
    LayerViolation, NoopJCodemunchOps, SymbolReference, UntestedSymbol,
};
#[cfg(any(test, feature = "test-support"))]
pub use jcodemunch_ops::MockJCodemunchOps;

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
