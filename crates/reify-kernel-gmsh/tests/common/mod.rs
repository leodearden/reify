//! Re-export shim over the workspace-canonical fixtures in
//! [`reify_test_support::fixtures`].
//!
//! # Current role
//!
//! This module holds no geometry. Task #6387 hoisted every fixture below into
//! `reify_test_support::fixtures`, which is now the single definition for the
//! whole workspace; this file survives ONLY so its three existing consumers
//! keep compiling against the `common::` paths they already spell.
//!
//! Those consumers — `tests/fill_metrics_tests.rs`, `tests/volume_fill_fraction.rs`
//! and `tests/classify_feature_angle.rs` — are outside #6387's locked file set,
//! which is why they were not re-pointed at `reify_test_support` directly. A
//! follow-up that re-points them can delete this file outright.
//!
//! # Why this file existed (history)
//!
//! The three consumers above are three views of the same defect (arithmetic /
//! symptom / mechanism) and therefore need the same geometry. Written
//! independently they carried three verbatim copies of `prismatic_box_mesh` and
//! two of `unwelded_prismatic_box_mesh` — ~150 lines of duplication whose real
//! cost is *drift*: a fixture corrected in one copy and not the others silently
//! makes the three guards disagree about what "a box" is, which is exactly the
//! class of gap that let #6200 survive. This module removed the duplication
//! #6200 itself introduced; #6387 finished the job by removing the duplication
//! between this module and the rest of the workspace.
//!
//! A `tests/common/` subdirectory (rather than a sibling `tests/*.rs` file) is
//! the cargo idiom for a shared test-support module: files under `tests/` are
//! each compiled as their own test binary, files under `tests/common/` are not.
//! That property is also why the lints below are needed — each consumer binary
//! compiles its own copy of this module and uses only part of the re-export
//! list, so the unused remainder must not be an error under `-D warnings`.
//! `dead_code` covered that when the bodies lived here; a `pub use` that a
//! given binary does not exercise is an `unused_imports` warning instead, so
//! both are allowed.

#![allow(dead_code, unused_imports)]

// Spelled with the explicit `fixtures::` module path rather than importing from
// the crate root: `reify_test_support`'s `lib.rs` glob-re-exports several
// modules (`pub use fixtures::*;`, `pub use helpers::*;`, …), so a root-path
// import would become an E0659 ambiguity the moment any other glob-exported
// module grew a same-named item.
pub use reify_test_support::fixtures::{
    F32_STORAGE_REL, assert_rel, prismatic_box_mesh, tessellated_cylinder_mesh,
    tessellated_cylinder_volume, unit_cube_mesh, unwelded_prismatic_box_mesh,
};
