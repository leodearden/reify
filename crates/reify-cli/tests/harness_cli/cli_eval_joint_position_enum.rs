//! CLI signal for S6 leaf η #8834
//! (docs/prds/v0_6/kinematics-typed-kinds-and-mechanism-record.md §4 C4):
//! `reify eval` on a fixture that constructs the stdlib `JointPosition` from
//! bare variant heads, with no local enum, prints each variant's named payload
//! fields after its `Type::Variant` head:
//!   `P.rev = JointPosition::RevolutePosition { angle: 0.7853981633974483 rad }`
//!   `P.planar = JointPosition::PlanarPosition { x: 0.001 m, y: 0.002 m, angle: 0.5 rad }`
//!   `P.planar_reordered = …` (the same line: fields print in declaration
//!                            order, whatever order the construction site used)
//!   `P.unit_variant = EulerConvention::XYZ` (a unit variant prints its head only)
//!
//! Every line is matched EXACTLY, so a payload that went missing, a field
//! printed out of declaration order, or a unit variant that gained a ` { }`
//! suffix all red.

use crate::common;

const FIXTURE: &str = "tests/prd-gate/fixtures/s6_joint_position_display.ri";

#[test]
fn eval_prints_joint_position_payload_fields() {
    let path = common::workspace_root().join(FIXTURE);
    let (status, stdout, stderr) = common::run_subcommand("eval", &path.to_string_lossy());

    assert!(
        status.success(),
        "reify eval should exit 0 for {FIXTURE}\nstdout: {stdout}\nstderr: {stderr}"
    );
    for expected in [
        "P.rev = JointPosition::RevolutePosition { angle: 0.7853981633974483 rad }",
        "P.planar = JointPosition::PlanarPosition { x: 0.001 m, y: 0.002 m, angle: 0.5 rad }",
        "P.planar_reordered = JointPosition::PlanarPosition { x: 0.001 m, y: 0.002 m, angle: 0.5 rad }",
        "P.unit_variant = EulerConvention::XYZ",
    ] {
        assert!(
            stdout.lines().any(|line| line == expected),
            "stdout should contain the exact line {expected:?}\nstdout: {stdout}"
        );
    }
}
