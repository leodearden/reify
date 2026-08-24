//! Consolidated integration-test harness for the process/DFM subsystem.
//!
//! Split out of `harness_fea_solver_e2e.rs` (task #4880). That harness — itself the
//! task #5281 consolidation of the fea_/tensegrity_/stress_/objective_/multi_/process_/
//! as_printed_/kinematic_ standalones — crossed the 20 kLOC per-compile-unit cap
//! enforced by `tests/infra/test_harness_kloc_cap.sh` rule (a) once task #4880's
//! `fea_in_the_loop_producer` submodule landed in it (20034 > 20000, of which
//! module_lines=19535). That rule's prescribed remedy for a module_lines-dominated
//! overage is exactly this: "a harness that grows past the cap must be SPLIT into a
//! second `harness_<subsystem2>.rs`, never allowed to balloon unbounded — and never
//! accommodated by raising the cap".
//!
//! Layout-only. No `#[test]` fn is added, removed or edited; each former submodule keeps
//! its stem-named `mod`, so its `<file>::<test>` module path — and thus every
//! `test(/^<file>::/)` filterset — resolves unchanged. Only the owning test BINARY
//! changes, from `harness_fea_solver_e2e` to `harness_process_dfm`.
//!
//! WHY THE `process_dfm_*` GROUP, and not the submodule that caused the overage. The
//! offending submodule cannot move: the heavy-test filter's 7th atom (task #6368) is
//! `package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_in_the_loop_producer::/)`,
//! and `tests/infra/test_heavy_filter_atoms.sh` Assertion F asserts that the
//! `mod fea_in_the_loop_producer;` declaration lives in the `harness_fea_solver_e2e`
//! root — moving it would silently empty that filterset and put a ~490s test back on the
//! merge gate. `process_dfm_*` was chosen instead because it is the largest prefix group
//! in that harness with ZERO inbound coupling: no `crate::common` use (so this unit
//! carries no out-of-module-dir include at all), no cross-submodule `super::`/`crate::`
//! reference in either direction, and no `binary(...)`/`test(...)` selector naming it in
//! `scripts/heavy-test-filter-lib.sh` or `.config/nextest.toml`. It leaves
//! `harness_fea_solver_e2e` at ~18.3 kLOC — back under the cap with real headroom rather
//! than at the boundary.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to a sibling `tests/<file>.rs`, not the
//! `harness_process_dfm/` subdir.

#[path = "harness_process_dfm/process_dfm_e2e.rs"]
mod process_dfm_e2e;
#[path = "harness_process_dfm/process_dfm_eval.rs"]
mod process_dfm_eval;
#[path = "harness_process_dfm/process_dfm_measure.rs"]
mod process_dfm_measure;
#[path = "harness_process_dfm/process_dfm_metrology_example.rs"]
mod process_dfm_metrology_example;
#[path = "harness_process_dfm/process_dfm_thickness_example.rs"]
mod process_dfm_thickness_example;
