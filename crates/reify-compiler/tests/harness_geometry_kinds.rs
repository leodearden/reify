//! Consolidated integration-test harness for the `GeometryOp` kind-family public-API
//! contracts (the `VARIANT_COUNT` registry backstops that `reify-eval`'s
//! registry-completeness locks depend on).
//!
//! Task #5754 (units-length κ; C1 contract in tests/infra/test_harness_kloc_cap.sh, PRD
//! docs/prds/merge-gate-compile-cost.md §5): κ's new public-API regression lock is landed
//! HERE, as a C1-sanctioned compile unit, rather than as a top-level standalone
//! `tests/geometry_kind_variant_count.rs`. That standalone form was flagged
//! `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh;
//! the sanctioned remedy is consolidation, NOT a new
//! tests/infra/harness-layout-baseline.manifest grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to
//! grow). No `#[test]` fn is added or removed relative to the standalone form
//! (invariant I3); the file keeps its original stem, so its post-consolidation selector
//! is `geometry_kind_variant_count::<test>`.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_geometry_kinds/` subdir. As in harness_doc_chunks.rs — and unlike
//! harness_langcore.rs / harness_patterns.rs — the shared `common` helper module is
//! deliberately NOT declared here: the absorbed file declares no `mod` and uses no
//! helper, so declaring it would pull tests/common/mod.rs into this compile unit for
//! nothing, in a PRD whose whole point is cutting merge-gate compile cost.
//!
//! Future kind-family public-API locks belong in this unit. In particular the sibling
//! precedent `tests/modify_kind_public_api.rs` (task 2238) — which this crate still
//! carries as a grandfathered standalone — is the natural next absorption, shrinking the
//! baseline ratchet by one row; it is left alone here only because it is outside #5754's
//! file scope.

#[path = "harness_geometry_kinds/geometry_kind_variant_count.rs"]
mod geometry_kind_variant_count;
