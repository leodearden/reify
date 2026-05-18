//! G0 / G1 linear-move parsing (PRD §7.1 Marlin subset).
//!
//! Step-1 RED test for task μ: pins the public shape of `parse_marlin` and
//! the `GcodeCommand::LinearMove(LinearMove { … })` AST node for the four
//! canonical linear-move cases.

use reify_gcode::ast::{GcodeCommand, LinearMove};
use reify_gcode::parse_marlin;

#[test]
fn g1_full_xyz_with_feedrate() {
    let got = parse_marlin("G1 X10 Y20 Z5 F1500").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::LinearMove(LinearMove {
            rapid: false,
            x: Some(10.0),
            y: Some(20.0),
            z: Some(5.0),
            e: None,
            feedrate: Some(1500.0),
        })]
    );
}

#[test]
fn g0_rapid_single_axis() {
    let got = parse_marlin("G0 X100").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::LinearMove(LinearMove {
            rapid: true,
            x: Some(100.0),
            y: None,
            z: None,
            e: None,
            feedrate: None,
        })]
    );
}

#[test]
fn g1_extruder_only_move() {
    let got = parse_marlin("G1 E0.5").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::LinearMove(LinearMove {
            rapid: false,
            x: None,
            y: None,
            z: None,
            e: Some(0.5),
            feedrate: None,
        })]
    );
}

#[test]
fn g1_negative_and_fractional_values() {
    let got = parse_marlin("G1 X1.5 Y-2.25 F800.0").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::LinearMove(LinearMove {
            rapid: false,
            x: Some(1.5),
            y: Some(-2.25),
            z: None,
            e: None,
            feedrate: Some(800.0),
        })]
    );
}
