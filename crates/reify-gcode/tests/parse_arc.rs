//! G2 / G3 arc-move parsing (PRD §7.1 Marlin subset, IJK form).
//!
//! Step-3 RED test for task μ: pins ArcMove + ArcDirection AST nodes
//! and the G2→Cw / G3→Ccw dispatch.

use reify_gcode::ast::{ArcDirection, ArcMove, GcodeCommand};
use reify_gcode::parse_marlin;

#[test]
fn g2_planar_arc_cw_with_feedrate() {
    let got = parse_marlin("G2 X10 Y10 I5 J0 F1500").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::ArcMove(ArcMove {
            direction: ArcDirection::Cw,
            x: Some(10.0),
            y: Some(10.0),
            z: None,
            i: Some(5.0),
            j: Some(0.0),
            k: None,
            e: None,
            feedrate: Some(1500.0),
        })]
    );
}

#[test]
fn g3_planar_arc_ccw_no_feedrate() {
    let got = parse_marlin("G3 X0 Y0 I-5 J0").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::ArcMove(ArcMove {
            direction: ArcDirection::Ccw,
            x: Some(0.0),
            y: Some(0.0),
            z: None,
            i: Some(-5.0),
            j: Some(0.0),
            k: None,
            e: None,
            feedrate: None,
        })]
    );
}

#[test]
fn g2_three_axis_arc_with_k_offset() {
    let got = parse_marlin("G2 X1 Y2 Z3 I0.5 J-0.5 K1.0 F600").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::ArcMove(ArcMove {
            direction: ArcDirection::Cw,
            x: Some(1.0),
            y: Some(2.0),
            z: Some(3.0),
            i: Some(0.5),
            j: Some(-0.5),
            k: Some(1.0),
            e: None,
            feedrate: Some(600.0),
        })]
    );
}
