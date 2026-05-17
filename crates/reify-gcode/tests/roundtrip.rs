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
