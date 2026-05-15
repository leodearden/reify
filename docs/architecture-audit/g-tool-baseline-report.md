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

- **Scanned:** 1337 `pub fn` declarations across 287 files
- **Orphan candidates:** 420  (zero non-test callers, no `// G-allow:`)
- **Allow-listed:** 10  (zero callers; marked legitimate API surface)

## Orphan candidates

| Crate | File:Line | Function |
|---|---|---|
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:257` | `is_valid_optimized` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:272` | `is_known_block_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/annotations.rs:277` | `is_module_only_pragma` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:556` | `enumerate_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:750` | `filter_feasible_candidates` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:880` | `select_candidate` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1298` | `resolve_auto_type_params_with_backtracking` |
| `reify-compiler` | `crates/reify-compiler/src/auto_type_param.rs:1924` | `build_constraint_blame_map` |
| `reify-compiler` | `crates/reify-compiler/src/compile_builder/defs_phase.rs:34` | `format_shadow_warning` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/checker.rs:32` | `resolve_let_advertised_type` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:302` | `emit_geometry_unbounded` |
| `reify-compiler` | `crates/reify-compiler/src/conformance/mod.rs:331` | `emit_geometry_trait_violation` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:69` | `auto_match_port_members` |
| `reify-compiler` | `crates/reify-compiler/src/connect.rs:133` | `is_forward_compatible` |
| `reify-compiler` | `crates/reify-compiler/src/functions.rs:130` | `resolve_field_type_name` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1025` | `extract_collection_count` |
| `reify-compiler` | `crates/reify-compiler/src/geometry.rs:1050` | `unsupported_geometry_fn_message` |
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
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:334` | `compile_guarded_members` |
| `reify-compiler` | `crates/reify-compiler/src/guards.rs:676` | `narrow_arms_under_guard` |
| `reify-compiler` | `crates/reify-compiler/src/ice.rs:16` | `as_phrase` |
| `reify-compiler` | `crates/reify-compiler/src/lib.rs:246` | `compile_with_prelude_context` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:275` | `compile_module` |
| `reify-compiler` | `crates/reify-compiler/src/module_dag.rs:618` | `compile_project_with_entry_source` |
| `reify-compiler` | `crates/reify-compiler/src/si_units.rs:61` | `includes` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:123` | `termination_args_contain_undef` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:142` | `termination_collect_refs` |
| `reify-compiler` | `crates/reify-compiler/src/termination.rs:161` | `termination_is_modifying` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:134` | `is_skipped_parametric_prelude` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:150` | `should_emit_skipped_parametric_prelude_info` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:235` | `resolve_dimension_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:291` | `evaluate_const_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:534` | `resolve_type_with_params` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:667` | `resolve_type_alias_expr` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:782` | `resolve_type_alias_expr_to_dimension` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:940` | `resolve_parameterized_alias` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1032` | `resolve_type_alias_expr_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1203` | `resolve_parameterized_builtin_type` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1399` | `resolve_parameterized_builtin_type_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1536` | `resolve_type_alias_expr_to_dim_with_subst` |
| `reify-compiler` | `crates/reify-compiler/src/type_resolution.rs:1606` | `collect_type_expr_names` |
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
| `reify-eval` | `crates/reify-eval/src/cache.rs:277` | `is_fresh` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:288` | `bump_version` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:345` | `record_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:356` | `get_imported_file_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:382` | `imported_file_hash_changed` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:572` | `get_dirty_reasons` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:801` | `pending_transition_count` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:852` | `derive_output_freshness_from_trace` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:879` | `derive_output_freshness_for_node` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:987` | `insert_synthetic_realization_entry` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1023` | `derive_output_freshness` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1080` | `derive_output_freshness_with_cause` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1143` | `compute_input_hash` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1171` | `check_early_cutoff` |
| `reify-eval` | `crates/reify-eval/src/cache.rs:1189` | `dirty_set` |
| `reify-eval` | `crates/reify-eval/src/demand.rs:55` | `cone_size` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:94` | `add_realization` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:117` | `build_from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:224` | `build_trace_map` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1022` | `extract_value_deps` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1178` | `from_graph` |
| `reify-eval` | `crates/reify-eval/src/deps.rs:1219` | `deps_of` |
| `reify-eval` | `crates/reify-eval/src/dirty.rs:95` | `compute_dirty_cone_with_realizations` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:125` | `is_long_chain_realization` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:179` | `long_chain_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:227` | `long_chain_threshold_from_env` |
| `reify-eval` | `crates/reify-eval/src/dispatcher.rs:244` | `long_chain_threshold_from_env_value` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:118` | `with_prelude` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:415` | `register_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:428` | `unregister_optimized_impl` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:435` | `optimized_targets` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:457` | `set_max_unfold_depth` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:474` | `set_max_unfold_nodes` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:480` | `with_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:497` | `register_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:504` | `unregister_solver` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:512` | `registered_solvers` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:593` | `cache_store` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:831` | `propagate_freshness_only` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:877` | `warm_pool_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:890` | `cache_store_mut` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:911` | `set_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:925` | `remove_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_admin.rs:938` | `clear_panic_on_eval` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:489` | `build_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:986` | `tessellate_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1145` | `compute_realization_tolerance_budget` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1198` | `budget_available_set` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1227` | `compute_demanded_tols` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:1273` | `compute_tessellation_budgets` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2211` | `tessellate_snapshot` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:2403` | `dispatch_volume_mesh` |
| `reify-eval` | `crates/reify-eval/src/engine_build.rs:4962` | `p2_substitution_diagnostic` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:29` | `dispatch_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:213` | `labeled_diagnostics` |
| `reify-eval` | `crates/reify-eval/src/engine_constraints.rs:408` | `collect_active_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:59` | `deactivate_if_not_auto` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:113` | `rewrite_port_placeholder` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:530` | `diff_value_cells` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:548` | `diff_constraints` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:566` | `diff_realizations` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:1968` | `edit_source` |
| `reify-eval` | `crates/reify-eval/src/engine_edit.rs:3237` | `edit_check` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:66` | `is_representable_cell_type` |
| `reify-eval` | `crates/reify-eval/src/engine_eval.rs:673` | `hash_imported_file_content` |
| `reify-eval` | `crates/reify-eval/src/engine_hash_algo.rs:201` | `compose_engine_version_hash` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:238` | `deactivate_purpose` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:290` | `is_purpose_active` |
| `reify-eval` | `crates/reify-eval/src/engine_purposes.rs:295` | `active_objectives` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:19` | `imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:69` | `check_imported_tolerance_promise` |
| `reify-eval` | `crates/reify-eval/src/engine_tolerance.rs:126` | `active_tolerance_for` |
| `reify-eval` | `crates/reify-eval/src/field_import_provenance.rs:65` | `build_field_import_provenance` |
| `reify-eval` | `crates/reify-eval/src/gating.rs:102` | `unblocked_gated_nodes` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:45` | `eval_named_arg` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:79` | `eval_named_arg_f64` |
| `reify-eval` | `crates/reify-eval/src/geometry_ops.rs:114` | `eval_all_args_to_f64` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:530` | `insert_compute_node` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:536` | `get_compute_node` |
| `reify-eval` | `crates/reify-eval/src/graph.rs:540` | `get_compute_node_mut` |
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
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1051` | `write_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1135` | `read_entry` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1315` | `evict_over_cap` |
| `reify-eval` | `crates/reify-eval/src/persistent_cache.rs:1516` | `eviction_score` |
| `reify-eval` | `crates/reify-eval/src/primitive_attribute_seed.rs:187` | `seed_primitive_attributes` |
| `reify-eval` | `crates/reify-eval/src/realization_cache.rs:119` | `bucket_len` |
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
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:852` | `adjacent_to_face` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:1008` | `siblings_of_face` |
| `reify-eval` | `crates/reify-eval/src/selector_vocabulary_v2.rs:1043` | `owner_body_of` |
| `reify-eval` | `crates/reify-eval/src/significance_filter.rs:75` | `is_opted_in` |
| `reify-eval` | `crates/reify-eval/src/source_location.rs:45` | `resolve_entity_at_source_position` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:74` | `realization_graph_shape_hash` |
| `reify-eval` | `crates/reify-eval/src/structural_classifier.rs:95` | `classify_cell` |
| `reify-eval` | `crates/reify-eval/src/topology_attribute_propagation.rs:114` | `propagate_attributes_via_brepalgoapi_history` |
| `reify-eval` | `crates/reify-eval/src/topology_attribute_resolver.rs:147` | `resolve_unique_by_attribute` |
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
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:502` | `drain_events` |
| `reify-eval` | `crates/reify-eval/src/warm_pool.rs:515` | `dropped_events` |
| `reify-expr` | `crates/reify-expr/src/lib.rs:81` | `_test_at_depth` |
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
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:553` | `repr_of` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2704` | `warm_start_failures` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2754` | `store_circle_face_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2838` | `store_nonmanifold_compound_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2850` | `store_malformed_solid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2863` | `store_nonorientable_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2876` | `store_closed_shell_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2888` | `store_edge_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2899` | `store_vertex_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2910` | `store_compsolid_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/lib.rs:2930` | `store_placed_for_test` |
| `reify-kernel-occt` | `crates/reify-kernel-occt/src/register.rs:91` | `occt_capability_descriptor` |
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
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/lib.rs:108` | `eligible` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/lib.rs:142` | `morph` |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/quality.rs:200` | `quality_check` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:102` | `set_instance` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:109` | `set_type` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:151` | `progress_estimate` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:190` | `check_commitment` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:302` | `is_committed` |
| `reify-runtime` | `crates/reify-runtime/src/commitment.rs:326` | `task_count` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:120` | `is_cancelled` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:125` | `child_token` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:132` | `cancelled` |
| `reify-runtime` | `crates/reify-runtime/src/concurrent.rs:214` | `execute_with_config` |
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
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:79` | `apply_point_load` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:225` | `apply_body_force` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/boundary/neumann.rs:478` | `apply_traction_load` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:165` | `solve_eigen_dense` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/eigensolve.rs:343` | `solve_eigen_shift_invert` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:120` | `bubble_grad_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mitc3_plus.rs:135` | `bubble_at` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/elements/mod.rs:80` | `from_matrix` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/error_estimator.rs:93` | `compute_zz_indicator` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:95` | `barycentric_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:156` | `point_in_tet_p1` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:188` | `interpolate_p1_at_point` |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/interpolation.rs:233` | `locate_element_p1` |
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
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/result.rs:115` | `element_stress_p1` |
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
| `reify-stdlib` | `crates/reify-stdlib/src/loads.rs:46` | `is_load_value` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:59` | `chain_transform` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure.rs:222` | `per_joint_jacobian_local` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:201` | `is_singular` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:339` | `newton_solve` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:653` | `mechanism_loop_closure_chains` |
| `reify-stdlib` | `crates/reify-stdlib/src/loop_closure_solver.rs:820` | `solve_loop_closure_with_diagnostics` |
| `reify-stdlib` | `crates/reify-stdlib/src/supports.rs:71` | `is_support_value` |
| `reify-types` | `crates/reify-types/src/diagnostics.rs:80` | `is_prelude` |
| `reify-types` | `crates/reify-types/src/dimension.rs:36` | `is_zero` |
| `reify-types` | `crates/reify-types/src/dimension.rs:40` | `is_integer` |
| `reify-types` | `crates/reify-types/src/dimension.rs:44` | `as_i8` |
| `reify-types` | `crates/reify-types/src/expr.rs:1403` | `user_function_call` |
| `reify-types` | `crates/reify-types/src/expr.rs:1429` | `match_expr` |
| `reify-types` | `crates/reify-types/src/geometry.rs:2393` | `try_nary` |
| `reify-types` | `crates/reify-types/src/geometry.rs:2417` | `nary` |
| `reify-types` | `crates/reify-types/src/node_traits.rs:226` | `default_traits` |
| `reify-types` | `crates/reify-types/src/persistent.rs:45` | `insert_functional` |
| `reify-types` | `crates/reify-types/src/value.rs:675` | `try_into_matrix` |
| `reify-types` | `crates/reify-types/src/value.rs:1066` | `infer_type` |
| `reify-types` | `crates/reify-types/src/value.rs:1177` | `try_infer_type` |
| `reify-types` | `crates/reify-types/src/value.rs:1427` | `format_display` |
| `reify-types` | `crates/reify-types/src/value.rs:1568` | `format_display_pair` |
| `reify-types` | `crates/reify-types/src/value.rs:1610` | `format_display_number` |
| `reify-types` | `crates/reify-types/src/value.rs:2425` | `has_hash` |

