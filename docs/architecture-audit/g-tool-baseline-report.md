# G-tool baseline report

**Captured:** 2026-06-27
**Tool:** `scripts/audit-orphan-producers.sh`
**Design:** `docs/architecture-audit/g-reviewer-tool-session-prompt.md`
**Portfolio slot:** approach G — corpus-wide reviewer aid for Type-A
orphan producers (per `preferences-implementation-chain-portfolio`).

## What this report measures

For every `pub fn` declared under `crates/reify-*/src/` (excluding
`reify-test-support` and `src/test_*.rs` files), the tool counts
non-test references across the codebase. A function is reported as an
**orphan candidate** when its only references are inside the defining
file, inside `#[cfg(test)]`-attributed regions, inside `use` /
`pub use` re-exports, or inside line comments.

The tool is heuristic by design — it does no semantic analysis. False
negatives (real orphans the tool misses) come from name collisions
(`union`, `complement`, `new` etc. shadowed by stdlib / trait
methods). False positives are suppressed via an inline marker:

```rust
// G-allow: load-bearing library API; in-tree consumer is task #NNN
pub fn intentional_orphan() -> ... { ... }
```

## Known-orphan-case coverage (audit dispositions)

Verified hits against the Phase-3 file synthesis (`phase-3-files-synthesis.md` §1):

| Cluster | Audit name | Hits / Expected |
|---|---|---|
| C-04 | Library-shipped / no-DSL-consumer (selector resolution) | 3 / 3 |
| C-05 | Auto-resolve / type-param orchestrator compile-pipeline call site | 5 / 5 |
| C-10 | Persistent-naming selector v2 (vocabulary) | 17 / 17 |
25/25 known producer-orphan signals flagged. (C-43 warm-pool `drain_events` wired/resolved by task #3582 — `drain_events` now has a real in-scope non-test caller via `Engine::drain_and_record_warm_pool_events`.) (C-25 `build_doc_model`
is Type-B consumer-with-stub and out of scope for v1; the function
doesn't exist as a `pub fn` so this tool cannot detect it. F-infra's
P2 detector covers Type-B.)

## How to read this report

The orphan list is a **reviewer aid**, not an action queue. Each row
is a candidate that warrants investigation:

- **Real orphan**: file a follow-up task to wire a production consumer,
  or remove the function if no consumer is planned.
- **Library API for downstream**: add a `// G-allow: <reason>` marker
  with the consuming task or external user.
- **Name collision (false-positive caller masked the orphan)**: this
  report won't list it — re-run after renaming the shadowing symbol if
  you suspect this.

## How to regenerate

> **Important:** this report has a hand-written preamble (lines 1–80,
> through the `---` separator before `# Orphan-producer audit`).  A plain
> redirect would overwrite it.  Use the splice procedure below.

```bash
# Step 1 — capture fresh script output to a temp file
./scripts/audit-orphan-producers.sh --format markdown > /tmp/orphan-body.md

# Step 2 — preserve the preamble, bump **Captured:** date, replace body
TODAY=$(date +%Y-%m-%d)
head -n 80 docs/architecture-audit/g-tool-baseline-report.md \
    | sed "3s/\*\*Captured:\*\* .*/**Captured:** ${TODAY}/" \
    > /tmp/orphan-preamble.md
cat /tmp/orphan-preamble.md /tmp/orphan-body.md \
    > docs/architecture-audit/g-tool-baseline-report.md

# Step 3 — verify (on-demand freshness check)
cargo test -p reify-audit --test baseline_report_freshness -- --ignored
```

---

audit-orphan-producers.sh: scanning crates/reify-*/src
audit-orphan-producers.sh: 395 source files
# Orphan-producer audit (Portfolio approach G)

Public functions in `crates/reify-*/src/` whose only callers are
tests, the defining file itself, comments, or `use`/`pub use`
re-exports.

- **Scanned:** 2410 `pub fn` declarations across 464 files
- **Orphan candidates:** 540  (zero non-test callers, no `// G-allow:`)
- **Allow-listed:** 113  (zero callers; marked legitimate API surface)

## Orphan candidates

