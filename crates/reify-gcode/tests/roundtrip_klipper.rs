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

    // Fixed-point: a second Display → parse cycle must yield the same
    // AST. Catches regressions where Display normalizes input on the
    // first pass but then mutates further on subsequent passes (a
    // class of bug pure ast1 == ast2 wouldn't catch, since the first
    // cycle hides drift between source and canonical form).
    let rendered2: String = ast2
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let ast3 = parse_klipper(&rendered2)
        .expect("second re-parse of Display output must succeed");
    assert_eq!(ast2, ast3);
}
