//! Consolidated integration-test harness for the geometry subsystem — the former
//! standalone `tests/<file>.rs` binaries for the geometry_/symbolic_/trait_ subsystems.
//! Task #5278 (leaf EVAL-1).
//!
//! Layout contract C1 (naming, the mandatory `#[path]`, kLOC cap, baseline ratchet):
//! see `tests/infra/test_harness_kloc_cap.sh` C1 header and
//! `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1 — kept there, not restated here.
//!
//! Test ids change with the layout, though no `#[test]` fn is added or removed: the binary
//! id is now `reify-eval::harness_geometry` (was `reify-eval::<file>`) and the nextest
//! test name gained a module prefix, `<file>::<test>` (was bare `<test>`). So a hand-written
//! `binary(…)`/`test(=…)` selector naming a former id must be updated; verify.sh's failed-only
//! retry is unaffected — it derives `test(=…)` at run time from its own attempt-0 and refuses
//! on tree drift (see `scripts/verify.sh` retry_failed_only).
//!
//! Task #6082 lands `euler_convention_surface.rs` here rather than as a top-level
//! standalone `tests/euler_convention_surface.rs` (flagged
//! `reason=unregistered-standalone` by scripts/check-harness-baseline-registration.sh;
//! the sanctioned remedy is consolidation, NOT a new grandfather row — SUPERSEDED, Leo
//! 2026-07-22, esc-5056-11). It is a geometry-domain surface test: the compile-and-eval
//! contract of the `EulerConvention` enum and the two orientation builtins it selects
//! for. It is deliberately NOT folded into `harness_fea_solver_e2e` alongside the other
//! eval-side consumer of those builtins (`kinematic_stdlib_smoke.rs`): that unit is
//! within ~700 lines of the 20 kLOC C1 cap (tests/infra/test_harness_kloc_cap.sh), while
//! this one has ample headroom.
#[path = "harness_geometry/euler_convention_surface.rs"]
mod euler_convention_surface;
#[path = "harness_geometry/geometry_conditional_e2e.rs"]
mod geometry_conditional_e2e;
#[path = "harness_geometry/geometry_dispatch_registry_guard.rs"]
mod geometry_dispatch_registry_guard;
#[path = "harness_geometry/geometry_error_handling.rs"]
mod geometry_error_handling;
#[path = "harness_geometry/geometry_handle_freshness.rs"]
mod geometry_handle_freshness;
#[path = "harness_geometry/geometry_handle_persistent_cache_round_trip.rs"]
mod geometry_handle_persistent_cache_round_trip;
#[path = "harness_geometry/geometry_handle_value_cell_e2e.rs"]
mod geometry_handle_value_cell_e2e;
#[path = "harness_geometry/geometry_let_value_cell_gamma.rs"]
mod geometry_let_value_cell_gamma;
#[path = "harness_geometry/geometry_query_kernel_dispatch.rs"]
mod geometry_query_kernel_dispatch;
#[path = "harness_geometry/geometry_sub_ref_e2e.rs"]
mod geometry_sub_ref_e2e;
#[path = "harness_geometry/symbolic_geometry_eval.rs"]
mod symbolic_geometry_eval;
#[path = "harness_geometry/symbolic_selector_composition_eval.rs"]
mod symbolic_selector_composition_eval;
#[path = "harness_geometry/symbolic_selector_eval.rs"]
mod symbolic_selector_eval;
#[path = "harness_geometry/trait_assoc_fn_cylinder.rs"]
mod trait_assoc_fn_cylinder;
#[path = "harness_geometry/trait_assoc_fn_static_e2e.rs"]
mod trait_assoc_fn_static_e2e;
#[path = "harness_geometry/trait_merge_eval.rs"]
mod trait_merge_eval;
