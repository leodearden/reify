//! AST round-trip — `parse(display(parse(src))) == parse(src)`.
//!
//! Step-15 RED test for task μ. Pins the round-trip contract (PRD §11.μ
//! + §10.1 G-code Marlin parser smoke):
//!
//! 1. A hand-authored fixture round-trips bit-exactly at the AST level.
//! 2. A programmatically-constructed LinearMove with fractional values
//!    round-trips bit-exactly, isolating the float-formatting path from
//!    fixture choice.

use reify_gcode::ast::{GcodeCommand, LinearMove};
use reify_gcode::parse_marlin;

const SIMPLE_MOVES: &str = include_str!("fixtures/simple_moves.gcode");

#[test]
fn fixture_roundtrip_preserves_ast() {
    let ast1 = parse_marlin(SIMPLE_MOVES).expect("fixture must parse");
    let rendered: String = ast1
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let ast2 = parse_marlin(&rendered)
        .expect("re-parse of Display output must succeed");
    assert_eq!(ast1, ast2);
}

#[test]
fn linear_move_with_fractional_values_roundtrips() {
    let original = vec![GcodeCommand::LinearMove(LinearMove {
        rapid: false,
        x: Some(1.5),
        y: Some(-2.25),
        z: None,
        e: None,
        feedrate: Some(800.0),
    })];
    let rendered = original[0].to_string();
    let reparsed = parse_marlin(&rendered).expect("re-parse must succeed");
    assert_eq!(original, reparsed);
}

// The previous case uses only values with exact binary representations
// (1.5, -2.25, 800.0) — so it doesn't actually exercise the Display
// impl's stated contract that `f64::from_str(format!("{}", x)) == x`
// for arbitrary finite x. This case pins that contract on
// representative inexact values: 0.1 (repeating binary), 1/3
// (repeating binary), and a subnormal-adjacent tiny value 1e-15.
#[test]
fn linear_move_with_inexact_floats_roundtrips_bit_exact() {
    let original = vec![GcodeCommand::LinearMove(LinearMove {
        rapid: false,
        x: Some(0.1),
        y: Some(1.0 / 3.0),
        z: Some(1e-15),
        e: None,
        feedrate: None,
    })];
    let rendered = original[0].to_string();
    let reparsed = parse_marlin(&rendered).expect("re-parse must succeed");
    assert_eq!(original, reparsed);
}
