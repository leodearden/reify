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

use crate::ast::{GcodeCommand, InputShaper, SetVelocityLimit};
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
        "INPUT_SHAPER" => {
            let params = parse_kv_params(line_no, tokens)?;
            Ok(GcodeCommand::InputShaper(InputShaper { params }))
        }
        // Fallback: delegation to `marlin::parse_line` is the
        // authoritative dispatch for shared G/M codes (G0/G1/G2/G3/G92,
        // standalone F, M82/M83/M104/M109). Do NOT duplicate those arms
        // here — extending dispatch belongs in `marlin.rs` so both
        // dialects stay in lock-step. Unknown leading tokens flow
        // through the same path and produce the marlin
        // `UnknownCommand(token)` diagnostic with the original line
        // number, matching the cross-dialect error contract.
        _ => marlin::parse_line(line_no, line),
    }
}

/// Walk `tokens` and parse each as `KEY=VALUE`, returning the ordered
/// vec.
///
/// # Split contract (design decision #5)
///
/// Tokens are split at the **first** `=` only — `str::find('=')` returns
/// the earliest match, so the value substring carries any subsequent
/// `=` chars verbatim. This means `K=A=B` parses to `("K", "A=B")`,
/// which is what makes round-trip viable for Klipper macro arguments
/// that legitimately carry `=` inside values (e.g. nested expressions,
/// comma-lists, default-value sentinels).
///
/// The KV asymmetry is intentional:
///
/// - **Empty value (`KEY=`) is ACCEPTED** — semantically meaningful
///   (e.g. "clear this parameter") and harmless to preserve through
///   the round-trip.
/// - **Empty key (`=200`) is REJECTED** — meaningless because there is
///   no symbol the consumer can dispatch on; surfacing it as a parse
///   error catches a real malformed-input bug.
/// - **No `=` at all** — same `InvalidParameter` treatment as empty
///   key; the token is structurally not a KV pair.
///
/// # Error reporting
///
/// Malformed-token errors are reported via
/// [`ParseErrorKind::InvalidParameter`] with `letter: '='` and the raw
/// token preserved in `value`. This is a **semantic approximation** —
/// the leading `letter` doesn't make perfect sense for a `KEY=VALUE`
/// token, but it preserves the raw text in `value` so diagnostics still
/// carry the offending input.
///
/// There is no dedicated `MalformedDirectiveParam` variant in
/// `error.rs` because that module is shared with the Marlin dialect
/// (touched by every dialect's scope-lock) and is out of scope for
/// task ν. See plan design decision #4 — when error-rendering needs
/// to distinguish KV-malformation from axis-letter malformation, the
/// fix is to add a dedicated variant in `error.rs` rather than to
/// reinterpret the `letter: '='` sentinel here.
fn parse_kv_params<'a>(
    line_no: usize,
    tokens: impl Iterator<Item = &'a str>,
) -> Result<Vec<(String, String)>, ParseError> {
    let mut out = Vec::new();
    for tok in tokens {
        // `find('=')` returns the FIRST occurrence — see split contract
        // above. The value substring (tok[eq_idx + 1..]) is allowed to be
        // empty (KEY= is accepted) or contain further `=` chars (K=A=B).
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
        // Empty key (`=value`) is rejected — see split contract above.
        if key.is_empty() {
            return Err(ParseError {
                line: line_no,
                kind: ParseErrorKind::InvalidParameter {
                    letter: '=',
                    value: tok.to_string(),
                },
            });
        }
        out.push((key.to_string(), value.to_string()));
    }
    Ok(out)
}
