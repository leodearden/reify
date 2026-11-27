//! Consolidated integration-test harness for the reify-cli crate.
//!
//! Task #5274 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, decomposition
//! leaf C-cli): folds the crate's former standalone `tests/<file>.rs` binaries into
//! consolidated compile units, cutting merge-gate link units. Layout-only — no `#[test]`
//! fn is added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset) still
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the sibling
//! `tests/<file>.rs`, not the `harness_cli/` subdir.
//!
//! Two sibling roots were carved out of this one under rule (a) — the SPLIT remedy, never a
//! CAP_LINES bump. This header is the ONE home of the ordered placement rule for a new
//! reify-cli integration test; the first rule that matches wins:
//!   1. a test that consumes none of the shared `tests/common/` helpers goes to
//!      `harness_cli_surface.rs` (task #7365);
//!   2. else a test whose stem is `cli_check` or begins `cli_check_` goes to
//!      `harness_cli_check.rs` (task #10100);
//!   3. else it goes here.
//!
//! Nothing enforces these rules mechanically, and two older members here predate rule 1 and
//! break it: `corpus_no_bare_scalar` and `units_length_boundary_ledger` consume no
//! `tests/common/` helper. An existing member's placement is therefore not evidence of a rule.
//!
//! All three roots share the `tests/fixtures/` `.ri` tree; a `.ri` file is not a seam signal
//! and costs nothing under C2. The #7365 provenance and pre-split measurement are stated
//! once, in `harness_cli_surface.rs`'s header. No post-split line count is hand-copied here:
//! `tests/infra/test_harness_kloc_cap.sh` measures every root live on every run, which is the
//! only figure that cannot go stale.
//!
//! `mod common;` below is bare (no `#[path]`): crate-root directory-relative resolution
//! finds the retained `tests/common/mod.rs` from here, so the shared CLI test helpers
//! compile ONCE for the whole harness instead of once per former binary (the PRD §3
//! "dedup common" bonus). Each moved file's own `mod common;` was rewritten to
//! `use crate::common;`, leaving its `common::foo()` call sites unchanged. The include is
//! compiled into this unit and into `harness_cli_check`, each copy charged under the C2 cap,
//! and never into `harness_cli_surface` — rule 1 is what keeps it out of that one.

mod common;

