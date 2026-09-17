//! Consolidated integration-test harness for the reify-kernel-occt crate.
//!
//! Task #5277 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, decomposition
//! leaf C-occt): folded ALL 51 former standalone `tests/<file>.rs` binaries in this crate
//! into a single compile unit, cutting 50 merge-gate link units. Layout-only — no
//! `#[test]` fn is added or removed. Each former file is included as a stem-named module
//! so its `<file>::<test>` module path (and thus every nextest `test(/^<file>::/)` NAME
//! filterset) still resolves unchanged. Explicit `#[path]` is required: this harness root is
//! an integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_occt/` subdir.
//!
//! What does NOT stay unchanged: the nextest `binary_id` for every former binary became
//! `harness_occt`, so a `--test <name>` / `binary(<name>)` selector keyed on the old per-file
//! name (e.g. `--test contains`, `binary(contains)`) no longer matches — only the `test(...)`
//! NAME predicate is preserved via module paths. No such binary-level selector exists anywhere
//! in this repo today (the 6 binary-name guards — test_heavy_filter_atoms,
//! test_nextest_slow_priority, test_verify_offline_partition, test_verify_gate_exclude_heavy,
//! test_run_offline_deep, test_verify_role_prio — contain no occt refs, and nextest.toml's
//! `binary()` filters name only the 7 override binaries, none of which live in this crate), so
//! this has no in-repo impact; noted here for any future script author.
//!
//! Task #7466 then SPLIT this harness in two under rule (a) of the kLOC cap: it had grown to
//! 19700 lines (156 root + 18385 across 56 module files + 1159 external) against
//! CAP_LINES = 20000. The 16 modules that consumed the shared `tests/common/` helpers moved to
//! the sibling `harness_occt_measurement.rs`, AND TOOK `mod common;` WITH THEM — so this root
//! no longer declares `mod common;` at all and the 1159-line external include is charged to
//! that unit alone (measured here: `external_files = 0`). The seam is mechanical, not
//! thematic: a test belongs HERE iff it consumes NONE of `tests/common/`, and
//! `grep -rn 'common::' crates/reify-kernel-occt/tests/harness_occt/` must stay empty. The
//! compiler enforces that direction — with no `mod common;` in this root, a new
//! `crate::common` use here fails to build. See `harness_occt_measurement.rs`'s header for the
//! other side of the seam and why the consumers were the side that moved.
//!
//! cfg retention: of the 40 modules that remain here, 34 carry a crate-level
//! `#![cfg(has_occt)]`, 2 carry `#![cfg(all(has_occt, feature = "test-fixtures"))]`
//! (conformance_integration, curve_curvature_integration), and 1 carries
//! `#![cfg(all(has_occt, feature = "mesh-morph"))]` (projector_impl). These inner attributes
//! are retained VERBATIM on the moved submodules rather than hoisted to an outer `#[cfg]` on
//! the `mod` declarations below — an inner `#![cfg(...)]` on a `#[path]`-loaded submodule
//! compiles correctly in every cfg state and preserves the `<file>::<test>` module path, so
//! hoisting would have bought nothing while costing error-prone body edits. The remaining 3
//! files (dispatcher_integration, inventory_registration, warm_startable_registration) carry
//! no crate-level cfg and compile/run without OCCT today. Accordingly THIS harness root is
//! deliberately NOT `#![cfg(has_occt)]`-gated — gating it would drop those 3 modules' tests
//! whenever `has_occt` is unset, a regression. All three stayed here through the #7466 split,
//! so that reasoning is unchanged by it. Each module's own retained inner attribute reproduces
//! the exact pre-consolidation compile matrix.

#[path = "harness_occt/apply_transform_integration.rs"]
mod apply_transform_integration;
#[path = "harness_occt/boolean_op_history_integration.rs"]
mod boolean_op_history_integration;
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
#[path = "harness_occt/empty_shape_consumer_guard_integration.rs"]
mod empty_shape_consumer_guard_integration;
#[path = "harness_occt/extrude_integration.rs"]
mod extrude_integration;
#[path = "harness_occt/extrude_with_history_integration.rs"]
mod extrude_with_history_integration;
#[path = "harness_occt/fuse_all_integration.rs"]
mod fuse_all_integration;
#[path = "harness_occt/geo_equiv.rs"]
mod geo_equiv;
#[path = "harness_occt/half_space_integration.rs"]
mod half_space_integration;
#[path = "harness_occt/helix_sweep_integration.rs"]
mod helix_sweep_integration;
#[path = "harness_occt/interference_integration.rs"]
mod interference_integration;
#[path = "harness_occt/inventory_registration.rs"]
mod inventory_registration;
#[path = "harness_occt/max_deviation_query.rs"]
mod max_deviation_query;
#[path = "harness_occt/mesh_deviation.rs"]
mod mesh_deviation;
#[path = "harness_occt/nurbs_surface_integration.rs"]
mod nurbs_surface_integration;
#[path = "harness_occt/offset_surface_integration.rs"]
mod offset_surface_integration;
#[path = "harness_occt/pattern_differential_integration.rs"]
mod pattern_differential_integration;
#[path = "harness_occt/pattern_single_pass_counter.rs"]
mod pattern_single_pass_counter;
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
#[path = "harness_occt/shell_shape_oob_face_index_integration.rs"]
mod shell_shape_oob_face_index_integration;
#[path = "harness_occt/split_integration.rs"]
mod split_integration;
#[path = "harness_occt/surface_angle_integration.rs"]
mod surface_angle_integration;
#[path = "harness_occt/sweep_with_history_integration.rs"]
mod sweep_with_history_integration;
#[path = "harness_occt/tessellate_sphere_nondegenerate_integration.rs"]
mod tessellate_sphere_nondegenerate_integration;
#[path = "harness_occt/tessellation_winding_integration.rs"]
mod tessellation_winding_integration;
#[path = "harness_occt/topology_cache_observability.rs"]
mod topology_cache_observability;
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
