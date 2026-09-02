//! Consolidated integration-test harness for the engine subsystem.
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/<file>.rs` binaries for the `engine_`/`m8_`/`m9_`
//! clusters — the Engine commit + milestone-integration pipeline — into this single
//! compile unit to cut the merge-gate link count. The `auto_*` cluster EVAL-3 also
//! consolidates is a sibling unit, `harness_auto_resolution.rs`, not a module here: folding
//! all four clusters into one root measured 20363 lines against
//! `tests/infra/test_harness_kloc_cap.sh`'s 20000-line rule (a) cap, and §7 of the PRD
//! resolves an over-cap harness by SPLIT, never by raising the cap (precedent #5620).
//! See that root's header for the seam. Task #5056 seeded this root ahead of
//! EVAL-3 with three modules so its new test would land under the C1 layout instead of
//! grandfathering another top-level standalone binary into the baseline manifest; this
//! is the leaf that fills it. Layout-only — no `#[test]` fn is added or removed. Each
//! former file is included as a stem-named module so its `<file>::<test>` module path
//! (and thus every `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]`
//! is required: this harness root is an integration-test crate root, where a bare
//! `mod <file>;` would resolve to the sibling `tests/<file>.rs`, not the
//! `harness_engine/` subdir.
//!
//! No path fixups were needed for the 9 files EVAL-3 moved here: every path-sensitive
//! construct in them is either `env!("CARGO_MANIFEST_DIR")`-anchored (crate-root
//! relative, so unaffected by an extra source subdirectory) or a runtime
//! `std::fs::read_to_string` (process-CWD relative — the crate root under `cargo test`;
//! `m8_stdlib_integration`'s 14 `../../examples/*.ri` reads are all of this kind).
//! Among those 9 there are no `include_str!`/`include!` sites and no
//! `#[global_allocator]`.
//!
//! The shared `common/differential.rs` harness is included HERE, at the unit root,
//! rather than inside the one submodule that consumes it — the same placement
//! `harness_selective_demand.rs` uses. rustc compiles one copy per binary either
//! way, so root placement keeps a second consumer from silently duplicating the
//! include, and keeps the C2 external attribution readable as a single unit-level
//! fact. Submodules reach it as `use crate::differential;`.
//!
//! Whole-unit size — this root, every `harness_engine/*.rs` module below, and the
//! `common/differential.rs` include above, which escapes the module directory — is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
//!
//! Module order: the modules carrying no rationale comment are listed alphabetically by
//! stem; the commented block at the end keeps the accretion order its comments refer to
//! ("… as #5196's above", and `underdetermined_support` before its two consumers).
#[path = "common/differential.rs"]
mod differential;

#[path = "harness_engine/diagnostics_cache_replay_migration.rs"]
mod diagnostics_cache_replay_migration;
#[path = "harness_engine/edit_param_cell_commit_migration.rs"]
mod edit_param_cell_commit_migration;
#[path = "harness_engine/engine_eval_commit_migration.rs"]
mod engine_eval_commit_migration;
#[path = "harness_engine/joint_drive_cluster_formation.rs"]
mod joint_drive_cluster_formation;
#[path = "harness_engine/m8_3_stdlib_integration.rs"]
mod m8_3_stdlib_integration;
#[path = "harness_engine/m8_4_stdlib_integration.rs"]
mod m8_4_stdlib_integration;
#[path = "harness_engine/m8_m11_regression_checkpoint.rs"]
mod m8_m11_regression_checkpoint;
#[path = "harness_engine/m8_stdlib_integration.rs"]
mod m8_stdlib_integration;
#[path = "harness_engine/m9_combined.rs"]
mod m9_combined;
#[path = "harness_engine/m9_constraint_def.rs"]
mod m9_constraint_def;
#[path = "harness_engine/m9_integration.rs"]
mod m9_integration;
#[path = "harness_engine/m9_trait_conformance.rs"]
mod m9_trait_conformance;
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
// Task #6756's driver-level triage probe (P7) lands here for the same
// anti-re-accretion reason as #5196's, #5045's and #5360's above. It is also a
// topical fit: it drives `compile_source_with_stdlib` -> `Engine::eval`, the
// engine-level entry point, to reproduce the objective seed-parking at the `.ri`
// driver level. Its solver-level siblings (P1-P6, P8) live in
// `crates/reify-constraints/tests/objective_seed_parking_triage.rs`, a crate
// outside the C1 consolidatable set.
#[path = "harness_engine/objective_seed_parking_e2e.rs"]
mod objective_seed_parking_e2e;
