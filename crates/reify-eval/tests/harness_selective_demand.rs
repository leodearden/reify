//! Consolidated integration-test harness for the selective-demand subsystem — the
//! `selective_demand_*` files split out of `harness_topology_selector`.
//! Task #5620.
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1/C2 — kept there, not restated here.
//!
//! Layout-only (invariant I3): no `#[test]` fn is added or removed by this split, and every
//! stem is unchanged, so each `<file>::<test_name>` module path — and thus every
//! `test(/^selective_demand_*/)`-shaped filterset — resolves exactly as before. Only the
//! binary id moves, from `reify-eval::harness_topology_selector` to
//! `reify-eval::harness_selective_demand`; no repo-resident selector names either
//! (mechanically gated by that guard's §7, PRD §6 BT-2).
//!
//! WHY THE BOUNDARY IS HERE. `#[path = "common/differential.rs"]` (2128 lines) is compiled
//! into every binary that includes it, so under the C2 kLOC cap it is charged to the whole
//! unit — which put `harness_topology_selector` at 21470 lines, over CAP_LINES = 20000. Rule
//! (a)'s own remedy is a SPLIT, never a cap bump. `differential` is referenced by exactly the
//! six `selective_demand_*` submodules below and by nothing else in the module dir, so
//! carrying it out with this group is the split that leaves `harness_topology_selector` with
//! no external attribution at all (16786) rather than merely redistributing the overage.
//! `selective_demand_measurement` references it not at all and travels with its prefix group
//! (PRD §3 W1 / §5 C1 group by subsystem module prefix).
#[path = "common/differential.rs"]
mod differential;

#[path = "harness_selective_demand/selective_demand_alpha.rs"]
mod selective_demand_alpha;
#[path = "harness_selective_demand/selective_demand_beta.rs"]
mod selective_demand_beta;
#[path = "harness_selective_demand/selective_demand_cone_structural_edit.rs"]
mod selective_demand_cone_structural_edit;
#[path = "harness_selective_demand/selective_demand_epsilon.rs"]
mod selective_demand_epsilon;
#[path = "harness_selective_demand/selective_demand_gamma.rs"]
mod selective_demand_gamma;
#[path = "harness_selective_demand/selective_demand_measurement.rs"]
mod selective_demand_measurement;
#[path = "harness_selective_demand/selective_demand_redemand_staleness.rs"]
mod selective_demand_redemand_staleness;
