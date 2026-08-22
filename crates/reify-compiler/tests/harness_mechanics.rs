//! Consolidated integration-test harness for the mechanics & simulation subsystem —
//! how a part MOVES and how it RESPONDS: trajectory planning and motion shaping,
//! kinematics, dynamics, modal analysis, mechanisms and their joints, motion-value
//! coupling, tensegrity, buckling, flexures, FEA supertrait conformance, constitutive
//! models, ground sugar and anisotropic bars — beside the motion-shaping worked
//! examples (`tots` optimal point-to-point, ZV-shaped ramp) swept in with them.
//!
//! Task #5694 (PRD `docs/prds/merge-gate-compile-cost.md` §3 W1 / §5 C1, leaf CMP-4,
//! batch 4 of 6): folds 18 former standalone `tests/*.rs` binaries into this single
//! compile unit to cut the merge-gate link count. Layout-only — no `#[test]` fn is
//! added or removed (invariant I3); the test *ids* change with the layout, which is
//! the designed consequence, not a regression.
//!
//! The layout contract those declarations obey — stem-named modules, the mandatory
//! `#[path]`, the kLOC cap and the baseline ratchet — is stated ONCE, in
//! `tests/infra/test_harness_kloc_cap.sh`'s C1/C2 header, and mechanically enforced
//! by that guard. Deliberately not restated here.
//!
//! Why `#[path]` is not optional in this file: a harness root is an integration-test
//! CRATE root, so a bare `mod trajectory_stdlib_compile;` resolves against
//! `tests/`, not against `tests/harness_mechanics/`. Mid-move — while the old
//! top-level `tests/<stem>.rs` still exists — that would SILENTLY bind the stale
//! file instead of failing, and the fold would look green while testing the wrong
//! source. The explicit `#[path]` makes the pre-move state a hard error.
//!
//! The one fact specific to THIS unit: it DOES declare `common`, by design rather
//! than by oversight — `flexure_dimension_types` consumes `tests/common/mod.rs`
//! (`common::stdlib_param_si_value`, `common::compile_with_stdlib_helper`). One
//! consumer is enough to require the include, so this unit's `external_lines` of 363
//! is expected and charged on purpose, not stray. It is declared ONCE here because a
//! per-member `mod common;` would load the same source repeatedly in one compile unit
//! (`clippy::duplicate_mod`, an error under `-D warnings`, even though nextest still
//! passes); members reach it as `use crate::common[::…]`.
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
