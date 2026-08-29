//! Consolidated integration-test harness for the engine subsystem.
//!
//! Task #5056 (PRD docs/prds/merge-gate-compile-cost.md §5 C1): this is the C1
//! consolidated harness compile unit for the engine subsystem cluster, seeded ahead
//! of leaf EVAL-3 (task #5282, the planned full engine-subsystem consolidation) so
//! #5056's new test lands under the C1 layout instead of grandfathering another
//! top-level standalone binary into the baseline manifest. Layout-only — no `#[test]`
//! fn is added or removed. Each former file is included as a stem-named module so its
//! `<file>::<test>` module path (and thus every `test(/^<file>::/)` filterset)
//! resolves unchanged. Explicit `#[path]` is required: this harness root is an
//! integration-test crate root, where a bare `mod <file>;` would resolve to the
//! sibling `tests/<file>.rs`, not the `harness_engine/` subdir.
//!
//! The shared `common/differential.rs` harness is included HERE, at the unit root,
//! rather than inside the one submodule that consumes it — the same placement
//! `harness_selective_demand.rs` uses. rustc compiles one copy per binary either
//! way, so root placement keeps a second consumer from silently duplicating the
//! include, and keeps the C2 external attribution readable as a single unit-level
//! fact. Submodules reach it as `use crate::differential;`.
#[path = "common/differential.rs"]
mod differential;

#[path = "harness_engine/edit_param_cell_commit_migration.rs"]
mod edit_param_cell_commit_migration;
#[path = "harness_engine/diagnostics_cache_replay_migration.rs"]
mod diagnostics_cache_replay_migration;
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
// Task #5045's flat-sort/Kahn-core delegation differential test lands here for
// the same anti-re-accretion reason as #5196's above.
#[path = "harness_engine/flat_sort_kahn_core_delegation.rs"]
mod flat_sort_kahn_core_delegation;
// Task #5360's nested-sub derived-let e2e lands here for the same anti-re-accretion
// reason as #5196's and #5045's above.
#[path = "harness_engine/nested_sub_derived_let_e2e.rs"]
mod nested_sub_derived_let_e2e;
// Task #5758's dimensioned-ctor SI-value pins land here for the same
// anti-re-accretion reason as #5196's, #5045's and #5360's above.
#[path = "harness_engine/dimensioned_ctor_migration_si_values.rs"]
mod dimensioned_ctor_migration_si_values;
// Task #6186's DSL→STEP length-unit-regime round-trip pin lands here for the
// same anti-re-accretion reason as #5196's, #5045's, #5360's and #5758's above.
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
// anti-re-accretion reason as #5196's, #5045's and #5360's above.
#[path = "harness_engine/let_tracing_transitive_e2e.rs"]
mod let_tracing_transitive_e2e;
// Task #5467's instance-path W_UNDERDETERMINED regression e2e lands here for the
// same anti-re-accretion reason as #5196's, #5045's and #5360's above.
#[path = "harness_engine/instance_path_underdetermined_e2e.rs"]
mod instance_path_underdetermined_e2e;
// Task #5240's `eval_cached` guarded-groups fall-through tests land here for the
// same anti-re-accretion reason as #5196's, #5045's and #5360's above. They are
// also a topical fit: they drive `Engine::eval_cached`, the engine-level
// incremental evaluation entry point.
#[path = "harness_engine/eval_cached_guarded_groups.rs"]
mod eval_cached_guarded_groups;
// Task #5951's geometry-redispatch template-order regressions land here for the
// same anti-re-accretion reason as #5196's, #5045's and #5360's above. They are
// also a topical fit: they drive `Engine::redispatch_geometry_consuming_compute_nodes`,
// the per-template post-hydration pass in `engine_build.rs`, through the mock
// geometry kernel — engine-level, kernel-independent.
#[path = "harness_engine/redispatch_template_order_regression.rs"]
mod redispatch_template_order_regression;
