//! G-code → motion-profile lowering for the `gcode_import` stdlib builtin
//! (PRD `docs/prds/v0_3/trajectory-input-shaping.md` §7.2, §11 task ο).
//!
//! Two layers live here:
//!
//! 1. A pure, registry-free `lower_gcode(source, dialect) -> GcodeImportResult`
//!    that bridges the `reify-gcode` AST (Marlin/Klipper) to a list of motion
//!    profiles — one per contiguous motion segment — plus typed diagnostics.
//!    Everything testable about lowering, segmentation, and diagnostics is
//!    asserted at this layer (no `Value` construction required).
//!
//! 2. A thin marshalling arm wired into
//!    [`crate::trajectory::eval_trajectory`] that maps `Value` arguments to the
//!    pure function and the `GcodeImportResult` back to a `Value::List` of
//!    `Value::Map` profile records (mirroring the `mechanism` builtin's
//!    `Value::Map` structured-result precedent, since the `eval_builtin` path
//!    has no `StructureRegistry` to mint a `Value::StructureInstance`).
//!
//! Diagnostics: `E_GcodeParseError` reuses `reify_gcode::ParseError` verbatim;
//! the two `W_` warnings (`DialectUnsupported`, `ShaperConflict`) are a local
//! enum (the shared `reify-core` `DiagnosticCode` set is out of this task's
//! scope). Eval-path diagnostic surfacing is deferred per the `mechanism.rs`
//! precedent and asserted only at the pure-function level.
//!
//! # Dead-code suppression
//!
//! The pure lowering layer (`lower_gcode` + its result types) is built and
//! tested here ahead of the `eval_trajectory` marshalling arm that consumes it
//! (task ο step-12). The collected `warnings` are asserted only at the
//! pure-function level — the `eval_builtin` path has no diagnostic sink to
//! surface them through (deferred per the `mechanism.rs` precedent) — so in a
//! non-test build several fields/variants are written-but-never-read. Suppress
//! the lint module-wide rather than scattering per-item allows, mirroring the
//! sibling `spline` submodule.
#![allow(dead_code)]

use reify_gcode::{GcodeCommand, ParseError};

/// Which dialect parser [`lower_gcode`] should drive.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GcodeImportDialect {
    /// Marlin dialect → [`reify_gcode::parse_marlin`].
    Marlin,
    /// Klipper dialect → [`reify_gcode::parse_klipper`].
    Klipper,
    /// An unrecognised dialect selector. The inner `String` is the offending
    /// structure `type_name`, surfaced via `W_GcodeDialectUnsupported`.
    Unsupported(String),
}

/// Non-fatal diagnostics collected while lowering (PRD §10.1). The two `W_`
/// codes are a local enum because they are not part of the shared `reify-core`
/// `DiagnosticCode` set (editing that shared crate is out of this task's scope).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GcodeImportWarning {
    /// `W_GcodeDialectUnsupported` — the dialect selector was not recognised;
    /// carries the offending dialect name.
    DialectUnsupported(String),
    /// `W_GcodeDialectShaperConflict` — a file-declared `INPUT_SHAPER` directive
    /// was consumed via `gcode_import` (the in-PRD shaper-design supersedes it).
    ShaperConflict,
}

/// A single resolved motion waypoint: absolute machine coordinates after
/// applying the running position and any `G92` rebases. `feedrate` is the feed
/// in effect for the move that produced this waypoint, if one is known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Waypoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub e: f64,
    pub feedrate: Option<f64>,
}

/// One contiguous motion segment, lowered to an ordered waypoint list.
/// Segments are split by non-motion commands (see [`lower_commands`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MotionProfile {
    pub waypoints: Vec<Waypoint>,
}

/// The outcome of lowering a g-code source: zero or more motion profiles, an
/// optional parse error (which short-circuits to zero profiles), and any
/// non-fatal warnings collected during lowering.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GcodeImportResult {
    pub profiles: Vec<MotionProfile>,
    pub parse_error: Option<ParseError>,
    pub warnings: Vec<GcodeImportWarning>,
}

impl GcodeImportResult {
    /// An empty result: no profiles, no parse error, no warnings.
    fn empty() -> Self {
        GcodeImportResult {
            profiles: Vec::new(),
            parse_error: None,
            warnings: Vec::new(),
        }
    }
}

/// Running kinematic state threaded through the command walk: the absolute
/// position on each axis plus the current feedrate.
#[derive(Debug, Clone, Copy)]
struct PosState {
    x: f64,
    y: f64,
    z: f64,
    e: f64,
    feedrate: Option<f64>,
}

