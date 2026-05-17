//! Parse-error diagnostics with 1-indexed source line numbers
//! (PRD §1 `E_GcodeParseError`).
//!
//! Step-13 RED test for task μ: pins UnknownCommand vs InvalidParameter
//! classification, and confirms that the line counter advances on every
//! physical line (blank + comment-only included).

use reify_gcode::error::{ParseError, ParseErrorKind};
use reify_gcode::parse_marlin;

#[test]
fn unknown_g_code_on_second_line() {
    let err = parse_marlin("G1 X10\nG99 X5").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 2,
            kind: ParseErrorKind::UnknownCommand("G99".to_string()),
        }
    );
}

#[test]
fn unknown_m_code_on_first_line() {
    let err = parse_marlin("M999 S1").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::UnknownCommand("M999".to_string()),
        }
    );
}

#[test]
fn invalid_parameter_float_two_dots() {
    let err = parse_marlin("G1 X1.2.3").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::InvalidParameter {
                letter: 'X',
                value: "1.2.3".to_string(),
            },
        }
    );
}

#[test]
fn line_counter_counts_blank_and_comment_lines() {
    let err = parse_marlin("G1 X10\n\n; comment\nGarbage").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 4,
            kind: ParseErrorKind::UnknownCommand("Garbage".to_string()),
        }
    );
}
