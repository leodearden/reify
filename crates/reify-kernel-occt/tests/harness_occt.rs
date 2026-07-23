//! Consolidated integration-test harness for the reify-kernel-occt crate.
//!
//! Task #5277 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, decomposition
//! leaf C-occt): folds ALL 51 former standalone `tests/<file>.rs` binaries in this crate
//! into this single compile unit, cutting 50 merge-gate link units. Layout-only — no
//! `#[test]` fn is added or removed. Each former file is included as a stem-named module
//! so its `<file>::<test>` module path (and thus every nextest `test(/^<file>::/)` NAME
//! filterset) still resolves unchanged. Explicit `#[path]` is required: this harness root is
//! an integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_occt/` subdir.
//!
//! What does NOT stay unchanged: the nextest `binary_id` for every one of the 51 former
//! binaries becomes `harness_occt`, so a `--test <name>` / `binary(<name>)` selector keyed on
//! the old per-file name (e.g. `--test contains`, `binary(contains)`) no longer matches — only
//! the `test(...)` NAME predicate is preserved via module paths. No such binary-level selector
//! exists anywhere in this repo today (the 6 binary-name guards — test_heavy_filter_atoms,
//! test_nextest_slow_priority, test_verify_offline_partition, test_verify_gate_exclude_heavy,
//! test_run_offline_deep, test_verify_role_prio — contain no occt refs, and nextest.toml's
//! `binary()` filters name only the 7 override binaries, none of which live in this crate), so
//! this has no in-repo impact; noted here for any future script author.
//!
//! `mod common;` below is bare (no `#[path]`): crate-root directory-relative resolution finds
//! the retained `tests/common/mod.rs` from here, so the shared OCCT test helpers compile ONCE
//! for the whole harness instead of once per former binary (the PRD §3 "dedup common" bonus).
//! The 3 files that declared their own `mod common;` (chamfer_with_history_integration,
//! fillet_with_history_integration, local_feature_helper_contract) were rewritten to
//! `use crate::common;`, leaving their `common::foo()` call sites unchanged.
//!
//! cfg retention: 44 of the 51 moved files carry a crate-level `#![cfg(has_occt)]`; 3 carry
//! `#![cfg(all(has_occt, feature = "test-fixtures"))]` (conformance_integration,
//! curve_curvature_integration, face_differential_integration); 1 carries
//! `#![cfg(all(has_occt, feature = "mesh-morph"))]` (projector_impl). These inner attributes
//! are retained VERBATIM on the moved submodules rather than hoisted to an outer `#[cfg]` on
//! the `mod` declaration below — an inner `#![cfg(...)]` on a `#[path]`-loaded submodule
//! compiles correctly in every cfg state and preserves the `<file>::<test>` module path, so
//! hoisting would have bought nothing while costing 48 error-prone body edits. The remaining 3
//! files (dispatcher_integration, inventory_registration, warm_startable_registration) carry
//! no crate-level cfg and compile/run without OCCT today. Accordingly THIS harness root is
//! deliberately NOT `#![cfg(has_occt)]`-gated — gating it would drop those 3 modules' tests
//! whenever `has_occt` is unset, a regression. Each module's own retained inner attribute
//! reproduces the exact pre-consolidation compile matrix.

mod common;

