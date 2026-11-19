//! Direction-value constants shared across crates (docs/prds/v0_6/direction-typing.md §5 C2/C3).

/// A vector whose magnitude is at or below this has no unit direction.
pub const DIRECTION_DEGENERACY_FLOOR: f64 = 1e-12;

/// The fix hint appended to a direction rejection. It ends every eval-decode
/// rejection (`reify-ir`'s `direction_decode`), but a compile-time `Direction`
/// slot appends it only when the rejected type is a `Vector3` (PRD C7, see
/// `direction_mismatch_hint` in `reify-compiler`'s `builtin_signatures.rs`);
/// Int, String, Axis, Plane, Vector2 and Point3 rejections there render un-hinted.
pub const DIRECTION_MIGRATION_HINT: &str = "write dir(x, y, z)";
