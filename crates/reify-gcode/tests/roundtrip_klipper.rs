//! Klipper-dialect AST round-trip —
//! `parse_klipper(display(parse_klipper(src))) == parse_klipper(src)`.
//!
//! Mirrors `tests/roundtrip.rs` for the Klipper-specific surface (PRD
//! §11 task ν). Pins the round-trip contract over a hand-authored
//! fixture that exercises every Klipper command kind the parser
//! supports — including the new `SET_VELOCITY_LIMIT` and `INPUT_SHAPER`
//! directives plus the shared core delegated to `marlin::parse_line`.

use reify_gcode::parse_klipper;

const KLIPPER_SAMPLE: &str = include_str!("fixtures/klipper_sample.gcode");

#[test]
fn fixture_roundtrip_preserves_ast() {
    let ast1 = parse_klipper(KLIPPER_SAMPLE).expect("fixture must parse");
    let rendered: String = ast1
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let ast2 = parse_klipper(&rendered)
        .expect("re-parse of Display output must succeed");
    assert_eq!(ast1, ast2);
}
