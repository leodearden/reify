//! Unified build-DAG fixpoint driver (task 4357 δ).
//!
//! This module holds `run_unified_pass` — an online Kahn topological worklist
//! over α's existing forward dependency-trace graph (O(V+E)) — plus the cycle
//! contract (Stage A hang-proof Kahn residue + Stage B Tarjan-SCC discriminator
//! → `E_EVAL_CYCLE`) and the geometry-backed-constraint-on-auto guard
//! (→ `E_EVAL_UNRESOLVED`).
//!
//! The driver is a PURE STRUCTURAL PLANNER: it returns a `(schedule, residue,
//! diagnostics)` triple and does NOT execute nodes (no kernel calls, no handle
//! inserts, no value writes). Node execution and the runtime `Determined`
//! readiness gate are layered on by the ε executors that consume the schedule.
//!
//! See `docs/prds/v0_6/engine-unified-build-dag.md` for the full design.
//!
//! The module and `run_unified_pass` compile unconditionally so the cycle
//! contract is always unit-testable; the `unified-dag` Cargo feature +
//! `REIFY_BUILD_SCHEDULER` env var gate ONLY the production activation of the
//! driver inside `Engine::build()`.
