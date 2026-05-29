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
