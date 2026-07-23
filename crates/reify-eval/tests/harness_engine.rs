//! Consolidated integration-test harness for the engine subsystem.
//!
//! Task #5056 (PRD docs/prds/merge-gate-compile-cost.md §5 C1): this is the C1
//! consolidated harness compile unit for the engine subsystem cluster, seeded ahead
//! of leaf EVAL-3 (task #5282, the planned full engine-subsystem consolidation) so
//! #5056's new test lands under the C1 layout instead of grandfathering another
//! top-level standalone binary into the baseline manifest. Layout-only — no `#[test]`
//! fn is added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the
//! sibling `tests/<file>.rs`, not the `harness_engine/` subdir.
#[path = "harness_engine/edit_param_cell_commit_migration.rs"]
mod edit_param_cell_commit_migration;
#[path = "harness_engine/diagnostics_cache_replay_migration.rs"]
mod diagnostics_cache_replay_migration;
