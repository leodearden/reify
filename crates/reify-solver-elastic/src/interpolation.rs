//! Point-in-tet location and P1 shape-function evaluation primitives.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #13.
//!
//! # Scope
//!
//! Ships the Rust math primitives the engine integration layer (PRD §16)
//! wraps as `Field<Point3<Length>, Vector3<Length>>` displacement queries:
//! "at any query point p: locate containing element, interpolate u via
//! element shape functions" (PRD §13). The public surface is plain `f64`
//! types — `Field`-typed wrapping happens at the engine layer, mirroring
//! the pattern in `shell_result.rs` for shells.
//!
//! # Public surface
//!
//! - [`barycentric_p1`] — barycentric coordinates of a query point in a
//!   P1 tetrahedron via the affine reference→physical map.
//! - [`point_in_tet_p1`] — tolerant point-in-tet inclusion test.
//! - [`interpolate_p1_at_point`] — linear interpolation of nodal vector
//!   values at a query point inside a P1 tet.
//! - [`locate_element_p1`] + [`LocatableTet`] — linear-scan search for the
//!   first P1 element containing a query point.
