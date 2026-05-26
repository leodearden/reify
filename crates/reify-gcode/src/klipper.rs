//! Klipper-dialect parser (PRD §11 task ν).
//!
//! Line-based hand-written tokenizer mirroring [`crate::marlin`]. The
//! Klipper parser dispatches Klipper-specific directives (currently
//! `SET_VELOCITY_LIMIT`) at the top of [`parse_klipper_line`] and falls
//! through to [`crate::marlin::parse_line`] for every other line — so
//! the core G0/G1/G2/G3/G92 + standalone `F` + ignored-M-code dispatch
//! is shared (not duplicated). See plan design decision #7.
//!
//! Klipper directive names are matched **uppercase-only**; mixed/lowercase
//! variants fall through to marlin and produce `UnknownCommand`. See
//! plan design decision #6 for why a per-arm case-insensitivity layer is
//! avoided.

use crate::ast::{GcodeCommand, SetVelocityLimit};
use crate::error::{ParseError, ParseErrorKind};
use crate::marlin;

/// Parse a Klipper-dialect G-code source into a sequence of commands.
///
/// # Error contract
///
/// Matches [`crate::parse_marlin`]: 1-indexed physical-line numbers
/// (blank/comment lines counted), short-circuit on first failure.
/// Malformed KEY=VALUE tokens are surfaced as
/// [`ParseErrorKind::InvalidParameter`] with `letter: '='` — a semantic
/// approximation (see design decision #4); the offending raw token is
/// preserved in `value`.
pub fn parse_klipper(src: &str) -> Result<Vec<GcodeCommand>, ParseError> {
    let mut out = Vec::new();
    for (idx, raw) in src.split('\n').enumerate() {
        let line_no = idx + 1;
        let trimmed = marlin::strip_comment_and_trim(raw);
        if trimmed.is_empty() {
            continue;
        }
        out.push(parse_klipper_line(line_no, trimmed)?);
    }
    Ok(out)
}

/// Parse a single non-empty Klipper-dialect line.
///
/// Klipper-specific directives are dispatched here by their leading
/// uppercase keyword. Everything else — every shared G/M code, every
/// standalone `F`, and unrecognised tokens — is delegated to
/// [`marlin::parse_line`]. That delegation is the authoritative dispatch
/// for shared codes; do not duplicate arms here. See plan design
/// decision #7.
fn parse_klipper_line(line_no: usize, line: &str) -> Result<GcodeCommand, ParseError> {
    let mut tokens = line.split_whitespace();
    // `line` is guaranteed non-empty / non-whitespace by `parse_klipper`'s
    // `trimmed.is_empty()` skip, so `split_whitespace` yields ≥1 token.
    let head = tokens
        .next()
        .expect("parse_klipper_line precondition: trimmed line is non-empty");
    match head {
        "SET_VELOCITY_LIMIT" => {
            let params = parse_kv_params(line_no, tokens)?;
            Ok(GcodeCommand::SetVelocityLimit(SetVelocityLimit { params }))
        }
        _ => marlin::parse_line(line_no, line),
    }
}

/// Walk `tokens` and parse each as `KEY=VALUE`, returning the ordered
/// vec.
///
/// Malformed-token errors are reported via
/// [`ParseErrorKind::InvalidParameter`] with `letter: '='` and the raw
/// token preserved in `value`. This is a semantic approximation — there
/// is no dedicated `MalformedDirectiveParam` variant in `error.rs`
/// because `error.rs` is shared with the Marlin dialect and out of
/// scope for task ν. See plan design decision #4.
fn parse_kv_params<'a>(
    line_no: usize,
    tokens: impl Iterator<Item = &'a str>,
) -> Result<Vec<(String, String)>, ParseError> {
    let mut out = Vec::new();
    for tok in tokens {
        let Some(eq_idx) = tok.find('=') else {
            return Err(ParseError {
                line: line_no,
                kind: ParseErrorKind::InvalidParameter {
                    letter: '=',
                    value: tok.to_string(),
                },
            });
        };
        let key = &tok[..eq_idx];
        let value = &tok[eq_idx + 1..];
        out.push((key.to_string(), value.to_string()));
    }
    Ok(out)
}
