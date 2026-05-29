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

    /// A short-circuited result carrying a parse error and no profiles. The
    /// `reify_gcode::ParseError` is reused verbatim as the `E_GcodeParseError`
    /// carrier (1-indexed line + dialect-specific reason).
    fn from_parse_error(error: ParseError) -> Self {
        GcodeImportResult {
            profiles: Vec::new(),
            parse_error: Some(error),
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
/// A parse failure short-circuits to a [`GcodeImportResult::from_parse_error`]
/// (no profiles). The Klipper and Unsupported arms are filled in by steps 8
/// and 10.
pub(crate) fn lower_gcode(source: &str, dialect: GcodeImportDialect) -> GcodeImportResult {
    match dialect {
        GcodeImportDialect::Marlin => match reify_gcode::parse_marlin(source) {
            Ok(commands) => lower_commands(&commands),
            Err(e) => GcodeImportResult::from_parse_error(e),
        },
        GcodeImportDialect::Klipper => match reify_gcode::parse_klipper(source) {
            Ok(commands) => lower_commands(&commands),
            Err(e) => GcodeImportResult::from_parse_error(e),
        },
        GcodeImportDialect::Unsupported(_name) => GcodeImportResult::empty(),
    }
}

/// Walk a parsed command stream, lowering it to contiguous motion segments.
///
/// - `LinearMove`/`ArcMove` append a waypoint (omitted axes inherit the running
///   position) to the open segment.
/// - `SetPosition` (G92) rebases the running position; `Feedrate` (standalone
///   F) updates the running feedrate. Both are in-segment state — no split.
/// - `IgnoredMCode`/`SetVelocityLimit`/`InputShaper` close the open segment
///   (emitting a profile only when it holds ≥1 waypoint). The running position
///   persists across the split.
///
/// The trailing open segment is flushed at end-of-input.
fn lower_commands(commands: &[GcodeCommand]) -> GcodeImportResult {
    let mut profiles: Vec<MotionProfile> = Vec::new();
    let mut segment: Vec<Waypoint> = Vec::new();
    let mut warnings: Vec<GcodeImportWarning> = Vec::new();
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
            // G92 set-position rebases the running position in place: each
            // supplied axis is set to its new logical value; omitted axes are
            // left untouched. No waypoint is emitted and the segment is not
            // split (a rebase is in-segment state, not a motion boundary).
            GcodeCommand::SetPosition(sp) => {
                if let Some(v) = sp.x {
                    cur.x = v;
                }
                if let Some(v) = sp.y {
                    cur.y = v;
                }
                if let Some(v) = sp.z {
                    cur.z = v;
                }
                if let Some(v) = sp.e {
                    cur.e = v;
                }
            }
            // A standalone `F` updates the running feedrate consumed by
            // subsequent moves; it is in-segment state and never splits.
            GcodeCommand::Feedrate(f) => {
                cur.feedrate = Some(f.value);
            }
            // Non-motion commands close the current motion segment (PRD §7.2):
            // emit a profile only when the open segment holds at least one
            // motion waypoint, so back-to-back splitters never produce empty
            // profiles. The running position persists across the split.
            //
            // An `INPUT_SHAPER` directive additionally raises a ShaperConflict
            // warning: its presence signals a file-declared shaper that the
            // in-PRD shaper-design supersedes (PRD §10.1; the directive's
            // variant identity is the cross-task signal — see ast.rs).
            GcodeCommand::IgnoredMCode(_)
            | GcodeCommand::SetVelocityLimit(_)
            | GcodeCommand::InputShaper(_) => {
                if matches!(cmd, GcodeCommand::InputShaper(_)) {
                    warnings.push(GcodeImportWarning::ShaperConflict);
                }
                if !segment.is_empty() {
                    profiles.push(MotionProfile {
                        waypoints: std::mem::take(&mut segment),
                    });
                }
            }
        }
    }

    // Flush the trailing open segment at end-of-input.
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

    /// An ignored M-code interleaved between two moves splits the contiguous
    /// run into TWO profiles. The running machine position persists across the
    /// split (the second profile's waypoint is the absolute `G1 X20` target).
    #[test]
    fn marlin_ignored_mcode_splits_into_two_profiles() {
        let src = "G1 X10\nM104 S200\nG1 X20";
        let result = lower_gcode(src, GcodeImportDialect::Marlin);

        assert!(result.parse_error.is_none(), "{:?}", result.parse_error);
        assert_eq!(
            result.profiles.len(),
            2,
            "the ignored M104 must split the run into two profiles"
        );
        assert_eq!(
            result.profiles[0].waypoints.len(),
            1,
            "first profile holds the pre-M104 move"
        );
        assert_eq!(result.profiles[0].waypoints[0].x, 10.0, "G1 X10");
        assert_eq!(
            result.profiles[1].waypoints.len(),
            1,
            "second profile holds the post-M104 move"
        );
        assert_eq!(result.profiles[1].waypoints[0].x, 20.0, "G1 X20");
    }

    /// A `G92` set-position and a standalone `F` feedrate line interleaved
    /// between two moves do NOT split the run (single profile). The `G92`
    /// rebases the running position so a subsequent move that omits the rebased
    /// axis inherits the rebased value, and the standalone `F` updates the
    /// running feedrate consumed by that move.
    #[test]
    fn marlin_g92_and_feedrate_are_in_segment_state_updates() {
        let src = "G1 X10 Y0\nG92 X0\nF1200\nG1 Y10";
        let result = lower_gcode(src, GcodeImportDialect::Marlin);

        assert!(result.parse_error.is_none(), "{:?}", result.parse_error);
        assert_eq!(
            result.profiles.len(),
            1,
            "G92 and standalone F must not split the segment"
        );

        let wps = &result.profiles[0].waypoints;
        assert_eq!(wps.len(), 2, "two motion waypoints in the single segment");
        assert_eq!((wps[0].x, wps[0].y), (10.0, 0.0), "first move target");
        // `G92 X0` rebased x→0; the `G1 Y10` move omits X, so it inherits the
        // rebased x=0 rather than the pre-rebase x=10.
        assert_eq!(
            (wps[1].x, wps[1].y),
            (0.0, 10.0),
            "G92 rebased x to 0; subsequent move inherits the rebased x"
        );
        assert_eq!(
            wps[1].feedrate,
            Some(1200.0),
            "standalone F1200 updates the running feedrate consumed by G1 Y10"
        );
    }

    /// A parse failure short-circuits: the `reify_gcode::ParseError` is carried
    /// verbatim in `parse_error` (1-indexed line + `UnknownCommand` reason) and
    /// no profiles are emitted (even the valid line-1 move is discarded).
    #[test]
    fn marlin_parse_error_is_carried_with_no_profiles() {
        let src = "G1 X10\nG99 BOGUS";
        let result = lower_gcode(src, GcodeImportDialect::Marlin);

        let err = result
            .parse_error
            .expect("a malformed line must surface as parse_error");
        assert_eq!(err.line, 2, "G99 is on the second physical line");
        assert!(
            matches!(err.kind, reify_gcode::error::ParseErrorKind::UnknownCommand(_)),
            "G99 is an unrecognised command, got {:?}",
            err.kind
        );
        assert!(
            result.profiles.is_empty(),
            "a parse error short-circuits to zero profiles"
        );
    }

    /// A Klipper source containing an `INPUT_SHAPER` directive collects exactly
    /// one `ShaperConflict` warning (the directive also splits the motion run,
    /// per step-4). Satisfies task ν's "emits W_GcodeDialectShaperConflict when
    /// consumed via gcode_import".
    #[test]
    fn klipper_input_shaper_collects_one_shaper_conflict_warning() {
        let src = "G1 X10 Y0\nINPUT_SHAPER SHAPER_TYPE_X=mzv SHAPER_FREQ_X=40\nG1 X20 Y0";
        let result = lower_gcode(src, GcodeImportDialect::Klipper);

        assert!(result.parse_error.is_none(), "{:?}", result.parse_error);
        assert_eq!(
            result.profiles.len(),
            2,
            "INPUT_SHAPER splits the run into two profiles"
        );
        let shaper_warnings = result
            .warnings
            .iter()
            .filter(|w| matches!(w, GcodeImportWarning::ShaperConflict))
            .count();
        assert_eq!(
            shaper_warnings, 1,
            "exactly one ShaperConflict warning for the single INPUT_SHAPER"
        );
    }

    /// A Klipper source with only `SET_VELOCITY_LIMIT` (no `INPUT_SHAPER`)
    /// collects zero shaper-conflict warnings — the velocity-limit directive
    /// still splits segments but is not a shaper conflict.
    #[test]
    fn klipper_velocity_limit_only_collects_no_shaper_conflict() {
        let src = "G1 X10\nSET_VELOCITY_LIMIT VELOCITY=200 ACCEL=3000\nG1 X20";
        let result = lower_gcode(src, GcodeImportDialect::Klipper);

        assert!(result.parse_error.is_none(), "{:?}", result.parse_error);
        assert_eq!(
            result.profiles.len(),
            2,
            "SET_VELOCITY_LIMIT splits the run into two profiles"
        );
        let shaper_warnings = result
            .warnings
            .iter()
            .filter(|w| matches!(w, GcodeImportWarning::ShaperConflict))
            .count();
        assert_eq!(
            shaper_warnings, 0,
            "SET_VELOCITY_LIMIT must not raise a ShaperConflict"
        );
    }

    /// An unsupported dialect selector skips parsing entirely: zero profiles
    /// and exactly one `DialectUnsupported` warning naming the offending
    /// dialect.
    #[test]
    fn unsupported_dialect_yields_no_profiles_and_one_warning() {
        let result = lower_gcode(
            "G1 X10\nG1 X20",
            GcodeImportDialect::Unsupported("FooDialect".to_string()),
        );

        assert!(result.parse_error.is_none(), "{:?}", result.parse_error);
        assert!(
            result.profiles.is_empty(),
            "an unsupported dialect parses nothing"
        );
        assert_eq!(result.warnings.len(), 1, "exactly one warning");
        assert!(
            matches!(
                &result.warnings[0],
                GcodeImportWarning::DialectUnsupported(name) if name == "FooDialect"
            ),
            "the warning names the offending dialect, got {:?}",
            result.warnings
        );
    }
}