impl PosState {
    /// The machine origin with no feedrate yet established.
    fn origin() -> Self {
        PosState {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            e: 0.0,
            feedrate: None,
        }
    }

    /// Snapshot the current absolute position as a [`Waypoint`].
    fn to_waypoint(self) -> Waypoint {
        Waypoint {
            x: self.x,
            y: self.y,
            z: self.z,
            e: self.e,
            feedrate: self.feedrate,
        }
    }
}

/// Lower a g-code source to motion profiles using the chosen dialect parser.
///
/// Marlin is the only dialect wired in step-2; the Klipper and Unsupported
/// arms are filled in by steps 8 and 10. Parse-error mapping is added in
/// step-6 (until then a parse failure yields an empty result).
pub(crate) fn lower_gcode(source: &str, dialect: GcodeImportDialect) -> GcodeImportResult {
    match dialect {
        GcodeImportDialect::Marlin => match reify_gcode::parse_marlin(source) {
            Ok(commands) => lower_commands(&commands),
            Err(_e) => GcodeImportResult::empty(),
        },
        GcodeImportDialect::Klipper => GcodeImportResult::empty(),
        GcodeImportDialect::Unsupported(_name) => GcodeImportResult::empty(),
    }
}

/// Walk a parsed command stream, accumulating motion waypoints into contiguous
/// segments. In step-2 every motion command feeds a single open segment that is
/// flushed at end-of-input; segment splitting on non-motion commands (and the
/// `G92`/`F` in-segment state updates) is added in step-4.
fn lower_commands(commands: &[GcodeCommand]) -> GcodeImportResult {
    let mut profiles: Vec<MotionProfile> = Vec::new();
    let mut segment: Vec<Waypoint> = Vec::new();
    let warnings: Vec<GcodeImportWarning> = Vec::new();
    let mut cur = PosState::origin();

    for cmd in commands {
        match cmd {
            GcodeCommand::LinearMove(m) => {
                cur.x = m.x.unwrap_or(cur.x);
                cur.y = m.y.unwrap_or(cur.y);
                cur.z = m.z.unwrap_or(cur.z);
                cur.e = m.e.unwrap_or(cur.e);
                if let Some(f) = m.feedrate {
                    cur.feedrate = Some(f);
                }
                segment.push(cur.to_waypoint());
            }
            GcodeCommand::ArcMove(a) => {
                cur.x = a.x.unwrap_or(cur.x);
                cur.y = a.y.unwrap_or(cur.y);
                cur.z = a.z.unwrap_or(cur.z);
                cur.e = a.e.unwrap_or(cur.e);
                if let Some(f) = a.feedrate {
                    cur.feedrate = Some(f);
                }
                segment.push(cur.to_waypoint());
            }
            // SetPosition (G92 rebase), Feedrate (F), and the segment-splitting
            // non-motion commands (IgnoredMCode / SetVelocityLimit / InputShaper)
            // are handled in step-4; InputShaper additionally raises a
            // ShaperConflict warning in step-8. Ignored for now.
            _ => {}
        }
    }

    if !segment.is_empty() {
        profiles.push(MotionProfile { waypoints: segment });
    }

    GcodeImportResult {
        profiles,
        parse_error: None,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A three-move Marlin run with no non-motion commands lowers to exactly
    /// ONE motion profile whose ordered waypoints are the absolute move
    /// targets (0,0) → (10,0) → (10,10). Establishes the `GcodeImportDialect`
    /// enum, the `GcodeImportResult` type, and the per-profile waypoint shape.
    #[test]
    fn marlin_three_linear_moves_lower_to_one_profile() {
        let src = "G1 X0 Y0\nG1 X10 Y0\nG1 X10 Y10";
        let result = lower_gcode(src, GcodeImportDialect::Marlin);

        assert!(
            result.parse_error.is_none(),
            "expected a clean parse, got {:?}",
            result.parse_error
        );
        assert_eq!(
            result.profiles.len(),
            1,
            "a contiguous motion run must lower to exactly one profile"
        );

        let wps = &result.profiles[0].waypoints;
        assert_eq!(wps.len(), 3, "expected three ordered waypoints");
        assert_eq!((wps[0].x, wps[0].y), (0.0, 0.0), "waypoint 0 = G1 X0 Y0");
        assert_eq!((wps[1].x, wps[1].y), (10.0, 0.0), "waypoint 1 = G1 X10 Y0");
        assert_eq!(
            (wps[2].x, wps[2].y),
            (10.0, 10.0),
            "waypoint 2 = G1 X10 Y10"
        );
    }
}
