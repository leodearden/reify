//! Consolidated integration-test harness for the mechanics & simulation subsystem —
//! how a part MOVES and how it RESPONDS: trajectory and motion shaping, kinematics,
//! dynamics, modal analysis, mechanisms and their joints, motion-value coupling,
//! tensegrity, buckling, flexures, FEA supertrait conformance, constitutive models,
//! ground sugar and anisotropic bars — beside the motion-shaping worked examples
//! (`tots` optimal point-to-point, ZV-shaped ramp) swept in with them.
//!
//! Task #5694 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-4,
//! batch 4 of 6): folds 18 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count.
//!
//! Layout contract C1 — stem-named modules, why the `#[path]` on every declaration
//! below is mandatory rather than stylistic, the kLOC cap, the baseline ratchet, and
//! the by-design test-id change (no `#[test]` fn added or removed, invariant I3):
//! see `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, kept there, not
//! restated here.
//!
//! Crate-local: this harness declares the shared `common` helper ONCE, because a
//! per-member `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`). Its one consumer, `flexure_dimension_types`, reaches it
//! as `use crate::common[::…]` — so this unit's `external_lines` charge is that one
//! shared `tests/common/mod.rs`, expected rather than stray. The line count itself is
//! computed by `harness_layout_unit_lines` (tests/infra/harness-layout-lib.sh) and
//! deliberately not pinned here, where nothing would recompute it.
//!
//! Routing vs. the sibling `harness_physical_modeling` root added by this same leaf —
//! the line is subsystem, not filename. THIS root holds mechanics and behaviour: what
//! the part DOES under load or motion. `harness_physical_modeling` holds what the part
//! is MADE of and HOW it is made (materials, units and constants, process, printer
//! envelope, stackup) beside the geometry-parameter primitives swept with it.
#[path = "common/mod.rs"]
mod common;
#[path = "harness_mechanics/anisotropic_bar_example_tests.rs"]
mod anisotropic_bar_example_tests;
#[path = "harness_mechanics/buckling_stdlib_compile.rs"]
mod buckling_stdlib_compile;
#[path = "harness_mechanics/constitutive_stdlib_compile.rs"]
mod constitutive_stdlib_compile;
#[path = "harness_mechanics/coupling_motionvalue_integration_gate.rs"]
mod coupling_motionvalue_integration_gate;
#[path = "harness_mechanics/dynamics_stdlib_compile.rs"]
mod dynamics_stdlib_compile;
#[path = "harness_mechanics/fea_supertrait_conformance_tests.rs"]
mod fea_supertrait_conformance_tests;
#[path = "harness_mechanics/flexure_dimension_types.rs"]
mod flexure_dimension_types;
#[path = "harness_mechanics/flexures_stdlib_compile.rs"]
mod flexures_stdlib_compile;
#[path = "harness_mechanics/ground_sugar_tests.rs"]
mod ground_sugar_tests;
#[path = "harness_mechanics/joint_dof_self_check_tests.rs"]
mod joint_dof_self_check_tests;
#[path = "harness_mechanics/kinematic_stdlib_compile.rs"]
mod kinematic_stdlib_compile;
#[path = "harness_mechanics/mechanism_nondriving_joint_compile.rs"]
mod mechanism_nondriving_joint_compile;
#[path = "harness_mechanics/modal_mechanism_compile.rs"]
mod modal_mechanism_compile;
#[path = "harness_mechanics/modal_options_validation_tests.rs"]
mod modal_options_validation_tests;
#[path = "harness_mechanics/tensegrity_stdlib_tests.rs"]
mod tensegrity_stdlib_tests;
#[path = "harness_mechanics/tots_optimal_ptp_example_tests.rs"]
mod tots_optimal_ptp_example_tests;
#[path = "harness_mechanics/trajectory_stdlib_compile.rs"]
mod trajectory_stdlib_compile;
#[path = "harness_mechanics/zv_shaped_ramp_example_tests.rs"]
mod zv_shaped_ramp_example_tests;
