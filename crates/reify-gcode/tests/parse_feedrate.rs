//! Standalone-feedrate parsing (PRD §7.1 Marlin subset, bare `F<value>`).
//!
//! Step-7 RED test for task μ: pins GcodeCommand::Feedrate plus the
//! coexistence of bare-F and in-line-F (G1's `feedrate` field).

use reify_gcode::ast::{Feedrate, GcodeCommand, LinearMove};
use reify_gcode::parse_marlin;

#[test]
fn standalone_feedrate_integer() {
    let got = parse_marlin("F2000").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::Feedrate(Feedrate { value: 2000.0 })]
    );
}

#[test]
fn standalone_feedrate_fractional() {
    let got = parse_marlin("F1500.5").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::Feedrate(Feedrate { value: 1500.5 })]
    );
}

#[test]
fn standalone_then_inline_feedrate_coexist() {
    let got = parse_marlin("F800\nG1 X10 F1200").unwrap();
    assert_eq!(
        got,
        vec![
            GcodeCommand::Feedrate(Feedrate { value: 800.0 }),
            GcodeCommand::LinearMove(LinearMove {
                rapid: false,
                x: Some(10.0),
                y: None,
                z: None,
                e: None,
                feedrate: Some(1200.0),
            }),
        ]
    );
}
