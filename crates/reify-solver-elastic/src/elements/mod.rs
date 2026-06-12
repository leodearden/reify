//! Reference-element primitives for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #7.
//!
//! # Canonical reference geometries
//!
//! This module contains four families of elements, each with its own
//! canonical reference geometry and coordinate type:
//!
//! - **3D tetrahedral elements** ([`tet_p1::TetP1`], [`tet_p2::TetP2`]) —
//!   defined on the **unit reference tetrahedron** with vertices at
//!   `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` in `(ξ, η, ζ)` coordinates
//!   (reference-tet volume `1/6`).  Use [`ReferenceCoord`] for these
//!   elements.
//!
//! - **3D hexahedral elements** ([`hex_p1::HexP1`]) — defined on the
//!   **reference cube** `[-1, 1]³` in `(ξ, η, ζ)` coordinates
//!   (reference-cube volume `8`).  Use [`ReferenceCoord`] for these
//!   elements.
//!
//! - **3D triangular-prism elements** ([`wedge_p1::WedgeP1`]) — defined on
//!   the **unit reference prism** = unit triangle × `[-1, +1]` in
//!   `(ξ, η, ζ)` coordinates (`ξ ≥ 0, η ≥ 0, ξ+η ≤ 1, ζ ∈ [-1, +1]`,
//!   reference-prism volume `1`).  Use [`ReferenceCoord`] for these
//!   elements.
//!
//! - **2D triangular shell elements** ([`mitc3_plus::Mitc3Plus`]) — defined
//!   on the **unit reference triangle** with vertices at `(0,0), (1,0),
//!   (0,1)` in local `(ξ, η)` mid-surface coordinates.  Use
//!   [`mitc3_plus::ShellReferenceCoord`] for these elements.

pub mod degenerate_shell;
pub mod hex_p1;
// Task 4417/ζ: dedicated 3-DOF/node CST membrane element (K_e).
pub mod membrane_cst;
pub mod mitc3_plus;
pub mod tet_p1;
pub mod tet_p2;
pub mod wedge_p1;

/// A 3D reference-element coordinate triple `(ξ, η, ζ)`.
///
/// The interpretation depends on the implementing element:
///
/// - **`TetP1` / `TetP2`** — unit-tet simplex with vertices at
///   `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`; barycentric coordinates are
///   `(1-ξ-η-ζ, ξ, η, ζ)`.
/// - **`HexP1`** — reference cube `[-1, 1]³` with corners at `{±1}³`.
/// - **`WedgeP1`** — unit triangle × `[-1, +1]`; `ξ ≥ 0, η ≥ 0, ξ+η ≤ 1,
///   ζ ∈ [-1, +1]`; barycentric coordinates on the base are `(1-ξ-η, ξ, η)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferenceCoord {
    pub xi: f64,
    pub eta: f64,
    pub zeta: f64,
}

impl ReferenceCoord {
    /// Construct a reference-coordinate triple.
    pub const fn new(xi: f64, eta: f64, zeta: f64) -> Self {
        Self { xi, eta, zeta }
    }
}

/// Reference→physical Jacobian of an element at a single reference
/// coordinate.
///
/// `matrix[i][j] = ∂x_i / ∂ξ_j` where `x` is the physical coordinate and
/// `ξ` the reference coordinate. `det` is the determinant of `matrix`.
///
/// This is the **forward** map only. The inverse / transpose-inverse map
/// (`Jᵀ⁻¹`) needed to push reference gradients into physical gradients
/// for stiffness assembly is the consumer's responsibility (PRD task #8).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jacobian {
    pub matrix: [[f64; 3]; 3],
    pub det: f64,
}

impl Jacobian {
    /// Construct from a 3×3 matrix; computes the determinant via cofactor
    /// expansion.
    pub fn from_matrix(matrix: [[f64; 3]; 3]) -> Self {
        let det = matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
            - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
            + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0]);
        Self { matrix, det }
    }
}

/// A quadrature point: a reference-coordinate location and its weight.
///
/// Weights sum to the implementing element's reference volume:
/// `1/6` for `TetP1`/`TetP2` (unit-tet simplex), `8` for `HexP1`
/// (reference cube `[-1, 1]³`), `1` for `WedgeP1` (unit-triangle × `[-1, +1]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadraturePoint {
    pub coord: ReferenceCoord,
    pub weight: f64,
}