## Allow-listed (zero callers, intentional)

| Crate | File:Line | Function | Reason |
|---|---|---|---|
| `reify-audit` | `crates/reify-audit/src/lib.rs:282` | `set_log_grep` | F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md |
| `reify-audit` | `crates/reify-audit/src/lib.rs:288` | `set_diff_changed_paths` | F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md |
| `reify-audit` | `crates/reify-audit/src/lib.rs:294` | `set_is_gitignored` | F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md |
| `reify-audit` | `crates/reify-audit/src/p5_phantom_done.rs:56` | `check_pre_done` | F-infra T-4 CLI consumer (crates/reify-audit-cli) — design pinned in docs/architecture-audit/f-infra-design.md |
| `reify-kernel-gmsh` | `crates/reify-kernel-gmsh/src/ffi.rs:229` | `gmshModelMeshSetSize` | same-file consumer `mesh_set_size_at_entity` → refine_volume.rs:262 (G-tool same-file-caller heuristic limitation). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:125` | `project_per_element_sizes_to_vertices` | same-file consumer `refine_with_size_field` (G-tool same-file-caller heuristic limitation). |
| `reify-solver-elastic` | `crates/reify-solver-elastic/src/volume_refine.rs:181` | `refine_with_size_field` | producer for pending task #2997 (a-posteriori-error-estimation PRD #2: adaptive refinement loop). |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:56` | `record_morph_attempt` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:65` | `record_remesh` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |
| `reify-mesh-morph` | `crates/reify-mesh-morph/src/stats.rs:72` | `record_rejection` | mesh-morph engine call-site wiring deferred to tasks #2947-#2949 |

---

Generated by `scripts/audit-orphan-producers.sh`.
Design: `docs/architecture-audit/g-reviewer-tool-session-prompt.md`.