| Crate | File:Line | Function |
|---|---|---|
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:172` | `emit_rpath_for_bins` |
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:197` | `emit_rpath_for_tests` |
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:218` | `read_soname_version` |
| `reify-cli` | `crates/reify-cli/src/dev.rs:25` | `parse_node_id` |
| `reify-cli` | `crates/reify-cli/src/dev.rs:143` | `format_node_traits` |
| `reify-cli` | `crates/reify-cli/src/dev.rs:170` | `render_inspection` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:113` | `is_known_block_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:118` | `is_module_only_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:573` | `enumerate_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:776` | `filter_feasible_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:823` | `filter_feasible_candidates_seeded` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:962` | `seed_candidate_value_map` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:988` | `seed_template_literal_params` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1181` | `select_candidate` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:2448` | `collect_unevaluated_constraint_cell_pairs` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:2572` | `build_constraint_blame_map` |
| `reify-compiler` | `crates/reify-compiler/src/builtin_signatures.rs:86` | `builtin_arg_slots` |
| `reify-compiler` | `crates/reify-compiler/src/coerce.rs:54` | `is_list_geometry` |
| `reify-compiler` | `crates/reify-compiler/src/compile_builder/defs_phase.rs:34` | `format_shadow_warning` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/checker.rs:35` | `resolve_let_advertised_type` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:578` | `emit_geometry_unbounded` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:649` | `emit_geometry_trait_violation` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:69` | `auto_match_port_members` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:133` | `is_forward_compatible` |
| `reify-compiler` | `crates/reify-compiler/src/diagnostics.rs:28` | `dup_member_key_error` |
| `reify-compiler` | `crates/reify-compiler/src/expr.rs:1256` | `compile_expr_guarded_with_expected` |
| `reify-compiler` | `crates/reify-compiler/src/expr.rs:6111` | `list_engagement` |
| `reify-compiler` | `crates/reify-compiler/src/expr.rs:6124` | `set_engagement` |
| `reify-compiler` | `crates/reify-compiler/src/expr.rs:6137` | `map_engagement` |
| `reify-compiler` | `crates/reify-compiler/src/functions.rs:705` | `resolve_field_type_name` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:462` | `try_hoist_geometry_conditional` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:2146` | `extract_collection_count` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:2171` | `unsupported_geometry_fn_message` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_modify.rs:8` | `compile_modify_2arg` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:189` | `unbounded_convex` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:203` | `bounded_only` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:217` | `bounded_connected` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:268` | `surface_nonconvex` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:287` | `surface_freeform` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:325` | `infer_primitive` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:347` | `combine_union` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:364` | `combine_difference` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:383` | `combine_intersection` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:401` | `combine_transform` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:411` | `combine_modify` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:428` | `combine_pattern` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:447` | `combine_sweep` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:521` | `infer_traits_for_expr` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:732` | `try_infer_traits_for_function_call_in_env` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:9` | `collect_body_refs_inner` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:368` | `compile_guarded_members` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:721` | `narrow_arms_under_guard` |
| `reify-compiler` | `crates/reify-compiler/src/ice.rs:16` | `as_phrase` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:250` | `compile_with_prelude_checked` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:382` | `compile_with_prelude_context` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:425` | `compile_with_prelude_context_checked_with_config` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:623` | `compile_with_stdlib_with_config` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:661` | `compile_with_prelude_refs_checked` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:313` | `compile_module` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:643` | `import_cfg_satisfied` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:683` | `compile_project_with_entry_source` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:812` | `merge_imported_pub_templates` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:887` | `compile_entry_with_stdlib_cfg` |
| `reify-compiler` | `crates/reify-compiler/src/relation_signatures.rs:265` | `relation_contract_string` |
| `reify-compiler` | `crates/reify-compiler/src/si_units.rs:61` | `includes` |
| `reify-compiler` | `crates/reify-compiler/src/stdlib_loader.rs:49` | `stdlib_sources` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:127` | `termination_args_contain_undef` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:146` | `termination_collect_refs` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:165` | `termination_is_modifying` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:165` | `resolve_dimension_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:231` | `evaluate_const_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:561` | `resolve_type_with_params` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1179` | `resolve_type_alias_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1353` | `resolve_type_alias_expr_to_dimension` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1677` | `resolve_parameterized_alias` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1961` | `normalize_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:2524` | `resolve_type_alias_expr_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:2744` | `is_parameterized_builtin_name` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:2811` | `resolve_parameterized_builtin_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:3117` | `resolve_parameterized_builtin_type_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:3280` | `resolve_type_alias_expr_to_dim_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:3358` | `collect_type_expr_names` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:476` | `test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:483` | `non_test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:492` | `test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:497` | `non_test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:504` | `test_functions` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:509` | `non_test_functions` |
| `reify-config` | `crates/reify-config/src/cache.rs:41` | `default_cache_dir` |
| `reify-config` | `crates/reify-config/src/cache.rs:77` | `parse_cache_config` |
| `reify-config` | `crates/reify-config/src/lib.rs:243` | `from_toml_str` |
| `reify-config` | `crates/reify-config/src/lib.rs:330` | `load_from_path` |
| `reify-constraints` | `crates/reify-constraints/src/registry.rs:70` | `with_solvers` |
| `reify-constraints` | `crates/reify-constraints/src/relate_solve.rs:265` | `kernel_local` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:200` | `Slvs_QuaternionU` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:210` | `Slvs_QuaternionV` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:220` | `Slvs_QuaternionN` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:230` | `Slvs_MakeQuaternion` |
| `reify-core` | `crates/reify-core/src/diagnostics.rs:3543` | `hex_wedge_mesh_diagnostic` |
| `reify-core` | `crates/reify-core/src/dimension.rs:40` | `is_integer` |
| `reify-core` | `crates/reify-core/src/dimension.rs:44` | `as_i8` |
| `reify-core` | `crates/reify-core/src/source_location.rs:26` | `build_line_offsets` |
| `reify-eval` | `crates/reify-eval/src/appearance.rs:145` | `resolve_color` |
| `reify-eval` | `crates/reify-eval/src/appearance.rs:271` | `resolve_appearance_opt` |
| `reify-eval` | `crates/reify-eval/src/appearance.rs:297` | `resolve_appearance` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:407` | `is_fresh` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:418` | `bump_version` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:480` | `node_traits_mut` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:517` | `write_intermediate` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:607` | `imported_file_hash_changed` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:838` | `get_dirty_reasons` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1426` | `pending_transition_count` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1539` | `derive_output_freshness_from_trace` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1566` | `derive_output_freshness_for_node` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1696` | `insert_synthetic_realization_entry` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1732` | `derive_output_freshness` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1792` | `derive_output_freshness_with_cause` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1857` | `compute_input_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1885` | `check_early_cutoff` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1903` | `dirty_set` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/as_printed_material.rs:503` | `rung_compute_key` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/bc_resolve.rs:72` | `resolve_selector_faces` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/elastic_static.rs:1212` | `loads_supports_to_bc_node_sets` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/elastic_static.rs:1815` | `solve_cantilever_fea` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/elastic_static.rs:3325` | `extract_execution_params` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/fdm_slice.rs:49` | `toolpath_to_value` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/fdm_slice.rs:68` | `degraded_toolpath_value` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/fdm_slice.rs:187` | `fdm_slice_dispatch` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/fea_diagnostics.rs:62` | `fea_structured_detail` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/shell_solve.rs:229` | `build_shell_channels` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:104` | `add_realization` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:127` | `build_from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:290` | `geometry_cell_realization_links` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:432` | `build_trace_map` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:2807` | `deps_of` |
| `reify-eval` | `crates/reify-eval/src/dirty.rs:95` | `compute_dirty_cone_with_realizations` |
| `reify-eval` | `crates/reify-eval/src/dirty.rs:334` | `check_dag_complete` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:129` | `is_long_chain_realization` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:154` | `kernel_pragma_satisfiable` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:211` | `long_chain_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:471` | `long_chain_threshold_from_env` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:488` | `long_chain_threshold_from_env_value` |
| `reify-eval` | `crates/reify-eval/src/dynamics_ops.rs:118` | `resolve_body_density` |
| `reify-eval` | `crates/reify-eval/src/dynamics_ops.rs:245` | `eval_body_mass_props_core` |
| `reify-eval` | `crates/reify-eval/src/dynamics_ops.rs:2092` | `run_inverse_dynamics` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:221` | `with_prelude` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:394` | `with_test_kernels_and_registry` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:423` | `test_terminal_handle` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:820` | `with_registered_kernels` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:868` | `with_registered_kernels_and_manifest` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:925` | `registered_kernel_names` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:936` | `kernel_count` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1055` | `register_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1068` | `unregister_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1075` | `optimized_targets` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1216` | `set_solver_progress_sink` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1226` | `set_active_solve_cancel` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1324` | `set_max_unfold_depth` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1341` | `set_max_unfold_nodes` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1379` | `register_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1386` | `unregister_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1394` | `registered_solvers` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1475` | `cache_store` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1509` | `snapshot_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1705` | `set_build_scheduler` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1738` | `geometry_revalidation_slow_path_count` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1781` | `last_substantive_value` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1844` | `propagate_freshness_only` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1890` | `warm_pool_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1903` | `cache_store_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1924` | `set_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1938` | `remove_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1951` | `clear_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:2015` | `imported_file_content_hash` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:2101` | `set_warm_state_budget` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1972` | `compute_demanded_reprs` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2151` | `compute_boundary_demands` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2390` | `build_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:3913` | `build_outputs` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4192` | `distance_between_placed` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4793` | `compute_realization_tolerance_budget` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4846` | `budget_available_set` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4875` | `compute_demanded_tols` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4921` | `compute_tessellation_budgets` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:8740` | `tessellate_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:9142` | `dispatch_volume_mesh` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:9319` | `build_mixed_region_mesh` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:17957` | `p2_substitution_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:77` | `dfm_rule_spec` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:178` | `dfm_thickness_spec` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:232` | `min_wall_verdict` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:252` | `min_feature_verdict` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:313` | `dispatch_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:606` | `labeled_diagnostics` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:823` | `collect_active_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:1174` | `measure_thickness_pair` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:1228` | `measure_dfm_rules` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:1478` | `measure_gdt_conformance` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:1888` | `enumerate_gdt_callouts` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:2024` | `check_gdt_legality` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:42` | `set_demand_selective` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:71` | `rebuild_demand_cone` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:82` | `set_demand_full_scope` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:87` | `demand_cone_size` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:93` | `demand_is_demanded` |
| `reify-eval` | `crates/reify-eval/src/engine_demand.rs:98` | `demand_is_full_scope` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:62` | `deactivate_if_not_auto` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:116` | `rewrite_port_placeholder` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:378` | `diff_value_cells` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:396` | `diff_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:414` | `diff_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:2137` | `edit_source` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:3522` | `edit_check` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:102` | `is_representable_cell_type` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:1480` | `hash_imported_file_content` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:5961` | `read_value_revalidated` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:6218` | `revalidate_geometry_handle` |
| `reify-eval` | `crates/reify-eval/src/engine_hash_algo.rs:220` | `compose_engine_version_hash` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:98` | `activate_purpose_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:554` | `deactivate_purpose` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:621` | `active_objectives` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:19` | `imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:69` | `check_imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/feature_datum.rs:140` | `axes_coaxial` |
| `reify-eval` | `crates/reify-eval/src/feature_datum.rs:159` | `planes_coplanar` |
| `reify-eval` | `crates/reify-eval/src/feature_datum.rs:170` | `points_coincident` |
| `reify-eval` | `crates/reify-eval/src/feature_datum.rs:178` | `directions_parallel` |
| `reify-eval` | `crates/reify-eval/src/gating.rs:102` | `unblocked_gated_nodes` |
| `reify-eval` | `crates/reify-eval/src/geometry_op_characterization_probe.rs:39` | `compile_geometry_op_probe` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:103` | `route_capability` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:166` | `eval_named_arg` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:200` | `eval_named_arg_f64` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:235` | `eval_all_args_to_f64` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:324` | `resolve_subhandle_list` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:4844` | `try_eval_symbolic_topology_selector` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:5318` | `try_eval_self_datum_projection` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:9022` | `eval_auto_sub_pose` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:9061` | `realization_is_aux` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:9081` | `decompose_transform_to_arrays` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:9128` | `decode_orientation_to_axis_angle` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:9295` | `walk_placed_realizations` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:693` | `get_compute_node` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:119` | `all_events` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:124` | `events_in_range` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:136` | `events_since` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:144` | `events_for_node` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:158` | `count_matching` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:163` | `count_donated` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:168` | `count_evicted` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:173` | `latest_for_node` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:133` | `pick_lexmin_kernel` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:211` | `pick_lexmin_brep_kernel_in` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:356` | `warn_if_duplicate_op_repr_pairs` |
| `reify-eval` | `crates/reify-eval/src/measure_min_feature.rs:41` | `measure_min_feature` |
| `reify-eval` | `crates/reify-eval/src/measure_min_wall.rs:41` | `measure_min_wall` |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:2054` | `run_transient_response` |
| `reify-eval` | `crates/reify-eval/src/multi_load_dispatch.rs:30` | `detect_multi_case_result` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:125` | `add_observed_demand` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:131` | `remove_observed_demand` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:141` | `rebuild_observed_cone` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:148` | `reset_observed_demand` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:154` | `observed_demand_is_demanded` |
| `reify-eval` | `crates/reify-eval/src/observed_demand.rs:159` | `observed_demand_cone_size` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:64` | `read_sidecar_mtime` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:98` | `touch_sidecar` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:122` | `write_sidecar` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:226` | `verify_format_version` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:255` | `verify_field_echoes` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:283` | `write_to` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:291` | `read_from` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:645` | `max_deflection_magnitude` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:664` | `max_deflection` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:939` | `shard_dir` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:2138` | `eviction_score` |
| `reify-eval` | `crates/reify-eval/src/primitive_attribute_seed.rs:228` | `seed_primitive_attributes` |
| `reify-eval` | `crates/reify-eval/src/realization_cache.rs:219` | `bucket_len` |
| `reify-eval` | `crates/reify-eval/src/relate_solve.rs:91` | `collect_relate_scope` |
| `reify-eval` | `crates/reify-eval/src/relate_solve.rs:250` | `realize_operand_datums` |
| `reify-eval` | `crates/reify-eval/src/relate_solve.rs:518` | `trace_to_ground` |
| `reify-eval` | `crates/reify-eval/src/relate_solve.rs:635` | `solve_relate_scope` |
| `reify-eval` | `crates/reify-eval/src/scope_containment.rs:113` | `nearest_container_objective` |
| `reify-eval` | `crates/reify-eval/src/scope_containment.rs:238` | `nearest_container_objective` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:182` | `complement` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:204` | `except` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:663` | `geom_universal` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:697` | `created_by_feature` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:730` | `split_by_feature` |
| `reify-eval` | `crates/reify-eval/src/shell_extract_compute.rs:580` | `shell_extract_compute_fn` |
| `reify-eval` | `crates/reify-eval/src/significance_filter.rs:187` | `geometry_handle_significance` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:31` | `find_parsed_decl_containing_offset` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:129` | `resolve_entity_at_source_position` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:75` | `realization_graph_shape_hash` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:96` | `classify_cell` |
| `reify-eval` | `crates/reify-eval/src/structural_query.rs:54` | `entity_ref_element` |
| `reify-eval` | `crates/reify-eval/src/structural_query.rs:66` | `enumerate_children` |
| `reify-eval` | `crates/reify-eval/src/structural_query.rs:88` | `enumerate_members` |
| `reify-eval` | `crates/reify-eval/src/structural_query.rs:129` | `enumerate_descendants` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:92` | `compose_sub_handle_hash` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:334` | `edges_by_length_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:413` | `faces_by_area_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:465` | `parse_xyz_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:497` | `parse_flat_number_object` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1036` | `edges_parallel_to_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1135` | `edges_at_height_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1185` | `resolve_unique_by_tag` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1229` | `parse_bbox_z_extents` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1248` | `parse_bbox_z_extents_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:1298` | `parse_bbox_axis_extents_json` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:139` | `with_budget` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:153` | `unlimited` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:227` | `from_config_or_env_value` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:264` | `with_test_events_cap` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:527` | `used_bytes` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:559` | `dropped_events` |
| `reify-expr` | `crates/reify-expr/src/lib.rs:127` | `_test_at_depth` |
| `reify-fdm` | `crates/reify-fdm/src/as_printed.rs:74` | `material_constants_at` |
| `reify-fdm` | `crates/reify-fdm/src/as_printed.rs:129` | `select_rungs` |
| `reify-fdm` | `crates/reify-fdm/src/as_printed.rs:161` | `orthotropic_constants_at` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:119` | `gibson_ashby_infill_factor` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:182` | `pattern_factors` |
| `reify-fdm` | `crates/reify-fdm/src/r0.rs:88` | `rodriguez_orthotropic` |
| `reify-fdm` | `crates/reify-fdm/src/r0.rs:173` | `halpin_tsai_modulus` |
| `reify-fdm` | `crates/reify-fdm/src/r0.rs:185` | `halpin_tsai_reinforced` |
| `reify-fdm` | `crates/reify-fdm/src/r0.rs:217` | `lumped_cooling_z_ratio` |
| `reify-fdm` | `crates/reify-fdm/src/slice.rs:137` | `compose_slicer_args` |
| `reify-fdm` | `crates/reify-fdm/src/slice.rs:210` | `run_slicer` |
| `reify-fdm` | `crates/reify-fdm/src/toolpath.rs:71` | `role_from_prusaslicer_type` |
| `reify-fdm` | `crates/reify-fdm/src/toolpath.rs:892` | `segment_segment_distance` |
| `reify-fdm` | `crates/reify-fdm/src/toolpath.rs:958` | `min_polyline_distance` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:96` | `is_top_or_bottom_normal` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:143` | `min_top_bottom_distance` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:158` | `min_side_distance` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:36` | `register_kernel` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:41` | `has_kernel` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:354` | `no_defaults_for` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:1668` | `user_function_call` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:1748` | `match_expr` |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:4435` | `try_nary` |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:4459` | `nary` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:79` | `id_for` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:84` | `name_for` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:94` | `declared_bounds` |
| `reify-ir` | `crates/reify-ir/src/value.rs:511` | `required_kind` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1321` | `try_into_matrix` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1790` | `infer_type` |
| `reify-ir` | `crates/reify-ir/src/value.rs:2436` | `format_display_number` |
| `reify-ir` | `crates/reify-ir/src/value.rs:3505` | `has_hash` |
| `reify-ir` | `crates/reify-ir/src/warm_registry.rs:74` | `from_inventory` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/kernel.rs:229` | `evaluate_sdf_at` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/kernel.rs:274` | `iso_mesh` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/register.rs:109` | `fidget_capability_descriptor` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/cache_key.rs:46` | `volume_mesh_cache_key` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:53` | `gmshIsInitialized` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:56` | `gmshFinalize` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:59` | `gmshClear` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:62` | `gmshFree` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:65` | `gmshLoggerGetLastError` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:70` | `gmshOptionSetNumber` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:73` | `gmshModelAdd` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:76` | `gmshModelAddDiscreteEntity` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:85` | `gmshModelMeshAddNodes` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:98` | `gmshModelMeshAddElementsByType` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:109` | `gmshModelMeshGetNodes` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:124` | `gmshModelMeshGetElementsByType` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:137` | `gmshModelMeshClassifySurfaces` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:147` | `gmshModelMeshCreateGeometry` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:150` | `gmshModelGeoAddSurfaceLoop` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:158` | `gmshModelGeoAddVolume` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:166` | `gmshModelGeoSynchronize` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:169` | `gmshModelMeshGenerate` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:172` | `gmshModelGetEntities` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:180` | `gmshModelGeoAddPoint` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:190` | `gmshModelGeoAddLine` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:198` | `gmshModelGeoAddCurveLoop` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:207` | `gmshModelGeoAddPlaneSurface` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:215` | `gmshModelMeshSetRecombine` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:332` | `finalize` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_boundary.rs:122` | `suggested_match_tolerance` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:64` | `apply_repair_if_requested` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/register.rs:102` | `gmsh_capability_descriptor` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/kernel.rs:228` | `manifold_from_reify_mesh` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/queries.rs:688` | `tri_area` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/queries.rs:698` | `tri_unit_normal` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:58` | `manifold_factory` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:111` | `manifold_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:828` | `extrude_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:870` | `revolve_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:916` | `sweep_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:959` | `loft_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:992` | `make_rect_profile_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1024` | `make_rect_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1057` | `make_triangle_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1103` | `face_outward_unit_normal_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1494` | `execute_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1508` | `query_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1526` | `export_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1545` | `tessellate_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1564` | `extract_edges_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1578` | `extract_faces_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1592` | `extract_vertices_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1614` | `warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1634` | `with_warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:549` | `repr_of` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:969` | `apply_transform_to_handle` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:1603` | `draft_faces` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:1668` | `shell_solid_faces` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4068` | `warm_start_failures` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4118` | `store_circle_face_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4202` | `store_nonmanifold_compound_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4214` | `store_malformed_solid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4227` | `store_nonorientable_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4240` | `store_closed_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4252` | `store_edge_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4263` | `store_vertex_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4287` | `store_compsolid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4307` | `store_placed_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4337` | `make_half_space_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:101` | `occt_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:197` | `occt_factory` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:542` | `validate_grid_units` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:80` | `realize_voxel_from_mesh` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:107` | `realize_voxel_from_mesh_with_options` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:154` | `active_voxel_count` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:171` | `sample_sdf_at` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:223` | `write_vdb_grid` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:259` | `open_vdb_grid_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:294` | `grid_name_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:336` | `realize_mesh_from_voxel_with_options` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/register.rs:132` | `openvdb_capability_descriptor` |
| `reify-lsp` | `crates/reify-lsp/src/bridge.rs:122` | `handle_request` |
| `reify-lsp` | `crates/reify-lsp/src/convert.rs:144` | `convert_severity` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:33` | `last_content_hash` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:38` | `is_engine_initialized` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:342` | `compute_diagnostics` |
| `reify-lsp` | `crates/reify-lsp/src/server.rs:659` | `take_calls` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:31` | `handle_message` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:52` | `run_on_streams` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:236` | `format_summary` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:77` | `set_instance` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:84` | `set_type` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:192` | `from_config_overrides` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:291` | `progress_estimate` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:330` | `check_commitment` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:442` | `is_committed` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:480` | `task_count` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:172` | `child_token` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:261` | `execute_with_config` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:125` | `from_setup` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:198` | `take_results` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:213` | `build_result_shared` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:249` | `into_result` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:332` | `poison_results` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:343` | `poison_values` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:354` | `poison_snapshot_values` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:465` | `edit_param_concurrent` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:524` | `edit_check_concurrent` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:39` | `effective_priority` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:80` | `promote_for_demand` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:141` | `effective_priority` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:178` | `promote_for_demand` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:836` | `world_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:847` | `sample_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:911` | `gradient_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:991` | `precompute_gradient_grid` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:1074` | `gradient_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:1108` | `bidirectional_distances` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:1207` | `surface_patches_distinct` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/partition.rs:182` | `partition_body` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/hex.rs:29` | `element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/hex.rs:53` | `element_stiffness_hex_p1_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/tet.rs:324` | `tet_p1_centroid` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/tet.rs:380` | `element_stiffness_p2_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/wedge.rs:30` | `element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/wedge.rs:54` | `element_stiffness_wedge_p1_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/diagnostics.rs:45` | `all_rigid_body_modes` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:513` | `lanczos_shift_invert` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/membrane_cst.rs:37` | `element_stiffness_membrane_cst` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:221` | `rotation_shape_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/error_estimator.rs:93` | `compute_zz_indicator` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/form_find.rs:108` | `form_find_anchored` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/form_find.rs:659` | `form_find_anchored_surfaces_aniso` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/form_find.rs:841` | `recover_principal_stress` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/form_find.rs:870` | `triangle_anisotropic_laplacian` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/bar.rs:43` | `geometric_element_stiffness_bar_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/membrane.rs:74` | `geometric_element_stiffness_membrane_cst` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/mod.rs:85` | `uniaxial_z` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:35` | `geometric_element_stiffness_shell` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:60` | `geometric_element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:82` | `geometric_element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:112` | `point_in_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:144` | `interpolate_p1_at_point` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:185` | `locate_element_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:411` | `locate` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/membrane_load.rs:648` | `membrane_stress_delta` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/membrane_load.rs:701` | `principal_stresses_2x2` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:169` | `compute_quad_skew` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:207` | `recombine_quality_ok` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:259` | `auto_mesh_size_from_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:387` | `mesh_swept_profile_2d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mpc.rs:146` | `apply_mpc_row_elimination` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:71` | `refinement_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:84` | `coarse_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:162` | `near_constraint_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:237` | `should_refine` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/resample.rs:81` | `resample_nodal_to_grid` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/resample.rs:99` | `resample_nodal_to_grid_instrumented` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/resample.rs:228` | `resample_multi_nodal_to_grid_instrumented` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:441` | `shell_element_stiffness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:78` | `derive_layer_count` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:107` | `check_sweep_through_thickness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:359` | `sweep_2d_mesh_to_3d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:40` | `from_displacement` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:48` | `from_arc` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:72` | `from_opaque_state` |
| `reify-stdlib` | `crates/reify-stdlib/src/analysis.rs:312` | `compute_stress_invariants_3x3` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/eval.rs:424` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/fea.rs:617` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/geometry.rs:1458` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/loads.rs:72` | `is_load_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:77` | `chain_transform` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:275` | `per_joint_jacobian_local` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:416` | `value_for_joint` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:227` | `is_singular` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:384` | `newton_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:410` | `newton_solve_with_projection` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:920` | `mechanism_loop_closure_chains` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:1087` | `solve_loop_closure_with_diagnostics` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:128` | `integrator` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:261` | `newmark_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:354` | `is_uniformly_sampled` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:434` | `duhamel_coefficients` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:480` | `duhamel_solve_with_coeffs` |
| `reify-stdlib` | `crates/reify-stdlib/src/modal/transient.rs:543` | `duhamel_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/snapshot.rs:1378` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup.rs:280` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup/rng.rs:156` | `next_uniform_f64` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup/rng.rs:164` | `next_u64` |
| `reify-stdlib` | `crates/reify-stdlib/src/supports.rs:77` | `is_support_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/gcode_import.rs:197` | `lower_gcode` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/trampoline.rs:548` | `track_data_to_value` |

