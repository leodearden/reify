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
/// Crate-internal: the git seam; its public surface is re-exported below.
mod git_ops;
/// Crate-internal: the jcodemunch seam; its public surface is re-exported below.
mod jcodemunch_ops;
/// Crate-internal: the finding and input shapes; re-exported below.
mod model;
/// Crate-internal: the caller lookup P1 and P5 H2 share.
pub(crate) mod caller_lookup;

pub use git_ops::{GitCommit, GitOps, RealGitOps, LOG_GREP_FORMAT};
#[cfg(any(test, feature = "test-support"))]
pub use git_ops::MockGitOps;
pub use jcodemunch_ops::{
    ChangedSymbol, DeadSymbol, DeclSuppression, JCodemunchCallFailed, JCodemunchOps,
    LayerViolation, NoopJCodemunchOps, SymbolReference, UntestedSymbol,
};
#[cfg(any(test, feature = "test-support"))]
pub use jcodemunch_ops::MockJCodemunchOps;
pub use model::{
    AuditContext, DoneProvenance, EvidenceRef, Finding, Pattern, Severity, TaskMetadata, TimeWindow,
};

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
