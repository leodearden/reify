//! Canonical-form [`std::fmt::Display`] impl for [`GcodeCommand`].
//!
//! # Round-trip rules
//!
//! Each variant renders to a canonical single-line Marlin form chosen so
//! that `parse_marlin(display(parse_marlin(src))) == parse_marlin(src)`.
//! Concretely:
//!
//! 1. Command code first (`G0` / `G1` / `G2` / `G3` / `G92` / `F<value>` /
//!    `M<code>`), followed by axis parameters separated by a single
//!    space.
//! 2. Axes are emitted only for `Some(_)` fields, in fixed order
//!    `X Y Z E` for [`LinearMove`] / [`SetPosition`] and
//!    `X Y Z I J K E` for [`ArcMove`], then `F` last when present.
//! 3. `f64` values use Rust's default `{}` formatter — the
//!    shortest-round-trippable IEEE-754 representation — so
//!    `f64::from_str(format!("{}", x)) == x` for every finite `x`.
//! 4. [`IgnoredMCode`] renders as `M{code}` plus a single space then
//!    `params_raw` iff `params_raw` is non-empty.
//! 5. [`Feedrate`] renders as `F{value}`.

use std::fmt::{self, Display, Formatter, Write};

use crate::ast::{
    ArcDirection, ArcMove, Feedrate, GcodeCommand, IgnoredMCode, LinearMove, SetPosition,
    SetVelocityLimit,
};

impl Display for GcodeCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GcodeCommand::LinearMove(mv) => mv.fmt(f),
            GcodeCommand::ArcMove(mv) => mv.fmt(f),
            GcodeCommand::SetPosition(sp) => sp.fmt(f),
            GcodeCommand::Feedrate(fr) => fr.fmt(f),
            GcodeCommand::IgnoredMCode(m) => m.fmt(f),
            GcodeCommand::SetVelocityLimit(svl) => svl.fmt(f),
        }
    }
}

impl Display for LinearMove {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let head = if self.rapid { "G0" } else { "G1" };
        f.write_str(head)?;
        write_axis(f, 'X', self.x)?;
        write_axis(f, 'Y', self.y)?;
        write_axis(f, 'Z', self.z)?;
        write_axis(f, 'E', self.e)?;
        write_axis(f, 'F', self.feedrate)?;
        Ok(())
    }
}

impl Display for ArcMove {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let head = match self.direction {
            ArcDirection::Cw => "G2",
            ArcDirection::Ccw => "G3",
        };
        f.write_str(head)?;
        write_axis(f, 'X', self.x)?;
        write_axis(f, 'Y', self.y)?;
        write_axis(f, 'Z', self.z)?;
        write_axis(f, 'I', self.i)?;
        write_axis(f, 'J', self.j)?;
        write_axis(f, 'K', self.k)?;
        write_axis(f, 'E', self.e)?;
        write_axis(f, 'F', self.feedrate)?;
        Ok(())
    }
}

impl Display for SetPosition {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("G92")?;
        write_axis(f, 'X', self.x)?;
        write_axis(f, 'Y', self.y)?;
        write_axis(f, 'Z', self.z)?;
        write_axis(f, 'E', self.e)?;
        Ok(())
    }
}

impl Display for Feedrate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "F{}", self.value)
    }
}

impl Display for IgnoredMCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "M{}", self.code)?;
        if !self.params_raw.is_empty() {
            f.write_char(' ')?;
            f.write_str(&self.params_raw)?;
        }
        Ok(())
    }
}

impl Display for SetVelocityLimit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("SET_VELOCITY_LIMIT")?;
        for (key, value) in &self.params {
            f.write_char(' ')?;
            f.write_str(key)?;
            f.write_char('=')?;
            f.write_str(value)?;
        }
        Ok(())
    }
}

/// Append ` <letter><value>` iff `value` is `Some(_)`. Float formatting
/// uses the default `{}` (shortest-round-trippable IEEE-754) so the
/// reparse path observes the same `f64` bits.
fn write_axis(f: &mut Formatter<'_>, letter: char, value: Option<f64>) -> fmt::Result {
    if let Some(v) = value {
        write!(f, " {letter}{v}")?;
    }
    Ok(())
}
