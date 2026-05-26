//! Marlin-dialect parser.
//!
//! Line-based hand-written tokenizer — each line is `[CMD] [PARAM]*`
//! where PARAM is `<letter><number>`. See plan design decision #2 for
//! why a parser-generator dep is avoided.
//!
//! Populated incrementally by the TDD steps in
//! `docs/prds/v0_3/trajectory-input-shaping.md` §11 task μ.

use crate::ast::{
    ArcDirection, ArcMove, Feedrate, GcodeCommand, IgnoredMCode, LinearMove, SetPosition,
};
use crate::error::{ParseError, ParseErrorKind};

/// Parse a Marlin-dialect G-code source into a sequence of commands.
///
/// # Error contract
///
/// - The 1-indexed `line` field of every [`ParseError`] is the physical
///   source line — the counter advances on **every** line passed to the
///   iterator, including blank lines and `;`-only comment lines, so
///   error messages line up with editor line numbers. (Regression-pinned
///   by `tests/parse_errors.rs::line_counter_counts_blank_and_comment_lines`.)
/// - Errors short-circuit: the first failing line aborts the whole parse
///   (via `?` propagation through `parse_line`). No further lines are
///   consumed after the first failure. See plan design decision #8 for
///   why partial-success recovery is out of scope at v0.3.
pub fn parse_marlin(src: &str) -> Result<Vec<GcodeCommand>, ParseError> {
    let mut out = Vec::new();
    for (idx, raw) in src.split('\n').enumerate() {
        let line_no = idx + 1;
        let trimmed = strip_comment_and_trim(raw);
        if trimmed.is_empty() {
            continue;
        }
        out.push(parse_line(line_no, trimmed)?);
    }
    Ok(out)
}

/// Strip a `;`-to-EOL Marlin comment (if any) and trim surrounding ASCII
/// whitespace. Tabs / multi-space runs between tokens are preserved
/// inside the returned slice; the per-line tokenizer relies on
/// `split_whitespace` to collapse them.
///
/// Visible to `crate::klipper` so the Klipper parser reuses the
/// comment-stripping logic verbatim (Klipper shares Marlin's `;`-to-EOL
/// comment syntax). See plan reuse-item #2 / design decision #7.
pub(crate) fn strip_comment_and_trim(line: &str) -> &str {
    let body = match line.find(';') {
        Some(idx) => &line[..idx],
        None => line,
    };
    body.trim()
}

/// Parse a single non-empty line. The caller is responsible for skipping
/// blank lines and bumping the 1-indexed line counter.
///
/// Visible to `crate::klipper` so the Klipper parser's non-directive
/// arm can delegate every shared G/M code line through this function —
/// the authoritative dispatch for G0/G1/G2/G3/G92, standalone `F`, and
/// the ignored-M-code allowlist (M82/M83/M104/M109). Do NOT duplicate
/// these arms in `klipper.rs`; extend them here so both dialects stay
/// in lock-step. See plan reuse-item #1 / design decision #7.
pub(crate) fn parse_line(line_no: usize, line: &str) -> Result<GcodeCommand, ParseError> {
    let mut tokens = line.split_whitespace();
    // Caller guarantees `line` is non-empty and non-whitespace (see
    // `strip_comment_and_trim` + the `trimmed.is_empty()` skip in
    // `parse_marlin`), so `split_whitespace` must yield at least one
    // token. The `expect` keeps `ParseErrorKind::MissingCommand`
    // statically-unreachable here, matching its doc-comment in
    // `error.rs`.
    let cmd = tokens
        .next()
        .expect("parse_line precondition: trimmed line is non-empty, split_whitespace yields ≥1 token");
    let params: Vec<&str> = tokens.collect();

    match cmd {
        "G0" => Ok(GcodeCommand::LinearMove(linear_move(
            line_no, true, &params,
        )?)),
        "G1" => Ok(GcodeCommand::LinearMove(linear_move(
            line_no, false, &params,
        )?)),
        "G2" => Ok(GcodeCommand::ArcMove(arc_move(
            line_no,
            ArcDirection::Cw,
            &params,
        )?)),
        "G3" => Ok(GcodeCommand::ArcMove(arc_move(
            line_no,
            ArcDirection::Ccw,
            &params,
        )?)),
        "G92" => Ok(GcodeCommand::SetPosition(set_position(line_no, &params)?)),
        // PRD §7.1 — these M-codes are parsed & retained but tagged as
        // trajectory-irrelevant. The allowlist gate is the explicit
        // match arm; new M-codes (e.g. task ν's Klipper additions) are
        // wired in by extending the arm rather than duplicating the
        // `ignored_mcode` body.
        "M104" => Ok(ignored_mcode(104, line, cmd)),
        "M109" => Ok(ignored_mcode(109, line, cmd)),
        "M82" => Ok(ignored_mcode(82, line, cmd)),
        "M83" => Ok(ignored_mcode(83, line, cmd)),
        other => {
            // Standalone feedrate: the leading token is itself an `F<number>`
            // parameter rather than a recognised G/M command code. Reject
            // `F` with no numeric body so the InvalidParameter diagnostic
            // path (per plan step-8) is observable.
            if let Some(body) = other.strip_prefix('F').or_else(|| other.strip_prefix('f')) {
                if body.is_empty() {
                    return Err(ParseError {
                        line: line_no,
                        kind: ParseErrorKind::InvalidParameter {
                            letter: 'F',
                            value: String::new(),
                        },
                    });
                }
                if !params.is_empty() {
                    // Bare-F lines carry no trailing params; anything else
                    // is malformed. Issue `UnexpectedTrailingTokens` (not
                    // `UnknownCommand`) so the diagnostic doesn't claim
                    // the F-prefix command itself was unrecognised — a
                    // user debugging `F100 X10` should be told the F is
                    // fine and the X10 is the problem.
                    return Err(ParseError {
                        line: line_no,
                        kind: ParseErrorKind::UnexpectedTrailingTokens {
                            command: other.to_string(),
                            tokens: params.iter().map(|t| (*t).to_string()).collect(),
                        },
                    });
                }
                let value = parse_value(line_no, 'F', body)?;
                return Ok(GcodeCommand::Feedrate(Feedrate { value }));
            }
            Err(ParseError {
                line: line_no,
                kind: ParseErrorKind::UnknownCommand(other.to_string()),
            })
        }
    }
}

