//! Consolidated integration-test harness for stdlib **source hygiene** — static,
//! text-level invariants over `crates/reify-compiler/stdlib/*.ri` that hold
//! independently of what the compiler does with those files.
//!
//! Task #5856 (C1 contract in tests/infra/test_harness_kloc_cap.sh, PRD
//! docs/prds/merge-gate-compile-cost.md §5): #5856's citation-hygiene guard is landed
//! HERE, as a C1-sanctioned compile unit, rather than as a top-level standalone
//! `tests/stdlib_citation_hygiene.rs`. That standalone form was flagged
//! `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh;
//! the sanctioned remedy is consolidation, NOT a new
//! tests/infra/harness-layout-baseline.manifest grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to
//! grow). No `#[test]` fn is added or removed relative to the standalone form
//! (invariant I3); the file keeps its original stem, so its post-consolidation selector
//! is `stdlib_citation_hygiene::<test>`.
//!
//! This is the home for any FUTURE static stdlib-text guard (the sibling defect classes
//! #5837/#5854 were each swept one-off with no guard at all) — add it as another module
//! here rather than creating a second stdlib-hygiene harness. Guards that must COMPILE
//! the stdlib belong instead with the per-module compile suites
//! (`ports_stdlib_compile.rs`, `stdlib_loader_tests.rs`, …), not here: this unit
//! deliberately reads `stdlib/` as text and links nothing.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_stdlib_hygiene/` subdir. Following harness_doc_chunks.rs (and unlike
//! harness_langcore.rs / harness_units.rs), the shared `common` helper module is
//! deliberately NOT declared here — the absorbed file uses only `std`, so declaring it
//! would pull tests/common/mod.rs into this compile unit for nothing, in a PRD whose
//! whole point is cutting merge-gate compile cost.

#[path = "harness_stdlib_hygiene/stdlib_citation_hygiene.rs"]
mod stdlib_citation_hygiene;