#[path = "harness_occt/analytic_datum_tests.rs"]
mod analytic_datum_tests;
#[path = "harness_occt/apply_transform_integration.rs"]
mod apply_transform_integration;
#[path = "harness_occt/boolean_op_history_integration.rs"]
mod boolean_op_history_integration;
#[path = "harness_occt/chamfer_with_history_integration.rs"]
mod chamfer_with_history_integration;
#[path = "harness_occt/closest_point_on_shape_integration.rs"]
mod closest_point_on_shape_integration;
#[path = "harness_occt/conformance_integration.rs"]
mod conformance_integration;
#[path = "harness_occt/contains.rs"]
mod contains;
#[path = "harness_occt/curve_constructors_integration.rs"]
mod curve_constructors_integration;
#[path = "harness_occt/curve_curvature_integration.rs"]
mod curve_curvature_integration;
#[path = "harness_occt/dispatcher_integration.rs"]
mod dispatcher_integration;
#[path = "harness_occt/extrude_infinite_integration.rs"]
mod extrude_infinite_integration;
#[path = "harness_occt/extrude_integration.rs"]
mod extrude_integration;
#[path = "harness_occt/extrude_symmetric_integration.rs"]
mod extrude_symmetric_integration;
#[path = "harness_occt/extrude_with_history_integration.rs"]
mod extrude_with_history_integration;
#[path = "harness_occt/face_differential_integration.rs"]
mod face_differential_integration;
#[path = "harness_occt/fillet_with_history_integration.rs"]
mod fillet_with_history_integration;
#[path = "harness_occt/fuse_all_integration.rs"]
mod fuse_all_integration;
#[path = "harness_occt/geo_equiv.rs"]
mod geo_equiv;
#[path = "harness_occt/half_space_integration.rs"]
mod half_space_integration;
#[path = "harness_occt/interference_integration.rs"]
mod interference_integration;
#[path = "harness_occt/inventory_registration.rs"]
mod inventory_registration;
#[path = "harness_occt/local_feature_helper_contract.rs"]
mod local_feature_helper_contract;
#[path = "harness_occt/loft_guided_integration.rs"]
mod loft_guided_integration;
#[path = "harness_occt/loft_with_history_integration.rs"]
mod loft_with_history_integration;
#[path = "harness_occt/max_deviation_query.rs"]
mod max_deviation_query;
#[path = "harness_occt/mesh_deviation.rs"]
mod mesh_deviation;
#[path = "harness_occt/nurbs_surface_integration.rs"]
mod nurbs_surface_integration;
#[path = "harness_occt/pattern_differential_integration.rs"]
mod pattern_differential_integration;
#[path = "harness_occt/pattern_single_pass_counter.rs"]
mod pattern_single_pass_counter;
#[path = "harness_occt/per_edge_chamfer.rs"]
mod per_edge_chamfer;
#[path = "harness_occt/per_edge_fillet.rs"]
mod per_edge_fillet;
#[path = "harness_occt/point_on_shape_integration.rs"]
mod point_on_shape_integration;
#[path = "harness_occt/projection_trait_delegation.rs"]
mod projection_trait_delegation;
#[path = "harness_occt/projector_impl.rs"]
mod projector_impl;
#[path = "harness_occt/revolve_with_history_integration.rs"]
mod revolve_with_history_integration;
#[path = "harness_occt/shell_open_curated_faces.rs"]
mod shell_open_curated_faces;
#[path = "harness_occt/shell_shape_oob_face_index_integration.rs"]
mod shell_shape_oob_face_index_integration;
#[path = "harness_occt/split_integration.rs"]
mod split_integration;
#[path = "harness_occt/surface_angle_integration.rs"]
mod surface_angle_integration;
#[path = "harness_occt/sweep_guided_integration.rs"]
mod sweep_guided_integration;
#[path = "harness_occt/sweep_with_history_integration.rs"]
mod sweep_with_history_integration;
#[path = "harness_occt/tessellate_sphere_nondegenerate_integration.rs"]
mod tessellate_sphere_nondegenerate_integration;
#[path = "harness_occt/tessellation_winding_integration.rs"]
mod tessellation_winding_integration;
#[path = "harness_occt/topology_cache_observability.rs"]
mod topology_cache_observability;
#[path = "harness_occt/topology_extract_integration.rs"]
mod topology_extract_integration;
#[path = "harness_occt/topology_selectors_integration.rs"]
mod topology_selectors_integration;
#[path = "harness_occt/transform_distance_integration.rs"]
mod transform_distance_integration;
#[path = "harness_occt/vertex_point_integration.rs"]
mod vertex_point_integration;
#[path = "harness_occt/warm_startable_registration.rs"]
mod warm_startable_registration;
#[path = "harness_occt/warm_start_failures_accessor_integration.rs"]
mod warm_start_failures_accessor_integration;
#[path = "harness_occt/wrap_occt_call_diagnostic_integration.rs"]
mod wrap_occt_call_diagnostic_integration;
