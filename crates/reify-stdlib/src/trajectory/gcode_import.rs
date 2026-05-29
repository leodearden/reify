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
