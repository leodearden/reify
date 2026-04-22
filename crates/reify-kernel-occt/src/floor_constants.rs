//! Single-source floor constants for `make_line_wire` length guards.
//!
//! This module is the **canonical source** for both the Rust-layer primary floor
//! (`RUST_LINE_WIRE_MIN_LENGTH_SQ`) and the C++ defense-in-depth floor
//! (`CPP_LINE_WIRE_MIN_LENGTH_SQ`). Consumers:
//!
//! - `lib.rs` — imports both constants via `use floor_constants::...` and enforces
//!   the layered invariant `RUST < CPP` with a compile-time `const _: () = assert!(...)`.
//! - `build.rs` — reads `CPP_LINE_WIRE_MIN_LENGTH_SQ` via `#[path = ...] mod floor_constants`
//!   and writes its value into `$OUT_DIR/line_wire_floors.h` so C++ can `#include` it.
//!
//! Changing either constant here automatically propagates to both layers on the next
//! `cargo build`; `cargo:rerun-if-changed=src/floor_constants.rs` ensures the generated
//! header is regenerated whenever this file changes.

/// Minimum squared length (m²) for `make_line_wire` endpoints — primary Rust-layer floor.
///
/// Line segments with squared point-to-point distance below this threshold are rejected
/// before the FFI call, catching sub-micrometer degenerate wires early.
///
/// This guard is the primary/early check. The C++ layer applies a looser
/// defense-in-depth floor (`CPP_LINE_WIRE_MIN_LENGTH_SQ`) so that any input
/// that bypasses Rust still gets rejected at the FFI boundary.
///
/// The invariant `RUST_LINE_WIRE_MIN_LENGTH_SQ < CPP_LINE_WIRE_MIN_LENGTH_SQ`
/// is enforced at compile time by `const _: () = assert!(...)` in `lib.rs`.
///
/// Value: 1e-12 m² → minimum segment length ~1 µm.
pub(crate) const RUST_LINE_WIRE_MIN_LENGTH_SQ: f64 = 1e-12;

/// Minimum squared length (m²) for `make_line_wire` endpoints — C++ defense-in-depth floor.
///
/// Rejects lengths shorter than √(1e-10) m = 1e-5 m ≈ 10 µm.
/// Sits between the Rust primary floor (1e-12 m²) and OCCT's own
/// `Precision::Confusion` guard (≈ 1e-7 m, ~0.1 µm), catching inputs
/// that bypass the Rust layer without colliding with axis-vector guard sites.
///
/// This value is emitted into `$OUT_DIR/line_wire_floors.h` by `build.rs` and
/// consumed by `cpp/occt_wrapper.cpp` via `#include "line_wire_floors.h"`.
///
/// Value: 1e-10 m² → minimum segment length ~10 µm.
pub(crate) const CPP_LINE_WIRE_MIN_LENGTH_SQ: f64 = 1e-10;
