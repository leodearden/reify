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
/// See the constant value below for the exact threshold; the corresponding
/// minimum segment length is its square root.
// `build.rs` includes this file via `#[path]` but only uses `CPP_LINE_WIRE_MIN_LENGTH_SQ`;
// `RUST_LINE_WIRE_MIN_LENGTH_SQ` is used in `lib.rs` but appears dead to the build-script
// compiler. Allow here so the build-script dead_code lint does not fire.
#[allow(dead_code)]
pub(crate) const RUST_LINE_WIRE_MIN_LENGTH_SQ: f64 = 1e-12;

/// Minimum squared length (m²) for `make_line_wire` endpoints — C++ defense-in-depth floor.
///
/// Rejects degenerate wires whose squared length is below this threshold.
/// Sits between the Rust primary floor (`RUST_LINE_WIRE_MIN_LENGTH_SQ`) and OCCT's
/// own `Precision::Confusion` guard, catching inputs that bypass the Rust layer
/// without colliding with axis-vector guard sites.
///
/// See the constant value below for the exact threshold; the corresponding
/// minimum segment length is its square root.
///
/// This value is emitted into `$OUT_DIR/line_wire_floors.h` by `build.rs` and
/// consumed by `cpp/occt_wrapper.cpp` via `#include "line_wire_floors.h"`.
pub(crate) const CPP_LINE_WIRE_MIN_LENGTH_SQ: f64 = 1e-10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_floor_rejects_with_rust_guard_marker() {
        // Derive a below-floor dx so that dist_sq = dx² < RUST_LINE_WIRE_MIN_LENGTH_SQ.
        // Using 0.9 × floor gives a 10% margin that survives fp round-trip.
        let below_dx = (0.9 * RUST_LINE_WIRE_MIN_LENGTH_SQ).sqrt();
        debug_assert!(
            below_dx * below_dx < RUST_LINE_WIRE_MIN_LENGTH_SQ,
            "below_dx² must be strictly < RUST_LINE_WIRE_MIN_LENGTH_SQ after fp round-trip"
        );

        let result = line_segment_rust_guard(below_dx, 0.0, 0.0);
        match result {
            Err(reify_types::GeometryError::OperationFailed(msg)) => {
                assert!(
                    msg.contains(RUST_GUARD_MARKER),
                    "below-floor rejection must contain the '[rust-guard]' marker, got: {msg:?}"
                );
            }
            Ok(()) => panic!(
                "below-floor case (dist_sq = 0.9 × RUST_LINE_WIRE_MIN_LENGTH_SQ) \
                 should return Err, got Ok"
            ),
        }
    }
}
