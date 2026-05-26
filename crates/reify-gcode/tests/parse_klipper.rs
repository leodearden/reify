//! Klipper-dialect parser tests — PRD §11 task ν.
//!
//! Cases here pin the `parse_klipper` entry point's behavior on
//! Klipper-specific directives (`SET_VELOCITY_LIMIT`, `INPUT_SHAPER`).
//! Per design decision #2 the `INPUT_SHAPER` AST identity is the only
//! signal this task ships — `W_GcodeDialectShaperConflict` emission is
//! deferred to consumer task ο (`gcode_import`).

use reify_gcode::ast::{GcodeCommand, InputShaper, SetVelocityLimit};
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
