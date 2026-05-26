//! Klipper-dialect parser tests — PRD §11 task ν.
//!
//! Cases here pin the `parse_klipper` entry point's behavior on
//! Klipper-specific directives (`SET_VELOCITY_LIMIT`, `INPUT_SHAPER`).
//! Per design decision #2 the `INPUT_SHAPER` AST identity is the only
//! signal this task ships — `W_GcodeDialectShaperConflict` emission is
//! deferred to consumer task ο (`gcode_import`).

use reify_gcode::ast::{
    ArcDirection, ArcMove, GcodeCommand, IgnoredMCode, InputShaper, LinearMove, SetPosition,
    SetVelocityLimit,
};
use reify_gcode::error::{ParseError, ParseErrorKind};
use reify_gcode::parse_klipper;

#[test]
fn set_velocity_limit_two_params_preserves_order() {
    let ast = parse_klipper("SET_VELOCITY_LIMIT VELOCITY=200 ACCEL=3000").expect("must parse");
    assert_eq!(
        ast,
        vec![GcodeCommand::SetVelocityLimit(SetVelocityLimit {
            params: vec![
                ("VELOCITY".to_string(), "200".to_string()),
                ("ACCEL".to_string(), "3000".to_string()),
            ],
        })]
    );
}

#[test]
fn set_velocity_limit_reversed_order_is_preserved() {
    let ast = parse_klipper("SET_VELOCITY_LIMIT ACCEL=3000 VELOCITY=200").expect("must parse");
    assert_eq!(
        ast,
        vec![GcodeCommand::SetVelocityLimit(SetVelocityLimit {
            params: vec![
                ("ACCEL".to_string(), "3000".to_string()),
                ("VELOCITY".to_string(), "200".to_string()),
            ],
        })]
    );
}

#[test]
fn bare_set_velocity_limit_parses_to_empty_params() {
    let ast = parse_klipper("SET_VELOCITY_LIMIT").expect("must parse");
    assert_eq!(
        ast,
        vec![GcodeCommand::SetVelocityLimit(SetVelocityLimit { params: vec![] })]
    );
}

#[test]
fn input_shaper_three_params_preserves_order() {
    let ast = parse_klipper("INPUT_SHAPER SHAPER_TYPE=ei SHAPER_FREQ_X=40 SHAPER_FREQ_Y=42")
        .expect("must parse");
    assert_eq!(
        ast,
        vec![GcodeCommand::InputShaper(InputShaper {
            params: vec![
                ("SHAPER_TYPE".to_string(), "ei".to_string()),
                ("SHAPER_FREQ_X".to_string(), "40".to_string()),
                ("SHAPER_FREQ_Y".to_string(), "42".to_string()),
            ],
        })]
    );
}

#[test]
fn bare_input_shaper_parses_to_empty_params() {
    let ast = parse_klipper("INPUT_SHAPER").expect("must parse");
    assert_eq!(
        ast,
        vec![GcodeCommand::InputShaper(InputShaper { params: vec![] })]
    );
}

// Mixed-source-order passthrough: proves that delegation to
// `marlin::parse_line` works for every non-Klipper command class
// (LinearMove, ArcMove, SetPosition, IgnoredMCode) and that source
// order is preserved across the parse output.
#[test]
fn mixed_marlin_klipper_passthrough_preserves_order() {
    let src = "G1 X10 Y5 F1200\n\
               SET_VELOCITY_LIMIT VELOCITY=150\n\
               M104 S200\n\
               G2 X0 Y10 I-2.5 J0\n\
               G92 E0";
    let ast = parse_klipper(src).expect("mixed source must parse");
    assert_eq!(
        ast,
        vec![
            GcodeCommand::LinearMove(LinearMove {
                rapid: false,
                x: Some(10.0),
                y: Some(5.0),
                z: None,
                e: None,
                feedrate: Some(1200.0),
            }),
            GcodeCommand::SetVelocityLimit(SetVelocityLimit {
                params: vec![("VELOCITY".to_string(), "150".to_string())],
            }),
            GcodeCommand::IgnoredMCode(IgnoredMCode {
                code: 104,
                params_raw: "S200".to_string(),
            }),
            GcodeCommand::ArcMove(ArcMove {
                direction: ArcDirection::Cw,
                x: Some(0.0),
                y: Some(10.0),
                z: None,
                i: Some(-2.5),
                j: Some(0.0),
                k: None,
                e: None,
                feedrate: None,
            }),
            GcodeCommand::SetPosition(SetPosition {
                x: None,
                y: None,
                z: None,
                e: Some(0.0),
            }),
        ]
    );
}

// SET_VELOCITY_LIMIT VELOCITY (no =) — malformed KV token surfaces as
// InvalidParameter with the raw offending token preserved.
// Line-number accuracy: error is on line 2, not line 1 (the prior G1
// line counts but is not the failure site).
#[test]
fn malformed_kv_no_equals_is_invalid_parameter() {
    let err = parse_klipper("G1 X10\nSET_VELOCITY_LIMIT VELOCITY").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 2,
            kind: ParseErrorKind::InvalidParameter {
                letter: '=',
                value: "VELOCITY".to_string(),
            },
        }
    );
}

// Unknown leading token flows through marlin::parse_line and produces
// the marlin UnknownCommand diagnostic — proves the delegation routes
// non-Klipper-directive leading tokens through the shared dispatch.
#[test]
fn unknown_directive_routes_through_marlin_unknown_command() {
    let err = parse_klipper("BOGUS_DIRECTIVE FOO=1").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::UnknownCommand("BOGUS_DIRECTIVE".to_string()),
        }
    );
}
