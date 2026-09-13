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
//! `harness_constructor_typing/` subdir. As in harness_langcore.rs / harness_patterns.rs
//! — and unlike harness_doc_chunks.rs / harness_geometry_kinds.rs — the shared `common`
//! helper module IS declared here, once, at the root: three members consume
//! `compile_with_stdlib_helper`, and declaring it per-member would load the same source
//! several times in one compile unit, which `clippy::duplicate_mod` rejects under
//! `-D warnings`. The 391 external lines it charges this unit are paid deliberately.
//!
//! Task #5695 (PRD §5 C1, leaf CMP-5) spent this unit's designated next absorption: the
//! math construction / transcendental / parse-length signature resolvers, which pin the
//! same `NoUserFunctions` result-type ladder, are members now and hold no baseline row.
//! Future constructor-family typing locks belong here too. Neither remaining sibling
//! precedent is a candidate — the affine family, whose structural template #5344
//! mirrored, was consolidated as `harness_units_materials/affine_constructor_typing_tests.rs`,
//! and `datum_constructor_tests.rs` as `harness_physical_modeling/datum_constructor_tests.rs`
//! by #5694; correspondingly neither holds a baseline row.

#[path = "common/mod.rs"]
mod common;

#[path = "harness_constructor_typing/math_construction_signatures_tests.rs"]
mod math_construction_signatures_tests;
#[path = "harness_constructor_typing/math_signatures.rs"]
mod math_signatures;
#[path = "harness_constructor_typing/math_transcendental_signatures.rs"]
mod math_transcendental_signatures;
#[path = "harness_constructor_typing/orientation_constructor_typing_tests.rs"]
mod orientation_constructor_typing_tests;
#[path = "harness_constructor_typing/parse_length_signatures.rs"]
mod parse_length_signatures;
