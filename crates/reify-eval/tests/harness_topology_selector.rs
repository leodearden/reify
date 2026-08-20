//! Consolidated integration-test harness for the topology/selector subsystem.
//!
//! Task #5281 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-2):
//! folds the former standalone `tests/<file>.rs` binaries for the topology_/selector_/
//! structural_/shell_ subsystems into this single compile unit to cut the
//! merge-gate link count. Layout-only — no `#[test]` fn is added or removed. Each former
//! file is included as a stem-named module so its `<file>::<test>` module path (and thus
//! every `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is required:
//! this harness root is an integration-test crate root, where a bare `mod <file>;` would
//! resolve to the sibling `tests/<file>.rs`, not the `harness_topology_selector/` subdir.
//!
//! The `selective_demand_*` subsystem now lives in the sibling `harness_selective_demand`
//! (task #5620), and took the shared `#[path = "common/differential.rs"]` include with it —
//! those six files were its only referents. Under the C2 kLOC cap that shared include is
//! charged to every unit that compiles a copy of it, which had put this harness at 21470
//! lines, over CAP_LINES = 20000; rule (a)'s remedy is a split, not a cap bump. This unit is
//! now 16786 lines with no out-of-module-dir include at all.
#[path = "harness_topology_selector/selector_boundary_gate.rs"]
mod selector_boundary_gate;
#[path = "harness_topology_selector/selector_coercion_golden.rs"]
mod selector_coercion_golden;
#[path = "harness_topology_selector/selector_vocabulary_v2_e2e.rs"]
mod selector_vocabulary_v2_e2e;
#[path = "harness_topology_selector/selector_vocabulary_v2_leaf_wiring_e2e.rs"]
mod selector_vocabulary_v2_leaf_wiring_e2e;
#[path = "harness_topology_selector/selector_vocabulary_v2_mock.rs"]
mod selector_vocabulary_v2_mock;
#[path = "harness_topology_selector/shell_channels_mapping.rs"]
mod shell_channels_mapping;
#[path = "harness_topology_selector/shell_channels_surfacing_e2e.rs"]
mod shell_channels_surfacing_e2e;
#[path = "harness_topology_selector/shell_extract_compute_integration.rs"]
mod shell_extract_compute_integration;
#[path = "harness_topology_selector/shell_extract_gui_accessor.rs"]
mod shell_extract_gui_accessor;
#[path = "harness_topology_selector/shell_extract_persistent_cache_round_trip.rs"]
mod shell_extract_persistent_cache_round_trip;
#[path = "harness_topology_selector/shell_mixed_region_validation.rs"]
mod shell_mixed_region_validation;
#[path = "harness_topology_selector/shell_too_thick_at_auto_falls_back.rs"]
mod shell_too_thick_at_auto_falls_back;
#[path = "harness_topology_selector/shell_too_thick_at_shell_annotation_errors.rs"]
mod shell_too_thick_at_shell_annotation_errors;
#[path = "harness_topology_selector/structural_query_bom.rs"]
mod structural_query_bom;
#[path = "harness_topology_selector/structural_query_descendants.rs"]
mod structural_query_descendants;
#[path = "harness_topology_selector/structural_query_eval.rs"]
mod structural_query_eval;
#[path = "harness_topology_selector/structural_query_filter.rs"]
mod structural_query_filter;
#[path = "harness_topology_selector/topology_attribute_boolean_e2e.rs"]
mod topology_attribute_boolean_e2e;
#[path = "harness_topology_selector/topology_attribute_e2e.rs"]
mod topology_attribute_e2e;
#[path = "harness_topology_selector/topology_attribute_extrude_revolve_e2e.rs"]
mod topology_attribute_extrude_revolve_e2e;
#[path = "harness_topology_selector/topology_attribute_local_features_e2e.rs"]
mod topology_attribute_local_features_e2e;
#[path = "harness_topology_selector/topology_attribute_primitives_direct.rs"]
mod topology_attribute_primitives_direct;
#[path = "harness_topology_selector/topology_attribute_primitives_e2e.rs"]
mod topology_attribute_primitives_e2e;
#[path = "harness_topology_selector/topology_attribute_resolver_e2e.rs"]
mod topology_attribute_resolver_e2e;
#[path = "harness_topology_selector/topology_attribute_sweep_loft_e2e.rs"]
mod topology_attribute_sweep_loft_e2e;
#[path = "harness_topology_selector/topology_attribute_vertex_seeding.rs"]
mod topology_attribute_vertex_seeding;
#[path = "harness_topology_selector/topology_correspondence_drop_diagnostic_e2e.rs"]
mod topology_correspondence_drop_diagnostic_e2e;
#[path = "harness_topology_selector/topology_filtered_selectors.rs"]
mod topology_filtered_selectors;
#[path = "harness_topology_selector/topology_filtered_selectors_mock.rs"]
mod topology_filtered_selectors_mock;
#[path = "harness_topology_selector/topology_selector_runtime.rs"]
mod topology_selector_runtime;
#[path = "harness_topology_selector/topology_selector_smoke_tests.rs"]
mod topology_selector_smoke_tests;
#[path = "harness_topology_selector/topology_selectors_tests.rs"]
mod topology_selectors_tests;
