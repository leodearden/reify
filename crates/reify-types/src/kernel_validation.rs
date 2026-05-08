//! Shared validation-message constants for geometry kernel operations.
//!
//! Both `reify-kernel-fidget` and `reify-kernel-occt` must emit byte-identical
//! error messages for Sphere radius and Box dimension validation so that
//! callers (tests, log parsers, UI) can match on a single string regardless of
//! which kernel is active.  The constants here are the single source of truth:
//!
//! - Fidget production site: `crates/reify-kernel-fidget/src/kernel.rs` —
//!   `execute(Sphere)` and `execute(Box)` arms.
//! - OCCT production site: `crates/reify-kernel-occt/src/lib.rs` —
//!   `OcctKernel::execute` `Sphere` and `Box` arms.
//!
//! Every test that asserts the error message should `assert_eq!` against these
//! constants rather than using substring containment, so that message drift
//! between the two kernels is caught at compile time rather than by accident.

/// Error message emitted when a Sphere `radius` value fails the
/// finite-and-strictly-positive check.
///
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const SPHERE_RADIUS_MUST_BE_FINITE_POSITIVE: &str =
    "sphere radius must be a finite positive value";

/// Error message emitted when any Box dimension (`width`, `height`, or `depth`)
/// fails the finite-and-strictly-positive check.
///
/// Note the plural "values": all three dimensions are validated in a single
/// combined check, so a single message covers any dimension failure.
/// Byte-identical across fidget and OCCT kernels; both must reference this
/// constant rather than inlining a literal.
pub const BOX_DIMENSIONS_MUST_BE_FINITE_POSITIVE: &str =
    "box dimensions must be finite positive values";