## Allow-listed (zero callers, intentional)

| Crate | File:Line | Function | Reason |
|---|---|---|---|
| `reify-audit` | `crates/reify-audit/src/lib.rs:536` | `fail_next_spawns` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:888` | `set_log_grep` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:894` | `set_diff_changed_paths` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:900` | `set_is_gitignored` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:905` | `set_diff_added_lines` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:917` | `set_path_tracked_on` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:923` | `set_is_ancestor` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:929` | `set_diff_added_lines_in_commit` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:940` | `set_file_lines_on` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:951` | `set_ls_files` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:956` | `set_last_commit_for_path` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1210` | `set_changed_symbols` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1221` | `set_find_references` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1226` | `set_dead_code` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1231` | `last_dead_code_min_confidence` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1236` | `set_untested_symbols` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1241` | `set_layer_violations` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:1327` | `is_symbol_suppressed` | shared suppression predicate; callers are intra-crate (p5_phantom_done::check_live_path_stranded) — orphan-audit script counts only inter-crate call sites |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:260` | `g_allow_marker_body` | test-facing pub fn (sole external caller: tests/engine_seam_g_allow_cites_live.rs, a separate crate; must stay pub). Pure grammar — no IO. |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:308` | `extract_g_allow_owner_cites` | test-facing pub fn (sole external caller: tests/engine_seam_g_allow_cites_live.rs, a separate crate; must stay pub). Pure grammar — no IO. |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:623` | `is_allowlisted` | reused by tests/ptodo_baseline.rs well-formedness test (separate crate; pub(crate) would break it). |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:821` | `tasks_db_path` | pub for external test callers (tests/engine_seam_g_allow_cites_live.rs) that need the DB path for the live anti-drift guard. Mirrors the resolve_liveness/resolve_inverse pub-for-integration-test pattern. |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:836` | `open_tasks_db` | pub for external test callers (tests/engine_seam_g_allow_cites_live.rs) that open the real tasks.db for the live anti-drift guard. Mirrors the resolve_liveness/resolve_inverse pub-for-integration-test pattern. |
| `reify-audit` | `crates/reify-audit/src/ptodo.rs:1043` | `resolve_g_allow_owner_liveness` | test-facing pub fn (sole external caller: tests/ptodo.rs + tests/engine_seam_g_allow_cites_live.rs — separate crates that cannot see crate-private items). |
| `reify-compiler` | `crates/reify-compiler/src/annotations/schema.rs:221` | `lookup_schema` | task #3530 (done) const-slice/OnceLock AnnotationSchema registry; consumer is the schema-delegating validate_annotations rewrite (task #3530 step-10, done) |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:126` | `__validate_annotations_for_parity_test` | task #3530 (done) parity shim — test-support-gated (feature = "test-support"), consumed by the validate_annotations parity test (tests/annotation_schema_registry_parity.rs); schema-delegation migration landed (validate_annotations now delegates to schema::validate_via_schema), shim retained for its live test consumer |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:336` | `merge_prelude_purposes` | same-file caller only; audit counts cross-file refs |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:271` | `with_cfg` | same-file caller only; audit counts cross-file refs |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:702` | `compile_project_with_entry_source_cfg` | same-file caller only; audit counts cross-file refs |
| `reify-eval` | `crates/reify-eval/src/compute_targets/elastic_static.rs:1329` | `shell_channels_to_value` | Bucket-1 fn-pointer ComputeFn registration blind spot; in-file production caller in `solve_elastic_static_trampoline` wired by #3594 (done); shipped by #4067 (done); permanent 0-external-caller by audit design. |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1978` | `drain_and_record_warm_pool_events` | task #3541 (done)/#3582 (done) eval-boundary warm-pool→journal drain; consumer EngineSession::drain_and_emit_warm_pool_events (gui/src-tauri/src/engine.rs) landed with #3541 (done); remains an in-scope orphan BY DESIGN (audit scopes to crates/reify-*/src, excludes gui/ + tests/); steady-state pinned by tests/warm_pool_drain_steady_state.rs |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:2133` | `set_achieved_repr_tol_for_test` | test-support setter; not consumed in production builds |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:2297` | `shell_gui_mesh_data` | library API; production consumer is the pending shell-extract GUI bridge |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:671` | `decode_axis` | same-file caller only; audit counts cross-file refs |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:8709` | `cap_kind_translation` | task #3463 (done) cap/role vocabulary table; consumer is try_eval_ad_hoc_selector @face/@edge dispatch (same-file, task #3463, done) + ad_hoc_selector smoke tests |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:69` | `build_beam_mesh` | modal::free_vibration ComputeFn pipeline (task #4066, done) — beam-mesh builder reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file. |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:267` | `assemble_modal_km` | modal::free_vibration ComputeFn pipeline (task #4066, done) — K/M assembler reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file. |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:331` | `eigensolve_modal` | modal::free_vibration ComputeFn pipeline (task #4066, done) — generalized eigensolve reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file. |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:521` | `solve_modal_core` | modal::free_vibration ComputeFn pipeline (task #4066, done) — composed assemble+eigensolve wrapper reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Exercised by the modal_ops unit tests. |
| `reify-eval` | `crates/reify-eval/src/modal_ops.rs:913` | `run_modal_analysis` | modal::free_vibration ComputeFn entry point (task #4066, done) — reached only via the fn-pointer registered in compute_targets::register_compute_fns (mod.rs:140), which the orphan audit cannot trace. Wired + tested in this file. |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:2197` | `sweep_stale_tempfiles` | task #2978 (done) stale-tempfile sweep; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:2325` | `prune_orphan_engine_version_dirs` | task #2978 (done) orphan-engine-version pruning; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-eval` | `crates/reify-eval/src/trajectory_ops.rs:53` | `worst_case_residual_fraction` | trajectory robustness metric seam (worst_case_residual_fraction), task #3869 (θ/ι — simulate_trajectory, done) + #3870 (κ — TOTS, done); wired pipeline entry points are in trampoline.rs; helper is 0-external-caller by design. |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:2090` | `capability_kind` | task #3623 (done) QueryCapability enum mapping; consumer is the capability-dispatch arm in subsequent #3623 (done) steps |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:2470` | `write_stl_ascii` | library API: STL ASCII serializer; no CLI/GUI consumer wired yet |
| `reify-ir` | `crates/reify-ir/src/value.rs:2400` | `format_display_triple` | task #3648 (done) auto-resolve emit feature; consumer is the auto-resolve diagnostic Display in subsequent #3648 (done) steps |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:229` | `gmshModelMeshSetSize` | same-file consumer `mesh_set_size_at_entity` → refine_volume.rs:262 (G-tool same-file-caller heuristic limitation). |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:4275` | `store_vertex_at_for_test` | task #3535 (done) vertex_point FFI test fixture; permanent integration-test support only (no production consumer intended) |
| `reify-lsp` | `crates/reify-lsp/src/analysis.rs:429` | `count_members_recursive` | same-file caller only; audit counts cross-file refs |
| `reify-lsp` | `crates/reify-lsp/src/analysis.rs:482` | `compute_document_symbols` | LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant |
| `reify-lsp` | `crates/reify-lsp/src/completion.rs:32` | `determine_context` | same-file caller only; audit counts cross-file refs |
| `reify-lsp` | `crates/reify-lsp/src/completion.rs:125` | `compute_completions` | LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant |
| `reify-lsp` | `crates/reify-lsp/src/goto_def.rs:14` | `compute_goto_definition` | LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant |
| `reify-lsp` | `crates/reify-lsp/src/goto_def.rs:89` | `compute_goto_definition_cross_file` | LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant |
| `reify-lsp` | `crates/reify-lsp/src/hover.rs:10` | `compute_hover` | LSP public API entry point; production caller uses the _in_context/_with_parsed/_from_parsed variant |
| `reify-lsp` | `crates/reify-lsp/src/references.rs:96` | `collect_references` | same-file caller only; audit counts cross-file refs |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/boundary.rs:149` | `compute_dirichlet_bcs` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:167` | `record_morphed` | live wiring owner: task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh, engine_build.rs); debug-RPC snapshot consumer #2949 (done); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:178` | `record_quality_remesh` | live wiring owner: task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh, engine_build.rs); debug-RPC snapshot consumer #2949 (done); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:209` | `record_ineligible` | live wiring owner: task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh, engine_build.rs); debug-RPC snapshot consumer #2949 (done); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:221` | `record_panicked` | live wiring owner: task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh, engine_build.rs); debug-RPC snapshot consumer #2949 (done); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/diagnostics.rs:283` | `reset` | diagnostic-state reset API; no consumer wired yet |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:231` | `elasticity_morph_with_cg_opts` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:444` | `elasticity_morph` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/laplacian.rs:90` | `laplacian_smooth` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/lib.rs:122` | `eligible` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/quality.rs:201` | `quality_check` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh); re-homed from cancelled #3429/#2947 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:54` | `record_morph_attempt` | mesh-morph engine call-site wiring pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh fires these counters); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:63` | `record_remesh` | mesh-morph engine call-site wiring pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh fires these counters); re-homed from cancelled #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:70` | `record_rejection` | mesh-morph engine call-site wiring pending task #4744 (volume-mesh-realization-and-morph-wiring §8 task β — morph arm in dispatch_volume_mesh fires these counters); re-homed from cancelled #3429 |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:262` | `default_overrides` | same-file caller only; audit counts cross-file refs |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:516` | `default_populate_priorities` | same-file caller only; audit counts cross-file refs |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/global.rs:206` | `detect_orphan_dofs` | task #3293 (done) orphan-DOF detector; cfg(debug_assertions) emit consumer + detector/assembler-consistency pin (task #3293, done) |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/degenerate_shell.rs:125` | `degenerate_position` | degenerate-shell (MITC3+) position interpolation, tasks (#4068/#4069, both done); reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/degenerate_shell.rs:200` | `mat3_inverse` | degenerate-shell (MITC3+) Jacobian-inverse helper, tasks (#4068/#4069, both done); reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/degenerate_shell.rs:970` | `directors_from_facets` | degenerate-shell (MITC3+) default director source, tasks (#4068/#4069, both done); reached via shell_element_stiffness_degenerate on the compute-target-wired shell-routing path (fn-pointer registration the orphan audit cannot trace); exercised by element unit tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/prestress_stability.rs:126` | `analyze_prestress_stability` | Tensegrity T2 stability API (analyze_prestress_stability), task #3796 (T2, provenance, done); Type-A: re-exported but consumed only by tests; live DSL/production consumer: tensegrity-membrane batch (#4412–#4419, all done — integration gate #4419 landed). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/prestress_stability.rs:277` | `count_self_stress_states` | Tensegrity T2 stability API (count_self_stress_states), task #3796 (T2, provenance, done); Type-A: consumed only by tests; live DSL/production consumer: tensegrity-membrane batch (#4412–#4419, all done — integration gate #4419 landed). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/prestress_stability.rs:327` | `assemble_equilibrium_matrix` | Tensegrity T2 stability API (assemble_equilibrium_matrix), task #3796 (T2, provenance, done); Type-A: consumed only by tests; live DSL/production consumer: tensegrity-membrane batch (#4412–#4419, all done — integration gate #4419 landed). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/prestress_stability.rs:490` | `extract_internal_mechanisms` | Tensegrity T2 stability API (extract_internal_mechanisms), task #3796 (T2, provenance, done); Type-A: consumed only by tests; live DSL/production consumer: tensegrity-membrane batch (#4412–#4419, all done — integration gate #4419 landed). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/prestress_stability.rs:541` | `assemble_geometric_stiffness` | Tensegrity T2 stability API (assemble_geometric_stiffness), task #3796 (T2, provenance, done); Type-A: consumed only by tests; live DSL/production consumer: tensegrity-membrane batch (#4412–#4419, all done — integration gate #4419 landed). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:1233` | `shell_element_stiffness_degenerate` | degenerate-shell element-stiffness entry point, task #4068 (displacement-based substrate, done); reached on the shell-routing compute path via fn-pointer registration the orphan audit cannot trace; guarded by degenerate-stiffness patch/rigid-body/symmetry tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:1254` | `shell_element_stiffness_degenerate_ans` | degenerate-shell element-stiffness entry point, task #4069 (ANS membrane variant of #4068, done); reached on the shell-routing compute path via fn-pointer registration the orphan audit cannot trace; guarded by degenerate-stiffness acceptance tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:1279` | `shell_element_stiffness_degenerate_ans_bubble` | degenerate-shell element-stiffness entry point, task #4065 (ANS membrane + rotation bubble, done); reached on the shell-routing compute path via fn-pointer registration the orphan audit cannot trace; guarded by bubble coupling and benchmark tests. |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/solver.rs:359` | `solve_cg_warm` | same-file caller: cold-start `solve_cg` delegate (solver.rs); audit counts only cross-file callers |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:125` | `project_per_element_sizes_to_vertices` | same-file consumer `refine_with_size_field` (G-tool same-file-caller heuristic limitation). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:181` | `refine_with_size_field` | producer for pending task #2997 (a-posteriori-error-estimation PRD #2: adaptive refinement loop). |
| `reify-stdlib` | `crates/reify-stdlib/src/dfm.rs:392` | `diagnose` | consumed via `dfm_diagnose` pub-use re-export alias (reify-expr/src/lib.rs); audit cannot trace renamed re-exports |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/mass_props.rs:112` | `uniform_box_inertia` | test-only analytic ground-truth closed form; KGQ wiring into body_mass_props landed via #3829 (done) + #4237 dynamics_ops seam (done); fn is permanent test-only helper, zero production callers by design |
| `reify-stdlib` | `crates/reify-stdlib/src/tolerancing.rs:197` | `diagnose` | exposed as `tolerancing_diagnose` re-export alias; consumer wiring tracked by a separate review task |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/impulse_shaper.rs:279` | `amplitude_sum` | impulse-shaping well-formedness helper (amplitude-sum check), task #3866 (ε, DONE); permanent internal helper called only within impulse_shaper.rs + unit tests; input_shape_value entry point is wired via `input_shape_trampoline` in trajectory_ops.rs. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/sampling.rs:73` | `resample_cubic` | profile→MotionTrajectory sampling bridge (resample: samples→clamped cubic spline), task #3855 (γ, DONE); permanent internal helper called only within sampling.rs; simulate_trajectory_value entry point is wired via `simulate_trajectory_trampoline` in trajectory_ops.rs. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/simulate.rs:62` | `modal_aware_dt` | simulate_trajectory forward-pass helper (modal-aware dt), task #3869 (θ — simulate_trajectory, DONE); entry point simulate_trajectory_value is wired via `simulate_trajectory_trampoline` in trajectory_ops.rs; helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/simulate.rs:203` | `nominal_fk_chain` | simulate_trajectory forward-pass helper (nominal FK chain), task #3869 (θ — simulate_trajectory, DONE); entry point simulate_trajectory_value is wired via `simulate_trajectory_trampoline` in trajectory_ops.rs; helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/simulate.rs:324` | `superpose_modes` | simulate_trajectory forward-pass helper (modal superposition), task #3869 (θ — simulate_trajectory, DONE); entry point simulate_trajectory_value is wired via `simulate_trajectory_trampoline` in trajectory_ops.rs; helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/simulate.rs:361` | `forces_to_forcing_history` | simulate_trajectory forward-pass helper (forces → modal forcing history), task #3869 (θ — simulate_trajectory, DONE); entry point simulate_trajectory_value is wired via `simulate_trajectory_trampoline` in trajectory_ops.rs; helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:82` | `n_vars` | TOTS SQP optimizer internal helper (n_vars), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:88` | `variable_vector` | TOTS SQP optimizer internal helper (variable_vector), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:102` | `unpack_variable_vector` | TOTS SQP optimizer internal helper (unpack_variable_vector), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:134` | `build_spline` | TOTS SQP optimizer internal helper (build_spline), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:261` | `constraint_violations` | TOTS SQP optimizer internal helper (constraint_violations), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:279` | `is_feasible` | TOTS SQP optimizer internal helper (is_feasible), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:285` | `max_violation` | TOTS SQP optimizer internal helper (max_violation), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:296` | `objective_gradient` | TOTS SQP optimizer internal helper (objective_gradient), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:312` | `constraint_jacobian` | TOTS SQP optimizer internal helper (constraint_jacobian), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:347` | `bfgs_update` | TOTS SQP optimizer internal helper (bfgs_update), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:425` | `solve_qp_step` | TOTS SQP optimizer internal helper (solve_qp_step), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:481` | `merit` | TOTS SQP optimizer internal helper (merit), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:500` | `line_search` | TOTS SQP optimizer internal helper (line_search), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/tots.rs:556` | `code_str` | TOTS SQP optimizer internal helper (code_str), task #3870 (κ — TOTS SQP, DONE); entry point solve_tots is wired (trampoline.rs → trajectory_ops.rs); helper is 0-external-caller by design. |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/trampoline.rs:196` | `value_to_multijoint_spline` | same-file caller only; audit counts cross-file refs |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/trampoline.rs:392` | `value_to_modal_model` | same-file caller only; audit counts cross-file refs |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/trampoline.rs:486` | `value_to_mechanism_model` | same-file caller only; audit counts cross-file refs |

---

Generated by `scripts/audit-orphan-producers.sh`.
Design: `docs/architecture-audit/g-reviewer-tool-session-prompt.md`.
