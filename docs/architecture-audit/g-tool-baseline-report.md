# G-tool baseline report

**Captured:** 2026-05-12
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

```
./scripts/audit-orphan-producers.sh --format markdown > docs/architecture-audit/g-tool-baseline-report.md
```

Or invoke via the cargo alias:

```
cargo audit-orphans
```

The tool runs in <1s against the full corpus. Diff the regenerated
report against this baseline to spot orphan accumulation or
remediation.

---

# Orphan-producer audit (Portfolio approach G)

Public functions in `crates/reify-*/src/` whose only callers are
tests, the defining file itself, comments, or `use`/`pub use`
re-exports.

- **Scanned:** 1425 `pub fn` declarations across 305 files
- **Orphan candidates:** 423  (zero non-test callers, no `// G-allow:`)
- **Allow-listed:** 28  (zero callers; marked legitimate API surface)

## Orphan candidates

| Crate | File:Line | Function |
|---|---|---|
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:93` | `is_known_block_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:98` | `is_module_only_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:556` | `enumerate_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:750` | `filter_feasible_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:880` | `select_candidate` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1298` | `resolve_auto_type_params_with_backtracking` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1922` | `build_constraint_blame_map` |
| `reify-compiler` | `crates/reify-compiler/src/compile_builder/defs_phase.rs:34` | `format_shadow_warning` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/checker.rs:33` | `resolve_let_advertised_type` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:302` | `emit_geometry_unbounded` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:331` | `emit_geometry_trait_violation` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:69` | `auto_match_port_members` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:133` | `is_forward_compatible` |
| `reify-compiler` | `crates/reify-compiler/src/functions.rs:214` | `resolve_field_type_name` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1045` | `extract_collection_count` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1070` | `unsupported_geometry_fn_message` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_modify.rs:8` | `compile_modify_2arg` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:133` | `bounded_only` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:144` | `bounded_connected` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:176` | `infer_primitive` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:192` | `combine_union` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:205` | `combine_difference` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:220` | `combine_intersection` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:234` | `combine_transform` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:244` | `combine_modify` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:257` | `combine_pattern` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:272` | `combine_sweep` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:342` | `infer_traits_for_expr` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:525` | `try_infer_traits_for_function_call` |
| `reify-compiler` | `crates/reify-compiler/src/geometry_traits_inference.rs:539` | `try_infer_traits_for_function_call_in_env` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:9` | `collect_body_refs_inner` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:349` | `compile_guarded_members` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:691` | `narrow_arms_under_guard` |
| `reify-compiler` | `crates/reify-compiler/src/ice.rs:16` | `as_phrase` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:268` | `compile_with_prelude_context` |
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
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:537` | `resolve_type_with_params` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:670` | `resolve_type_alias_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:788` | `resolve_type_alias_expr_to_dimension` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:952` | `resolve_parameterized_alias` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1045` | `resolve_type_alias_expr_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1219` | `resolve_parameterized_builtin_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1415` | `resolve_parameterized_builtin_type_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1552` | `resolve_type_alias_expr_to_dim_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1625` | `collect_type_expr_names` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:308` | `test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:315` | `non_test_templates` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:324` | `test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:329` | `non_test_constraint_defs` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:336` | `test_functions` |
| `reify-compiler` | `crates/reify-compiler/src/types.rs:341` | `non_test_functions` |
| `reify-config` | `crates/reify-config/src/cache.rs:41` | `default_cache_dir` |
| `reify-config` | `crates/reify-config/src/cache.rs:77` | `parse_cache_config` |
| `reify-config` | `crates/reify-config/src/cache.rs:256` | `load_cache_config_from_path` |
| `reify-config` | `crates/reify-config/src/lib.rs:146` | `from_toml_str` |
| `reify-config` | `crates/reify-config/src/lib.rs:206` | `load_from_path` |
| `reify-config` | `crates/reify-config/src/lib.rs:212` | `kernel_pins` |
| `reify-config` | `crates/reify-config/src/lib.rs:224` | `auto_type_params` |
| `reify-constraints` | `crates/reify-constraints/src/registry.rs:45` | `with_solvers` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:200` | `Slvs_QuaternionU` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:210` | `Slvs_QuaternionV` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:220` | `Slvs_QuaternionN` |
| `reify-constraints` | `crates/reify-constraints/src/slvs_sys.rs:230` | `Slvs_MakeQuaternion` |
| `reify-doc` | `crates/reify-doc/src/fmt_html.rs:149` | `render_html` |
| `reify-doc-build` | `crates/reify-doc-build/src/cross_refs.rs:22` | `build_cross_refs` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:314` | `is_fresh` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:325` | `bump_version` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:382` | `record_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:393` | `get_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:419` | `imported_file_hash_changed` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:609` | `get_dirty_reasons` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:993` | `pending_transition_count` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1044` | `derive_output_freshness_from_trace` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1071` | `derive_output_freshness_for_node` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1179` | `insert_synthetic_realization_entry` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1215` | `derive_output_freshness` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1275` | `derive_output_freshness_with_cause` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1340` | `compute_input_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1368` | `check_early_cutoff` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1386` | `dirty_set` |
| `reify-eval` | `crates/reify-eval/src/demand.rs:55` | `cone_size` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:94` | `add_realization` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:117` | `build_from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:224` | `build_trace_map` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1023` | `extract_value_deps` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1179` | `from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1220` | `deps_of` |
| `reify-eval` | `crates/reify-eval/src/dirty.rs:95` | `compute_dirty_cone_with_realizations` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:125` | `is_long_chain_realization` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:179` | `long_chain_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:456` | `long_chain_threshold_from_env` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:473` | `long_chain_threshold_from_env_value` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:166` | `with_prelude` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:490` | `register_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:503` | `unregister_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:510` | `optimized_targets` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:659` | `set_max_unfold_depth` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:676` | `set_max_unfold_nodes` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:682` | `with_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:699` | `register_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:706` | `unregister_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:714` | `registered_solvers` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:795` | `cache_store` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1045` | `propagate_freshness_only` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1091` | `warm_pool_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1104` | `cache_store_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1125` | `set_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1139` | `remove_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1152` | `clear_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:651` | `build_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1138` | `tessellate_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1297` | `compute_realization_tolerance_budget` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1350` | `budget_available_set` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1379` | `compute_demanded_tols` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1425` | `compute_tessellation_budgets` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2459` | `tessellate_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2651` | `dispatch_volume_mesh` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:5210` | `p2_substitution_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:29` | `dispatch_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:213` | `labeled_diagnostics` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:408` | `collect_active_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:59` | `deactivate_if_not_auto` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:113` | `rewrite_port_placeholder` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:530` | `diff_value_cells` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:548` | `diff_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:566` | `diff_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:1968` | `edit_source` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:3234` | `edit_check` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:66` | `is_representable_cell_type` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:673` | `hash_imported_file_content` |
| `reify-eval` | `crates/reify-eval/src/engine_hash_algo.rs:222` | `compose_engine_version_hash` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:238` | `deactivate_purpose` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:290` | `is_purpose_active` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:295` | `active_objectives` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:19` | `imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:69` | `check_imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:126` | `active_tolerance_for` |
| `reify-eval` | `crates/reify-eval/src/field_import_provenance.rs:65` | `build_field_import_provenance` |
| `reify-eval` | `crates/reify-eval/src/gating.rs:102` | `unblocked_gated_nodes` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:155` | `eval_named_arg` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:189` | `eval_named_arg_f64` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:224` | `eval_all_args_to_f64` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:578` | `get_compute_node` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:582` | `get_compute_node_mut` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:118` | `all_events` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:123` | `events_in_range` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:135` | `events_since` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:143` | `events_for_node` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:157` | `count_matching` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:162` | `count_donated` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:167` | `count_evicted` |
| `reify-eval` | `crates/reify-eval/src/journal.rs:172` | `latest_for_node` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:133` | `pick_lexmin_kernel` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:211` | `pick_lexmin_brep_kernel_in` |
| `reify-eval` | `crates/reify-eval/src/kernel_registry.rs:345` | `warn_if_duplicate_op_repr_pairs` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:64` | `read_sidecar_mtime` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:98` | `touch_sidecar` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:283` | `write_to` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1047` | `write_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1146` | `read_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1624` | `eviction_score` |
| `reify-eval` | `crates/reify-eval/src/primitive_attribute_seed.rs:220` | `seed_primitive_attributes` |
| `reify-eval` | `crates/reify-eval/src/realization_cache.rs:185` | `bucket_len` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:103` | `as_byte` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:147` | `intersect` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:185` | `complement` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:207` | `except` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:245` | `faces_perpendicular_to` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:305` | `edges_perpendicular_to` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:385` | `extremal_by_bbox` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:480` | `extremal_by_centroid` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:604` | `faces_by_surface_kind` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:637` | `edges_by_curve_kind` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:666` | `geom_universal` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:700` | `created_by_feature` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:733` | `split_by_feature` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:774` | `has_user_label` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:799` | `user_label_eq` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:1008` | `siblings_of_face` |
| `reify-eval` | `crates/reify-eval/src/significance_filter.rs:75` | `is_opted_in` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:45` | `resolve_entity_at_source_position` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:74` | `realization_graph_shape_hash` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:95` | `classify_cell` |
| `reify-eval` | `crates/reify-eval/src/topology_attribute_propagation.rs:114` | `propagate_attributes_via_brepalgoapi_history` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:230` | `edges_by_length_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:309` | `faces_by_area_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:361` | `parse_xyz_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:393` | `parse_flat_number_object` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:631` | `edges_parallel_to_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:730` | `edges_at_height_with_tags` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:780` | `resolve_unique_by_tag` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:824` | `parse_bbox_z_extents` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:843` | `parse_bbox_z_extents_json` |
| `reify-eval` | `crates/reify-eval/src/topology_selectors.rs:893` | `parse_bbox_axis_extents_json` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:136` | `with_budget` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:150` | `unlimited` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:184` | `from_env_value` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:220` | `with_test_events_cap` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:236` | `donate_with_cost` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:357` | `cost_per_byte_of` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:457` | `checkout` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:483` | `used_bytes` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:493` | `budget_bytes` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:515` | `dropped_events` |
| `reify-expr` | `crates/reify-expr/src/lib.rs:82` | `_test_at_depth` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:39` | `register_kernel` |
| `reify-geometry` | `crates/reify-geometry/src/lib.rs:44` | `has_kernel` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/kernel.rs:191` | `evaluate_sdf_at` |
| `reify-kernel-fidget` | `crates/reify-kernel-fidget/src/register.rs:102` | `fidget_capability_descriptor` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/cache_key.rs:45` | `volume_mesh_cache_key` |
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
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:64` | `apply_repair_if_requested` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:91` | `resolve_mesh_size` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:130` | `compute_thickness_warnings` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/mesh_volume.rs:161` | `mesh_surface_to_volume_with_diagnostics` |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/register.rs:92` | `gmsh_capability_descriptor` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/kernel.rs:131` | `store_mesh_for_test` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:58` | `manifold_factory` |
| `reify-kernel-manifold` | `crates/reify-kernel-manifold/src/register.rs:100` | `manifold_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:474` | `extrude_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:516` | `revolve_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:562` | `sweep_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:605` | `loft_with_history` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:638` | `make_rect_profile_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:670` | `make_rect_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:703` | `make_triangle_profile_at_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:749` | `face_outward_unit_normal_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1012` | `execute_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1026` | `query_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1044` | `export_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1063` | `tessellate_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1082` | `extract_edges_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1096` | `extract_faces_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1110` | `extract_vertices_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1132` | `warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/handle.rs:1152` | `with_warm_state_async` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:566` | `repr_of` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2738` | `warm_start_failures` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2788` | `store_circle_face_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2872` | `store_nonmanifold_compound_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2884` | `store_malformed_solid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2897` | `store_nonorientable_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2910` | `store_closed_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2922` | `store_edge_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2933` | `store_vertex_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2957` | `store_compsolid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2977` | `store_placed_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:89` | `occt_capability_descriptor` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:172` | `occt_factory` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:296` | `lower_to_sampled` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:491` | `validate_grid_units` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:542` | `read_vdb_file` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/ingest.rs:577` | `read_vdb_file` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:82` | `realize_voxel_from_mesh` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:102` | `active_voxel_count` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:119` | `sample_sdf_at` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:171` | `write_vdb_grid` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:207` | `open_vdb_grid_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/kernel_real.rs:242` | `grid_name_for_test` |
| `reify-kernel-openvdb` | `crates/reify-kernel-openvdb/src/register.rs:100` | `openvdb_capability_descriptor` |
| `reify-lsp` | `crates/reify-lsp/src/analysis.rs:311` | `count_members_recursive` |
| `reify-lsp` | `crates/reify-lsp/src/bridge.rs:120` | `handle_request` |
| `reify-lsp` | `crates/reify-lsp/src/completion.rs:31` | `determine_context` |
| `reify-lsp` | `crates/reify-lsp/src/convert.rs:128` | `convert_severity` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:35` | `last_content_hash` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:40` | `is_engine_initialized` |
| `reify-lsp` | `crates/reify-lsp/src/diagnostics.rs:344` | `compute_diagnostics` |
| `reify-lsp` | `crates/reify-lsp/src/server.rs:393` | `take_calls` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:31` | `handle_message` |
| `reify-mcp` | `crates/reify-mcp/src/transport.rs:52` | `run_on_streams` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/boundary.rs:69` | `associate` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/boundary.rs:212` | `compute_dirichlet_bcs` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:230` | `elasticity_morph_with_cg_opts` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/elasticity.rs:440` | `elasticity_morph` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/laplacian.rs:89` | `laplacian_smooth` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/lib.rs:110` | `eligible` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/quality.rs:200` | `quality_check` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:74` | `set_instance` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:81` | `set_type` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:123` | `progress_estimate` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:162` | `check_commitment` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:274` | `is_committed` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:298` | `task_count` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:166` | `is_cancelled` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:171` | `child_token` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:260` | `execute_with_config` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:126` | `from_setup` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:199` | `take_results` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:214` | `build_result_shared` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:250` | `into_result` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:333` | `poison_results` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:344` | `poison_values` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:355` | `poison_snapshot_values` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:466` | `edit_param_concurrent` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent_eval.rs:525` | `edit_check_concurrent` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:39` | `effective_priority` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:47` | `promote` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:80` | `promote_for_demand` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:141` | `effective_priority` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:149` | `promote` |
| `reify-runtime` | `crates/reify-runtime/src/priority_promotion.rs:178` | `promote_for_demand` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:286` | `compute_medial_mask` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:591` | `world_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:602` | `sample_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:666` | `gradient_at_index` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:746` | `precompute_gradient_grid` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:829` | `gradient_at_world` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:863` | `bidirectional_distances` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/medial.rs:962` | `surface_patches_distinct` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/mesher.rs:487` | `mesh_mid_surface` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/mid_surface.rs:504` | `extract_mid_surface` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/mid_surface_naming.rs:134` | `populate_mid_surface_attributes` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/pruning.rs:294` | `prune_branches` |
| `reify-shell-extract` | `crates/reify-shell-extract/src/segmentation.rs:242` | `segment_regions` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/hex.rs:28` | `element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/wedge.rs:29` | `element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:96` | `apply_point_load` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:242` | `apply_body_force` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:608` | `apply_traction_load` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:165` | `solve_eigen_dense` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:343` | `solve_eigen_shift_invert` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:120` | `bubble_grad_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:135` | `bubble_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mod.rs:80` | `from_matrix` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/error_estimator.rs:93` | `compute_zz_indicator` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/mod.rs:76` | `uniaxial_z` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:35` | `geometric_element_stiffness_shell` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:60` | `geometric_element_stiffness_hex_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/stubs.rs:82` | `geometric_element_stiffness_wedge_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/geometric_stiffness/tet.rs:74` | `geometric_element_stiffness_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:51` | `barycentric_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:112` | `point_in_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:144` | `interpolate_p1_at_point` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:189` | `locate_element_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:169` | `compute_quad_skew` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:207` | `recombine_quality_ok` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:259` | `auto_mesh_size_from_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mesher.rs:387` | `mesh_swept_profile_2d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mpc.rs:144` | `apply_mpc_row_elimination` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/mpc.rs:421` | `shell_tet_tying` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:71` | `refinement_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:84` | `coarse_pass_tuning` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:160` | `near_constraint_boundary` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/progressive.rs:235` | `should_refine` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/result.rs:69` | `element_stress_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_assembly.rs:194` | `shell_element_stiffness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_boundary.rs:107` | `build_support_bcs` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_result.rs:41` | `shell_element_frame` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_result.rs:109` | `shell_element_stress` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/shell_result.rs:267` | `homogeneous` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:78` | `derive_layer_count` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:107` | `check_sweep_through_thickness` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/sweep.rs:359` | `sweep_2d_mesh_to_3d` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:40` | `from_displacement` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:48` | `from_arc` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:63` | `into_opaque_state` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:72` | `from_opaque_state` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/warm_state.rs:99` | `solve_cg_with_warm_state` |
| `reify-stdlib` | `crates/reify-stdlib/src/loads.rs:51` | `is_load_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:59` | `chain_transform` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:222` | `per_joint_jacobian_local` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:201` | `is_singular` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:339` | `newton_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:653` | `mechanism_loop_closure_chains` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:820` | `solve_loop_closure_with_diagnostics` |
| `reify-stdlib` | `crates/reify-stdlib/src/supports.rs:76` | `is_support_value` |
| `reify-types` | `crates/reify-types/src/diagnostics.rs:80` | `is_prelude` |
| `reify-types` | `crates/reify-types/src/dimension.rs:36` | `is_zero` |
| `reify-types` | `crates/reify-types/src/dimension.rs:40` | `is_integer` |
| `reify-types` | `crates/reify-types/src/dimension.rs:44` | `as_i8` |
| `reify-types` | `crates/reify-types/src/expr.rs:1538` | `user_function_call` |
| `reify-types` | `crates/reify-types/src/expr.rs:1608` | `match_expr` |
| `reify-types` | `crates/reify-types/src/geometry.rs:2480` | `try_nary` |
| `reify-types` | `crates/reify-types/src/geometry.rs:2504` | `nary` |
| `reify-types` | `crates/reify-types/src/node_traits.rs:334` | `set_instance` |
| `reify-types` | `crates/reify-types/src/node_traits.rs:339` | `set_type` |
| `reify-types` | `crates/reify-types/src/persistent.rs:45` | `insert_functional` |
| `reify-types` | `crates/reify-types/src/source_location.rs:26` | `build_line_offsets` |
| `reify-types` | `crates/reify-types/src/structure_registry.rs:79` | `id_for` |
| `reify-types` | `crates/reify-types/src/structure_registry.rs:84` | `name_for` |
| `reify-types` | `crates/reify-types/src/structure_registry.rs:94` | `declared_bounds` |
| `reify-types` | `crates/reify-types/src/value.rs:701` | `try_into_matrix` |
| `reify-types` | `crates/reify-types/src/value.rs:1114` | `infer_type` |
| `reify-types` | `crates/reify-types/src/value.rs:1229` | `try_infer_type` |
| `reify-types` | `crates/reify-types/src/value.rs:1494` | `format_display` |
| `reify-types` | `crates/reify-types/src/value.rs:1649` | `format_display_pair` |
| `reify-types` | `crates/reify-types/src/value.rs:1723` | `format_display_number` |
| `reify-types` | `crates/reify-types/src/value.rs:2568` | `has_hash` |
| `reify-types` | `crates/reify-types/src/warm_registry.rs:62` | `kinds` |
| `reify-types` | `crates/reify-types/src/warm_registry.rs:74` | `from_inventory` |

