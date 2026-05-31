//! Degenerated (continuum-based) shell substrate: per-node directors and a
//! varying element Jacobian, carrying the MITC3+ assumed transverse-shear field.
//!
//! # References
//!
//! - Ahmad, S., Irons, B. M. & Zienkiewicz, O. C. (1970). "Analysis of thick
//!   and thin shell structures by curved finite elements." *Int. J. Numer.
//!   Methods Eng.*, 2(3), 419–451. — the original *degenerated solid* shell.
//! - Bathe, K.-J. (2014). *Finite Element Procedures*, 2nd ed., §5.4.2 — the
//!   continuum-based (degenerated) shell kinematics used here.
//! - Lee, Y., Lee, P.-S. & Bathe, K.-J. (2014). "The MITC3+ shell element and
//!   its performance." *Computers & Structures*, 138, 12–23. — the assumed
//!   transverse-shear field this substrate *carries* (task 3392 owns it).
//!
//! # Geometry map
//!
//! The element interpolates a mid-surface plus a per-node *director* fibre:
//!
//! ```text
//! X(ξ, η, ζ) = Σ_i N_i(ξ, η) · x_i  +  (ζ / 2) · Σ_i N_i(ξ, η) · t_i · V_i
//! ```
//!
//! where `N_i` are the three linear triangle shape functions
//! ([`crate::elements::mitc3_plus::Mitc3Plus::shape_at`]), `x_i` are the
//! mid-surface vertex positions, `t_i` the nodal thicknesses, `V_i` the
//! per-node **unit directors** (vertex normals), and `ζ ∈ [-1, 1]` the
//! through-thickness natural coordinate (`ζ = +1` top surface, `ζ = -1`
//! bottom).
//!
//! # Why a degenerate substrate (the varying-Jacobian deliverable)
//!
//! On a flat facet with all directors parallel to the facet normal, the 3×3
//! Jacobian `J = ∂X/∂(ξ,η,ζ)` is **invariant** in `ζ` and the element reduces
//! to the flat MITC3+ of task 3392. When the directors tilt (curved geometry),
//! the `(ζ/2) Σ ∇N_i t_i V_i` term makes `J` **vary** across the element —
//! that director-tilt-induced variation IS the varying Jacobian, and it
//! recovers the intra-element membrane–bending coupling a single flat facet
//! cannot represent.
//!
//! # Director provenance (cross-PRD seam G4)
//!
//! The element *consumes* explicit per-node directors (provenance-agnostic).
//! This module additionally ships a neighbour-averaged facet-normal fallback
//! for meshes without extraction-supplied vertex normals; curved benchmarks
//! supply analytic (e.g. radial) directors as the extraction stand-in. Actual
//! voxel-extraction wiring is deferred to integration (tasks 4065 / 4069).
//!
//! # Scope
//!
//! This module owns the *substrate*: directors, the geometry map, the varying
//! Jacobian, the membrane+bending strain–displacement operator, and the
//! covariant→physical re-expression of the carried MITC3+ shear field. The
//! transverse-shear *formulation* itself is task 3392's; ANS-membrane is task
//! 4065's. The element stiffness assembled from these pieces lives beside its
//! flat-facet sibling in [`crate::shell_assembly`].
