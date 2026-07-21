//! Consolidated integration-test harness for the geometry subsystem.
//!
//! Task #5278 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-1):
//! folds the former standalone `tests/<file>.rs` binaries for the geometry_/symbolic_/trait_
//! subsystems into this single compile unit to cut the merge-gate link count. Layout-only —
//! no `#[test]` fn is added or removed. Each former file is included as a stem-named module so
//! its `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) resolves
//! unchanged. Explicit `#[path]` is required: this harness root is an integration-test crate
//! root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_geometry/` subdir.
#[path = "harness_geometry/geometry_conditional_e2e.rs"]
mod geometry_conditional_e2e;
#[path = "harness_geometry/geometry_dispatch_registry_guard.rs"]
mod geometry_dispatch_registry_guard;
#[path = "harness_geometry/geometry_error_handling.rs"]
mod geometry_error_handling;
#[path = "harness_geometry/geometry_handle_freshness.rs"]
mod geometry_handle_freshness;
#[path = "harness_geometry/geometry_handle_persistent_cache_round_trip.rs"]
mod geometry_handle_persistent_cache_round_trip;
#[path = "harness_geometry/geometry_handle_value_cell_e2e.rs"]
mod geometry_handle_value_cell_e2e;
#[path = "harness_geometry/geometry_let_value_cell_gamma.rs"]
mod geometry_let_value_cell_gamma;
#[path = "harness_geometry/geometry_query_kernel_dispatch.rs"]
mod geometry_query_kernel_dispatch;
#[path = "harness_geometry/geometry_sub_ref_e2e.rs"]
mod geometry_sub_ref_e2e;
#[path = "harness_geometry/symbolic_geometry_eval.rs"]
mod symbolic_geometry_eval;
#[path = "harness_geometry/symbolic_selector_composition_eval.rs"]
mod symbolic_selector_composition_eval;
#[path = "harness_geometry/symbolic_selector_eval.rs"]
mod symbolic_selector_eval;
#[path = "harness_geometry/trait_assoc_fn_cylinder.rs"]
mod trait_assoc_fn_cylinder;
#[path = "harness_geometry/trait_assoc_fn_static_e2e.rs"]
mod trait_assoc_fn_static_e2e;
#[path = "harness_geometry/trait_merge_eval.rs"]
mod trait_merge_eval;
