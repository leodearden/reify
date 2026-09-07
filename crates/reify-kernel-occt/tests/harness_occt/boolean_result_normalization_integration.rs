//! Integration tests for OCCT boolean-result NORMALIZATION (task #7054).
//!
//! `BRepAlgoAPI_Fuse` / `_Cut` / `_Common` always hand back their answer wrapped
//! in a bare `TopoDS_COMPOUND`, even when the operands merged into a single
//! body, and they leave every coplanar seam face/edge introduced by the boolean
//! in place. Storing that raw shape produces three user-visible defects:
//!
//!   1. `is_watertight` / `is_closed` return **false** on a genuinely closed
//!      solid, because both guard on `SOLID | COMPSOLID | SHELL` and a COMPOUND
//!      is none of those (cpp/occt_wrapper.cpp).
//!   2. `query_distance` / `min_clearance` miss containment: OCCT's
//!      `BRepExtrema_DistShapeShape` only runs its inner-solution /
//!      SolidTreatment test when a top-level operand IS a `TopAbs_SOLID`, so a
//!      fully-buried probe reads the boundary-to-boundary distance instead of 0.
//!   3. Coplanar seam fragmentation: a fuse chain leaves each logical planar
//!      face split into many same-domain fragments, so a bbox-based edge
//!      selector picks up dozens of phantom seam edges instead of the handful of
//!      real ones, and a curated fillet over that selection produces a body with
//!      a wildly inflated face count.
//!
//! The fix is `normalize_boolean_result()` in cpp/occt_wrapper.cpp: unwrap the
//! COMPOUND to the tightest topology-preserving type (one solid → bare SOLID;
//! many → COMPSOLID; none → untouched), then run
//! `ShapeUpgrade_UnifySameDomain` to merge same-domain faces and edges. All four
//! boolean entry points (`boolean_fuse`, `boolean_cut`, `boolean_common`,
//! `fuse_shape_list`) and the shared `extract_boolean_history` chokepoint route
//! through it.
//!
//! # Measured OCCT 7.8 baseline (architect probe, this worktree)
//!
//! Numbers below are direct measurements, not estimates — they are what makes
//! RED distinguishable from GREEN without re-deriving anything:
//!
//! | quantity                                                  | RED (today) | GREEN (fixed) |
//! |-----------------------------------------------------------|-------------|---------------|
//! | `rounded_box(100,60,20,10)` fuse chain: faces / edges      | 50 / 88     | 10 / 24       |
//! | edges selected at the top face (`edges_at_height` predicate)| 40          | 8             |
//! | faces after a 1 mm fillet of that rim selection            | 66          | 18            |
//! | cut-derived container → distance to a fully buried probe   | 90 mm       | 0 mm          |
//! | two abutting 10 mm cubes fused: faces                      | 10          | 6             |
//!
//! Two measured facts that constrain how these tests may be written:
//!
//!   * The filleted body's VOLUME is bit-identical before and after the fix
//!     (118218.498221 mm³ / 150.13749274 g in both probe runs) — the 40 phantom
//!     seam edges sweep exactly the same material as the 8 real ones. A
//!     mass-only assertion for symptom 3 is therefore a guaranteed FALSE GREEN;
//!     the RED signal must be anchored on face/edge/selection COUNTS.
//!   * The un-unified rim fillet SUCCEEDED on this fixture (66 faces) rather
//!     than throwing. Symptom 3's hard failure is fixture-dependent ("which
//!     fuse seams land at the selected height decides"), so no test here may
//!     assert that the fillet errors today.
//!
//! # Observability note
//!
//! `ffi::ffi::shape_type_name` is crate-private (`mod ffi;` at lib.rs:46), so an
//! integration test cannot read the raw `TopAbs` name. The reachable
//! equivalents, via `OcctKernel::repr_of` and `GeometryQuery::IsWatertight`:
//!
//!   * `"Solid"`    ⇔ `BRepKind::Solid` (only `"Solid"` maps to it —
//!     `brep_kind_of_shape` at lib.rs:660 collapses `"CompSolid"`/`"Compound"`
//!     onto `BRepKind::Compound`).
//!   * `"CompSolid"` ⇔ `BRepKind::Compound` **and** `IsWatertight == true`
//!     (a bare `"Compound"` fails the `SOLID|COMPSOLID|SHELL` guard, so
//!     watertightness is what separates the two).
//!
//! Gated on `has_occt` like the 44 sibling OCCT integration modules.

#![cfg(has_occt)]
