//! Marlin M-code parsing (PRD §7.1 — M104/M109/M82/M83 "ignored for
//! trajectory").
//!
//! Step-9 RED test for task μ: pins GcodeCommand::IgnoredMCode with
//! verbatim `params_raw` and confirms source-order preservation in a
//! mixed M/G sequence.

use reify_gcode::ast::{GcodeCommand, IgnoredMCode, LinearMove};
use reify_gcode::parse_marlin;

#[test]
fn m104_extruder_temp_with_param() {
    let got = parse_marlin("M104 S200").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::IgnoredMCode(IgnoredMCode {
            code: 104,
            params_raw: "S200".to_string(),
        })]
    );
}

#[test]
fn m109_extruder_temp_wait() {
    let got = parse_marlin("M109 S210").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::IgnoredMCode(IgnoredMCode {
            code: 109,
            params_raw: "S210".to_string(),
        })]
    );
}

#[test]
fn m82_no_params() {
    let got = parse_marlin("M82").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::IgnoredMCode(IgnoredMCode {
            code: 82,
            params_raw: String::new(),
        })]
    );
}

#[test]
fn m83_no_params() {
    let got = parse_marlin("M83").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::IgnoredMCode(IgnoredMCode {
            code: 83,
            params_raw: String::new(),
        })]
    );
}

#[test]
fn mixed_mcode_gcode_sequence_preserves_order() {
    let got = parse_marlin("M104 S200\nG1 X10\nM82").unwrap();
    assert_eq!(
        got,
        vec![
            GcodeCommand::IgnoredMCode(IgnoredMCode {
                code: 104,
                params_raw: "S200".to_string(),
            }),
            GcodeCommand::LinearMove(LinearMove {
                rapid: false,
                x: Some(10.0),
                y: None,
                z: None,
                e: None,
                feedrate: None,
            }),
            GcodeCommand::IgnoredMCode(IgnoredMCode {
                code: 82,
                params_raw: String::new(),
            }),
        ]
    );
}
