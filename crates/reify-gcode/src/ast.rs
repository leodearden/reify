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
