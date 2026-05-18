//! Comment and whitespace handling.
//!
//! Step-11 RED test for task μ: pins the `;`-to-EOL comment strip,
//! blank/whitespace-only line skipping, and tolerance to inter-token
//! whitespace runs.

use reify_gcode::ast::{GcodeCommand, LinearMove};
use reify_gcode::parse_marlin;

fn single_g1_x10() -> GcodeCommand {
    GcodeCommand::LinearMove(LinearMove {
        rapid: false,
        x: Some(10.0),
        y: None,
        z: None,
        e: None,
        feedrate: None,
    })
}

#[test]
fn full_line_comment_is_skipped() {
    let got = parse_marlin("; full-line comment\nG1 X10").unwrap();
    assert_eq!(got, vec![single_g1_x10()]);
}

#[test]
fn trailing_comment_stripped_before_param_parse() {
    let got = parse_marlin("G1 X10 ; trailing comment").unwrap();
    assert_eq!(got, vec![single_g1_x10()]);
}

#[test]
fn blank_and_whitespace_only_lines_are_skipped() {
    let got = parse_marlin("\n   \n\t\nG1 X10\n\n").unwrap();
    assert_eq!(got, vec![single_g1_x10()]);
}

#[test]
fn extra_inter_token_whitespace_tolerated() {
    let got = parse_marlin("  G1   X10    Y20  ").unwrap();
    assert_eq!(
        got,
        vec![GcodeCommand::LinearMove(LinearMove {
            rapid: false,
            x: Some(10.0),
            y: Some(20.0),
            z: None,
            e: None,
            feedrate: None,
        })]
    );
}

// CRLF line endings are common in slicer output (Cura/PrusaSlicer on
// Windows). The parser splits on `\n` only; the trailing `\r` is
// consumed by `strip_comment_and_trim`'s generic `.trim()`. Pin that
// contract so a future refactor that switches to a more restrictive
// trim won't silently break Windows-authored G-code.
#[test]
fn crlf_line_endings_accepted() {
    let got = parse_marlin("G1 X10\r\nG1 Y20\r\n").unwrap();
    assert_eq!(
        got,
        vec![
            GcodeCommand::LinearMove(LinearMove {
                rapid: false,
                x: Some(10.0),
                y: None,
                z: None,
                e: None,
                feedrate: None,
            }),
            GcodeCommand::LinearMove(LinearMove {
                rapid: false,
                x: None,
                y: Some(20.0),
                z: None,
                e: None,
                feedrate: None,
            }),
        ]
    );
}
