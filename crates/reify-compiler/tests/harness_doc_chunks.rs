//! Consolidated integration-test harness for the doc-chunk truth-enforcement
//! suites (MCP doc chunks vs. what the compiler actually accepts).
//!
//! Task #5477 (PRD docs/prds/v0_6/doc-chunk-truth-enforcement.md task α; C1 contract
//! in tests/infra/test_harness_kloc_cap.sh, PRD docs/prds/merge-gate-compile-cost.md §5):
//! folds the former standalone `tests/geometry_chunk_smoke.rs` (#5364) and
//! `tests/stdlib_chunk_geometry_ops_smoke.rs` (#5347) binaries into this single compile
//! unit, reversing their grandfather rows in tests/infra/harness-layout-baseline.manifest
//! rather than growing that shrinking ratchet. Layout-only — no `#[test]` fn is added or
//! removed. Each former file is included as a stem-named module so its `<file>::<test>`
//! module path (and thus every `test(/^<file>::/)` filterset) resolves unchanged.
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_doc_chunks/` subdir. Unlike harness_langcore.rs / harness_patterns.rs, the
//! shared `common` helper module is deliberately NOT declared here — neither absorbed
//! file declares any `mod` or uses that helper, so declaring it would pull
//! tests/common/mod.rs into this compile unit for nothing, in a PRD whose whole point is
//! cutting merge-gate compile cost.

#[path = "harness_doc_chunks/angle_crossings_diagnostics_smoke.rs"]
mod angle_crossings_diagnostics_smoke;
#[path = "harness_doc_chunks/enums_chunk_option_smoke.rs"]
mod enums_chunk_option_smoke;
#[path = "harness_doc_chunks/geometry_chunk_smoke.rs"]
mod geometry_chunk_smoke;
#[path = "harness_doc_chunks/stdlib_chunk_geometry_ops_smoke.rs"]
mod stdlib_chunk_geometry_ops_smoke;
