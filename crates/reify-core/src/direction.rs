//! Direction-value constants shared across crates (docs/prds/v0_6/direction-typing.md §5 C2/C3).

/// A vector whose magnitude is at or below this has no unit direction.
pub const DIRECTION_DEGENERACY_FLOOR: f64 = 1e-12;

/// The one fix hint every direction rejection ends with.
pub const DIRECTION_MIGRATION_HINT: &str = "write dir(x, y, z)";