## Allow-listed (zero callers, intentional)

| Crate | File:Line | Function | Reason |
|---|---|---|---|
| `reify-audit` | `crates/reify-audit/src/lib.rs:515` | `set_log_grep` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:521` | `set_diff_changed_paths` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:527` | `set_is_gitignored` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:532` | `set_diff_added_lines` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:659` | `set_changed_symbols` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-audit` | `crates/reify-audit/src/lib.rs:670` | `set_find_references` | test-support fixture (feature = "test-support"); not consumed in production builds |
| `reify-compiler` | `crates/reify-compiler/src/annotations/schema.rs:220` | `lookup_schema` | task #3530 const-slice/OnceLock AnnotationSchema registry; consumer is the schema-delegating validate_annotations rewrite (task #3530 step-10) |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:104` | `__validate_annotations_for_parity_test` | task #3530 parity shim — consumed by validate_annotations parity tests during the schema-delegation migration; remove when delegation is complete |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:258` | `no_kernel_chain_diagnostic` | task #3434 no-kernel-chain diagnostic builder; in-tree consumer wiring follows the long_chain_diagnostic precedent |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:308` | `kernel_pragma_unsatisfiable_diagnostic` | task #3434 #kernel(...) pragma diagnostic builder; consumer wiring lands in subsequent #3434 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:349` | `pinned_kernel_missing_diagnostic` | task #3434 reify.toml [kernels] pinned-missing diagnostic builder; consumer wiring lands in subsequent #3434 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:386` | `unpinned_kernel_loaded_diagnostic` | task #3434 unpinned-kernel-loaded diagnostic builder; consumer wiring lands in subsequent #3434 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:425` | `kernel_version_mismatch_diagnostic` | task #3434 kernel-version-mismatch diagnostic builder; consumer wiring lands in subsequent #3434 steps (multi-kernel-phase-3 PRD) |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:535` | `register_compute_fn` | task #3422 ComputeDispatchRegistry + Engine API; engine call-site wiring lands in subsequent #3422 steps |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:1179` | `drain_and_record_warm_pool_events` | task #3541 eval-boundary warm-pool→journal drain; consumer EngineSession::drain_and_emit_warm_pool_events (engine.rs) wiring lands in subsequent #3541 steps |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:2992` | `cap_kind_translation` | task #3463 cap/role vocabulary table; consumer is try_eval_ad_hoc_selector @face/@edge dispatch (same-file, task #3463) + ad_hoc_selector smoke tests |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1683` | `sweep_stale_tempfiles` | task #2978 stale-tempfile sweep; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1811` | `prune_orphan_engine_version_dirs` | task #2978 orphan-engine-version pruning; called by the sweep_persistent_cache_at_startup engine-admin wrapper |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:229` | `gmshModelMeshSetSize` | same-file consumer `mesh_set_size_at_entity` → refine_volume.rs:262 (G-tool same-file-caller heuristic limitation). |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2945` | `store_vertex_at_for_test` | task #3535 vertex_point FFI test fixture; consumed by integration tests in subsequent #3535 steps |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:54` | `record_morph_attempt` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:63` | `record_remesh` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:70` | `record_rejection` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/assembly/global.rs:206` | `detect_orphan_dofs` | task #3293 orphan-DOF detector; cfg(debug_assertions) emit consumer + detector/assembler-consistency pin (task #3293) |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:125` | `project_per_element_sizes_to_vertices` | same-file consumer `refine_with_size_field` (G-tool same-file-caller heuristic limitation). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:181` | `refine_with_size_field` | producer for pending task #2997 (a-posteriori-error-estimation PRD #2: adaptive refinement loop). |
| `reify-types` | `crates/reify-types/src/geometry.rs:1061` | `capability_kind` | task #3623 QueryCapability enum mapping; consumer is the capability-dispatch arm in subsequent #3623 steps |
| `reify-types` | `crates/reify-types/src/value.rs:1687` | `format_display_triple` | task #3648 auto-resolve emit feature; consumer is the auto-resolve diagnostic Display in subsequent #3648 steps |

---

Generated by `scripts/audit-orphan-producers.sh`.
Design: `docs/architecture-audit/g-reviewer-tool-session-prompt.md`.
