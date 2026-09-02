//! Consolidated integration-test harness for the language/geometry robustness
//! ("stress") scenario subsystem.
//!
//! Split out of `harness_fea_solver_e2e.rs` (task #6121). That harness — itself the task
//! #5281 consolidation of the fea_/tensegrity_/stress_/objective_/multi_/process_/
//! as_printed_/kinematic_ standalones, already once split by task #4880 to create
//! `harness_process_dfm.rs` — had climbed back to 19265 of the 20000-line
//! per-compile-unit cap enforced by `tests/infra/test_harness_kloc_cap.sh` rule (a):
//! 96.3%, with module_lines=18769 across 42 files and only 735 lines of headroom. That
//! rule's prescribed remedy for a module_lines-dominated squeeze is exactly this: "a
//! harness that grows past the cap must be SPLIT into a second `harness_<subsystem2>.rs`,
//! never allowed to balloon unbounded — and never accommodated by raising the cap".
//! Splitting BEFORE the cap breaks (rather than on the commit that breaks it) is what the
//! same task's new advisory WARN tier exists to prompt.
//!
//! Layout-only. No `#[test]` fn is added, removed or edited; each moved submodule keeps
//! its stem-named `mod`, so its `<file>::<test>` module path — and thus every
//! `test(/^<file>::/)` filterset — resolves unchanged. Only the owning test BINARY
//! changes, from `harness_fea_solver_e2e` to `harness_stress_scenarios`.
//!
//! WHY THE `stress_*` GROUP. It is the group that satisfies every criterion the #4880
//! precedent (`harness_process_dfm.rs`) wrote down, verified rather than assumed:
//!   - no `crate::common` use — the only 5 users in that harness are the `as_printed_*`
//!     quartet and `fea_cold_start_heuristic_e2e`, all of which stay. So this root
//!     declares no `mod common;` at all and its unit carries ZERO out-of-module-dir
//!     include lines;
//!   - no cross-submodule `super::`/`crate::` reference in either direction — the whole
//!     harness holds exactly ONE such edge, `fea_bracket_minimize_mass_e2e` ->
//!     `fea_design_loop_support`, and both ends are `fea_*` and stay together;
//!   - no `binary(...)`/`test(...)` selector names it. `.config/nextest.toml` never
//!     mentions this harness, and both `scripts/heavy-test-filter-lib.sh` atoms that do
//!     are test-scoped to `fea_in_the_loop_producer` and `fea_bracket_minimize_mass_e2e`
//!     — again `fea_*`, again staying, so no filterset is silently emptied.
//!
//! Substantively it is also the group that does not belong: these are language and
//! geometry robustness scenarios over `.ri` fixtures (dimensional type system, error
//! message quality, large assembly, pattern composition, geometry query consistency,
//! degenerate sweeps, trait hierarchy) — not FEA/solver e2e at all. The split therefore
//! makes BOTH roots more accurately named, not merely smaller. `tensegrity_*` would have
//! freed more lines, but form-finding IS solver work and belongs with its siblings.
//!
//! Registry note: the departed group runs with a strictly SMALLER kernel inventory here,
//! and that was checked rather than assumed. `harness_fea_solver_e2e` links gmsh via the
//! deliberate `extern crate reify_kernel_gmsh as _;` anchor in `fea_face_selector_bc_e2e`;
//! this binary has no such anchor. The modules below reference only `reify_kernel_occt`,
//! register it explicitly via `register_kernel(OcctKernelHandle::spawn())`, gate on
//! `OCCT_AVAILABLE`, demand no `ReprKind::*`, and never touch `ensure_gmsh_kernel` or
//! VolumeMesh — so an OCCT-only registry is not a behaviour change for them. Adding a
//! manifold/gmsh anchor "just in case" is explicitly forbidden by the dead-strip
//! invariant documented at `crates/reify-eval/Cargo.toml`: pulling in a kernel's
//! `inventory::submit!` breaks the registry-size and default-kernel assertions.
//!
//! Explicit `#[path]` is required: this harness root is an integration-test crate root,
//! where a bare `mod <file>;` would resolve to a sibling `tests/<file>.rs`, not the
//! `harness_stress_scenarios/` subdir.

#[path = "harness_stress_scenarios/stress_dimensional_chains.rs"]
mod stress_dimensional_chains;
#[path = "harness_stress_scenarios/stress_error_messages.rs"]
mod stress_error_messages;
#[path = "harness_stress_scenarios/stress_large_assembly.rs"]
mod stress_large_assembly;
#[path = "harness_stress_scenarios/stress_pattern_composition.rs"]
mod stress_pattern_composition;
#[path = "harness_stress_scenarios/stress_query_consistency.rs"]
mod stress_query_consistency;
#[path = "harness_stress_scenarios/stress_sweep_degenerate.rs"]
mod stress_sweep_degenerate;
#[path = "harness_stress_scenarios/stress_trait_hierarchy.rs"]
mod stress_trait_hierarchy;