/// Build an `IgnoredMCode` command — `params_raw` is the post-code
/// remainder of the source line, trimmed of leading whitespace, so a
/// bare `M82` yields an empty `params_raw` and round-trip Display
/// reconstructs the original. The caller (the `M104`/`M109`/`M82`/`M83`
/// match arms in `parse_line`) supplies the literal code so the
/// allowlist gate stays explicit at the dispatch site.
fn ignored_mcode(code: u16, line: &str, cmd: &str) -> GcodeCommand {
    GcodeCommand::IgnoredMCode(IgnoredMCode {
        code,
        params_raw: line[cmd.len()..].trim_start().to_string(),
    })
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
    parse_axis_params(line_no, params, |letter, value| match letter {
        'X' => {
            mv.x = Some(value);
            Ok(())
        }
        'Y' => {
            mv.y = Some(value);
            Ok(())
        }
        'Z' => {
            mv.z = Some(value);
            Ok(())
        }
        'E' => {
            mv.e = Some(value);
            Ok(())
        }
        'F' => {
            mv.feedrate = Some(value);
            Ok(())
        }
        _ => Err(letter),
    })?;
    Ok(mv)
}

/// Materialise an `ArcMove` from the parameter slice of a G2/G3 line.
fn arc_move(
    line_no: usize,
    direction: ArcDirection,
    params: &[&str],
) -> Result<ArcMove, ParseError> {
    let mut mv = ArcMove {
        direction,
        x: None,
        y: None,
        z: None,
        i: None,
        j: None,
        k: None,
        e: None,
        feedrate: None,
    };
    parse_axis_params(line_no, params, |letter, value| match letter {
        'X' => {
            mv.x = Some(value);
            Ok(())
        }
        'Y' => {
            mv.y = Some(value);
            Ok(())
        }
        'Z' => {
            mv.z = Some(value);
            Ok(())
        }
        'I' => {
            mv.i = Some(value);
            Ok(())
        }
        'J' => {
            mv.j = Some(value);
            Ok(())
        }
        'K' => {
            mv.k = Some(value);
            Ok(())
        }
        'E' => {
            mv.e = Some(value);
            Ok(())
        }
        'F' => {
            mv.feedrate = Some(value);
            Ok(())
        }
        _ => Err(letter),
    })?;
    Ok(mv)
}

/// Materialise a `SetPosition` from the parameter slice of a G92 line.
/// All axes default to `None`; a bare `G92` is therefore a valid all-None
/// command per Marlin semantics.
fn set_position(line_no: usize, params: &[&str]) -> Result<SetPosition, ParseError> {
    let mut sp = SetPosition {
        x: None,
        y: None,
        z: None,
        e: None,
    };
    parse_axis_params(line_no, params, |letter, value| match letter {
        'X' => {
            sp.x = Some(value);
            Ok(())
        }
        'Y' => {
            sp.y = Some(value);
            Ok(())
        }
        'Z' => {
            sp.z = Some(value);
            Ok(())
        }
        'E' => {
            sp.e = Some(value);
            Ok(())
        }
        _ => Err(letter),
    })?;
    Ok(sp)
}

/// Shared `<letter><number>` parameter walker.
///
/// For each token, splits the leading letter from the numeric body,
/// parses the body as `f64`, then invokes `assign(letter, value)`.
/// `assign` returns `Err(letter)` to reject a letter it does not handle
/// for this command; the walker maps that into an `InvalidParameter`
/// error preserving the original raw value.
fn parse_axis_params<F>(line_no: usize, params: &[&str], mut assign: F) -> Result<(), ParseError>
where
    F: FnMut(char, f64) -> Result<(), char>,
{
    for tok in params {
        let (letter, raw_value) = split_param(line_no, tok)?;
        let value = parse_value(line_no, letter, raw_value)?;
        if let Err(bad_letter) = assign(letter, value) {
            return Err(ParseError {
                line: line_no,
                kind: ParseErrorKind::InvalidParameter {
                    letter: bad_letter,
                    value: raw_value.to_string(),
                },
            });
        }
    }
    Ok(())
}

/// Split a `<letter><value>` token into its parts. Errors if the leading
/// character is non-ASCII-alphabetic.
fn split_param(line_no: usize, tok: &str) -> Result<(char, &str), ParseError> {
    let mut chars = tok.chars();
    // `tok` is sourced from `split_whitespace`, which never yields an
    // empty &str; the `expect` pins that invariant rather than emitting
    // a `MissingCommand` that the caller can never observe.
    let letter = chars
        .next()
        .expect("split_param precondition: tok is from split_whitespace, non-empty");
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
