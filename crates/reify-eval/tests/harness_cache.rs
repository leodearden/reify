//! Consolidated integration-test harness for the incremental-recompute / cache
//! subsystem.
//!
//! Task #5282 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-3):
//! folds the former standalone `tests/<file>.rs` binaries for the `freshness_`/`warm_`/
//! `unified_dag_` clusters plus the four cache-named files (`compute_cache_key_population`,
//! `eval_cached_diagnostics`, `persistent_cache_compute_round_trip`,
//! `snapshot_cache_divergence_gate`) into this single compile unit to cut the merge-gate
//! link count. Layout-only — no `#[test]` fn is added or removed. Each former file is
//! included as a stem-named module so its `<file>::<test>` module path (and thus every
//! `test(/^<file>::/)` filterset) resolves unchanged. Explicit `#[path]` is required:
//! this harness root is an integration-test crate root, where a bare `mod <file>;` would
//! resolve to the sibling `tests/<file>.rs`, not the `harness_cache/` subdir.
//!
//! Task #7033 then folded in the `selective_demand_*` cluster, previously its own root
//! (`harness_selective_demand`, split out of `harness_topology_selector` by task #5620).
//! Those modules test demand-scoped incremental recompute (warm tessellate pruning, cone
//! maintenance across structural edits, re-demand staleness), which is this unit's
//! subsystem, and their fixtures already live in `differential.rs`. Same layout-only
//! contract: stems preserved, only the binary id moved.
//!
//! # Shared `differential` module
//!
//! `differential` (tests/common/differential.rs) is declared ONCE here. This unit holds
//! EVERY consumer of `differential.rs`, so it is compiled into exactly one integration-test
//! binary. Keep those consumers together: moving one into another root re-adds a full copy
//! of `differential.rs` there, and rule (a) charges each unit for its copy. Submodules reach
//! it via `use crate::differential::{…}`. No extra `#![allow]` is needed at this root —
//! differential.rs carries its own `#![allow(dead_code)]`.
//!
//! # Path fixups
//!
//! `include_str!` resolves relative to the *including source file*, so the extra
//! subdirectory level required deepening 3 call sites by one `../`:
//! `fixtures/compute_identity.ri` → `../fixtures/…` in `freshness_pending_compute_dispatch`,
//! and `../../../examples/fea_cantilever_smoke.ri` → `../../../../examples/…` in
//! `compute_cache_key_population` and `persistent_cache_compute_round_trip`. A wrong
//! `../` depth is a compile error rather than a silent skip, but these modules were RUN
//! as well as compiled so the fixture/example *content* is confirmed to still reach the
//! assertions.
//!
//! Whole-unit size — this root, every `harness_cache/*.rs` module below, and the
//! `common/differential.rs` include above, which escapes the module directory — is
//! measured and capped by `tests/infra/test_harness_kloc_cap.sh` rule (a).
#[path = "common/differential.rs"]
mod differential;

#[path = "harness_cache/compute_cache_key_population.rs"]
mod compute_cache_key_population;
#[path = "harness_cache/eval_cached_diagnostics.rs"]
mod eval_cached_diagnostics;
#[path = "harness_cache/flat_sort_kahn_core_delegation.rs"]
mod flat_sort_kahn_core_delegation;
#[path = "harness_cache/freshness_only_production_trigger.rs"]
mod freshness_only_production_trigger;
#[path = "harness_cache/freshness_only_propagation.rs"]
mod freshness_only_propagation;
#[path = "harness_cache/freshness_pending_compute_dispatch.rs"]
mod freshness_pending_compute_dispatch;
#[path = "harness_cache/freshness_propagation.rs"]
mod freshness_propagation;
#[path = "harness_cache/persistent_cache_compute_round_trip.rs"]
mod persistent_cache_compute_round_trip;
#[path = "harness_cache/selective_demand_alpha.rs"]
mod selective_demand_alpha;
#[path = "harness_cache/selective_demand_beta.rs"]
mod selective_demand_beta;
#[path = "harness_cache/selective_demand_cone_structural_edit.rs"]
mod selective_demand_cone_structural_edit;
#[path = "harness_cache/selective_demand_epsilon.rs"]
mod selective_demand_epsilon;
#[path = "harness_cache/selective_demand_gamma.rs"]
mod selective_demand_gamma;
#[path = "harness_cache/selective_demand_measurement.rs"]
mod selective_demand_measurement;
#[path = "harness_cache/selective_demand_redemand_staleness.rs"]
mod selective_demand_redemand_staleness;
#[path = "harness_cache/snapshot_cache_divergence_gate.rs"]
mod snapshot_cache_divergence_gate;
#[path = "harness_cache/unified_dag_boundary_cases.rs"]
mod unified_dag_boundary_cases;
#[path = "harness_cache/unified_dag_cycle_contract.rs"]
mod unified_dag_cycle_contract;
#[path = "harness_cache/unified_dag_differential_corpus.rs"]
mod unified_dag_differential_corpus;
#[path = "harness_cache/unified_dag_edit_path.rs"]
mod unified_dag_edit_path;
#[path = "harness_cache/unified_dag_geometry_executors.rs"]
mod unified_dag_geometry_executors;
#[path = "harness_cache/unified_dag_warm_path.rs"]
mod unified_dag_warm_path;
#[path = "harness_cache/warm_determinacy_predicate.rs"]
mod warm_determinacy_predicate;
#[path = "harness_cache/warm_pool_drain_steady_state.rs"]
mod warm_pool_drain_steady_state;
#[path = "harness_cache/warm_state_budget_config.rs"]
mod warm_state_budget_config;
