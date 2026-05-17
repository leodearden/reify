//! Marlin-dialect parser.
//!
//! Line-based hand-written tokenizer — each line is `[CMD] [PARAM]*`
//! where PARAM is `<letter><number>`. See plan design decision #2 for
//! why a parser-generator dep is avoided.
//!
//! Populated incrementally by the TDD steps in
//! `docs/prds/v0_3/trajectory-input-shaping.md` §11 task μ.

use crate::ast::{GcodeCommand, LinearMove};
use crate::error::{ParseError, ParseErrorKind};

/// Parse a Marlin-dialect G-code source into a sequence of commands.
///
/// Errors short-circuit: the first failing line aborts the whole parse.
/// See plan design decision #8 for why partial-success recovery is out
/// of scope at v0.3.
pub fn parse_marlin(src: &str) -> Result<Vec<GcodeCommand>, ParseError> {
    let mut out = Vec::new();
    for (idx, raw) in src.split('\n').enumerate() {
        let line_no = idx + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(parse_line(line_no, trimmed)?);
    }
    Ok(out)
}

/// Parse a single non-empty line. The caller is responsible for skipping
/// blank lines and bumping the 1-indexed line counter.
fn parse_line(line_no: usize, line: &str) -> Result<GcodeCommand, ParseError> {
    let mut tokens = line.split_whitespace();
    // First token is always the command code in the cases supported so far.
    let cmd = tokens.next().ok_or(ParseError {
        line: line_no,
        kind: ParseErrorKind::MissingCommand,
    })?;
    let params: Vec<&str> = tokens.collect();

    match cmd {
        "G0" => Ok(GcodeCommand::LinearMove(linear_move(
            line_no, true, &params,
        )?)),
        "G1" => Ok(GcodeCommand::LinearMove(linear_move(
            line_no, false, &params,
        )?)),
        other => Err(ParseError {
            line: line_no,
            kind: ParseErrorKind::UnknownCommand(other.to_string()),
        }),
    }
}

/// Materialise a `LinearMove` from the parameter slice of a G0/G1 line.
fn linear_move(line_no: usize, rapid: bool, params: &[&str]) -> Result<LinearMove, ParseError> {
    let mut mv = LinearMove {
        rapid,
        x: None,
        y: None,
        z: None,
        e: None,
        feedrate: None,
    };
    for tok in params {
        let (letter, value) = split_param(line_no, tok)?;
        let parsed = parse_value(line_no, letter, value)?;
        match letter {
            'X' => mv.x = Some(parsed),
            'Y' => mv.y = Some(parsed),
            'Z' => mv.z = Some(parsed),
            'E' => mv.e = Some(parsed),
            'F' => mv.feedrate = Some(parsed),
            _ => {
                return Err(ParseError {
                    line: line_no,
                    kind: ParseErrorKind::InvalidParameter {
                        letter,
                        value: value.to_string(),
                    },
                });
            }
        }
    }
    Ok(mv)
}

/// Split a `<letter><value>` token into its parts. Errors if the letter
/// is missing or non-ASCII-alphabetic.
fn split_param(line_no: usize, tok: &str) -> Result<(char, &str), ParseError> {
    let mut chars = tok.chars();
    let letter = chars.next().ok_or(ParseError {
        line: line_no,
        kind: ParseErrorKind::MissingCommand,
    })?;
    if !letter.is_ascii_alphabetic() {
        return Err(ParseError {
            line: line_no,
            kind: ParseErrorKind::InvalidParameter {
                letter,
                value: tok.to_string(),
            },
        });
    }
    let value = &tok[letter.len_utf8()..];
    Ok((letter.to_ascii_uppercase(), value))
}

/// Parse a parameter's numeric body as `f64`, translating failures into
/// `InvalidParameter` with the offending letter + raw value preserved.
fn parse_value(line_no: usize, letter: char, value: &str) -> Result<f64, ParseError> {
    value.parse::<f64>().map_err(|_| ParseError {
        line: line_no,
        kind: ParseErrorKind::InvalidParameter {
            letter,
            value: value.to_string(),
        },
    })
}
