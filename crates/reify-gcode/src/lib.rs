//! `reify-gcode` — G-code parser with a value-typed AST shared across dialects.
//!
//! # PRD reference
//!
//! `docs/prds/v0_3/trajectory-input-shaping.md` §7 Phase 5 task **μ**
//! (Marlin parser; OPTIONAL bolt-on). Task **ν** extends with Klipper
//! macros (SET_VELOCITY_LIMIT, INPUT_SHAPER, …) on the same [`GcodeCommand`]
//! enum. Task **ο** (`gcode_import`) consumes parsed ASTs and lowers them
//! into a uniform list of motion profiles for the trajectory pipeline.
//!
//! # Crate surface
//!
//! - [`parse_marlin`] — line-by-line Marlin-dialect parser entry point.
//! - [`GcodeCommand`] — value-typed AST shared by all dialects.
//! - [`ParseError`] — diagnostic with 1-indexed source line + dialect-specific
//!   reason (PRD §1 `E_GcodeParseError`).
//!
//! # Round-trip contract
//!
//! The parser + [`std::fmt::Display`] impl satisfy
//! `parse(display(parse(src))) == parse(src)` for every accepted input,
//! observable via the `roundtrip` integration test (PRD §10.1 G-code Marlin
//! parser smoke).

pub mod ast;
pub mod display;
pub mod error;
pub mod marlin;

// Re-exports of public crate surface (`GcodeCommand`, `ParseError`,
// `parse_marlin`) are added by step-2 when the underlying items are
// introduced; keeping them out of the prereq lets the scaffold pass
// `cargo check` with empty module stubs.
