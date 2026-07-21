//! Consolidated integration-test harness for the kernel + realization subsystems.
//!
//! Task #5278 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-1):
//! folds the former standalone `tests/<file>.rs` binaries for the kernel_/realization_
//! subsystems into this single compile unit to cut the merge-gate link count. Layout-only —
//! no `#[test]` fn is added or removed. Each former file is included as a stem-named module so
//! its `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) resolves
//! unchanged. Explicit `#[path]` is required: this harness root is an integration-test crate
//! root, where a bare `mod <file>;` would resolve to the sibling `tests/<file>.rs`.
//!
//! EXCLUDED (kept standalone): realization_cache_alloc.rs and
//! realization_cache_alloc_rotating_options_hash.rs each install a process-wide
//! `#[global_allocator]`; two in one binary is a compile error, and one shared across siblings
//! would corrupt their alloc-count delta assertions.
#[path = "harness_kernel_realization/kernel_attribute_hook_wiring.rs"]
mod kernel_attribute_hook_wiring;
#[path = "harness_kernel_realization/kernel_pin_enforcement.rs"]
mod kernel_pin_enforcement;
#[path = "harness_kernel_realization/kernel_queries_adjacent_faces.rs"]
mod kernel_queries_adjacent_faces;
#[path = "harness_kernel_realization/kernel_queries_angle_smoke.rs"]
mod kernel_queries_angle_smoke;
#[path = "harness_kernel_realization/kernel_queries_contains.rs"]
mod kernel_queries_contains;
#[path = "harness_kernel_realization/kernel_queries_curvature_smoke.rs"]
mod kernel_queries_curvature_smoke;
#[path = "harness_kernel_realization/kernel_queries_directional_selectors.rs"]
mod kernel_queries_directional_selectors;
#[path = "harness_kernel_realization/kernel_queries_distance_smoke.rs"]
mod kernel_queries_distance_smoke;
#[path = "harness_kernel_realization/kernel_queries_filtered_edges.rs"]
mod kernel_queries_filtered_edges;
#[path = "harness_kernel_realization/kernel_queries_geo_equiv_smoke.rs"]
mod kernel_queries_geo_equiv_smoke;
#[path = "harness_kernel_realization/kernel_queries_integration.rs"]
mod kernel_queries_integration;
#[path = "harness_kernel_realization/kernel_queries_intersects_smoke.rs"]
mod kernel_queries_intersects_smoke;
#[path = "harness_kernel_realization/kernel_queries_length_perimeter.rs"]
mod kernel_queries_length_perimeter;
#[path = "harness_kernel_realization/kernel_queries_moment_of_inertia_smoke.rs"]
mod kernel_queries_moment_of_inertia_smoke;
#[path = "harness_kernel_realization/kernel_queries_normal_smoke.rs"]
mod kernel_queries_normal_smoke;
#[path = "harness_kernel_realization/kernel_registry_inventory.rs"]
mod kernel_registry_inventory;
#[path = "harness_kernel_realization/kernel_version_enforcement.rs"]
mod kernel_version_enforcement;
#[path = "harness_kernel_realization/realization_input_cone_hash_pinning.rs"]
mod realization_input_cone_hash_pinning;
#[path = "harness_kernel_realization/realization_kernel_provenance.rs"]
mod realization_kernel_provenance;
#[path = "harness_kernel_realization/realization_produced_repr_pinning.rs"]
mod realization_produced_repr_pinning;
#[path = "harness_kernel_realization/realization_read_api.rs"]
mod realization_read_api;
