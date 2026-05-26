//! Value-typed G-code AST shared across dialects.
//!
//! Populated incrementally by the TDD steps in
//! `docs/prds/v0_3/trajectory-input-shaping.md` §11 task μ.
//!
//! Design note: variants are *semantic* (LinearMove, ArcMove, …) rather
//! than literal G-code numbers — see plan design decision #1 for the
//! rationale (shared-AST contract with task ν Klipper).

/// A single parsed G-code command from any supported dialect.
#[derive(Debug, Clone, PartialEq)]
pub enum GcodeCommand {
    /// G0 (rapid) / G1 (feed) linear move. `rapid` discriminates the two.
    LinearMove(LinearMove),
    /// G2 (CW) / G3 (CCW) arc move, IJK form.
    ArcMove(ArcMove),
    /// G92 set-position. Each `Some(_)` axis is rebased to the supplied
    /// value; `None` axes are left untouched. A bare `G92` with all-None
    /// axes is permitted (Marlin treats it as a no-op).
    SetPosition(SetPosition),
    /// Standalone `F<value>` feedrate update on its own line.
    Feedrate(Feedrate),
    /// M-code that is parsed and preserved in the AST but contributes
    /// nothing to trajectory planning (PRD §7.1 "ignored for trajectory":
    /// M104/M109 extruder temp, M82/M83 extruder absolute/relative mode).
    /// `params_raw` preserves the post-code remainder of the source line
    /// verbatim so the round-trip contract holds without revalidating
    /// parameters we don't otherwise care about.
    IgnoredMCode(IgnoredMCode),
    /// Klipper-dialect `SET_VELOCITY_LIMIT KEY=VAL ...` directive. Carries
    /// ordered KEY=VALUE pairs as raw strings; semantic interpretation of
    /// the keys/values is the consumer's responsibility (task ο,
    /// `gcode_import`). See plan design decision #3 for why this is
    /// `Vec<(String, String)>` rather than a typed struct or HashMap.
    SetVelocityLimit(SetVelocityLimit),
}

/// Parameters for a G0/G1 linear move.
///
/// Each axis is `Option<f64>` because Marlin permits omitting axes that
/// keep their current target; the parser preserves the omission rather
/// than substituting a default so the AST round-trip stays loss-less.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearMove {
    /// `true` for G0 (rapid traverse), `false` for G1 (feed move).
    pub rapid: bool,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    /// Extruder axis (E) — distinct from XYZ for printer dialects.
    pub e: Option<f64>,
    /// In-line `F<value>` feedrate override, if present.
    pub feedrate: Option<f64>,
}

/// Rotation sense for an [`ArcMove`]. G2 → [`ArcDirection::Cw`],
/// G3 → [`ArcDirection::Ccw`].
#[derive(Debug, Clone, PartialEq)]
pub enum ArcDirection {
    Cw,
    Ccw,
}

/// Parameters for a G2/G3 arc move in IJK form.
///
/// `i`/`j`/`k` are the center-offset components relative to the current
/// position (Marlin's default arc form); see the PRD §7.1 Marlin subset
/// for the supported parameter set.
#[derive(Debug, Clone, PartialEq)]
pub struct ArcMove {
    pub direction: ArcDirection,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub i: Option<f64>,
    pub j: Option<f64>,
    pub k: Option<f64>,
    pub e: Option<f64>,
    pub feedrate: Option<f64>,
}

/// Parameters for a G92 set-position command. Each `Some(_)` axis is
/// the new logical position; `None` axes are left untouched.
#[derive(Debug, Clone, PartialEq)]
pub struct SetPosition {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    pub e: Option<f64>,
}

/// Payload for a standalone `F<value>` feedrate line. In-line feedrate
/// overrides (e.g. `G1 X10 F1200`) are captured by the host command's
/// `feedrate` field, not as a separate `Feedrate` AST node.
#[derive(Debug, Clone, PartialEq)]
pub struct Feedrate {
    pub value: f64,
}

/// Trajectory-irrelevant M-code retained for round-trip fidelity.
/// `params_raw` is the trimmed post-code source-line remainder (empty
/// when the M-code has no parameters).
#[derive(Debug, Clone, PartialEq)]
pub struct IgnoredMCode {
    pub code: u16,
    pub params_raw: String,
}

/// Payload for Klipper's `SET_VELOCITY_LIMIT KEY=VAL ...` directive
/// (PRD §7.1 Klipper subset; task ν).
///
/// `params` holds ordered `(KEY, VALUE)` pairs in source order, both as
/// raw strings (no semantic coercion at parse time). The round-trip
/// `Display` impl writes the pairs back in stored order so a directive
/// like `SET_VELOCITY_LIMIT ACCEL=3000 VELOCITY=200` re-emits exactly
/// the same source spelling. Empty `params` corresponds to a bare
/// `SET_VELOCITY_LIMIT` line.
#[derive(Debug, Clone, PartialEq)]
pub struct SetVelocityLimit {
    pub params: Vec<(String, String)>,
}
