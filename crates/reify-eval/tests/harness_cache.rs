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
//! # Shared `differential` module — the tests/common dedup
//!
//! `differential` (tests/common/differential.rs) is declared ONCE here rather than once
//! per includer. Four submodules below reference it —
//! `unified_dag_boundary_cases`, `unified_dag_differential_corpus`,
//! `unified_dag_edit_path`, `unified_dag_warm_path` — and each previously carried its own
//! `#[path = "common/differential.rs"] mod differential;`, so the same source was
//! compiled four times. One shared copy removes 3 of those 4 duplicate compilations of
//! `differential.rs` with no behavioral difference: differential.rs declares 0
//! `#[test]` fns, so collapsing the copies cannot change the nextest test count.
//! Submodules reach it via `use crate::differential::{…}`. No extra `#![allow]` is needed
//! at this root — differential.rs carries its own `#![allow(dead_code)]`.
//!
//! # Residual — what the BONUS asked for vs. what shipped
//!
//! Stated rather than silently dropped, and stated against the ask so the delta is on the
//! record rather than inferred. Task #5282's BONUS was to dedup `tests/common/` "as a
//! single `mod common;` under the harnesses". What shipped is deliberately NARROWER:
//!
//! DONE — `common/differential.rs`, by far the largest member of `tests/common/`, is now
//! declared once per compile unit instead of once per includer. Inside this unit that is
//! 4 copies collapsed to the 1 declaration below, which is where nearly all of the
//! available duplicated-compilation payload lived.
//!
//! NOT DONE — `differential.rs` is still compiled separately into `harness_engine` and
//! `harness_selective_demand` (one root-level declaration each). That is not an oversight
//! and no further `#[path]` bookkeeping can fix it: each integration-test root is its own
//! crate, so a module cannot be shared across binaries. Removing those two compilations
//! means merging the three units into one, which the PRD §7 cap forbids outright — their
//! combined size, even after the two duplicate `differential` copies are netted out, is
//! well past the cap, and `harness_engine` is already the unit that cap forced a split
//! out of (see `harness_auto_resolution`). Re-measure with
//! `tests/infra/test_harness_kloc_cap.sh` rather than trusting a number pinned here.
//!
//! NOT DONE — the `mod common;` half of the BONUS is untouched. Its includers are left
//! standalone, each for a reason that consolidating would violate:
//!   - `realization_cache_alloc` and `realization_cache_alloc_rotating_options_hash` each
//!     declare their own `#[global_allocator] static GLOBAL`. Rust permits exactly one
//!     global allocator per compile unit, so folding both here is a hard rustc error, and
//!     folding either would instrument every allocation in this harness. Both files'
//!     headers state the process isolation as deliberate design.
//!   - `fdm_bracket_e2e` and `fdm_progressive_refinement_e2e` (`common::as_printed`
//!     users) belong to `harness_fea_solver_e2e`, which already sits close to the PRD §7
//!     20,000-line cap — folding them would risk breaching the band.
//!   - `edit_source` and `guard_eval` are ~8 kLOC of out-of-subsystem tests, and their
//!     only use of `common` is the single small `ten_bool_guarded_groups` helper; pulling
//!     two foreign harnesses in here just to dedup that helper is not a trade worth
//!     making.
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
