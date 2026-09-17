//! Consolidated integration-test harness for the mechanism subsystem.
//!
//! Task #5278 (PRD docs/prds/merge-gate-compile-cost.md §3 W1 / §5 C1, leaf EVAL-1):
//! folds the former standalone mechanism_ tests/*.rs binaries into this single compile
//! unit to cut the merge-gate link count. Layout-only — no `#[test]` fn added or removed.
//! Each former file is included as a stem-named module (explicit `#[path]`, required at a
//! crate root) so its `<file>::<test>` module path resolves unchanged.
//!
//! Task #6096 added `revolute_trajectory_limits_e2e` here rather than as a new
//! standalone `tests/*.rs` binary (which
//! `scripts/check-harness-baseline-registration.sh` rejects): its per-joint-kind
//! actuator/kinematic limit fixtures are mechanism-domain, so this is the unit
//! they belong to. The stem is not `mechanism_`-prefixed because the naming
//! contract keys on the harness ROOT (`harness_<subsystem>.rs`), not on the
//! module stems it absorbs.
#[path = "harness_mechanism/mechanism_builder_smoke.rs"]
mod mechanism_builder_smoke;
#[path = "harness_mechanism/mechanism_duplicate_solid_diag_e2e.rs"]
mod mechanism_duplicate_solid_diag_e2e;
#[path = "harness_mechanism/mechanism_interference_smoke.rs"]
mod mechanism_interference_smoke;
#[path = "harness_mechanism/mechanism_modal_damping_e2e.rs"]
mod mechanism_modal_damping_e2e;
#[path = "harness_mechanism/mechanism_nondriving_joint_diag_e2e.rs"]
mod mechanism_nondriving_joint_diag_e2e;
#[path = "harness_mechanism/revolute_trajectory_limits_e2e.rs"]
mod revolute_trajectory_limits_e2e;
