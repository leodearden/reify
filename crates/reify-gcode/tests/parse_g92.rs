//! G92 set-position parsing (PRD §7.1 Marlin subset).
//!
//! Step-5 RED test for task μ: pins GcodeCommand::SetPosition for the
//! three canonical shapes — full XYZ, extruder-only filament reset, and
//! bare G92 (all-None placeholder).

use reify_gcode::ast::{GcodeCommand, SetPosition};
use reify_gcode::parse_marlin;

#[test]
fn g92_full_xyz_zero() {
    let got = parse_marlin("G92 X0 Y0 Z0").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::SetPosition(SetPosition {
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
            e: None,
        })]
    );
}

#[test]
fn g92_extruder_only_filament_reset() {
    let got = parse_marlin("G92 E0").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::SetPosition(SetPosition {
            x: None,
            y: None,
            z: None,
            e: Some(0.0),
        })]
    );
}

#[test]
fn g92_no_params_all_none() {
    let got = parse_marlin("G92").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::SetPosition(SetPosition {
            x: None,
            y: None,
            z: None,
            e: None,
        })]
    );
}
