//! Re-export shim over the workspace-canonical fixtures in
//! [`reify_test_support::fixtures`]. Holds no geometry, and is SCHEDULED FOR
//! DELETION.
//!
//! Task #6387 hoisted every fixture below into `reify_test_support::fixtures`,
//! now the single definition for the whole workspace. This file survives only
//! so its three remaining consumers — `tests/fill_metrics_tests.rs`,
//! `tests/volume_fill_fraction.rs`, `tests/classify_feature_angle.rs` — keep
//! compiling against the `common::` paths they already spell; all three sit
//! outside #6387's locked file set, so re-pointing them was not #6387's to do.
//!
//! That re-point-and-delete is tracked, not aspirational: it is filed as
//! follow-up ticket `tkt_0RTBP18NC8PBRZGQ49RYZD7ATJ` (escalation id
//! `agent-followup-6387`), which changes three import lines plus
//! `volume_fill_fraction.rs`'s `common::assert_rel` call sites and then removes
//! `tests/common/` outright. Nothing here should grow: any new shared fixture
//! belongs in `reify_test_support`, not in this shim.
//!
//! The lints below are load-bearing while it lives. Files under `tests/` are
//! each compiled as their own test binary and files under `tests/common/` are
//! not, so every consumer binary compiles its own copy of this module and uses
//! only part of the re-export list; the unused remainder must not be an error
//! under `-D warnings`. A `pub use` a given binary does not exercise is an
//! `unused_imports` warning rather than the `dead_code` the inlined bodies
//! produced, so both are allowed.

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