#[path = "harness_cli/cli_affine_eval.rs"]
mod cli_affine_eval;
#[path = "harness_cli/cli_affine_tapered_spacer.rs"]
mod cli_affine_tapered_spacer;
#[path = "harness_cli/cli_auto_type_param_select.rs"]
mod cli_auto_type_param_select;
#[path = "harness_cli/cli_bbox_eval.rs"]
mod cli_bbox_eval;
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
#[path = "harness_cli/cli_cache_concurrent_writers.rs"]
mod cli_cache_concurrent_writers;
#[path = "harness_cli/cli_curated_unit_labels_eval.rs"]
mod cli_curated_unit_labels_eval;
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
#[path = "harness_cli/cli_eval_annotation_after_valued_member.rs"]
mod cli_eval_annotation_after_valued_member;
#[path = "harness_cli/cli_eval_auto_resolve.rs"]
mod cli_eval_auto_resolve;
#[path = "harness_cli/cli_eval_build_multi_file.rs"]
mod cli_eval_build_multi_file;
#[path = "harness_cli/cli_eval_data_carrying_enum.rs"]
mod cli_eval_data_carrying_enum;
#[path = "harness_cli/cli_eval_datum_units.rs"]
mod cli_eval_datum_units;
#[path = "harness_cli/cli_eval_dir_builtin.rs"]
mod cli_eval_dir_builtin;
#[path = "harness_cli/cli_eval_direction_display.rs"]
mod cli_eval_direction_display;
#[path = "harness_cli/cli_eval_fallback_recovery.rs"]
mod cli_eval_fallback_recovery;
#[path = "harness_cli/cli_eval_generic_enum.rs"]
mod cli_eval_generic_enum;
#[path = "harness_cli/cli_eval_geometry.rs"]
mod cli_eval_geometry;
#[path = "harness_cli/cli_eval_joint_direction_axis.rs"]
mod cli_eval_joint_direction_axis;
#[path = "harness_cli/cli_eval_joint_position_enum.rs"]
mod cli_eval_joint_position_enum;
#[path = "harness_cli/cli_eval_pattern_primitive_direction_forms.rs"]
mod cli_eval_pattern_primitive_direction_forms;
#[path = "harness_cli/cli_eval_registry_determinism.rs"]
mod cli_eval_registry_determinism;
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
#[path = "harness_cli/cli_materials_starter_library_golden.rs"]
mod cli_materials_starter_library_golden;
#[path = "harness_cli/cli_module_decl_remedy.rs"]
mod cli_module_decl_remedy;
#[path = "harness_cli/cli_module_visibility_example.rs"]
mod cli_module_visibility_example;
#[path = "harness_cli/cli_objective_inheritance_golden.rs"]
mod cli_objective_inheritance_golden;
#[path = "harness_cli/cli_one_sided_auto.rs"]
mod cli_one_sided_auto;
#[path = "harness_cli/cli_orientation_rotvec_dimension.rs"]
mod cli_orientation_rotvec_dimension;
#[path = "harness_cli/cli_pub_import_reexport.rs"]
mod cli_pub_import_reexport;
#[path = "harness_cli/cli_std_prelude_facade.rs"]
mod cli_std_prelude_facade;
#[path = "harness_cli/cli_purpose.rs"]
mod cli_purpose;
#[path = "harness_cli/cli_purpose_stdlib.rs"]
mod cli_purpose_stdlib;
#[path = "harness_cli/cli_report.rs"]
mod cli_report;
#[path = "harness_cli/cli_representation_within.rs"]
mod cli_representation_within;
#[path = "harness_cli/cli_reset_per_build_interleaving.rs"]
mod cli_reset_per_build_interleaving;
#[path = "harness_cli/cli_run.rs"]
mod cli_run;
#[path = "harness_cli/cli_shell_eval.rs"]
mod cli_shell_eval;
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
#[path = "harness_cli/cli_structure_instance_golden.rs"]
mod cli_structure_instance_golden;
#[path = "harness_cli/cli_sub_placement_assembly.rs"]
mod cli_sub_placement_assembly;
#[path = "harness_cli/cli_surface_finish_cost.rs"]
mod cli_surface_finish_cost;
#[path = "harness_cli/cli_sweep_direction_forms.rs"]
mod cli_sweep_direction_forms;
#[path = "harness_cli/cli_test.rs"]
mod cli_test;
#[path = "harness_cli/cli_tensegrity_e2e.rs"]
mod cli_tensegrity_e2e;
#[path = "harness_cli/cli_tensegrity_t0a_golden.rs"]
mod cli_tensegrity_t0a_golden;
#[path = "harness_cli/cli_tolerancing_eval.rs"]
mod cli_tolerancing_eval;
#[path = "harness_cli/cli_trait_assoc_fn_overload.rs"]
mod cli_trait_assoc_fn_overload;
#[path = "harness_cli/cli_transform_consumer_length_diag.rs"]
mod cli_transform_consumer_length_diag;
#[path = "harness_cli/cli_transform_twist_dimension_diag.rs"]
mod cli_transform_twist_dimension_diag;
#[path = "harness_cli/cli_type_hygiene_strict.rs"]
mod cli_type_hygiene_strict;
#[path = "harness_cli/cli_undef_self_describing.rs"]
mod cli_undef_self_describing;
#[path = "harness_cli/cli_vc_clearance.rs"]
mod cli_vc_clearance;
#[path = "harness_cli/corpus_no_bare_scalar.rs"]
mod corpus_no_bare_scalar;
#[path = "harness_cli/units_length_boundary_gate.rs"]
mod units_length_boundary_gate;
#[path = "harness_cli/units_length_boundary_ledger.rs"]
mod units_length_boundary_ledger;
