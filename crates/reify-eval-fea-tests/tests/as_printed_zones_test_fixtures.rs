// Shared `AsPrintedZones` Value-fixture builders for heterogeneous FEA tests.
//
// This file is a thin `include!` shim, NOT an independent copy. The single
// source of truth is `crates/reify-eval/tests/as_printed_zones_test_fixtures.rs`
// — see that file's header for the full rationale (it replaced two
// hand-duplicated ~150-line implementations, one per consumer, so that an
// `AsPrintedZones` lambda-layout change in `as_printed_material.rs` only
// needs one edit). Splicing the canonical file in here (instead of
// re-duplicating its contents across the crate split performed by task
// 4937) keeps that single-source-of-truth property intact:
// `crates/reify-eval/src/compute_targets/elastic_static.rs`'s
// `#[cfg(test)]` block includes the SAME canonical file directly, and this
// crate's `solve_elastic_static_heterogeneous_e2e.rs` reaches it here via
// `mod as_printed_zones_test_fixtures;` (sibling-file module resolution).
//
// The path below is relative to THIS file's directory
// (`crates/reify-eval-fea-tests/tests/`), so `../../reify-eval/tests/`
// reaches `crates/reify-eval/tests/`.
include!("../../reify-eval/tests/as_printed_zones_test_fixtures.rs");
