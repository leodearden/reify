//! Consolidated integration-test harness for the reify-cli crate.
//!
//! Task #5274 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, decomposition
//! leaf C-cli): folds ALL 73 former standalone `tests/<file>.rs` binaries in this crate
//! into this single compile unit, cutting 72 merge-gate link units. Layout-only — no
//! `#[test]` fn is added or removed. Each former file is included as a stem-named module
//! so its `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) still
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_cli/` subdir.
//!
//! `mod common;` below is bare (no `#[path]`): crate-root directory-relative resolution
//! finds the retained `tests/common/mod.rs` from here, so the shared CLI test helpers
//! compile ONCE for the whole harness instead of once per former binary (the PRD §3
//! "dedup common" bonus). Each moved file's own `mod common;` was rewritten to
//! `use crate::common;`, leaving its `common::foo()` call sites unchanged.
//!
//! `rpath_smoke` is linux-only: its former crate-level `#![cfg(target_os = "linux")]` is
//! hoisted to a `#[cfg(target_os = "linux")]` on its `mod` declaration below, since a
//! submodule cannot carry a crate-level inner attribute.

mod common;

#[path = "harness_cli/cli_affine_eval.rs"]
mod cli_affine_eval;
#[path = "harness_cli/cli_affine_tapered_spacer.rs"]
mod cli_affine_tapered_spacer;
#[path = "harness_cli/cli_auto_type_param_select.rs"]
mod cli_auto_type_param_select;
#[path = "harness_cli/cli_build.rs"]
mod cli_build;
#[path = "harness_cli/cli_build_3mf.rs"]
mod cli_build_3mf;
#[path = "harness_cli/cli_build_3mf_coating.rs"]
mod cli_build_3mf_coating;
#[path = "harness_cli/cli_build_fea.rs"]
mod cli_build_fea;
#[path = "harness_cli/cli_build_outputs.rs"]
mod cli_build_outputs;
#[path = "harness_cli/cli_build_verbose.rs"]
mod cli_build_verbose;
#[path = "harness_cli/cli_build_voxel_to_mesh.rs"]
mod cli_build_voxel_to_mesh;
#[path = "harness_cli/cli_cache.rs"]
mod cli_cache;
#[path = "harness_cli/cli_check.rs"]
mod cli_check;
#[path = "harness_cli/cli_check_cfg.rs"]
mod cli_check_cfg;
#[path = "harness_cli/cli_check_cfg_example.rs"]
mod cli_check_cfg_example;
#[path = "harness_cli/cli_check_parametric_rate.rs"]
mod cli_check_parametric_rate;
#[path = "harness_cli/cli_check_parametric_vec3.rs"]
mod cli_check_parametric_vec3;
#[path = "harness_cli/cli_check_result_prelude.rs"]
mod cli_check_result_prelude;
#[path = "harness_cli/cli_check_variant_construction.rs"]
mod cli_check_variant_construction;
#[path = "harness_cli/cli_datum_projection_check.rs"]
mod cli_datum_projection_check;
#[path = "harness_cli/cli_determinacy_gate.rs"]
mod cli_determinacy_gate;
#[path = "harness_cli/cli_determinacy_intrinsics.rs"]
mod cli_determinacy_intrinsics;
#[path = "harness_cli/cli_dfm_overhang.rs"]
mod cli_dfm_overhang;
#[path = "harness_cli/cli_dfm_thickness.rs"]
mod cli_dfm_thickness;
#[path = "harness_cli/cli_doc.rs"]
mod cli_doc;
#[path = "harness_cli/cli_eval_auto_resolve.rs"]
mod cli_eval_auto_resolve;
#[path = "harness_cli/cli_eval_data_carrying_enum.rs"]
mod cli_eval_data_carrying_enum;
#[path = "harness_cli/cli_eval_fallback_recovery.rs"]
mod cli_eval_fallback_recovery;
#[path = "harness_cli/cli_eval_generic_enum.rs"]
mod cli_eval_generic_enum;
#[path = "harness_cli/cli_eval_geometry.rs"]
mod cli_eval_geometry;
#[path = "harness_cli/cli_eval_result_fallback.rs"]
mod cli_eval_result_fallback;
#[path = "harness_cli/cli_eval_result_match.rs"]
mod cli_eval_result_match;
#[path = "harness_cli/cli_eval_result_recovery.rs"]
mod cli_eval_result_recovery;
#[path = "harness_cli/cli_eval_shell_no_tet_warning.rs"]
mod cli_eval_shell_no_tet_warning;
#[path = "harness_cli/cli_explain.rs"]
mod cli_explain;
#[path = "harness_cli/cli_gdt_conformance.rs"]
mod cli_gdt_conformance;
#[path = "harness_cli/cli_gdt_integration_gate.rs"]
mod cli_gdt_integration_gate;
#[path = "harness_cli/cli_gdt_legality.rs"]
mod cli_gdt_legality;
#[path = "harness_cli/cli_gdt_zones_eval.rs"]
mod cli_gdt_zones_eval;
#[path = "harness_cli/cli_generate_eval.rs"]
mod cli_generate_eval;
#[path = "harness_cli/cli_generics_eval.rs"]
mod cli_generics_eval;
#[path = "harness_cli/cli_gui.rs"]
mod cli_gui;
#[path = "harness_cli/cli_imported_field_eval.rs"]
mod cli_imported_field_eval;
#[path = "harness_cli/cli_inspect_node.rs"]
mod cli_inspect_node;
#[path = "harness_cli/cli_integration_smoke.rs"]
mod cli_integration_smoke;
#[path = "harness_cli/cli_kernel_registry.rs"]
mod cli_kernel_registry;
#[path = "harness_cli/cli_keyed_eval.rs"]
mod cli_keyed_eval;
#[path = "harness_cli/cli_keyed_forall.rs"]
mod cli_keyed_forall;
#[path = "harness_cli/cli_lsp.rs"]
mod cli_lsp;
#[path = "harness_cli/cli_lsp_protocol.rs"]
mod cli_lsp_protocol;
#[path = "harness_cli/cli_module_visibility_example.rs"]
mod cli_module_visibility_example;
#[path = "harness_cli/cli_purpose.rs"]
mod cli_purpose;
#[path = "harness_cli/cli_purpose_stdlib.rs"]
mod cli_purpose_stdlib;
#[path = "harness_cli/cli_report.rs"]
mod cli_report;
#[path = "harness_cli/cli_representation_within.rs"]
mod cli_representation_within;
#[path = "harness_cli/cli_run.rs"]
mod cli_run;
#[path = "harness_cli/cli_shell_eval.rs"]
mod cli_shell_eval;
#[path = "harness_cli/cli_smoke.rs"]
mod cli_smoke;
#[path = "harness_cli/cli_spatial_ops_eval.rs"]
mod cli_spatial_ops_eval;
#[path = "harness_cli/cli_stackup_3part.rs"]
mod cli_stackup_3part;
#[path = "harness_cli/cli_stackup_eval.rs"]
mod cli_stackup_eval;
#[path = "harness_cli/cli_string_interp_eval.rs"]
mod cli_string_interp_eval;
#[path = "harness_cli/cli_structural_query_bom.rs"]
mod cli_structural_query_bom;
#[path = "harness_cli/cli_sub_placement_assembly.rs"]
mod cli_sub_placement_assembly;
#[path = "harness_cli/cli_surface_finish_cost.rs"]
mod cli_surface_finish_cost;
#[path = "harness_cli/cli_test.rs"]
mod cli_test;
#[path = "harness_cli/cli_tolerancing_eval.rs"]
mod cli_tolerancing_eval;
#[path = "harness_cli/cli_trait_assoc_fn_overload.rs"]
mod cli_trait_assoc_fn_overload;
#[path = "harness_cli/cli_type_hygiene_strict.rs"]
mod cli_type_hygiene_strict;
#[path = "harness_cli/cli_undef_self_describing.rs"]
mod cli_undef_self_describing;
#[path = "harness_cli/cli_vc_clearance.rs"]
mod cli_vc_clearance;
#[path = "harness_cli/corpus_no_bare_scalar.rs"]
mod corpus_no_bare_scalar;
#[path = "harness_cli/mcp_integration.rs"]
mod mcp_integration;
#[cfg(target_os = "linux")]
#[path = "harness_cli/rpath_smoke.rs"]
mod rpath_smoke;
