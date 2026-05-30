# G-tool baseline report

**Captured:** 2026-05-30
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
| C-43 | warm-state-pool `drain_events` no engine caller | 1 / 1 |

26/26 known producer-orphan signals flagged. (C-25 `build_doc_model`
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

> **Important:** this report has a hand-written preamble (lines 1–74,
> through the `---` separator before `# Orphan-producer audit`).  A plain
> redirect would overwrite it.  Use the splice procedure below.

```bash
# Step 1 — capture fresh script output to a temp file
./scripts/audit-orphan-producers.sh --format markdown > /tmp/orphan-body.md

# Step 2 — preserve the preamble, bump **Captured:** date, replace body
TODAY=$(date +%Y-%m-%d)
head -n 74 docs/architecture-audit/g-tool-baseline-report.md \
    | sed "3s/\*\*Captured:\*\* .*/**Captured:** ${TODAY}/" \
    > /tmp/orphan-preamble.md
cat /tmp/orphan-preamble.md /tmp/orphan-body.md \
    > docs/architecture-audit/g-tool-baseline-report.md

# Orphan-producer audit (Portfolio approach G)

Public functions in `crates/reify-*/src/` whose only callers are
tests, the defining file itself, comments, or `use`/`pub use`
re-exports.

- **Scanned:** 1570 `pub fn` declarations across 345 files
- **Orphan candidates:** 452  (zero non-test callers, no `// G-allow:`)
- **Allow-listed:** 36  (zero callers; marked legitimate API surface)

## Orphan candidates

| Crate | File:Line | Function |
|---|---|---|
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:172` | `emit_rpath_for_bins` |
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:197` | `emit_rpath_for_tests` |
| `reify-build-utils` | `crates/reify-build-utils/src/lib.rs:218` | `read_soname_version` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:107` | `is_known_block_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:112` | `is_module_only_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:565` | `enumerate_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:759` | `filter_feasible_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:889` | `select_candidate` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1931` | `build_constraint_blame_map` |
| `reify-compiler` | `crates/reify-compiler/src/compile_builder/defs_phase.rs:34` | `format_shadow_warning` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/checker.rs:33` | `resolve_let_advertised_type` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:302` | `emit_geometry_unbounded` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:331` | `emit_geometry_trait_violation` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:69` | `auto_match_port_members` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:133` | `is_forward_compatible` |
| `reify-compiler` | `crates/reify-compiler/src/functions.rs:241` | `resolve_field_type_name` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:331` | `try_hoist_geometry_conditional` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1301` | `extract_collection_count` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1326` | `unsupported_geometry_fn_message` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_modify.rs:8` | `compile_modify_2arg` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:134` | `bounded_only` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:145` | `bounded_connected` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:177` | `infer_primitive` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:193` | `combine_union` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:206` | `combine_difference` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:221` | `combine_intersection` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:235` | `combine_transform` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:245` | `combine_modify` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:258` | `combine_pattern` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:273` | `combine_sweep` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:343` | `infer_traits_for_expr` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:526` | `try_infer_traits_for_function_call` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:540` | `try_infer_traits_for_function_call_in_env` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:9` | `collect_body_refs_inner` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:351` | `compile_guarded_members` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:685` | `narrow_arms_under_guard` |
| `reify-compiler` | `crates/reify-compiler/src/ice.rs:16` | `as_phrase` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:262` | `compile_with_prelude_context` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:275` | `compile_module` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:618` | `compile_project_with_entry_source` |
| `reify-compiler` | `crates/reify-compiler/src/si_units.rs:61` | `includes` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:123` | `termination_args_contain_undef` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:142` | `termination_collect_refs` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:161` | `termination_is_modifying` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:134` | `is_skipped_parametric_prelude` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:150` | `should_emit_skipped_parametric_prelude_info` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:235` | `resolve_dimension_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:294` | `evaluate_const_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:556` | `resolve_type_with_params` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:689` | `resolve_type_alias_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:807` | `resolve_type_alias_expr_to_dimension` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:971` | `resolve_parameterized_alias` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1064` | `resolve_type_alias_expr_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1238` | `resolve_parameterized_builtin_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1434` | `resolve_parameterized_builtin_type_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1571` | `resolve_type_alias_expr_to_dim_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1644` | `collect_type_expr_names` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:308` | `test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:315` | `non_test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:324` | `test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:329` | `non_test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:336` | `test_functions` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:341` | `non_test_functions` |
| `reify-compiler` | `crates/reify-compiler/src/units.rs:592` | `resolve_unit_expr` |
| `reify-config` | `crates/reify-config/src/cache.rs:41` | `default_cache_dir` |
| `reify-config` | `crates/reify-config/src/cache.rs:77` | `parse_cache_config` |
| `reify-config` | `crates/reify-config/src/cache.rs:256` | `load_cache_config_from_path` |
| `reify-config` | `crates/reify-config/src/lib.rs:146` | `from_toml_str` |
| `reify-config` | `crates/reify-config/src/lib.rs:206` | `load_from_path` |
| `reify-config` | `crates/reify-config/src/lib.rs:212` | `kernel_pins` |
| `reify-config` | `crates/reify-config/src/lib.rs:224` | `auto_type_params` |
| `reify-constraints` | `crates/reify-constraints/src/registry.rs:43` | `with_solvers` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:200` | `Slvs_QuaternionU` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:210` | `Slvs_QuaternionV` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:220` | `Slvs_QuaternionN` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:230` | `Slvs_MakeQuaternion` |
| `reify-core` | `crates/reify-core/src/dimension.rs:36` | `is_zero` |
| `reify-core` | `crates/reify-core/src/dimension.rs:40` | `is_integer` |
| `reify-core` | `crates/reify-core/src/dimension.rs:44` | `as_i8` |
| `reify-core` | `crates/reify-core/src/source_location.rs:26` | `build_line_offsets` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:327` | `is_fresh` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:338` | `bump_version` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:395` | `record_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:406` | `get_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:432` | `imported_file_hash_changed` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:624` | `get_dirty_reasons` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1130` | `pending_transition_count` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1243` | `derive_output_freshness_from_trace` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1270` | `derive_output_freshness_for_node` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1400` | `insert_synthetic_realization_entry` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1436` | `derive_output_freshness` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1496` | `derive_output_freshness_with_cause` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1561` | `compute_input_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1589` | `check_early_cutoff` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1607` | `dirty_set` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/elastic_static.rs:252` | `solve_cantilever_fea` |
| `reify-eval` | `crates/reify-eval/src/compute_targets/mod.rs:29` | `register_compute_fns` |
| `reify-eval` | `crates/reify-eval/src/demand.rs:55` | `cone_size` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:103` | `add_realization` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:126` | `build_from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:252` | `geometry_cell_realization_links` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:295` | `build_trace_map` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1176` | `extract_value_deps` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1333` | `from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1374` | `deps_of` |
| `reify-eval` | `crates/reify-eval/src/dirty.rs:95` | `compute_dirty_cone_with_realizations` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:129` | `is_long_chain_realization` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:183` | `long_chain_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:460` | `long_chain_threshold_from_env` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:477` | `long_chain_threshold_from_env_value` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:184` | `with_prelude` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:597` | `with_registered_kernels` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:634` | `registered_kernel_names` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:645` | `kernel_count` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:687` | `register_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:700` | `unregister_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:707` | `optimized_targets` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:788` | `dispatch_compute_node` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:856` | `set_max_unfold_depth` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:873` | `set_max_unfold_nodes` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:879` | `with_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:896` | `register_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:903` | `unregister_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:911` | `registered_solvers` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:992` | `cache_store` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1026` | `snapshot_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1228` | `geometry_revalidation_slow_path_count` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1315` | `propagate_freshness_only` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1361` | `warm_pool_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1374` | `cache_store_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1395` | `set_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1409` | `remove_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1422` | `clear_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1023` | `build_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1682` | `tessellate_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1854` | `compute_realization_tolerance_budget` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1907` | `budget_available_set` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1936` | `compute_demanded_tols` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1982` | `compute_tessellation_budgets` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:3607` | `tessellate_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:7310` | `p2_substitution_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:26` | `dispatch_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:210` | `labeled_diagnostics` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:405` | `collect_active_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:56` | `deactivate_if_not_auto` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:110` | `rewrite_port_placeholder` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:527` | `diff_value_cells` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:545` | `diff_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:563` | `diff_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:1965` | `edit_source` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:3342` | `edit_check` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:61` | `is_representable_cell_type` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:669` | `hash_imported_file_content` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:3315` | `read_value_revalidated` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:3499` | `revalidate_geometry_handle` |
| `reify-eval` | `crates/reify-eval/src/engine_hash_algo.rs:222` | `compose_engine_version_hash` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:164` | `activate_purpose_constraints_with_bindings_inner` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:417` | `deactivate_purpose` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:474` | `active_objectives` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:19` | `imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:69` | `check_imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:126` | `active_tolerance_for` |
| `reify-eval` | `crates/reify-eval/src/field_import_provenance.rs:66` | `build_field_import_provenance` |
| `reify-eval` | `crates/reify-eval/src/gating.rs:102` | `unblocked_gated_nodes` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:157` | `eval_named_arg` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:191` | `eval_named_arg_f64` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:226` | `eval_all_args_to_f64` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:604` | `get_compute_node` |
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
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:345` | `warn_if_duplicate_op_repr_pairs` |
| `reify-eval` | `crates/reify-eval/src/multi_load_dispatch.rs:30` | `detect_multi_case_result` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:64` | `read_sidecar_mtime` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:98` | `touch_sidecar` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:283` | `write_to` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1009` | `write_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1108` | `read_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1586` | `eviction_score` |
| `reify-eval` | `crates/reify-eval/src/primitive_attribute_seed.rs:217` | `seed_primitive_attributes` |
| `reify-eval` | `crates/reify-eval/src/realization_cache.rs:186` | `bucket_len` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:100` | `as_byte` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:144` | `intersect` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:182` | `complement` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:204` | `except` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:242` | `faces_perpendicular_to` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:302` | `edges_perpendicular_to` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:382` | `extremal_by_bbox` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:477` | `extremal_by_centroid` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:601` | `faces_by_surface_kind` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:634` | `edges_by_curve_kind` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:663` | `geom_universal` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:697` | `created_by_feature` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:730` | `split_by_feature` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:771` | `has_user_label` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:796` | `user_label_eq` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:1005` | `siblings_of_face` |
| `reify-eval` | `crates/reify-eval/src/shell_extract_compute.rs:347` | `shell_extract_compute_fn` |
| `reify-eval` | `crates/reify-eval/src/shell_extract_compute.rs:518` | `register_shell_extract_compute_fns` |
| `reify-eval` | `crates/reify-eval/src/significance_filter.rs:75` | `is_opted_in` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:31` | `find_parsed_decl_containing_offset` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:129` | `resolve_entity_at_source_position` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:75` | `realization_graph_shape_hash` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:96` | `classify_cell` |
| `reify-eval` | `crates/reify-eval/src/topology_attribute_propagation.rs:111` | `propagate_attributes_via_brepalgoapi_history` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:228` | `edges_by_length_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:307` | `faces_by_area_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:359` | `parse_xyz_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:391` | `parse_flat_number_object` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:629` | `edges_parallel_to_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:728` | `edges_at_height_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:778` | `resolve_unique_by_tag` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:822` | `parse_bbox_z_extents` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:841` | `parse_bbox_z_extents_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:891` | `parse_bbox_axis_extents_json` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:136` | `with_budget` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:150` | `unlimited` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:184` | `from_env_value` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:220` | `with_test_events_cap` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:483` | `used_bytes` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:493` | `budget_bytes` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:515` | `dropped_events` |
| `reify-expr` | `crates/reify-expr/src/lib.rs:79` | `_test_at_depth` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:117` | `gibson_ashby_infill_factor` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:180` | `pattern_factors` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:310` | `effective_transverse_isotropic` |
| `reify-fdm` | `crates/reify-fdm/src/correlation.rs:390` | `effective_orthotropic` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:96` | `is_top_or_bottom_normal` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:143` | `min_top_bottom_distance` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:158` | `min_side_distance` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:175` | `build_zone_probe` |
| `reify-fdm` | `crates/reify-fdm/src/zone.rs:225` | `classify_zone` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:36` | `register_kernel` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:41` | `has_kernel` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:324` | `no_defaults_for` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:1566` | `user_function_call` |
| `reify-ir` | `crates/reify-ir/src/expr.rs:1636` | `match_expr` |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:2523` | `try_nary` |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:2547` | `nary` |
| `reify-ir` | `crates/reify-ir/src/node_traits.rs:334` | `set_instance` |
| `reify-ir` | `crates/reify-ir/src/node_traits.rs:339` | `set_type` |
| `reify-ir` | `crates/reify-ir/src/persistent.rs:45` | `insert_functional` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:79` | `id_for` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:84` | `name_for` |
| `reify-ir` | `crates/reify-ir/src/structure_registry.rs:94` | `declared_bounds` |
| `reify-ir` | `crates/reify-ir/src/value.rs:724` | `try_into_matrix` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1171` | `infer_type` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1286` | `try_infer_type` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1566` | `format_display` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1733` | `format_display_pair` |
| `reify-ir` | `crates/reify-ir/src/value.rs:1807` | `format_display_number` |
| `reify-ir` | `crates/reify-ir/src/value.rs:2740` | `has_hash` |
| `reify-ir` | `crates/reify-ir/src/warm_registry.rs:62` | `kinds` |
| `reify-ir` | `crates/reify-ir/src/warm_registry.rs:74` | `from_inventory` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/kernel.rs:187` | `evaluate_sdf_at` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/register.rs:102` | `fidget_capability_descriptor` |
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
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_boundary.rs:210` | `mesh_surface_to_volume_with_attribution` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:64` | `apply_repair_if_requested` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/register.rs:92` | `gmsh_capability_descriptor` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:58` | `manifold_factory` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:100` | `manifold_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:470` | `extrude_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:512` | `revolve_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:558` | `sweep_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:601` | `loft_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:634` | `make_rect_profile_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:666` | `make_rect_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:699` | `make_triangle_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:745` | `face_outward_unit_normal_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1008` | `execute_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1022` | `query_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1040` | `export_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1059` | `tessellate_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1078` | `extract_edges_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1092` | `extract_faces_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1106` | `extract_vertices_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1128` | `warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1148` | `with_warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:562` | `repr_of` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2780` | `warm_start_failures` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2830` | `store_circle_face_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2914` | `store_nonmanifold_compound_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2926` | `store_malformed_solid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2939` | `store_nonorientable_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2952` | `store_closed_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2964` | `store_edge_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2975` | `store_vertex_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2999` | `store_compsolid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:3019` | `store_placed_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:89` | `occt_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:172` | `occt_factory` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:294` | `lower_to_sampled` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:489` | `validate_grid_units` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:540` | `read_vdb_file` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:575` | `read_vdb_file` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:79` | `realize_voxel_from_mesh` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:106` | `realize_voxel_from_mesh_with_options` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:153` | `active_voxel_count` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:170` | `sample_sdf_at` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:222` | `write_vdb_grid` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:258` | `open_vdb_grid_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:293` | `grid_name_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/register.rs:113` | `openvdb_capability_descriptor` |
| `reify-lsp` | `crates/reify-lsp/src/analysis.rs:312` | `count_members_recursive` |
| `reify-lsp` | `crates/reify-lsp/src/bridge.rs:120` | `handle_request` |
| `reify-lsp` | `crates/reify-lsp/src/completion.rs:31` | `determine_context` |
| `reify-lsp` | `crates/reify-lsp/src/convert.rs:128` | `convert_severity` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:33` | `last_content_hash` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:38` | `is_engine_initialized` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:342` | `compute_diagnostics` |
| `reify-lsp` | `crates/reify-lsp/src/server.rs:393` | `take_calls` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:31` | `handle_message` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:52` | `run_on_streams` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:74` | `set_instance` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:81` | `set_type` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:123` | `progress_estimate` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:162` | `check_commitment` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:274` | `is_committed` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:298` | `task_count` |
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
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:47` | `promote` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:80` | `promote_for_demand` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:141` | `effective_priority` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:149` | `promote` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:178` | `promote_for_demand` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:591` | `world_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:602` | `sample_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:666` | `gradient_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:746` | `precompute_gradient_grid` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:829` | `gradient_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:863` | `bidirectional_distances` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:962` | `surface_patches_distinct` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/hex.rs:29` | `element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/hex.rs:53` | `element_stiffness_hex_p1_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/tet.rs:324` | `tet_p1_centroid` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/tet.rs:380` | `element_stiffness_p2_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/wedge.rs:30` | `element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/wedge.rs:54` | `element_stiffness_wedge_p1_with_field` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:242` | `apply_body_force` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:608` | `apply_traction_load` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:322` | `solve_eigen_dense` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:513` | `lanczos_shift_invert` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:205` | `bubble_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:221` | `rotation_shape_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mod.rs:80` | `from_matrix` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/error_estimator.rs:93` | `compute_zz_indicator` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/mod.rs:76` | `uniaxial_z` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:35` | `geometric_element_stiffness_shell` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:60` | `geometric_element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:82` | `geometric_element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:51` | `barycentric_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:112` | `point_in_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:144` | `interpolate_p1_at_point` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:189` | `locate_element_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mass_matrix.rs:64` | `consistent_element_mass_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:169` | `compute_quad_skew` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:207` | `recombine_quality_ok` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:259` | `auto_mesh_size_from_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:387` | `mesh_swept_profile_2d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mpc.rs:146` | `apply_mpc_row_elimination` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mpc.rs:411` | `shell_tet_tying` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:71` | `refinement_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:84` | `coarse_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:160` | `near_constraint_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:235` | `should_refine` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:357` | `shell_element_stiffness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:549` | `shell_element_stiffness_mitc3_plus` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_boundary.rs:107` | `build_support_bcs` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_result.rs:41` | `shell_element_frame` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_result.rs:109` | `shell_element_stress` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/solver.rs:312` | `solve_cg_with_progress` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:78` | `derive_layer_count` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:107` | `check_sweep_through_thickness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:359` | `sweep_2d_mesh_to_3d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:40` | `from_displacement` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:48` | `from_arc` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:72` | `from_opaque_state` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:38` | `from_array` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:192` | `as_matrix` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:206` | `from_frame3` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:326` | `from_mass_com_inertia` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:359` | `as_matrix` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:386` | `cross_m` |
| `reify-stdlib` | `crates/reify-stdlib/src/dynamics/spatial.rs:414` | `cross_f` |
| `reify-stdlib` | `crates/reify-stdlib/src/joints.rs:943` | `motion_subspace_columns` |
| `reify-stdlib` | `crates/reify-stdlib/src/loads.rs:58` | `is_load_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:77` | `chain_transform` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:270` | `per_joint_jacobian_local` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:411` | `value_for_joint` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:227` | `is_singular` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:384` | `newton_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:412` | `newton_solve_with_projection` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:922` | `mechanism_loop_closure_chains` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:1089` | `solve_loop_closure_with_diagnostics` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup.rs:280` | `diagnose` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup/rng.rs:156` | `next_uniform_f64` |
| `reify-stdlib` | `crates/reify-stdlib/src/stackup/rng.rs:164` | `next_u64` |
| `reify-stdlib` | `crates/reify-stdlib/src/supports.rs:77` | `is_support_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/gcode_import.rs:197` | `lower_gcode` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:350` | `eval_dot` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:357` | `eval_ddot` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:544` | `eval_dot` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:553` | `eval_ddot` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:581` | `new_cubic` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:600` | `new_quintic` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:623` | `eval_dot` |
| `reify-stdlib` | `crates/reify-stdlib/src/trajectory/spline.rs:633` | `eval_ddot` |

## Allow-listed (zero callers, intentional)

| Crate | File:Line | Function | Reason |
|---|---|---|---|
| `reify-audit` | `crates/reify-audit/src/lib.rs:515` | `set_log_grep` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:521` | `set_diff_changed_paths` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:527` | `set_is_gitignored` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:532` | `set_diff_added_lines` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:659` | `set_changed_symbols` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:670` | `set_find_references` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-compiler` | `crates/reify-compiler/src/annotations/schema.rs:221` | `lookup_schema` | task #3530 const-slice/OnceLock AnnotationSchema registry; consumer is the schema-delegating validate_annotations rewrite (task #3530 step-10) |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:98` | `__validate_annotations_for_parity_test` | task #3530 parity shim — test-support-gated (feature = "test-support"), consumed by validate_annotations parity tests during schema-delegation migration; remove when delegation is complete |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:312` | `kernel_pragma_unsatisfiable_diagnostic` | task #3443 #kernel(...) pragma diagnostic builder; consumer wiring lands in subsequent #3443 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:353` | `pinned_kernel_missing_diagnostic` | task #3444 reify.toml [kernels] pinned-missing diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:390` | `unpinned_kernel_loaded_diagnostic` | task #3444 unpinned-kernel-loaded diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:429` | `kernel_version_mismatch_diagnostic` | task #3444 kernel-version-mismatch diagnostic builder; consumer wiring lands in subsequent #3444 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1449` | `drain_and_record_warm_pool_events` | task #3541 eval-boundary warm-pool→journal drain; consumer EngineSession::drain_and_emit_warm_pool_events (engine.rs) wiring lands in subsequent #3541 steps |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:3813` | `dispatch_volume_mesh` | §3.2 realization-kind dispatch seam (VolumeMesh) per engine-integration-norm §3.2; consumer pending task #3429 (CN-contract §8 task κ — adds execute_realization_ops call edge) / mesh-morph #2947 |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:3104` | `cap_kind_translation` | task #3463 cap/role vocabulary table; consumer is try_eval_ad_hoc_selector @face/@edge dispatch (same-file, task #3463) + ad_hoc_selector smoke tests |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1645` | `sweep_stale_tempfiles` | task #2978 stale-tempfile sweep; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1773` | `prune_orphan_engine_version_dirs` | task #2978 orphan-engine-version pruning; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-ir` | `crates/reify-ir/src/geometry.rs:1061` | `capability_kind` | task #3623 QueryCapability enum mapping; consumer is the capability-dispatch arm in subsequent #3623 steps |
| `reify-ir` | `crates/reify-ir/src/value.rs:1771` | `format_display_triple` | task #3648 auto-resolve emit feature; consumer is the auto-resolve diagnostic Display in subsequent #3648 steps |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:229` | `gmshModelMeshSetSize` | same-file consumer `mesh_set_size_at_entity` → refine_volume.rs:262 (G-tool same-file-caller heuristic limitation). |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:162` | `mesh_surface_to_volume_with_diagnostics` | §3.2 Gmsh tet-mesher producer per engine-integration-norm §3.2; consumer pending task #3429 (eval-side tet fall-back binding) / mesh-morph #2947 |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2987` | `store_vertex_at_for_test` | task #3535 vertex_point FFI test fixture; consumed by integration tests in subsequent #3535 steps |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/boundary.rs:149` | `compute_dirichlet_bcs` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:231` | `elasticity_morph_with_cg_opts` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:442` | `elasticity_morph` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/laplacian.rs:90` | `laplacian_smooth` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/lib.rs:111` | `eligible` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/quality.rs:201` | `quality_check` | mesh-morph public API — §3.2 realization-kind dispatch producer per engine-integration-norm §3.2; consumer pending task #2947 (mesh-morph VolumeMesh realization wiring) / CN-contract §8 task κ #3429 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:54` | `record_morph_attempt` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:63` | `record_remesh` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:70` | `record_rejection` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/global.rs:206` | `detect_orphan_dofs` | task #3293 orphan-DOF detector; cfg(debug_assertions) emit consumer + detector/assembler-consistency pin (task #3293) |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:125` | `project_per_element_sizes_to_vertices` | same-file consumer `refine_with_size_field` (G-tool same-file-caller heuristic limitation). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:181` | `refine_with_size_field` | producer for pending task #2997 (a-posteriori-error-estimation PRD #2: adaptive refinement loop). |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_value.rs:163` | `renormalize_quaternion` | KCC-α task #3764 / KCC-γ #3765 Newton-step unit-quaternion manifold projection (PRD §5.3), consumed by the widened solver path. |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_value.rs:232` | `flatten_dofs` | KCC-γ task #3765 widens chain_transform to consume &flatten_dofs(&[JointValue]) at the chain boundary (PRD §5.1). |

---

Generated by `scripts/audit-orphan-producers.sh`.
Design: `docs/architecture-audit/g-reviewer-tool-session-prompt.md`.
