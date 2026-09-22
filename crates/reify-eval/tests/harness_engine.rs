//! Consolidated integration-test harness for the engine subsystem.
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/<file>.rs` binaries for the `engine_` cluster —
//! the Engine commit/eval migration pipeline — into this single compile unit to cut the
//! merge-gate link count. Task #5056 seeded this root ahead of EVAL-3 with three modules
//! so its new test would land under the C1 layout instead of grandfathering another
//! top-level standalone binary into the baseline manifest; EVAL-3 is the leaf that filled
//! it. Everything accreted since is an engine-LEVEL end-to-end module: each drives an
//! `Engine` entry point (`eval`, `eval_cached`, `build_outputs`,
//! `redispatch_geometry_consuming_compute_nodes`) over inputs the test itself constructs,
//! and each landed here rather than as a new top-level `tests/*.rs` for the
//! anti-re-accretion reason its own comment below states.
//!
//! Layout-only — none of the moves recorded here adds or removes a `#[test]` fn. Each
//! module is included under its own stem so its `<file>::<test>` module path (and thus
//! every `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is
//! required: this harness root is an integration-test crate root, where a bare
//! `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_engine/` subdir.
//!
//! # What is NOT here
//!
//! `tests/infra/test_harness_kloc_cap.sh` rule (a) relieves a harness approaching its cap
//! by splitting it, never by raising the cap. These left this unit that way:
//!   - the `auto_*` cluster, for `harness_auto_resolution.rs` (#6760; that root's header
//!     records the seam);
//!   - the `m8_`/`m9_` milestone acceptance corpora, for
//!     `harness_milestone_integration.rs` (#7654);
//!   - `flat_sort_kahn_core_delegation`, this unit's one consumer of the shared
//!     `common/differential.rs` harness, for `harness_cache.rs`, which already declares
//!     that include (#7654).
//!
//! No `#[path]` below escapes `harness_engine/`, so this unit carries no external lines.
//! That is a measured property, not a guarded one — re-check it with
//! `harness_layout_unit_lines`, whose `external_lines`/`external_files` read `0 0` while
//! it holds. The compiler enforces only the narrower fact that, with no
//! `mod differential;` at this root, a submodule's `use crate::differential` fails to
//! build.
//!
//! No path fixups were needed for the files EVAL-3 moved here: every path-sensitive
//! construct in them is either `env!("CARGO_MANIFEST_DIR")`-anchored (crate-root
//! relative, so unaffected by an extra source subdirectory) or a runtime
//! `std::fs::read_to_string` (process-CWD relative — the crate root under `cargo test`).
//! Among them there are no `include_str!`/`include!` sites and no `#[global_allocator]`.
//!
//! Whole-unit size — this root plus every `harness_engine/*.rs` module below — is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a). Re-measure
//! with that guard's `harness_layout_unit_lines` rather than trusting a number pinned
//! here.
//!
//! Module order: the modules carrying no rationale comment are listed alphabetically by
//! stem; the commented block at the end keeps the accretion order its comments refer to
//! ("… as #5196's above", and `underdetermined_support` before its two consumers).
#[path = "harness_engine/diagnostics_cache_replay_migration.rs"]
mod diagnostics_cache_replay_migration;
#[path = "harness_engine/edit_param_cell_commit_migration.rs"]
mod edit_param_cell_commit_migration;
#[path = "harness_engine/engine_eval_commit_migration.rs"]
mod engine_eval_commit_migration;
#[path = "harness_engine/joint_drive_cluster_formation.rs"]
mod joint_drive_cluster_formation;
#[path = "harness_engine/reset_per_build_state_classification.rs"]
mod reset_per_build_state_classification;
// Task #5196's capstone acceptance e2e lands here for the same reason #5056's
// did: a NEW top-level `tests/*.rs` in a consolidatable crate would be an
// anti-re-accretion violation (scripts/check-harness-baseline-registration.sh,
// task #5300) unless grandfathered into the shrinking baseline ratchet, and
// growing that ratchet works against the C1 consolidation direction.
#[path = "harness_engine/topology_diagnostic_denoise_e2e.rs"]
mod topology_diagnostic_denoise_e2e;
// Task #5360's nested-sub derived-let e2e lands here for the same anti-re-accretion
// reason as #5196's above.
#[path = "harness_engine/nested_sub_derived_let_e2e.rs"]
mod nested_sub_derived_let_e2e;
// Task #5758's dimensioned-ctor SI-value pins land here for the same
// anti-re-accretion reason as #5196's and #5360's above.
#[path = "harness_engine/dimensioned_ctor_migration_si_values.rs"]
mod dimensioned_ctor_migration_si_values;
// Task #6186's DSL→STEP length-unit-regime round-trip pin lands here for the
// same anti-re-accretion reason as #5196's, #5360's and #5758's above.
// It is also a topical fit: it drives `Engine::build_outputs`, the engine-level
// entry point that composes evaluation, kernel realization and export.
#[path = "harness_engine/export_unit_regime_e2e.rs"]
mod export_unit_regime_e2e;
// Scaffolding shared by task #5467's two e2e modules below (review suggestion
// 5). Declared BEFORE them so the `use crate::underdetermined_support::…` in
// each reads top-down; both are in this same binary, so the helpers were
// literal copy-paste across one compilation unit before this module existed.
#[path = "harness_engine/underdetermined_support.rs"]
mod underdetermined_support;
// Task #5467's let-tracing transitive closure e2e lands here for the same
// anti-re-accretion reason as #5196's and #5360's above.
#[path = "harness_engine/let_tracing_transitive_e2e.rs"]
mod let_tracing_transitive_e2e;
// Task #5467's instance-path W_UNDERDETERMINED regression e2e lands here for the
// same anti-re-accretion reason as #5196's and #5360's above.
#[path = "harness_engine/instance_path_underdetermined_e2e.rs"]
mod instance_path_underdetermined_e2e;
// Task #5240's `eval_cached` guarded-groups fall-through tests land here for the
// same anti-re-accretion reason as #5196's and #5360's above. They are
// also a topical fit: they drive `Engine::eval_cached`, the engine-level
// incremental evaluation entry point.
#[path = "harness_engine/eval_cached_guarded_groups.rs"]
mod eval_cached_guarded_groups;
// Task #5951's geometry-redispatch template-order regressions land here for the
// same anti-re-accretion reason as #5196's and #5360's above. They are
// also a topical fit: they drive `Engine::redispatch_geometry_consuming_compute_nodes`,
// the per-template post-hydration pass in `engine_build.rs`, through the mock
// geometry kernel — engine-level, kernel-independent.
#[path = "harness_engine/redispatch_template_order_regression.rs"]
mod redispatch_template_order_regression;
// Task #6756's driver-level triage probe (P7) lands here for the same
// anti-re-accretion reason as #5196's and #5360's above. It is also a
// topical fit: it drives `compile_source_with_stdlib` -> `Engine::eval`, the
// engine-level entry point, to reproduce the objective seed-parking at the `.ri`
// driver level. Its solver-level siblings (P1-P6, P8) live in
// `crates/reify-constraints/tests/objective_seed_parking_triage.rs`, a crate
// outside the C1 consolidatable set.
#[path = "harness_engine/objective_seed_parking_e2e.rs"]
mod objective_seed_parking_e2e;
// Task #5392's INV-SF-7 value-faithfulness corpus lands here for the same
// anti-re-accretion reason as the above. It cannot live beside its syntax-layer
// sibling in `reify-syntax`: it needs `reify-test-support`'s `eval-helpers`
// feature, and `reify-eval` depends on `reify-syntax`, so the reverse dep would
// be a cycle.
#[path = "harness_engine/fn_body_separator_value_faithfulness.rs"]
mod fn_body_separator_value_faithfulness;
// Task #6038's trait-body eval pins for the polymorphic-zero coercion land here
// for the same anti-re-accretion reason as #5196's, #5360's and
// #5758's above.
#[path = "harness_engine/polymorphic_zero_trait_eval.rs"]
mod polymorphic_zero_trait_eval;
// Task #7418's instance-scope param-default ordering pins land here for the same
// anti-re-accretion reason as #5196's and #5360's above. They are also a
// topical fit: they drive `Engine::eval` over a `sub` instantiation to pin
// `unfold.rs::elaborate_child_params_only`'s visit order — engine-level,
// kernel-independent. Their template-scope counterpart,
// `tests/param_default_sibling_let_order.rs` (task #4317), predates the C1
// layout and stays a grandfathered top-level standalone.
#[path = "harness_engine/instance_scope_param_default_order.rs"]
mod instance_scope_param_default_order;
// Task #5417's DIC γ runtime-half e2e lands here for the same anti-re-accretion
// reason as #5196's and #5360's above.
#[path = "harness_engine/objective_consumption_e2e.rs"]
mod objective_consumption_e2e;