/// Reference-element Lagrangian trait for 3D volumetric elements
/// (tetrahedral and hexahedral).
///
/// Implementors expose:
/// - the number of nodes (`N_NODES`),
/// - the shape functions evaluated at a reference coordinate
///   (`shape_at`), returning a `Vec<f64>` of length `N_NODES`,
/// - the shape-function gradients in reference coordinates
///   (`shape_grad_at`), returning a `Vec<[f64; 3]>` of length `N_NODES`,
/// - a Gauss quadrature rule (`quad_points`) covering the implementing
///   element's canonical reference geometry.
///
/// The default `jacobian` method composes `shape_grad_at` with
/// caller-supplied physical-node coordinates to produce the
/// reference→physical Jacobian (forward map only; inverse / Jᵀ⁻¹
/// mapping for physical-gradient assembly is the consumer's
/// responsibility — see PRD task #8).
///
/// # Performance note (deferred refactor)
///
/// `shape_at` and `shape_grad_at` currently return `Vec<f64>` /
/// `Vec<[f64; 3]>`, which heap-allocates on every call. The skeleton
/// crate has no hot-path consumers yet, so this is acceptable for the
/// v0.3 reference-element ship, but stiffness assembly (PRD task #8)
/// calls these once per element per quadrature point — millions of
/// times for nontrivial meshes.
///
/// When task #8 wires assembly, switch the return type to a
/// stack-friendly form before consumers proliferate. Two viable shapes:
///
/// 1. Fill-in-place variants — `fn shape_at_into(&self, coord, out:
///    &mut [f64])` and `fn shape_grad_at_into(&self, coord, out:
///    &mut [[f64; 3]])` — letting the caller reuse a single buffer
///    across the inner quadrature loop.
/// 2. Fixed-capacity returns — `SmallVec<[f64; 16]>` /
///    `SmallVec<[[f64; 3]; 16]>` (16 covers P1 / P2 / future hex8 with
///    no spill), giving an inline buffer without changing the call
///    site's signature.
///
/// `N_NODES` is an associated `const`, so either rework is a local
/// change in `tet_p1.rs` / `tet_p2.rs`; the trait-level signature
/// change is the only cross-cutting concern.
pub trait ReferenceElement {
    /// Number of Lagrangian nodes per element (e.g., 4 for P1, 10 for P2).
    const N_NODES: usize;

    /// Shape-function values `[N_0, …, N_{N-1}]` at the given reference
    /// coordinate. The returned `Vec` has length `N_NODES`.
    fn shape_at(&self, coord: ReferenceCoord) -> Vec<f64>;

    /// Shape-function gradients in reference coordinates,
    /// `[∇N_0, …, ∇N_{N-1}]`, where each gradient is `[∂N/∂ξ, ∂N/∂η,
    /// ∂N/∂ζ]`. The returned `Vec` has length `N_NODES`.
    fn shape_grad_at(&self, coord: ReferenceCoord) -> Vec<[f64; 3]>;

    /// Gauss quadrature rule for integration over the implementing element's
    /// canonical reference geometry.
    ///
    /// Weights sum to that geometry's volume: `1/6` for `TetP1`/`TetP2`,
    /// `8` for `HexP1`, `1` for `WedgeP1`.
    fn quad_points(&self) -> &'static [QuadraturePoint];

    /// Reference→physical Jacobian at `ref_coord`.
    ///
    /// Computes `J_ij = Σ_k phys_nodes[k][i] · shape_grad_at(ref_coord)[k][j]`.
    ///
    /// `phys_nodes.len()` must equal `Self::N_NODES` and the entries must
    /// be ordered to match the canonical reference-vertex ordering pinned
    /// in the implementing element module:
    ///
    /// - **`TetP1`** — vertices in `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`
    ///   order.
    /// - **`TetP2`** — same vertex order followed by the 6 edge midpoints
    ///   in canonical Hughes/Gmsh order `(0,1), (1,2), (2,0), (0,3),
    ///   (1,3), (2,3)`.
    /// - **`HexP1`** — 8 vertices at the corners `{±1}³` of the reference
    ///   cube `[-1, 1]³` in canonical Hughes/Gmsh hex8 order: bottom face
    ///   (ζ = −1) traversed counter-clockwise from `(-1,-1,-1)`, then top
    ///   face (ζ = +1) in the same cyclic order.
    /// - **`WedgeP1`** — 6 vertices of the unit reference prism in Gmsh
    ///   PRI6 order: bottom face (ζ = −1) at barycentric vertices
    ///   `(0,0,-1), (1,0,-1), (0,1,-1)` (nodes 0–2), then top face
    ///   (ζ = +1) in the same cyclic barycentric order (nodes 3–5).
    fn jacobian(&self, phys_nodes: &[[f64; 3]], ref_coord: ReferenceCoord) -> Jacobian {
        // Intentionally unconditional (`assert_eq!`, not `debug_assert_eq!`):
        // the public contract is explicit in every build profile per the
        // project's contract-explicitness convention (see Task 2544).  The
        // cost is two `usize` comparisons against the 9·N flop Jacobian loop
        // that follows — negligible relative to that work.
        assert_eq!(
            phys_nodes.len(),
            Self::N_NODES,
            "phys_nodes.len() must equal Self::N_NODES",
        );
        let grads = self.shape_grad_at(ref_coord);
        assert_eq!(
            grads.len(),
            Self::N_NODES,
            "shape_grad_at must return N_NODES rows",
        );
        let mut m = [[0.0_f64; 3]; 3];
        for k in 0..Self::N_NODES {
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += phys_nodes[k][i] * grads[k][j];
                }
            }
        }
        Jacobian::from_matrix(m)
    }
}

// Behavioral coverage for the trait surface lives in the per-element
// modules (`tet_p1.rs`, `tet_p2.rs`): Kronecker delta, partition of
// unity, quadrature integration, and Jacobian determinants exercise
// every public method of `ReferenceElement`. A doc-test on the crate
// root (`lib.rs`) smoke-tests the re-export wiring.
