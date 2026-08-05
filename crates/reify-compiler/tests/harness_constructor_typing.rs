//! Consolidated integration-test harness for the builtin CONSTRUCTOR-FAMILY static
//! return-type contracts — the `expr.rs` `NoUserFunctions` classification ladder and the
//! per-family `*_signatures.rs` resolvers behind it (orientation / frame / transform,
//! the axis-aligned datum constructors, and the math construction names).
//!
//! Task #5344: this crate's orientation/frame/transform typing lock is landed HERE, as a
//! C1-sanctioned compile unit, rather than as a top-level standalone
//! `tests/orientation_constructor_typing_tests.rs`. That standalone form was flagged
//! `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh; the
//! sanctioned remedy is consolidation, NOT a new
//! tests/infra/harness-layout-baseline.manifest grandfather row (SUPERSEDED — Leo
//! 2026-07-22, esc-5056-11: the baseline is a shrinking ratchet, not an allow-list to
//! grow). No `#[test]` fn is added or removed relative to the standalone form
//! (invariant I3); the file keeps its original stem, so its post-consolidation selector
//! is `orientation_constructor_typing_tests::<test>`.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_constructor_typing/` subdir. As in harness_doc_chunks.rs /
//! harness_geometry_kinds.rs — and unlike harness_langcore.rs / harness_patterns.rs — the
//! shared `common` helper module is deliberately NOT declared here: the absorbed file
//! declares no `mod` and uses no helper, so declaring it would pull tests/common/mod.rs
//! into this compile unit for nothing, in a PRD whose whole point is cutting merge-gate
//! compile cost.
//!
//! Future constructor-family typing locks belong in this unit. In particular the sibling
//! precedents this crate still carries as grandfathered standalones —
//! `tests/affine_constructor_typing_tests.rs` (the family whose template #5344 mirrored),
//! `tests/datum_constructor_tests.rs`, and `tests/math_construction_signatures_tests.rs` —
//! are the natural next absorptions, each shrinking the baseline ratchet by one row; they
//! are left alone here only because they are outside #5344's file scope.

#[path = "harness_constructor_typing/orientation_constructor_typing_tests.rs"]
mod orientation_constructor_typing_tests;
